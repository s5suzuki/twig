use crate::color::Rgb;

pub type Span = (usize, usize, Rgb);

#[cfg(feature = "syntax-highlight")]
pub use imp::DiffHighlighter;

#[cfg(not(feature = "syntax-highlight"))]
pub use noop::DiffHighlighter;

#[cfg(not(feature = "syntax-highlight"))]
mod noop {
    use super::Span;
    use crate::repo::DiffRow;

    #[derive(Default)]
    pub struct DiffHighlighter;

    impl DiffHighlighter {
        pub fn new(_path: &str, _rows: &[DiffRow], _dark: bool) -> Self {
            Self
        }
        pub fn ensure_upto(&mut self, _rows: &[DiffRow], _upto: usize) {}
        pub fn left(&self, _i: usize) -> &[Span] {
            &[]
        }
        pub fn right(&self, _i: usize) -> &[Span] {
            &[]
        }
    }
}

#[cfg(feature = "syntax-highlight")]
mod imp {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::OnceLock;

    use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, Style, Theme, ThemeSet};
    use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

    use super::Span;
    use crate::color::Rgb;
    use crate::repo::{DiffRow, LineKind};

    const MAX_ROWS: usize = 8000;

    struct Assets {
        ss: SyntaxSet,
        dark: Theme,
        light: Theme,
    }

    fn assets() -> &'static Assets {
        static A: OnceLock<Assets> = OnceLock::new();
        A.get_or_init(|| {
            let ts = ThemeSet::load_defaults();
            Assets {
                ss: two_face::syntax::extra_newlines(),
                dark: ts.themes["base16-ocean.dark"].clone(),
                light: ts.themes["InspiredGitHub"].clone(),
            }
        })
    }

    fn syntax_for<'a>(ss: &'a SyntaxSet, path: &str) -> Option<&'a SyntaxReference> {
        let p = Path::new(path);
        if let Some(ext) = p.extension().and_then(|e| e.to_str())
            && let Some(s) = ss.find_syntax_by_extension(ext)
        {
            return Some(s);
        }
        let name = p.file_name().and_then(|n| n.to_str())?;
        ss.find_syntax_by_extension(name)
    }

    fn header_path(header: &str) -> &str {
        header.split("  ").next().unwrap_or(header).trim()
    }

    fn to_spans(ranges: &[(Style, &str)], text_len: usize) -> Vec<Span> {
        let mut spans = Vec::with_capacity(ranges.len());
        let mut pos = 0usize;
        for (style, piece) in ranges {
            let start = pos;
            pos += piece.len();
            if start >= text_len {
                break;
            }
            let end = pos.min(text_len);
            if end > start {
                let c = style.foreground;
                spans.push((start, end, Rgb::new(c.r, c.g, c.b)));
            }
        }
        spans
    }

    struct Stream {
        parse: ParseState,
        state: HighlightState,
    }

    struct Engine {
        ss: &'static SyntaxSet,
        hl: Highlighter<'static>,
        stream: Option<Stream>,
    }

    impl Engine {
        fn set_syntax(&mut self, syn: Option<&'static SyntaxReference>) {
            self.stream = syn.map(|s| Stream {
                parse: ParseState::new(s),
                state: HighlightState::new(&self.hl, ScopeStack::new()),
            });
        }

        // Highlight `text`, advancing the shared parser state (right side / current file).
        fn advance(&mut self, text: &str) -> Vec<Span> {
            let Some(st) = self.stream.as_mut() else {
                return Vec::new();
            };
            let mut line = text.to_owned();
            line.push('\n');
            let Ok(ops) = st.parse.parse_line(&line, self.ss) else {
                return Vec::new();
            };
            let ranges: Vec<(Style, &str)> =
                HighlightIterator::new(&mut st.state, &ops, &line, &self.hl).collect();
            to_spans(&ranges, text.len())
        }

        // Highlight `text` from a snapshot of the current state without advancing it
        // (left side of removed / changed lines — old content).
        fn snapshot(&self, text: &str) -> Vec<Span> {
            let Some(st) = self.stream.as_ref() else {
                return Vec::new();
            };
            let mut parse = st.parse.clone();
            let mut state = st.state.clone();
            let mut line = text.to_owned();
            line.push('\n');
            let Ok(ops) = parse.parse_line(&line, self.ss) else {
                return Vec::new();
            };
            let ranges: Vec<(Style, &str)> =
                HighlightIterator::new(&mut state, &ops, &line, &self.hl).collect();
            to_spans(&ranges, text.len())
        }
    }

    #[derive(Default)]
    pub struct DiffHighlighter {
        left: HashMap<usize, Vec<Span>>,
        right: HashMap<usize, Vec<Span>>,
        engine: Option<Engine>,
        next: usize,
    }

    impl DiffHighlighter {
        pub fn new(path: &str, rows: &[DiffRow], dark: bool) -> Self {
            if rows.len() > MAX_ROWS {
                return Self::default();
            }
            let a = assets();
            let initial = syntax_for(&a.ss, path);
            if initial.is_none() && !rows.iter().any(|r| matches!(r, DiffRow::FileHeader(_))) {
                return Self::default();
            }
            let theme = if dark { &a.dark } else { &a.light };
            let mut engine = Engine {
                ss: &a.ss,
                hl: Highlighter::new(theme),
                stream: None,
            };
            engine.set_syntax(initial);
            Self {
                engine: Some(engine),
                ..Self::default()
            }
        }

        pub fn ensure_upto(&mut self, rows: &[DiffRow], upto: usize) {
            if rows.is_empty() {
                return;
            }
            let Some(engine) = self.engine.as_mut() else {
                return;
            };
            let end = upto.min(rows.len() - 1);
            while self.next <= end {
                let i = self.next;
                match &rows[i] {
                    DiffRow::FileHeader(header) => {
                        let syn = syntax_for(engine.ss, header_path(header));
                        engine.set_syntax(syn);
                    }
                    DiffRow::Hunk { .. } => {}
                    DiffRow::Line {
                        left, right, kind, ..
                    } => match kind {
                        LineKind::Context => {
                            if let Some(t) = right.as_deref().or(left.as_deref()) {
                                let spans = engine.advance(t);
                                if !spans.is_empty() {
                                    self.left.insert(i, spans.clone());
                                    self.right.insert(i, spans);
                                }
                            }
                        }
                        LineKind::Added => {
                            if let Some(t) = right.as_deref() {
                                let spans = engine.advance(t);
                                if !spans.is_empty() {
                                    self.right.insert(i, spans);
                                }
                            }
                        }
                        LineKind::Removed => {
                            if let Some(t) = left.as_deref() {
                                let spans = engine.snapshot(t);
                                if !spans.is_empty() {
                                    self.left.insert(i, spans);
                                }
                            }
                        }
                        LineKind::Changed => {
                            if let Some(t) = left.as_deref() {
                                let spans = engine.snapshot(t);
                                if !spans.is_empty() {
                                    self.left.insert(i, spans);
                                }
                            }
                            if let Some(t) = right.as_deref() {
                                let spans = engine.advance(t);
                                if !spans.is_empty() {
                                    self.right.insert(i, spans);
                                }
                            }
                        }
                    },
                }
                self.next += 1;
            }
        }

        pub fn left(&self, i: usize) -> &[Span] {
            self.left.get(&i).map(Vec::as_slice).unwrap_or(&[])
        }

        pub fn right(&self, i: usize) -> &[Span] {
            self.right.get(&i).map(Vec::as_slice).unwrap_or(&[])
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn line(text: &str, kind: LineKind) -> DiffRow {
            let (left, right) = match kind {
                LineKind::Added => (None, Some(text.to_string())),
                LineKind::Removed => (Some(text.to_string()), None),
                _ => (Some(text.to_string()), Some(text.to_string())),
            };
            DiffRow::Line {
                old_no: Some(1),
                new_no: Some(1),
                left,
                right,
                kind,
                left_emph: Vec::new(),
                right_emph: Vec::new(),
            }
        }

        #[test]
        fn empty_diff_is_noop() {
            let mut h = DiffHighlighter::new("foo.rs", &[], true);
            h.ensure_upto(&[], 100);
            assert!(h.left(0).is_empty() && h.right(0).is_empty());
        }

        #[test]
        fn lazy_only_covers_requested_rows() {
            let rows: Vec<DiffRow> = (0..50)
                .map(|_| line("let x = 1;", LineKind::Context))
                .collect();
            let mut h = DiffHighlighter::new("a.rs", &rows, true);
            h.ensure_upto(&rows, 3);
            // requested rows are highlighted...
            assert!(!h.right(0).is_empty(), "row 0 should be colored");
            assert!(!h.right(3).is_empty(), "row 3 should be colored");
            // ...rows beyond the watermark are untouched until requested.
            assert!(h.right(10).is_empty(), "row 10 not requested yet");
            h.ensure_upto(&rows, 10);
            assert!(!h.right(10).is_empty(), "row 10 colored after request");
        }

        #[test]
        fn context_lines_share_left_and_right_spans() {
            let rows = vec![line("fn main() {}", LineKind::Context)];
            let mut h = DiffHighlighter::new("a.rs", &rows, true);
            h.ensure_upto(&rows, 0);
            assert_eq!(
                h.left(0),
                h.right(0),
                "context line must share spans on both sides"
            );
            assert!(!h.right(0).is_empty());
        }
    }
}
