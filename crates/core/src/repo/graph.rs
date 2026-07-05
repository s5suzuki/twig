use std::collections::{HashMap, HashSet};
use std::path::Path;

use git2::{Oid, ReferenceType, Repository, Sort, StatusOptions};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
    DetachedHead,
    Stash,
}

pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,

    pub is_head: bool,
}

pub struct GraphRow {
    pub id: Oid,
    pub short_id: String,
    pub summary: String,
    pub author: String,
    pub date: String,
    pub node_col: usize,
    pub node_color: usize,
    pub segments: Vec<Segment>,
    pub refs: Vec<RefLabel>,

    pub is_head: bool,
    pub is_uncommitted: bool,
}

#[derive(Clone, Copy)]
pub enum Segment {
    Through { col: usize, color: usize },
    TopToNode { col: usize, color: usize },
    NodeToBottom { col: usize, color: usize },
}

pub struct Graph {
    pub rows: Vec<GraphRow>,
    pub max_col: usize,
}

struct Lane {
    oid: Oid,
    color: usize,
}

pub fn build(repo_path: &Path, limit: usize) -> Result<Graph, git2::Error> {
    let mut repo = Repository::open(repo_path)?;

    let mut labels = collect_refs(&repo);
    let head_oid = repo
        .head()
        .ok()
        .and_then(|h| h.peel_to_commit().ok())
        .map(|c| c.id());

    let mut stash_commits: HashSet<Oid> = HashSet::new();
    let mut exclude: HashSet<Oid> = HashSet::new();
    for (oid, index, _msg) in collect_stashes(&mut repo) {
        stash_commits.insert(oid);
        if let Ok(c) = repo.find_commit(oid) {
            for p in c.parent_ids().skip(1) {
                exclude.insert(p);
            }
        }
        labels.entry(oid).or_default().push(RefLabel {
            name: format!("stash@{{{index}}}"),
            kind: RefKind::Stash,
            is_head: false,
        });
    }

    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;

    let mut pushed = false;
    for oid in labels.keys() {
        if walk.push(*oid).is_ok() {
            pushed = true;
        }
    }
    if !pushed && walk.push_head().is_err() {
        return Ok(Graph {
            rows: Vec::new(),
            max_col: 0,
        });
    }

    let mut rows = Vec::new();
    let mut active: Vec<Option<Lane>> = Vec::new();
    let mut next_color = 0usize;
    let mut max_col = 0usize;

    if let Some(head) = head_oid {
        let n = uncommitted_count(&repo);
        if n > 0 {
            let color = next_color;
            next_color += 1;
            ensure_len(&mut active, 1);
            active[0] = Some(Lane { oid: head, color });
            rows.push(GraphRow {
                id: Oid::ZERO_SHA1,
                short_id: String::new(),
                summary: format!("Uncommitted Changes ({n})"),
                author: String::new(),
                date: String::new(),
                node_col: 0,
                node_color: color,
                segments: vec![Segment::NodeToBottom { col: 0, color }],
                refs: Vec::new(),
                is_head: false,
                is_uncommitted: true,
            });
        }
    }

    for oid in walk.take(limit) {
        let oid = oid?;

        if exclude.contains(&oid) {
            continue;
        }
        let commit = repo.find_commit(oid)?;
        let parents: Vec<Oid> = if stash_commits.contains(&oid) {
            commit.parent_ids().take(1).collect()
        } else {
            commit.parent_ids().collect()
        };

        let incoming: Vec<usize> = active
            .iter()
            .enumerate()
            .filter_map(|(i, l)| match l {
                Some(l) if l.oid == oid => Some(i),
                _ => None,
            })
            .collect();

        let (node_col, node_color) = match incoming.first() {
            Some(&first) => (first, active[first].as_ref().unwrap().color),
            None => {
                let col = first_free(&active);
                ensure_len(&mut active, col + 1);
                let color = next_color;
                next_color += 1;
                (col, color)
            }
        };

        let mut segments = Vec::new();
        for (i, lane) in active.iter().enumerate() {
            let Some(lane) = lane else { continue };
            if i == node_col {
                continue;
            }
            if incoming.contains(&i) {
                segments.push(Segment::TopToNode {
                    col: i,
                    color: lane.color,
                });
            } else {
                segments.push(Segment::Through {
                    col: i,
                    color: lane.color,
                });
            }
        }
        if incoming.contains(&node_col) {
            segments.push(Segment::TopToNode {
                col: node_col,
                color: node_color,
            });
        }

        for &i in &incoming {
            active[i] = None;
        }
        if let Some((&p0, rest)) = parents.split_first() {
            active[node_col] = Some(Lane {
                oid: p0,
                color: node_color,
            });
            segments.push(Segment::NodeToBottom {
                col: node_col,
                color: node_color,
            });
            for &pk in rest {
                if let Some(existing) = active
                    .iter()
                    .position(|l| matches!(l, Some(l) if l.oid == pk))
                {
                    let color = active[existing].as_ref().unwrap().color;
                    segments.push(Segment::NodeToBottom {
                        col: existing,
                        color,
                    });
                } else {
                    let col = first_free(&active);
                    ensure_len(&mut active, col + 1);
                    let color = next_color;
                    next_color += 1;
                    active[col] = Some(Lane { oid: pk, color });
                    segments.push(Segment::NodeToBottom { col, color });
                }
            }
        } else {
            active[node_col] = None;
        }

        max_col = max_col.max(node_col).max(active.len().saturating_sub(1));

        let id = oid.to_string();
        let mut refs = labels.remove(&oid).unwrap_or_default();
        sort_refs(&mut refs);
        rows.push(GraphRow {
            id: oid,
            short_id: id[..7.min(id.len())].to_string(),
            summary: commit.summary().ok().flatten().unwrap_or("").to_string(),
            author: commit.author().name().unwrap_or("").to_string(),
            date: fmt_date(commit.time()),
            node_col,
            node_color,
            segments,
            refs,
            is_head: head_oid == Some(oid),
            is_uncommitted: false,
        });
    }

    Ok(Graph { rows, max_col })
}

fn uncommitted_count(repo: &Repository) -> usize {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(true);
    repo.statuses(Some(&mut opts)).map(|s| s.len()).unwrap_or(0)
}

fn collect_refs(repo: &Repository) -> HashMap<Oid, Vec<RefLabel>> {
    let mut map: HashMap<Oid, Vec<RefLabel>> = HashMap::new();

    let detached = repo.head_detached().unwrap_or(false);
    let head_ref_name = repo
        .head()
        .ok()
        .and_then(|h| h.name().ok().map(String::from));

    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            if r.kind() != Some(ReferenceType::Direct) {
                continue;
            }
            let kind = if r.is_branch() {
                RefKind::LocalBranch
            } else if r.is_remote() {
                RefKind::RemoteBranch
            } else if r.is_tag() {
                RefKind::Tag
            } else {
                continue;
            };
            let Ok(oid) = r.peel_to_commit().map(|c| c.id()) else {
                continue;
            };
            let name = r.shorthand().unwrap_or("?").to_string();
            let is_head = !detached
                && kind == RefKind::LocalBranch
                && head_ref_name.as_deref() == r.name().ok();
            map.entry(oid).or_default().push(RefLabel {
                name,
                kind,
                is_head,
            });
        }
    }

    if detached
        && let Some(oid) = repo
            .head()
            .ok()
            .and_then(|h| h.peel_to_commit().ok())
            .map(|c| c.id())
    {
        map.entry(oid).or_default().push(RefLabel {
            name: "HEAD".to_string(),
            kind: RefKind::DetachedHead,
            is_head: true,
        });
    }

    map
}

fn collect_stashes(repo: &mut Repository) -> Vec<(Oid, usize, String)> {
    let mut out = Vec::new();
    let _ = repo.stash_foreach(|index, message, oid| {
        out.push((*oid, index, message.to_string()));
        true
    });
    out
}

fn sort_refs(refs: &mut [RefLabel]) {
    fn rank(r: &RefLabel) -> u8 {
        if r.is_head {
            return 0;
        }
        match r.kind {
            RefKind::DetachedHead => 0,
            RefKind::LocalBranch => 1,
            RefKind::RemoteBranch => 2,
            RefKind::Tag => 3,
            RefKind::Stash => 4,
        }
    }
    refs.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.name.cmp(&b.name)));
}

fn fmt_date(t: git2::Time) -> String {
    let secs = t.seconds() + (t.offset_minutes() as i64) * 60;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (y + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(18_993), (2022, 1, 1));
        assert_eq!(civil_from_days(20_634), (2026, 6, 30));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }
}

fn first_free(active: &[Option<Lane>]) -> usize {
    active
        .iter()
        .position(|l| l.is_none())
        .unwrap_or(active.len())
}

fn ensure_len(active: &mut Vec<Option<Lane>>, len: usize) {
    if active.len() < len {
        active.resize_with(len, || None);
    }
}
