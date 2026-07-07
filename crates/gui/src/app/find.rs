use super::*;

pub struct FindMatch {
    pub row: usize,
    pub line_no: u32,
    pub start: usize,
    pub end: usize,
}

#[derive(Default)]
pub struct FindBar {
    pub open: bool,
    pub query: String,
    pub replace: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub error: Option<String>,
    pub focus_request: bool,
    pub matches: Vec<FindMatch>,
    pub current: usize,
    sig: Option<(String, bool, bool)>,
}

impl FindBar {
    pub fn invalidate(&mut self) {
        self.sig = None;
    }

    pub fn recompute(&mut self, diff: &FileDiff) {
        let sig = (self.query.clone(), self.regex, self.case_sensitive);
        if self.sig.as_ref() == Some(&sig) {
            return;
        }
        self.sig = Some(sig);
        self.matches.clear();
        self.error = None;
        if self.query.is_empty() {
            self.current = 0;
            return;
        }
        let matcher = match search::Matcher::new(&self.query, self.regex, self.case_sensitive) {
            Ok(m) => m,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        for (row, r) in diff.rows.iter().enumerate() {
            if let DiffRow::Line {
                right: Some(text),
                new_no,
                ..
            } = r
            {
                for (start, end) in search::line_ranges(&matcher, text) {
                    self.matches.push(FindMatch {
                        row,
                        line_no: new_no.unwrap_or(0),
                        start,
                        end,
                    });
                }
            }
        }
        if self.current >= self.matches.len() {
            self.current = 0;
        }
    }
}

impl App {
    pub fn open_find(&mut self) {
        if self.selected_file.is_none() {
            return;
        }
        self.active_tab = Tab::Diff;
        self.focus = Pane::RightTab;
        self.find.open = true;
        self.find.focus_request = true;
    }

    pub fn close_find(&mut self) {
        self.find.open = false;
    }

    pub fn toggle_find(&mut self) {
        if self.find.open {
            self.close_find();
        } else {
            self.open_find();
        }
    }

    pub(super) fn scroll_to_find(&mut self) {
        if let Some(m) = self.find.matches.get(self.find.current) {
            self.diff_nav.cursor = m.row.min(self.diff_last_row());
            self.diff_scroll_pending = true;
        }
    }

    pub fn find_next(&mut self) {
        if self.find.matches.is_empty() {
            return;
        }
        self.find.current = (self.find.current + 1) % self.find.matches.len();
        self.scroll_to_find();
    }

    pub fn find_prev(&mut self) {
        if self.find.matches.is_empty() {
            return;
        }
        let n = self.find.matches.len();
        self.find.current = (self.find.current + n - 1) % n;
        self.scroll_to_find();
    }

    pub(super) fn find_matcher(&mut self) -> Option<search::Matcher> {
        match search::Matcher::new(&self.find.query, self.find.regex, self.find.case_sensitive) {
            Ok(m) => Some(m),
            Err(e) => {
                self.find.error = Some(e);
                None
            }
        }
    }

    pub fn find_replace_current(&mut self) {
        let Some((path, staged)) = self.selected_file.clone() else {
            return;
        };
        if staged {
            return;
        }
        let Some(m) = self
            .find
            .matches
            .get(self.find.current)
            .map(|m| (m.line_no, m.start))
        else {
            return;
        };
        let Some(matcher) = self.find_matcher() else {
            return;
        };
        let replacement = self.find.replace.clone();
        let abs = self.selected.join(&path);
        let Ok(text) = std::fs::read_to_string(&abs) else {
            return;
        };
        if let Some(new) = search::replace_one_in_text(&matcher, &text, m.0, m.1, &replacement)
            && std::fs::write(&abs, new).is_ok()
        {
            self.find.invalidate();
            self.after_index_change();
        }
    }

    pub fn find_replace_all(&mut self) {
        let Some((path, staged)) = self.selected_file.clone() else {
            return;
        };
        if staged {
            return;
        }
        let Some(matcher) = self.find_matcher() else {
            return;
        };
        let replacement = self.find.replace.clone();
        let abs = self.selected.join(&path);
        let Ok(text) = std::fs::read_to_string(&abs) else {
            return;
        };
        let (new, n) = search::replace_all_in_text(&matcher, &text, &replacement);
        if n > 0 && new != text && std::fs::write(&abs, new).is_ok() {
            self.find.invalidate();
            self.after_index_change();
        }
    }
}
