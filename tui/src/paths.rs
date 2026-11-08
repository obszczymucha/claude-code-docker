//! Turning a path argument into a project key.

use std::{
    env,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{config::expand, die};

/// Lexical: drops `.`, resolves `..`, strips trailing slashes. No symlinks.
pub(crate) fn normalize(p: &Path) -> String {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out.to_string_lossy().into_owned()
}

/// $PWD (keeps symlinks, like the shell) when it's still accurate.
fn cwd() -> PathBuf {
    let real = env::current_dir().unwrap_or_else(|e| die(&format!("cwd: {e}")));
    match env::var_os("PWD").map(PathBuf::from) {
        Some(p) if p.is_absolute() && p.canonicalize().ok() == real.canonicalize().ok() => p,
        _ => real,
    }
}

fn git(dir: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Same rule as project_key in claude-docker.sh.
fn detect_key(dir: &str) -> String {
    let is_true = |args: &[&str]| git(dir, args).as_deref() == Some("true");
    if (is_true(&["rev-parse", "--is-bare-repository"])
        || is_true(&["config", "--get", "core.bare"]))
        && let Some(d) = git(
            dir,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
    {
        return d;
    }
    git(dir, &["rev-parse", "--show-toplevel"]).unwrap_or_else(|| dir.to_string())
}

/// "." is detected (unless forced); anything else is taken as given, made absolute.
pub(crate) fn resolve_key(arg: &str, force: bool) -> String {
    let home = env::var("HOME").unwrap_or_default();
    let arg_expanded = match arg.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{home}{rest}"),
        _ => expand(arg),
    };
    let path = normalize(&cwd().join(arg_expanded));
    if arg == "." && !force {
        detect_key(&path)
    } else {
        path
    }
}

/// `~` for $HOME, for display and fuzzy matching.
pub(crate) fn tilde(k: &str) -> String {
    let home = env::var("HOME").unwrap_or_default();
    match k.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            format!("~{rest}")
        }
        _ => k.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks() {
        assert_eq!(normalize(Path::new("/a/./b/../c/")), "/a/c");
        assert_eq!(normalize(Path::new("/..")), "/");
    }
}
