//! The per-project config file claude-docker.sh reads, and checks on its entries.

use std::{collections::BTreeMap, env, fs, io, net::IpAddr, path::{Path, PathBuf}};

use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Entry {
    #[serde(default)]
    pub(crate) mounts: Vec<String>,
    #[serde(default)]
    pub(crate) hosts: Vec<String>,
}

impl Entry {
    pub(crate) fn list(&self, i: usize) -> &Vec<String> {
        [&self.mounts, &self.hosts][i]
    }
    pub(crate) fn list_mut(&mut self, i: usize) -> &mut Vec<String> {
        if i == 0 { &mut self.mounts } else { &mut self.hosts }
    }
}

// ponytail: BTreeMap, so saving sorts projects by key and drops fields this
// tool doesn't know. Switch to serde_json preserve_order if that ever matters.
pub(crate) type Config = BTreeMap<String, Entry>;

// Must match CONFIG_FILE in claude-docker.sh.
pub(crate) fn config_path() -> PathBuf {
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("claude-code-docker/config.json")
}

pub(crate) fn load(path: &Path) -> Result<Config, String> {
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub(crate) fn save(path: &Path, cfg: &Config) -> io::Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(cfg)? + "\n")?;
    fs::rename(tmp, path)
}

/// `$VAR` / `${VAR}` like envsubst (unset = empty). Only for checks and display:
/// the config keeps the unexpanded text and claude-docker.sh expands it.
pub(crate) fn expand(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let (name, tail) = match after.strip_prefix('{') {
            Some(b) => match b.find('}') {
                Some(j) => (&b[..j], &b[j + 1..]),
                None => ("", after),
            },
            None => {
                let j = after
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(after.len());
                (&after[..j], &after[j..])
            }
        };
        if name.is_empty() {
            out.push('$');
        } else {
            out.push_str(&env::var(name).unwrap_or_default());
        }
        rest = tail;
    }
    out + rest
}

pub(crate) fn validate(list: usize, s: &str) -> Result<(), &'static str> {
    let s = expand(s);
    let ok = if list == 0 {
        let mut parts = s.splitn(3, ':');
        let (host, container) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        !host.is_empty() && container.starts_with('/')
    } else {
        // Like docker --add-host: split on the first colon, so IPv6 works.
        matches!(s.split_once(':'), Some((name, ip))
            if !name.is_empty() && (ip == "host-gateway" || ip.parse::<IpAddr>().is_ok()))
    };
    if ok {
        Ok(())
    } else if list == 0 {
        Err("expected host:/container[:ro]")
    } else {
        Err("expected hostname:ip")
    }
}

/// claude-docker.sh skips mounts whose host path doesn't exist.
pub(crate) fn mount_missing(s: &str) -> bool {
    !Path::new(expand(s).split(':').next().unwrap_or("")).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks() {
        // SAFETY: single test, no other threads read the env.
        unsafe { env::set_var("CDC_T", "/x") };
        assert_eq!(expand("$CDC_T/a:${CDC_T}b:$:${CDC_UNSET}z"), "/x/a:/xb:$:z");
        assert!(validate(0, "$CDC_T/a:/a:ro").is_ok());
        assert!(validate(0, "/a:rel").is_err());
        assert!(validate(0, "/a").is_err());
        assert!(validate(1, "pi2:192.168.1.50").is_ok());
        assert!(validate(1, "v6:::1").is_ok());
        assert!(validate(1, "gw:host-gateway").is_ok());
        assert!(validate(1, "pi2:nope").is_err());
        assert!(validate(1, ":1.2.3.4").is_err());
    }
}
