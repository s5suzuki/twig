use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RemoteKind {
    Fetch,
    Pull,
    Push,
    DeleteRemote,
    SubmoduleInit,
    SubmoduleUpdate,
}

impl RemoteKind {
    fn verb(self) -> &'static str {
        match self {
            RemoteKind::Fetch => "Fetch",
            RemoteKind::Pull => "Pull",
            RemoteKind::Push => "Push",
            RemoteKind::DeleteRemote => "Delete remote branch",
            RemoteKind::SubmoduleInit => "Initialize submodule",
            RemoteKind::SubmoduleUpdate => "Update submodule",
        }
    }

    fn rediscovers(self) -> bool {
        matches!(
            self,
            RemoteKind::SubmoduleInit | RemoteKind::SubmoduleUpdate
        )
    }
}

pub(super) enum RemoteMsg {
    Progress { received: usize, total: usize },
    Done(Result<repo::SeqOutcome, String>),
}

pub(super) fn collect_expanded(node: &repo::RepoNode, out: &mut Vec<PathBuf>) {
    if node.expanded {
        out.push(node.path.clone());
    }
    for child in &node.children {
        collect_expanded(child, out);
    }
}

pub(super) fn set_node_expanded(node: &mut repo::RepoNode, path: &Path) -> bool {
    if node.path == path {
        node.expanded = true;
        return true;
    }
    node.children.iter_mut().any(|c| set_node_expanded(c, path))
}

pub(super) fn node_is_initialized(node: &repo::RepoNode, path: &Path) -> bool {
    if node.path == path {
        return node.initialized;
    }
    node.children.iter().any(|c| node_is_initialized(c, path))
}

impl App {
    pub fn fetch(&mut self, ctx: &egui::Context) {
        let remote = repo::primary_remote(&self.selected);
        self.start_remote(ctx, RemoteKind::Fetch, remote, Vec::new());
    }

    pub fn pull(&mut self, ctx: &egui::Context) {
        if self.busy_with_seq() {
            return;
        }
        let remote = repo::primary_remote(&self.selected);
        self.start_remote(ctx, RemoteKind::Pull, remote, Vec::new());
    }

    pub fn request_force_push(&mut self) {
        if repo::head_push_refspec(&self.selected).is_none() {
            self.error = Some("Not on a branch to push".to_string());
            return;
        }
        self.confirm_force_push = true;
    }

    pub fn push(&mut self, ctx: &egui::Context, force: bool) {
        let remote = repo::primary_remote(&self.selected);
        let Some(mut refspec) = repo::head_push_refspec(&self.selected) else {
            self.error = Some("Not on a branch to push".to_string());
            return;
        };
        if force {
            refspec.insert(0, '+');
        }
        self.start_remote(ctx, RemoteKind::Push, remote, vec![refspec]);
    }

    pub fn delete_remote_branch(&mut self, ctx: &egui::Context, remote_ref: String) {
        let Some((remote, branch)) = remote_ref.split_once('/') else {
            self.error = Some(format!("Invalid remote branch: {remote_ref}"));
            return;
        };
        self.start_remote(
            ctx,
            RemoteKind::DeleteRemote,
            Some(remote.to_string()),
            vec![branch.to_string()],
        );
    }

    pub(super) fn start_remote(
        &mut self,
        ctx: &egui::Context,
        kind: RemoteKind,
        remote: Option<String>,
        refspecs: Vec<String>,
    ) {
        if self.remote_busy {
            self.error = Some("A remote operation is already running".to_string());
            return;
        }
        let Some(remote) = remote else {
            self.error = Some("No remote configured".to_string());
            return;
        };

        let (tx, rx) = mpsc::channel();
        let path = self.selected.clone();
        let ctx = ctx.clone();
        let gate = self.repaint_gate.clone();

        self.remote_busy = true;
        self.remote_kind = kind;
        self.remote_progress = None;
        self.remote_task = Some(rx);
        self.error = None;

        thread::spawn(move || {
            let progress = |received, total| {
                let _ = tx.send(RemoteMsg::Progress { received, total });
                if gate.load(Ordering::Relaxed) {
                    ctx.request_repaint();
                }
            };
            let result = match kind {
                RemoteKind::Fetch => {
                    repo::fetch(&path, &remote, progress).map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::Push => {
                    repo::push(&path, &remote, &refspecs, progress).map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::Pull => repo::pull(&path, &remote, progress),
                RemoteKind::DeleteRemote => {
                    let branch = refspecs.first().cloned().unwrap_or_default();
                    repo::delete_remote_branch(&path, &remote, &branch, progress)
                        .map(|()| repo::SeqOutcome::Done)
                }
                RemoteKind::SubmoduleInit | RemoteKind::SubmoduleUpdate => {
                    Err(git2::Error::from_str("not a remote operation"))
                }
            };
            let _ = tx.send(RemoteMsg::Done(result.map_err(|e| e.to_string())));
            if gate.load(Ordering::Relaxed) {
                ctx.request_repaint();
            }
        });
    }

    pub fn submodule_init(&mut self, ctx: &egui::Context, parent: PathBuf, name: String) {
        self.start_submodule(ctx, RemoteKind::SubmoduleInit, parent, name);
    }

    pub fn submodule_update(&mut self, ctx: &egui::Context, parent: PathBuf, name: String) {
        self.start_submodule(ctx, RemoteKind::SubmoduleUpdate, parent, name);
    }

    pub(super) fn start_submodule(
        &mut self,
        ctx: &egui::Context,
        kind: RemoteKind,
        parent: PathBuf,
        name: String,
    ) {
        if self.remote_busy {
            self.error = Some("A remote operation is already running".to_string());
            return;
        }

        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let gate = self.repaint_gate.clone();

        self.remote_busy = true;
        self.remote_kind = kind;
        self.remote_progress = None;
        self.remote_task = Some(rx);
        self.error = None;

        thread::spawn(move || {
            let progress = |received, total| {
                let _ = tx.send(RemoteMsg::Progress { received, total });
                if gate.load(Ordering::Relaxed) {
                    ctx.request_repaint();
                }
            };
            let result = match kind {
                RemoteKind::SubmoduleUpdate => repo::submodule_update(&parent, &name, progress),
                _ => repo::submodule_init(&parent, &name, progress),
            }
            .map(|()| repo::SeqOutcome::Done);
            let _ = tx.send(RemoteMsg::Done(result.map_err(|e| e.to_string())));
            if gate.load(Ordering::Relaxed) {
                ctx.request_repaint();
            }
        });
    }

    pub fn poll_remote(&mut self) {
        let Some(rx) = &self.remote_task else {
            return;
        };
        let mut done = None;
        loop {
            match rx.try_recv() {
                Ok(RemoteMsg::Progress { received, total }) => {
                    self.remote_progress = Some((received, total));
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
            self.finish_remote(res);
        }
    }

    pub(super) fn finish_remote(&mut self, res: Result<repo::SeqOutcome, String>) {
        self.remote_task = None;
        self.remote_busy = false;
        self.remote_progress = None;
        let rediscover = self.remote_kind.rediscovers();
        if rediscover && res.is_ok() {
            self.rediscover();
        }
        self.after_commit_topology_change();
        match res {
            Ok(_) => self.error = None,
            Err(e) => self.error = Some(format!("{} failed: {e}", self.remote_kind.verb())),
        }
    }

    pub(super) fn rediscover(&mut self) {
        let mut expanded = Vec::new();
        if let Some(root) = &self.root {
            collect_expanded(root, &mut expanded);
        }
        match repo::discover(&self.watch_root) {
            Ok(mut node) => {
                for path in &expanded {
                    set_node_expanded(&mut node, path);
                }
                self.root = Some(node);
                if !self.selected_is_valid() {
                    self.select_repo(self.watch_root.clone());
                }
            }
            Err(e) => self.error = Some(format!("Cannot reload repositories: {e}")),
        }
    }

    pub(super) fn selected_is_valid(&self) -> bool {
        self.root
            .as_ref()
            .map(|r| node_is_initialized(r, &self.selected))
            .unwrap_or(false)
    }
}
