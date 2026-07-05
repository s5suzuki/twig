use crate::repo::{DiffRow, LineKind};

#[derive(Clone, Copy, Default)]
pub struct DiffNavState {
    pub cursor: usize,
    pub anchor: Option<usize>,
}

pub fn last_row(rows: &[DiffRow]) -> usize {
    rows.len().saturating_sub(1)
}

pub fn is_line_row(rows: &[DiffRow], i: usize) -> bool {
    matches!(rows.get(i), Some(DiffRow::Line { .. }))
}

pub fn step_line_row(rows: &[DiffRow], from: usize, delta: isize) -> usize {
    let last = last_row(rows);
    let step = if delta >= 0 { 1 } else { -1 };
    let mut i = from as isize;
    loop {
        let ni = i + step;
        if ni < 0 || ni > last as isize {
            return from;
        }
        i = ni;
        if is_line_row(rows, i as usize) {
            return i as usize;
        }
    }
}

pub fn hunk_starts(rows: &[DiffRow]) -> Vec<usize> {
    let has_header = rows.iter().any(|r| matches!(r, DiffRow::Hunk { .. }));
    let mut starts = Vec::new();
    let is_change = |k: &LineKind| *k != LineKind::Context;
    if has_header {
        let mut awaiting = false;
        for (i, r) in rows.iter().enumerate() {
            match r {
                DiffRow::Hunk { .. } => awaiting = true,
                DiffRow::Line { kind, .. } if awaiting && is_change(kind) => {
                    starts.push(i);
                    awaiting = false;
                }
                _ => {}
            }
        }
    } else {
        let mut in_run = false;
        for (i, r) in rows.iter().enumerate() {
            match r {
                DiffRow::Line { kind, .. } if is_change(kind) => {
                    if !in_run {
                        starts.push(i);
                        in_run = true;
                    }
                }
                _ => in_run = false,
            }
        }
    }
    starts
}

impl DiffNavState {
    pub fn reset(&mut self) {
        self.cursor = 0;
        self.anchor = None;
    }

    pub fn clamp(&mut self, rows: &[DiffRow]) {
        let last = last_row(rows);
        if self.cursor > last {
            self.cursor = last;
        }
        if let Some(a) = self.anchor
            && a > last
        {
            self.anchor = Some(last);
        }

        if !rows.is_empty() && !is_line_row(rows, self.cursor) {
            let fwd = (self.cursor..=last).find(|&i| is_line_row(rows, i));
            let back = (0..self.cursor).rev().find(|&i| is_line_row(rows, i));
            if let Some(i) = fwd.or(back) {
                self.cursor = i;
            }
        }
    }

    pub fn set_cursor(&mut self, rows: &[DiffRow], row: usize) {
        self.cursor = row.min(last_row(rows));
    }

    pub fn step(&mut self, rows: &[DiffRow], delta: isize) {
        let cur = self.cursor.min(last_row(rows));
        self.cursor = step_line_row(rows, cur, delta);
    }

    pub fn jump_hunk(&mut self, rows: &[DiffRow], forward: bool) -> bool {
        let starts = hunk_starts(rows);
        if starts.is_empty() {
            return false;
        }
        let cur = self.cursor.min(last_row(rows));
        let target = if forward {
            starts.iter().copied().find(|&s| s > cur)
        } else {
            starts.iter().rev().copied().find(|&s| s < cur)
        };
        match target {
            Some(t) => {
                self.cursor = t;
                true
            }
            None => false,
        }
    }

    pub fn first_hunk(&mut self, rows: &[DiffRow]) -> bool {
        match hunk_starts(rows).first() {
            Some(&first) => {
                self.cursor = first;
                true
            }
            None => false,
        }
    }

    pub fn scroll(&mut self, rows: &[DiffRow], visible_rows: usize, fraction: f32, down: bool) {
        let steps = ((visible_rows as f32 * fraction).round() as isize).max(1);
        let mut cur = self.cursor.min(last_row(rows));
        let dir = if down { 1 } else { -1 };
        for _ in 0..steps {
            let next = step_line_row(rows, cur, dir);
            if next == cur {
                break;
            }
            cur = next;
        }
        self.cursor = cur;
    }

    pub fn toggle_visual(&mut self) {
        self.anchor = if self.anchor.is_some() {
            None
        } else {
            Some(self.cursor)
        };
    }

    pub fn highlight(&self, rows: &[DiffRow]) -> Option<(usize, usize)> {
        self.anchor.map(|a| {
            let last = last_row(rows);
            let a = a.min(last);
            let c = self.cursor.min(last);
            (a.min(c), a.max(c))
        })
    }

    pub fn action_range(&self, rows: &[DiffRow]) -> Option<(usize, usize)> {
        if rows.is_empty() {
            return None;
        }
        let last = last_row(rows);
        let c = self.cursor.min(last);
        Some(match self.anchor {
            Some(a) => {
                let a = a.min(last);
                (a.min(c), a.max(c))
            }
            None => (c, c),
        })
    }

    pub fn selection_text(&self, rows: &[DiffRow]) -> Option<String> {
        let (lo, hi) = self.action_range(rows)?;
        let mut out = String::new();
        for row in rows[lo..=hi].iter() {
            let DiffRow::Line { left, right, .. } = row else {
                continue;
            };
            let text = right.as_deref().or(left.as_deref()).unwrap_or("");
            out.push_str(text);
            out.push('\n');
        }
        if out.is_empty() { None } else { Some(out) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: LineKind, left: Option<&str>, right: Option<&str>) -> DiffRow {
        DiffRow::Line {
            old_no: left.map(|_| 1),
            new_no: right.map(|_| 1),
            left: left.map(str::to_string),
            right: right.map(str::to_string),
            kind,
            left_emph: Vec::new(),
            right_emph: Vec::new(),
        }
    }

    fn sample() -> Vec<DiffRow> {
        vec![
            DiffRow::Hunk {
                index: 0,
                header: "@@ -1,3 +1,3 @@".to_string(),
            },
            line(LineKind::Context, Some("a"), Some("a")),
            line(LineKind::Removed, Some("old"), None),
            line(LineKind::Added, None, Some("new")),
            line(LineKind::Context, Some("b"), Some("b")),
            DiffRow::Hunk {
                index: 1,
                header: "@@ -9,2 +9,2 @@".to_string(),
            },
            line(LineKind::Changed, Some("x1"), Some("x2")),
            line(LineKind::Context, Some("c"), Some("c")),
        ]
    }

    #[test]
    fn step_skips_non_line_rows() {
        let rows = sample();
        assert_eq!(step_line_row(&rows, 4, 1), 6);
        assert_eq!(step_line_row(&rows, 6, -1), 4);
        assert_eq!(step_line_row(&rows, 7, 1), 7);
    }

    #[test]
    fn clamp_moves_cursor_onto_a_line_row() {
        let rows = sample();
        let mut nav = DiffNavState {
            cursor: 99,
            anchor: Some(99),
        };
        nav.clamp(&rows);
        assert_eq!(nav.cursor, 7);
        assert_eq!(nav.anchor, Some(7));

        let mut nav = DiffNavState {
            cursor: 5,
            anchor: None,
        };
        nav.clamp(&rows);
        assert_eq!(nav.cursor, 6);
    }

    #[test]
    fn hunk_starts_follow_headers() {
        let rows = sample();
        assert_eq!(hunk_starts(&rows), vec![2, 6]);
    }

    #[test]
    fn jump_hunk_moves_between_starts() {
        let rows = sample();
        let mut nav = DiffNavState::default();
        assert!(nav.first_hunk(&rows));
        assert_eq!(nav.cursor, 2);
        assert!(nav.jump_hunk(&rows, true));
        assert_eq!(nav.cursor, 6);
        assert!(!nav.jump_hunk(&rows, true));
        assert!(nav.jump_hunk(&rows, false));
        assert_eq!(nav.cursor, 2);
    }

    #[test]
    fn selection_text_prefers_right_and_falls_back_to_left() {
        let rows = sample();
        let nav = DiffNavState {
            cursor: 3,
            anchor: Some(1),
        };
        assert_eq!(nav.selection_text(&rows), Some("a\nold\nnew\n".to_string()));

        let single = DiffNavState {
            cursor: 6,
            anchor: None,
        };
        assert_eq!(single.selection_text(&rows), Some("x2\n".to_string()));
    }

    #[test]
    fn action_range_covers_anchor_to_cursor() {
        let rows = sample();
        let nav = DiffNavState {
            cursor: 2,
            anchor: Some(6),
        };
        assert_eq!(nav.action_range(&rows), Some((2, 6)));
        assert_eq!(nav.highlight(&rows), Some((2, 6)));
        assert_eq!(nav.action_range(&[]), None);
    }
}
