//! Edits claude-docker.sh's per-project extra mounts/hosts config.
//!   claude-docker-config                  open the TUI
//!   claude-docker-config add <path>       add an entry for <path>
//!   claude-docker-config add .            add the current repo (bare repo dir / repo root / $PWD)
//!   claude-docker-config add . --force    add $PWD even inside a repo

use std::{
    collections::BTreeMap,
    env, fs, io,
    net::IpAddr,
    path::{Component, Path, PathBuf},
    process::{exit, Command, Stdio},
};

use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    DefaultTerminal, Frame,
};
use serde::{Deserialize, Serialize};

const USAGE: &str = "usage: claude-docker-config [add <path> [--force]]";
// Panes: 0 = projects, then one per list.
const LISTS: [&str; 2] = ["mounts", "hosts"];

#[derive(Default, Serialize, Deserialize)]
struct Entry {
    #[serde(default)]
    mounts: Vec<String>,
    #[serde(default)]
    hosts: Vec<String>,
}

impl Entry {
    fn list(&self, i: usize) -> &Vec<String> {
        [&self.mounts, &self.hosts][i]
    }
    fn list_mut(&mut self, i: usize) -> &mut Vec<String> {
        if i == 0 { &mut self.mounts } else { &mut self.hosts }
    }
}

// ponytail: BTreeMap, so saving sorts projects by key and drops fields this
// tool doesn't know. Switch to serde_json preserve_order if that ever matters.
type Config = BTreeMap<String, Entry>;

// Must match CONFIG_FILE in claude-docker.sh.
fn config_path() -> PathBuf {
    let base = env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("claude-code-docker/config.json")
}

fn load(path: &Path) -> Result<Config, String> {
    match fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn save(path: &Path, cfg: &Config) -> io::Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(cfg)? + "\n")?;
    fs::rename(tmp, path)
}

/// `$VAR` / `${VAR}` like envsubst (unset = empty). Only for checks and display:
/// the config keeps the unexpanded text and claude-docker.sh expands it.
fn expand(s: &str) -> String {
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

fn validate(list: usize, s: &str) -> Result<(), &'static str> {
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
fn mount_missing(s: &str) -> bool {
    !Path::new(expand(s).split(':').next().unwrap_or("")).exists()
}

/// Lexical: drops `.`, resolves `..`, strips trailing slashes. No symlinks.
fn normalize(p: &Path) -> String {
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
    let out = Command::new("git").args(args).current_dir(dir).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Same rule as project_key in claude-docker.sh.
fn detect_key(dir: &str) -> String {
    let is_true = |args: &[&str]| git(dir, args).as_deref() == Some("true");
    if is_true(&["rev-parse", "--is-bare-repository"]) || is_true(&["config", "--get", "core.bare"]) {
        if let Some(d) = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"]) {
            return d;
        }
    }
    git(dir, &["rev-parse", "--show-toplevel"]).unwrap_or_else(|| dir.to_string())
}

/// "." is detected (unless forced); anything else is taken as given, made absolute.
fn resolve_key(arg: &str, force: bool) -> String {
    let home = env::var("HOME").unwrap_or_default();
    let arg_expanded = match arg.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{home}{rest}"),
        _ => expand(arg),
    };
    let path = normalize(&cwd().join(arg_expanded));
    if arg == "." && !force { detect_key(&path) } else { path }
}

/// `~` for $HOME, for display and fuzzy matching.
fn tilde(k: &str) -> String {
    let home = env::var("HOME").unwrap_or_default();
    match k.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => format!("~{rest}"),
        _ => k.to_string(),
    }
}

/// fzf-style: every space-separated term must match as a subsequence; smart
/// case. Returns a score (higher is better) and the matched char indices.
// ponytail: greedy leftmost match, can miss the best-scoring alignment.
// Swap in nucleo-matcher if the ranking ever feels off.
fn fuzzy(query: &str, text: &str) -> Option<(i64, Vec<usize>)> {
    let fold = !query.chars().any(char::is_uppercase);
    let norm = |c: char| if fold { c.to_ascii_lowercase() } else { c };
    let chars: Vec<char> = text.chars().collect();
    let (mut score, mut hits) = (0, vec![]);
    for term in query.split_whitespace() {
        let mut from = 0;
        for q in term.chars().map(norm) {
            let j = (from..chars.len()).find(|&j| norm(chars[j]) == q)?;
            score += 1;
            if j > 0 && hits.last() == Some(&(j - 1)) {
                score += 5; // consecutive
            }
            if j == 0 || matches!(chars[j - 1], '/' | '-' | '_' | '.' | ' ') {
                score += 3; // start of a word
            }
            hits.push(j);
            from = j + 1;
        }
    }
    hits.sort_unstable();
    hits.dedup();
    Some((score, hits))
}

fn die(msg: &str) -> ! {
    eprintln!("claude-docker-config: {msg}");
    exit(1)
}

enum Mode {
    Normal,
    Input { prompt: &'static str, buf: String, edit: bool },
    ConfirmDelete,
}

struct App {
    path: PathBuf,
    cfg: Config,
    focus: usize,
    sel: [usize; 3],
    mode: Mode,
    status: String,
    filter: String,
}

impl App {
    /// Projects matching the filter, best first, with the matched char
    /// indices in their `tilde` form.
    fn visible(&self) -> Vec<(&String, Vec<usize>)> {
        if self.filter.trim().is_empty() {
            return self.cfg.keys().map(|k| (k, vec![])).collect();
        }
        let mut v: Vec<_> = self
            .cfg
            .keys()
            .filter_map(|k| fuzzy(&self.filter, &tilde(k)).map(|(s, hits)| (s, k, hits)))
            .collect();
        v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
        v.into_iter().map(|(_, k, hits)| (k, hits)).collect()
    }

    fn key(&self) -> Option<String> {
        self.visible().get(self.sel[0]).map(|(k, _)| (*k).clone())
    }

    fn len(&self, pane: usize) -> usize {
        match pane {
            0 => self.visible().len(),
            p => self.key().map_or(0, |k| self.cfg[&k].list(p - 1).len()),
        }
    }

    /// Clears the filter so the key is surely visible, then selects it.
    fn select_key(&mut self, key: &str) {
        self.filter.clear();
        self.sel[0] = self.cfg.keys().position(|k| k == key).unwrap_or(0);
    }

    fn edit_filter(&mut self, change: impl FnOnce(&mut String)) {
        change(&mut self.filter);
        self.focus = 0;
        self.sel = [0; 3];
    }

    fn save(&mut self) {
        if let Err(e) = save(&self.path, &self.cfg) {
            self.status = format!("save failed: {e}");
        }
    }

    fn run(&mut self, t: &mut DefaultTerminal) -> io::Result<()> {
        loop {
            t.draw(|f| self.draw(f))?;
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press && self.handle(k.code, k.modifiers) {
                    return Ok(());
                }
                for p in 0..3 {
                    self.sel[p] = self.sel[p].min(self.len(p).saturating_sub(1));
                }
            }
        }
    }

    /// Returns true to quit.
    fn handle(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            return true;
        }
        match &mut self.mode {
            Mode::Input { buf, .. } => match code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => self.submit(),
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char('u') if mods.contains(KeyModifiers::CONTROL) => buf.clear(),
                KeyCode::Char(c) => buf.push(c),
                _ => {}
            },
            Mode::ConfirmDelete => {
                if code == KeyCode::Char('y') {
                    if let Some(k) = self.key() {
                        self.cfg.remove(&k);
                        self.save();
                    }
                }
                self.mode = Mode::Normal;
            }
            Mode::Normal => {
                self.status.clear();
                return self.normal(code, mods);
            }
        }
        false
    }

    /// Plain typing goes to the filter; commands are on Alt.
    fn normal(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        let f = self.focus;
        let alt = mods.contains(KeyModifiers::ALT);
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        match code {
            KeyCode::Esc if !self.filter.is_empty() => self.edit_filter(String::clear),
            KeyCode::Esc => return true,
            KeyCode::Char('q') if alt => return true,
            KeyCode::Tab | KeyCode::Right => self.focus = (f + 1) % 3,
            KeyCode::Char('l') if alt => self.focus = (f + 1) % 3,
            KeyCode::BackTab | KeyCode::Left => self.focus = (f + 2) % 3,
            KeyCode::Char('h') if alt => self.focus = (f + 2) % 3,
            KeyCode::Enter if f == 0 => self.focus = 1,
            KeyCode::Down => self.move_sel(1),
            KeyCode::Char('j') if alt => self.move_sel(1),
            KeyCode::Up => self.move_sel(-1),
            KeyCode::Char('k') if alt => self.move_sel(-1),
            KeyCode::Backspace => self.edit_filter(|s| {
                s.pop();
            }),
            KeyCode::Char('u') if ctrl => self.edit_filter(String::clear),
            KeyCode::Char(c) if !alt && !ctrl => self.edit_filter(|s| s.push(c)),
            KeyCode::Char('a') | KeyCode::Char('e') if alt => {
                let edit = code == KeyCode::Char('e');
                if f > 0 && self.key().is_none() {
                    self.status = "add a project first".into();
                } else if edit && self.len(f) == 0 {
                    // nothing to edit
                } else {
                    let buf = if !edit {
                        String::new()
                    } else if f == 0 {
                        self.key().unwrap()
                    } else {
                        self.cfg[&self.key().unwrap()].list(f - 1)[self.sel[f]].clone()
                    };
                    let prompt = match f {
                        0 => "project path (. = current repo)",
                        1 => "mount host:/container[:ro]",
                        _ => "host hostname:ip",
                    };
                    self.mode = Mode::Input { prompt, buf, edit };
                }
            }
            KeyCode::Char('d') if alt && self.len(f) > 0 => {
                if f == 0 {
                    self.mode = Mode::ConfirmDelete;
                } else {
                    let k = self.key().unwrap();
                    self.cfg.get_mut(&k).unwrap().list_mut(f - 1).remove(self.sel[f]);
                    self.save();
                }
            }
            _ => {}
        }
        false
    }

    fn move_sel(&mut self, d: isize) {
        let (f, n) = (self.focus, self.len(self.focus));
        if n > 0 {
            self.sel[f] = self.sel[f].saturating_add_signed(d).min(n - 1);
        }
        if f == 0 {
            self.sel[1] = 0;
            self.sel[2] = 0;
        }
    }

    fn submit(&mut self) {
        let Mode::Input { buf, edit, .. } = &self.mode else { return };
        let (input, edit) = (buf.trim().to_string(), *edit);
        let f = self.focus;
        if input.is_empty() {
            self.mode = Mode::Normal;
            return;
        }
        if f == 0 {
            let key = resolve_key(&input, false);
            let old = if edit { self.key() } else { None };
            if old.as_deref() == Some(key.as_str()) {
            } else if self.cfg.contains_key(&key) {
                self.status = format!("{key} already exists");
            } else {
                let entry = old.and_then(|o| self.cfg.remove(&o)).unwrap_or_default();
                self.cfg.insert(key.clone(), entry);
                self.save();
            }
            self.select_key(&key);
        } else {
            if let Err(e) = validate(f - 1, &input) {
                self.status = e.into(); // stay in input mode so it can be fixed
                return;
            }
            let (k, i) = (self.key().unwrap(), self.sel[f]);
            let list = self.cfg.get_mut(&k).unwrap().list_mut(f - 1);
            if list.iter().enumerate().any(|(j, s)| *s == input && !(edit && j == i)) {
                self.status = "already in the list".into();
                return;
            }
            if edit {
                list[i] = input;
            } else {
                list.push(input);
                self.sel[f] = list.len() - 1;
            }
            self.save();
        }
        self.mode = Mode::Normal;
    }

    fn draw(&self, f: &mut Frame) {
        let [main, bottom] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
        let [left, right] = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(main);
        let [top, bot] = Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(right);

        let [filter_box, left] = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(left);
        let visible = self.visible();
        let title = format!(" filter {}/{} ", visible.len(), self.cfg.len());
        f.render_widget(Paragraph::new(self.filter.as_str()).block(Block::bordered().title(title)), filter_box);
        if matches!(self.mode, Mode::Normal) {
            let x = filter_box.x + 1 + self.filter.chars().count() as u16;
            f.set_cursor_position((x.min(filter_box.right().saturating_sub(2)), filter_box.y + 1));
        }

        let projects = visible.iter().map(|(k, hits)| {
            let e = &self.cfg[*k];
            let base = if Path::new(k).exists() { Style::new() } else { Style::new().red() };
            let mut spans: Vec<Span> = tilde(k)
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    let style = if hits.contains(&i) { base.yellow().bold() } else { base };
                    Span::styled(c.to_string(), style)
                })
                .collect();
            spans.push(Span::raw(format!("  {}m {}h", e.mounts.len(), e.hosts.len())).dark_gray());
            ListItem::new(Line::from(spans))
        });
        self.list(f, left, "projects", projects.collect(), 0);

        let entry = self.key().map(|k| &self.cfg[&k]);
        for (i, area) in [top, bot].into_iter().enumerate() {
            let items = entry.map_or(vec![], |e| {
                e.list(i)
                    .iter()
                    .map(|s| match i == 0 && mount_missing(s) {
                        true => ListItem::new(Line::from(vec![
                            Span::raw(s.as_str()).red(),
                            Span::raw("  missing, skipped").dark_gray(),
                        ])),
                        false => ListItem::new(s.as_str()),
                    })
                    .collect()
            });
            self.list(f, area, LISTS[i], items, i + 1);
        }

        let line = match &self.mode {
            Mode::Input { prompt, buf, .. } => {
                let head = format!("{prompt}: ");
                let x = bottom.x + (head.chars().count() + buf.chars().count()) as u16;
                f.set_cursor_position((x.min(bottom.right().saturating_sub(1)), bottom.y));
                let mut spans = vec![Span::raw(head).yellow(), Span::raw(buf.as_str())];
                if !self.status.is_empty() {
                    spans.push(Span::raw(format!("   {}", self.status)).red());
                }
                Line::from(spans)
            }
            Mode::ConfirmDelete => Line::from(
                format!("delete {} and all its entries? y/n", self.key().unwrap_or_default()).yellow(),
            ),
            Mode::Normal if !self.status.is_empty() => Line::from(self.status.as_str().red()),
            Mode::Normal => Line::from(
                "type to filter  alt+a add  alt+e edit  alt+d delete  tab/alt+h/l pane  ↑↓/alt+j/k move  esc clear/quit"
                    .dark_gray(),
            ),
        };
        f.render_widget(Paragraph::new(line), bottom);
    }

    fn list(&self, f: &mut Frame, area: Rect, title: &str, items: Vec<ListItem>, pane: usize) {
        let focused = self.focus == pane;
        let n = items.len();
        let border = if focused { Style::new().fg(Color::Yellow) } else { Style::new() };
        let highlight = if focused { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new().bold() };
        let list = List::new(items)
            .block(Block::bordered().title(format!(" {title} ")).border_style(border))
            .highlight_style(highlight);
        let mut state = ListState::default().with_selected((n > 0).then(|| self.sel[pane].min(n - 1)));
        f.render_stateful_widget(list, area, &mut state);
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let force = args.iter().any(|a| a == "--force");
    let rest: Vec<&str> = args.iter().map(String::as_str).filter(|a| *a != "--force").collect();
    let path = config_path();
    let mut cfg = load(&path).unwrap_or_else(|e| die(&e)); // never overwrite a config we can't parse

    match rest.as_slice() {
        [] if !force => {
            let mut app = App {
                path,
                cfg,
                focus: 0,
                sel: [0; 3],
                mode: Mode::Normal,
                status: String::new(),
                filter: String::new(),
            };
            if let Err(e) = ratatui::run(|t| app.run(t)) {
                die(&e.to_string());
            }
        }
        ["add", p] => {
            let key = resolve_key(p, force);
            if cfg.contains_key(&key) {
                println!("already exists: {key}");
            } else {
                cfg.insert(key.clone(), Entry::default());
                save(&path, &cfg).unwrap_or_else(|e| die(&format!("{}: {e}", path.display())));
                println!("added: {key}");
            }
        }
        _ => {
            eprintln!("{USAGE}");
            exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks() {
        // SAFETY: single test, no other threads read the env.
        unsafe { env::set_var("CDC_T", "/x") };
        assert_eq!(expand("$CDC_T/a:${CDC_T}b:$:${CDC_UNSET}z"), "/x/a:/xb:$:z");
        assert_eq!(normalize(Path::new("/a/./b/../c/")), "/a/c");
        assert_eq!(normalize(Path::new("/..")), "/");
        assert!(validate(0, "$CDC_T/a:/a:ro").is_ok());
        assert!(validate(0, "/a:rel").is_err());
        assert!(validate(0, "/a").is_err());
        assert!(validate(1, "pi2:192.168.1.50").is_ok());
        assert!(validate(1, "v6:::1").is_ok());
        assert!(validate(1, "gw:host-gateway").is_ok());
        assert!(validate(1, "pi2:nope").is_err());
        assert!(validate(1, ":1.2.3.4").is_err());

        assert_eq!(fuzzy("rlf", "~/lua/rollfor.git").unwrap().1, vec![6, 8, 10]);
        assert!(fuzzy("lua roll", "~/lua/rollfor.git").is_some()); // terms in any order
        assert!(fuzzy("roll lua", "~/lua/rollfor.git").is_some());
        assert!(fuzzy("xyz", "~/lua/rollfor.git").is_none());
        assert!(fuzzy("Lua", "~/lua/rollfor.git").is_none()); // smart case
        let score = |q, t| fuzzy(q, t).unwrap().0;
        assert!(score("roll", "~/rollfor") > score("roll", "~/r-o-l-l")); // consecutive wins
    }
}
