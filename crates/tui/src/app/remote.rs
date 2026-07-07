use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemoteKind {
    Fetch,
    Pull,
    Push,
    ForcePush,
    DeleteRemote,
    SubmoduleInit,
    SubmoduleUpdate,
}

impl RemoteKind {
    pub fn verb(self) -> &'static str {
        match self {
            RemoteKind::Fetch => "fetch",
            RemoteKind::Pull => "pull",
            RemoteKind::Push => "push",
            RemoteKind::ForcePush => "force push",
            RemoteKind::DeleteRemote => "delete remote branch",
            RemoteKind::SubmoduleInit => "submodule init",
            RemoteKind::SubmoduleUpdate => "submodule update",
        }
    }

    pub fn running(self) -> &'static str {
        match self {
            RemoteKind::Fetch => "Fetching",
            RemoteKind::Pull => "Pulling",
            RemoteKind::Push | RemoteKind::ForcePush => "Pushing",
            RemoteKind::DeleteRemote => "Deleting remote branch",
            RemoteKind::SubmoduleInit => "Initializing submodule",
            RemoteKind::SubmoduleUpdate => "Updating submodule",
        }
    }
}

pub(super) enum RemoteMsg {
    Progress(usize, usize),
    Done(Result<repo::SeqOutcome, String>),
}

pub struct RemoteJob {
    pub kind: RemoteKind,
    pub progress: Option<(usize, usize)>,
    rx: Receiver<RemoteMsg>,
}

impl TuiApp {
    pub(super) fn push(&mut self, force: bool) {
        let remote = repo::primary_remote(&self.selected);
        let Some(refspec) = repo::head_push_refspec(&self.selected) else {
            self.error = Some("not on a branch to push".to_string());
            return;
        };
        if force {
            let Some(remote) = remote else {
                self.error = Some("no remote configured".to_string());
                return;
            };
            self.prompt = Some((Prompt::ConfirmForcePush { remote, refspec }, String::new()));
            return;
        }
        self.start_remote(RemoteKind::Push, remote, vec![refspec]);
    }

    pub(super) fn submodule_prompt(&mut self, row: &SidebarRow, update: bool) {
        let Some(parent) = row.parent.clone() else {
            self.error = Some("not a submodule".to_string());
            return;
        };
        if update && !row.initialized {
            self.error = Some("initialize the submodule first".to_string());
            return;
        }
        if !update && row.initialized {
            self.error = Some("submodule is already initialized".to_string());
            return;
        }
        let kind = if update {
            RemoteKind::SubmoduleUpdate
        } else {
            RemoteKind::SubmoduleInit
        };
        self.prompt = Some((
            Prompt::ConfirmSubmodule {
                kind,
                parent,
                name: row.name.clone(),
            },
            String::new(),
        ));
    }

    pub(super) fn start_remote_submodule(
        &mut self,
        kind: RemoteKind,
        parent: PathBuf,
        name: String,
    ) {
        if self.remote.is_some() {
            self.error = Some("a remote operation is already running".to_string());
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ptx = tx.clone();
            let progress = move |received, total| {
                let _ = ptx.send(RemoteMsg::Progress(received, total));
            };
            let result = match kind {
                RemoteKind::SubmoduleUpdate => repo::submodule_update(&parent, &name, progress),
                _ => repo::submodule_init(&parent, &name, progress),
            }
            .map(|()| repo::SeqOutcome::Done);
            let _ = tx.send(RemoteMsg::Done(result.map_err(|e| e.to_string())));
        });
        self.error = None;
        self.remote = Some(RemoteJob {
            kind,
            progress: None,
            rx,
        });
    }

    pub(super) fn rediscover(&mut self) {
        let root_path = self.root.path.clone();
        match repo::discover(&root_path) {
            Ok(node) => self.root = node,
            Err(e) => self.error = Some(format!("rediscover failed: {e}")),
        }
    }

    pub(super) fn start_remote(
        &mut self,
        kind: RemoteKind,
        remote: Option<String>,
        refspecs: Vec<String>,
    ) {
        if self.remote.is_some() {
            self.error = Some("a remote operation is already running".to_string());
            return;
        }
        let Some(remote) = remote else {
            self.error = Some("no remote configured".to_string());
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let path = self.selected.clone();
        let refspecs = if kind == RemoteKind::ForcePush {
            refspecs
                .into_iter()
                .map(|r| {
                    if r.starts_with('+') {
                        r
                    } else {
                        format!("+{r}")
                    }
                })
                .collect()
        } else {
            refspecs
        };
        std::thread::spawn(move || {
            let ptx = tx.clone();
            let progress = move |received, total| {
                let _ = ptx.send(RemoteMsg::Progress(received, total));
            };
            let result = match kind {
                RemoteKind::Fetch => {
                    repo::fetch(&path, &remote, progress).map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::Pull => repo::pull(&path, &remote, progress),
                RemoteKind::Push | RemoteKind::ForcePush => {
                    repo::push(&path, &remote, &refspecs, progress).map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::DeleteRemote => {
                    let branch = refspecs.first().cloned().unwrap_or_default();
                    repo::delete_remote_branch(&path, &remote, &branch, progress)
                        .map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::SubmoduleInit | RemoteKind::SubmoduleUpdate => Err(
                    twit_core::git2::Error::from_str("not a plain remote operation"),
                ),
            };
            let _ = tx.send(RemoteMsg::Done(result.map_err(|e| e.to_string())));
        });
        self.error = None;
        self.remote = Some(RemoteJob {
            kind,
            progress: None,
            rx,
        });
    }

    pub fn poll_remote(&mut self) -> bool {
        let Some(job) = self.remote.as_mut() else {
            return false;
        };
        let mut dirty = false;
        let mut done = None;
        loop {
            match job.rx.try_recv() {
                Ok(RemoteMsg::Progress(received, total)) => {
                    job.progress = Some((received, total));
                    dirty = true;
                }
                Ok(RemoteMsg::Done(res)) => {
                    done = Some(res);
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    done = Some(Err("remote worker exited unexpectedly".to_string()));
                    break;
                }
            }
        }
        if let Some(res) = done {
            let kind = self
                .remote
                .take()
                .map(|j| j.kind)
                .unwrap_or(RemoteKind::Fetch);
            match res {
                Ok(_) => {
                    self.error = None;
                    if matches!(
                        kind,
                        RemoteKind::SubmoduleInit | RemoteKind::SubmoduleUpdate
                    ) {
                        self.rediscover();
                    }
                }
                Err(e) => self.error = Some(format!("{} failed: {e}", kind.verb())),
            }
            self.refresh();
            dirty = true;
        }
        dirty
    }
}
