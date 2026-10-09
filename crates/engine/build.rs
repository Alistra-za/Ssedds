//! Embeds the git commit of the source tree as `SDC_GIT_COMMIT` (manifest provenance, spec §3.6): 12 hex digits,
//! with a `-dirty` suffix when tracked files differ from HEAD. Never fails the build: without git, or outside a
//! git checkout, the commit is `unknown`.
//!
//! The dirty flag is evaluated whenever this script runs, which is when HEAD or a branch ref moves (or this file
//! changes), not on every source edit; edits made after the last such run do not toggle it.
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// True when a tracked file differs from HEAD (untracked files are ignored). `false` when git cannot say.
fn dirty(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain", "--untracked-files=no"]).is_some()
}

/// Re-run only when the checked-out commit can change: `HEAD`, the branch refs (`refs/heads` is scanned, so a
/// loose ref appearing or moving is seen), and `packed-refs`. cargo treats a missing watched path as always
/// stale, so only paths that exist are named.
fn watch_git(dir: &Path) {
    let Some(git_dir) = git(dir, &["rev-parse", "--absolute-git-dir"]).map(PathBuf::from) else {
        return;
    };
    // In a linked worktree the refs live in the common directory, not the per-worktree one.
    let common = git(dir, &["rev-parse", "--git-common-dir"])
        .map(|c| dir.join(c))
        .unwrap_or_else(|| git_dir.clone());
    for p in [
        git_dir.join("HEAD"),
        common.join("refs").join("heads"),
        common.join("packed-refs"),
    ] {
        if p.exists() {
            println!("cargo::rerun-if-changed={}", p.display());
        }
    }
}

fn main() {
    let dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    // Without this, an absent .git would make cargo re-run the script on every source change.
    println!("cargo::rerun-if-changed=build.rs");
    let commit = match git(&dir, &["rev-parse", "--short=12", "HEAD"]) {
        Some(c) if dirty(&dir) => format!("{c}-dirty"),
        Some(c) => c,
        None => "unknown".into(),
    };
    watch_git(&dir);
    println!("cargo::rustc-env=SDC_GIT_COMMIT={commit}");
}
