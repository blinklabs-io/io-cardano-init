//! Small git helpers for the CLI edge — the only place the tool shells out to
//! `git`. Used by the update commands' clean-tree gate and by init to give a new
//! project a repo + initial commit (so `add`/`remove` work immediately).

use std::path::Path;
use std::process::{Command, Output};

/// Run `git -C <root> <args>`, returning `None` if `git` couldn't be executed.
fn git(root: &Path, args: &[&str]) -> Option<Output> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()
}

/// Whether `git` is available on the host at all.
fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Whether `root` is already inside a git work tree (so we must not nest a repo).
fn inside_repo(root: &Path) -> bool {
    match git(root, &["rev-parse", "--is-inside-work-tree"]) {
        Some(out) => out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "true",
        None => false,
    }
}

/// Whether the git working tree at `root` is clean (no changes, no untracked
/// files). A missing `git`, a non-repo, or any error counts as **not** clean —
/// so the update commands require `--force` rather than proceeding blindly.
pub fn is_clean(root: &Path) -> bool {
    match git(root, &["status", "--porcelain"]) {
        Some(out) => out.status.success() && out.stdout.is_empty(),
        None => false,
    }
}

/// Result of trying to set up a repo for a freshly scaffolded project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitOutcome {
    /// `git init` + `add -A` + initial commit all succeeded.
    Committed,
    /// Initialized and staged, but the commit failed (typically no
    /// `user.name`/`user.email` configured).
    InitializedNoCommit,
    /// Skipped — `git` is not installed.
    GitMissing,
    /// Skipped — already inside an existing repository (don't nest one).
    AlreadyRepo,
}

impl InitOutcome {
    /// Stable slug for the JSON envelope.
    pub fn as_str(&self) -> &'static str {
        match self {
            InitOutcome::Committed => "committed",
            InitOutcome::InitializedNoCommit => "initialized",
            InitOutcome::GitMissing => "git_missing",
            InitOutcome::AlreadyRepo => "already_repo",
        }
    }
}

/// Best-effort: initialize a repo and make an initial commit for the new project
/// at `root`. Never fails scaffolding — every problem degrades to a lesser
/// [`InitOutcome`]. Skips cleanly when `git` is absent or `root` is already
/// inside a repo.
pub fn init_project_repo(root: &Path) -> InitOutcome {
    if !git_available() {
        return InitOutcome::GitMissing;
    }
    if inside_repo(root) {
        return InitOutcome::AlreadyRepo;
    }
    if git(root, &["init", "--quiet"]).map(|o| o.status.success()) != Some(true) {
        return InitOutcome::GitMissing;
    }
    let _ = git(root, &["add", "-A"]);
    let committed = git(
        root,
        &[
            "commit",
            "--quiet",
            "-m",
            "chore: scaffold project with cardano-init",
        ],
    )
    .map(|o| o.status.success())
    .unwrap_or(false);
    if committed {
        InitOutcome::Committed
    } else {
        InitOutcome::InitializedNoCommit
    }
}
