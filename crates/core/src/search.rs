use std::path::Path;

use ignore::WalkBuilder;
use ignore::overrides::{Override, OverrideBuilder};
use regex::{Regex, RegexBuilder};

pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Default, Clone)]
pub struct SearchFilter {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

impl SearchFilter {
    pub fn parse(include: &str, exclude: &str) -> SearchFilter {
        let split = |s: &str| {
            s.split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect()
        };
        SearchFilter {
            include: split(include),
            exclude: split(exclude),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }

    fn build(&self, root: &Path) -> Result<Override, String> {
        let mut b = OverrideBuilder::new(root);
        for g in &self.include {
            b.add(g).map_err(|e| format!("include `{g}`: {e}"))?;
        }
        for g in &self.exclude {
            b.add(&format!("!{g}"))
                .map_err(|e| format!("exclude `{g}`: {e}"))?;
        }
        b.build().map_err(|e| e.to_string())
    }
}

pub struct Matcher {
    re: Regex,
    literal_replace: bool,
}

impl Matcher {
    pub fn new(pattern: &str, regex: bool, case_sensitive: bool) -> Result<Matcher, String> {
        if pattern.is_empty() {
            return Err("empty pattern".to_string());
        }
        let pat = if regex {
            pattern.to_string()
        } else {
            regex::escape(pattern)
        };
        let re = RegexBuilder::new(&pat)
            .case_insensitive(!case_sensitive)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Matcher {
            re,
            literal_replace: !regex,
        })
    }
}

pub struct LineHit {
    pub line_no: u32,
    pub text: String,
    pub ranges: Vec<(usize, usize)>,
}

pub struct FileHit {
    pub path: String,
    pub lines: Vec<LineHit>,
}

pub fn line_ranges(matcher: &Matcher, line: &str) -> Vec<(usize, usize)> {
    matcher
        .re
        .find_iter(line)
        .filter(|m| m.start() != m.end())
        .map(|m| (m.start(), m.end()))
        .collect()
}

pub fn search_text(matcher: &Matcher, text: &str) -> Vec<LineHit> {
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        let mut ranges = Vec::new();
        for m in matcher.re.find_iter(line) {
            if m.start() != m.end() {
                ranges.push((m.start(), m.end()));
            }
        }
        if !ranges.is_empty() {
            out.push(LineHit {
                line_no: (i + 1) as u32,
                text: line.to_string(),
                ranges,
            });
        }
    }
    out
}

pub fn replace_all_in_text(matcher: &Matcher, text: &str, replacement: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    for (i, seg) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        count += matcher
            .re
            .find_iter(seg)
            .filter(|m| m.start() != m.end())
            .count();
        let replaced = if matcher.literal_replace {
            matcher.re.replace_all(seg, regex::NoExpand(replacement))
        } else {
            matcher.re.replace_all(seg, replacement)
        };
        out.push_str(&replaced);
    }
    (out, count)
}

pub fn replace_line_in_text(
    matcher: &Matcher,
    text: &str,
    line_no: u32,
    replacement: &str,
) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    for (i, seg) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if (i + 1) as u32 == line_no {
            let replaced = if matcher.literal_replace {
                matcher.re.replace_all(seg, regex::NoExpand(replacement))
            } else {
                matcher.re.replace_all(seg, replacement)
            };
            if replaced != seg {
                changed = true;
            }
            out.push_str(&replaced);
        } else {
            out.push_str(seg);
        }
    }
    changed.then_some(out)
}

pub fn replace_one_in_text(
    matcher: &Matcher,
    text: &str,
    line_no: u32,
    start: usize,
    replacement: &str,
) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut changed = false;
    for (i, seg) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if (i + 1) as u32 == line_no
            && let Some(caps) = matcher
                .re
                .captures_iter(seg)
                .find(|c| c.get(0).map(|m| m.start()) == Some(start))
        {
            let m = caps.get(0).unwrap();
            let rep = if matcher.literal_replace {
                replacement.to_string()
            } else {
                let mut s = String::new();
                caps.expand(replacement, &mut s);
                s
            };
            out.push_str(&seg[..m.start()]);
            out.push_str(&rep);
            out.push_str(&seg[m.end()..]);
            changed = true;
        } else {
            out.push_str(seg);
        }
    }
    changed.then_some(out)
}

fn readable_text(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

pub fn search_repo(root: &Path, matcher: &Matcher, filter: &SearchFilter) -> Vec<FileHit> {
    search_repo_filtered(root, matcher, filter).unwrap_or_default()
}

pub fn search_repo_filtered(
    root: &Path,
    matcher: &Matcher,
    filter: &SearchFilter,
) -> Result<Vec<FileHit>, String> {
    let mut walk = WalkBuilder::new(root);
    if !filter.is_empty() {
        walk.overrides(filter.build(root)?);
    }
    let mut out = Vec::new();
    for entry in walk.build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_FILE_BYTES {
            continue;
        }
        let path = entry.path();
        let Some(text) = readable_text(path) else {
            continue;
        };
        let lines = search_text(matcher, &text);
        if !lines.is_empty() {
            let rel = path.strip_prefix(root).unwrap_or(path);
            out.push(FileHit {
                path: rel.to_string_lossy().replace('\\', "/"),
                lines,
            });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_search_and_replace() {
        let m = Matcher::new("foo", false, true).unwrap();
        let hits = search_text(&m, "foo bar\nno match\nfoo foo");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line_no, 1);
        assert_eq!(hits[1].ranges.len(), 2);

        let (new, n) = replace_all_in_text(&m, "foo bar\nfoo", "X");
        assert_eq!(new, "X bar\nX");
        assert_eq!(n, 2);
    }

    #[test]
    fn case_insensitive() {
        let m = Matcher::new("foo", false, false).unwrap();
        let hits = search_text(&m, "FOO Foo foo");
        assert_eq!(hits[0].ranges.len(), 3);
    }

    #[test]
    fn regex_capture_replace() {
        let m = Matcher::new(r"(\w+)=(\d+)", true, true).unwrap();
        let (new, n) = replace_all_in_text(&m, "a=1 b=22", "$2:$1");
        assert_eq!(new, "1:a 22:b");
        assert_eq!(n, 2);
    }

    #[test]
    fn literal_replace_keeps_dollar() {
        let m = Matcher::new("x", false, true).unwrap();
        let (new, _) = replace_all_in_text(&m, "x", "$1");
        assert_eq!(new, "$1");
    }

    #[test]
    fn replace_one_targets_single_occurrence() {
        let m = Matcher::new("a", false, true).unwrap();
        let text = "a a a";
        let new = replace_one_in_text(&m, text, 1, 2, "Z").unwrap();
        assert_eq!(new, "a Z a");
    }

    #[test]
    fn empty_pattern_errors() {
        assert!(Matcher::new("", false, true).is_err());
        assert!(Matcher::new("(", true, true).is_err());
    }

    #[test]
    fn filter_parse_splits_and_trims() {
        let f = SearchFilter::parse(" a/**, *.rs ,, b ", "**/target/**");
        assert_eq!(f.include, vec!["a/**", "*.rs", "b"]);
        assert_eq!(f.exclude, vec!["**/target/**"]);
        assert!(!f.is_empty());
        assert!(SearchFilter::parse("  ", " , ").is_empty());
    }

    #[test]
    fn filter_builds_override() {
        let f = SearchFilter::parse("*.rs", "target/**");
        let ov = f.build(Path::new(".")).unwrap();
        assert!(ov.matched("main.rs", false).is_whitelist());
        assert!(ov.matched("target/x", false).is_ignore());
    }

    #[test]
    fn preserves_trailing_newline() {
        let m = Matcher::new("a", false, true).unwrap();
        let (new, _) = replace_all_in_text(&m, "a\n", "b");
        assert_eq!(new, "b\n");
    }
}
