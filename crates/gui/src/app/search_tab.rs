use super::*;

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub replace: String,
    pub include: String,
    pub exclude: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub error: Option<String>,
    pub results: Vec<search::FileHit>,
    pub selected: HashSet<(String, u32)>,
    pub folded_dirs: HashSet<String>,
    pub folded_files: HashSet<String>,
    pub searched: bool,
    pub focus_request: bool,
}

impl SearchState {
    pub fn selected_count(&self) -> usize {
        self.results
            .iter()
            .flat_map(|f| f.lines.iter().map(move |l| (f.path.clone(), l.line_no)))
            .filter(|k| self.selected.contains(k))
            .count()
    }
}

impl App {
    pub fn search_run(&mut self) {
        self.search.error = None;
        self.search.results.clear();
        self.search.selected.clear();
        self.search.searched = true;
        let matcher = match search::Matcher::new(
            &self.search.query,
            self.search.regex,
            self.search.case_sensitive,
        ) {
            Ok(m) => m,
            Err(e) => {
                self.search.error = Some(e);
                return;
            }
        };
        let filter = search::SearchFilter::parse(&self.search.include, &self.search.exclude);
        let hits = match search::search_repo_filtered(&self.selected, &matcher, &filter) {
            Ok(h) => h,
            Err(e) => {
                self.search.error = Some(e);
                return;
            }
        };
        for f in &hits {
            for l in &f.lines {
                self.search.selected.insert((f.path.clone(), l.line_no));
            }
        }
        self.search.results = hits;
    }

    pub fn search_apply(&mut self) {
        self.search_confirm = false;
        let matcher = match search::Matcher::new(
            &self.search.query,
            self.search.regex,
            self.search.case_sensitive,
        ) {
            Ok(m) => m,
            Err(e) => {
                self.search.error = Some(e);
                return;
            }
        };
        let replacement = self.search.replace.clone();
        let mut errs = Vec::new();
        for f in &self.search.results {
            let lines: Vec<u32> = f
                .lines
                .iter()
                .map(|l| l.line_no)
                .filter(|ln| self.search.selected.contains(&(f.path.clone(), *ln)))
                .collect();
            if lines.is_empty() {
                continue;
            }
            let abs = self.selected.join(&f.path);
            let Ok(mut text) = std::fs::read_to_string(&abs) else {
                continue;
            };
            for ln in lines {
                if let Some(new) = search::replace_line_in_text(&matcher, &text, ln, &replacement) {
                    text = new;
                }
            }
            if let Err(e) = std::fs::write(&abs, text) {
                errs.push(format!("{}: {e}", f.path));
            }
        }
        if !errs.is_empty() {
            self.error = Some(errs.join("; "));
        }
        self.search_run();
        self.reload();
    }

    pub fn search_toggle_line(&mut self, path: &str, line_no: u32) {
        let key = (path.to_string(), line_no);
        if !self.search.selected.remove(&key) {
            self.search.selected.insert(key);
        }
    }

    pub fn search_file_all_selected(&self, f: &search::FileHit) -> bool {
        f.lines
            .iter()
            .all(|l| self.search.selected.contains(&(f.path.clone(), l.line_no)))
    }

    pub fn search_toggle_file(&mut self, idx: usize) {
        let Some(f) = self.search.results.get(idx) else {
            return;
        };
        let all = self.search_file_all_selected(f);
        let keys: Vec<(String, u32)> = f
            .lines
            .iter()
            .map(|l| (f.path.clone(), l.line_no))
            .collect();
        for k in keys {
            if all {
                self.search.selected.remove(&k);
            } else {
                self.search.selected.insert(k);
            }
        }
    }

    pub(super) fn search_dir_keys(&self, prefix: &str) -> Vec<(String, u32)> {
        let pfx = format!("{prefix}/");
        self.search
            .results
            .iter()
            .filter(|f| f.path.starts_with(&pfx))
            .flat_map(|f| f.lines.iter().map(move |l| (f.path.clone(), l.line_no)))
            .collect()
    }

    pub fn search_dir_all_selected(&self, prefix: &str) -> bool {
        let keys = self.search_dir_keys(prefix);
        !keys.is_empty() && keys.iter().all(|k| self.search.selected.contains(k))
    }

    pub fn search_toggle_dir(&mut self, prefix: &str) {
        let all = self.search_dir_all_selected(prefix);
        for k in self.search_dir_keys(prefix) {
            if all {
                self.search.selected.remove(&k);
            } else {
                self.search.selected.insert(k);
            }
        }
    }

    pub fn search_select_all(&mut self, select: bool) {
        self.search.selected.clear();
        if select {
            for f in &self.search.results {
                for l in &f.lines {
                    self.search.selected.insert((f.path.clone(), l.line_no));
                }
            }
        }
    }
}
