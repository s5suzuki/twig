use std::collections::HashMap;

use egui::Color32;

pub type Span = (usize, usize, Color32);

#[derive(Default)]
pub struct DiffHighlight {
    pub left: HashMap<usize, Vec<Span>>,
    pub right: HashMap<usize, Vec<Span>>,
}

#[cfg(not(feature = "syntax-highlight"))]
pub fn highlight_diff(_path: &str, _rows: &[crate::repo::DiffRow], _dark: bool) -> DiffHighlight {
    DiffHighlight::default()
}

#[cfg(feature = "syntax-highlight")]
pub use imp::highlight_diff;

#[cfg(feature = "syntax-highlight")]
mod imp {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::OnceLock;

    use egui::Color32;
    use syntect::easy::HighlightLines;
    use syntect::highlighting::{Style, Theme, ThemeSet};
    use syntect::parsing::{SyntaxReference, SyntaxSet};

    use super::{DiffHighlight, Span};
    use crate::repo::DiffRow;

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
                spans.push((start, end, Color32::from_rgb(c.r, c.g, c.b)));
            }
        }
        spans
    }

    fn highlight_side(
        ss: &SyntaxSet,
        theme: &Theme,
        initial: Option<&SyntaxReference>,
        rows: &[DiffRow],
        left: bool,
        out: &mut HashMap<usize, Vec<Span>>,
    ) {
        let mut hl = initial.map(|s| HighlightLines::new(s, theme));
        for (i, row) in rows.iter().enumerate() {
            match row {
                DiffRow::FileHeader(header) => {
                    hl = syntax_for(ss, header_path(header)).map(|s| HighlightLines::new(s, theme));
                }
                DiffRow::Line {
                    left: l, right: r, ..
                } => {
                    let Some(text) = (if left { l } else { r }) else {
                        continue;
                    };
                    let Some(h) = hl.as_mut() else {
                        continue;
                    };
                    let mut line = text.clone();
                    line.push('\n');
                    let Ok(ranges) = h.highlight_line(&line, ss) else {
                        continue;
                    };
                    let spans = to_spans(&ranges, text.len());
                    if !spans.is_empty() {
                        out.insert(i, spans);
                    }
                }
                DiffRow::Hunk { .. } => {}
            }
        }
    }

    pub fn highlight_diff(path: &str, rows: &[DiffRow], dark: bool) -> DiffHighlight {
        let mut out = DiffHighlight::default();
        if rows.len() > MAX_ROWS {
            return out;
        }
        let a = assets();
        let theme = if dark { &a.dark } else { &a.light };
        let initial = syntax_for(&a.ss, path);
        if initial.is_none() && !rows.iter().any(|r| matches!(r, DiffRow::FileHeader(_))) {
            return out;
        }

        highlight_side(&a.ss, theme, initial, rows, true, &mut out.left);
        highlight_side(&a.ss, theme, initial, rows, false, &mut out.right);
        out
    }
}
