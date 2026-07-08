use std::fs;
use std::path::PathBuf;

use twit_core::search::{Matcher, SearchFilter, search_repo};

fn fixture(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("twit_search_filter_{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("sub")).unwrap();
    for (p, body) in [
        ("a.rs", "needle here\n"),
        ("b.txt", "needle here\n"),
        ("sub/c.rs", "needle here\n"),
        ("sub/skip.log", "needle here\n"),
    ] {
        fs::write(dir.join(p), body).unwrap();
    }
    dir
}

fn paths(hits: &[twit_core::search::FileHit]) -> Vec<String> {
    let mut v: Vec<String> = hits.iter().map(|h| h.path.clone()).collect();
    v.sort();
    v
}

#[test]
fn include_whitelist_excludes_others() {
    let dir = fixture("include");
    let m = Matcher::new("needle", false, true).unwrap();
    let f = SearchFilter::parse("**/*.rs", "");
    let hits = search_repo(&dir, &m, &f);
    assert_eq!(paths(&hits), vec!["a.rs", "sub/c.rs"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn exclude_only_drops_matching() {
    let dir = fixture("exclude");
    let m = Matcher::new("needle", false, true).unwrap();
    let f = SearchFilter::parse("", "*.log, *.txt");
    let hits = search_repo(&dir, &m, &f);
    assert_eq!(paths(&hits), vec!["a.rs", "sub/c.rs"]);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn empty_filter_walks_all() {
    let dir = fixture("empty");
    let m = Matcher::new("needle", false, true).unwrap();
    let hits = search_repo(&dir, &m, &SearchFilter::default());
    assert_eq!(
        paths(&hits),
        vec!["a.rs", "b.txt", "sub/c.rs", "sub/skip.log"]
    );
    let _ = fs::remove_dir_all(&dir);
}
