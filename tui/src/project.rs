//! The default TUI: the entries of one project, with a fuzzy directory finder
//! for adding mounts.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table,
        TableState,
    },
};

use crate::{
    dialog,
    config::{Config, mount_missing, save, validate},
    fuzzy::{fuzzy, highlight},
    paths::{resolve_key, tilde},
};

enum Mode {
    Normal,
    /// A popup editing one entry. `idx` is None when adding. The entry is
    /// `prefix` + `buf`: for a mount, `prefix` is the fixed `host:` part, `buf`
    /// the container path without `:ro`, which is kept in `ro`.
    Input { list: usize, idx: Option<usize>, prefix: String, buf: String, ro: bool },
    /// A popup asking before deleting entry `idx` of `list`.
    /// `yes` is the focused button.
    Confirm { list: usize, idx: usize, yes: bool },
    /// `dirs` holds `root` itself first, then the dirs under it.
    Finder { root: String, dirs: Vec<String>, query: String, sel: usize },
}

fn finder(root: String) -> Mode {
    let mut dirs = vec![root.clone()];
    dirs.extend(self::dirs(Path::new(&root)));
    Mode::Finder { root, dirs, query: String::new(), sel: 0 }
}

/// A mount without its `:ro`/`:rw` suffix, and whether it's read-only.
fn split_ro(s: &str) -> (&str, bool) {
    match s.strip_suffix(":ro") {
        Some(base) => (base, true),
        None => (s.strip_suffix(":rw").unwrap_or(s), false),
    }
}

fn with_ro(base: &str, ro: bool) -> String {
    if ro { format!("{base}:ro") } else { base.to_string() }
}

pub(crate) struct Project {
    path: PathBuf,
    cfg: Config,
    key: String,
    sel: usize,
    mode: Mode,
    status: String,
    border: bool,
}

impl Project {
    /// Adds `key` to `cfg` if missing; it's saved with the first change.
    pub(crate) fn new(path: PathBuf, mut cfg: Config, key: String, border: bool) -> Self {
        cfg.entry(key.clone()).or_default();
        Self { path, cfg, key, sel: 0, mode: Mode::Normal, status: String::new(), border }
    }

    fn lists(&self) -> [&Vec<String>; 2] {
        let e = &self.cfg[&self.key];
        [&e.mounts, &e.hosts]
    }

    /// Row -> (list, index): mounts first, then hosts.
    fn at(&self, row: usize) -> Option<(usize, usize)> {
        let m = self.lists()[0].len();
        match row {
            r if r < m => Some((0, r)),
            r if r - m < self.lists()[1].len() => Some((1, r - m)),
            _ => None,
        }
    }

    fn rows(&self) -> usize {
        self.lists().iter().map(|l| l.len()).sum()
    }

    /// Validates `value` and stores it at `idx` (None appends), then saves
    /// and selects it.
    fn put(&mut self, list: usize, idx: Option<usize>, value: String) -> Result<(), String> {
        validate(list, &value)?;
        let l = self.cfg.get_mut(&self.key).unwrap().list_mut(list);
        // Mounts differing only in ro/rw are the same mount.
        let same = |s: &str| if list == 0 { split_ro(s).0 == split_ro(&value).0 } else { s == value };
        if l.iter().enumerate().any(|(j, s)| same(s) && idx != Some(j)) {
            return Err("already in the list".into());
        }
        let i = match idx {
            Some(i) => {
                l[i] = value;
                i
            }
            None => {
                l.push(value);
                l.len() - 1
            }
        };
        self.sel = if list == 0 { i } else { self.lists()[0].len() + i };
        if let Err(e) = save(&self.path, &self.cfg) {
            self.status = format!("save failed: {e}");
        }
        Ok(())
    }

    pub(crate) fn run(&mut self, t: &mut DefaultTerminal) -> io::Result<()> {
        loop {
            t.draw(|f| self.draw(f))?;
            if let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                if self.handle(k.code, k.modifiers) {
                    return Ok(());
                }
                self.sel = self.sel.min(self.rows().saturating_sub(1));
            }
        }
    }

    /// Returns true to quit.
    fn handle(&mut self, code: KeyCode, mods: KeyModifiers) -> bool {
        let ctrl = mods.contains(KeyModifiers::CONTROL);
        let alt = mods.contains(KeyModifiers::ALT);
        if ctrl && code == KeyCode::Char('c') {
            return true;
        }
        self.status.clear();
        match &mut self.mode {
            Mode::Input { buf, .. } => match code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => self.submit(),
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Char('u') if ctrl => buf.clear(),
                KeyCode::Char(c) => buf.push(c),
                _ => {}
            },
            Mode::Finder { root, query, sel, .. } => match code {
                KeyCode::Esc => self.mode = Mode::Normal,
                KeyCode::Enter => {
                    if let Some(d) = self.picked().filter(|d| Path::new(d).is_dir()) {
                        self.mode = finder(d);
                    }
                }
                KeyCode::Tab => {
                    if let Some(d) = self.picked() {
                        match self.put(0, None, format!("{d}:{d}:ro")) {
                            Ok(()) => self.mode = Mode::Normal,
                            Err(e) => self.status = e,
                        }
                    }
                }
                KeyCode::Down => *sel += 1,
                KeyCode::Char('j' | 'n') if ctrl || alt => *sel += 1,
                KeyCode::Up => *sel = sel.saturating_sub(1),
                KeyCode::Char('k' | 'p') if ctrl || alt => *sel = sel.saturating_sub(1),
                KeyCode::Backspace if query.is_empty() => {
                    if let Some(up) = Path::new(root).parent() {
                        self.mode = finder(up.to_string_lossy().into_owned());
                    }
                }
                KeyCode::Backspace => {
                    query.pop();
                    *sel = 0;
                }
                KeyCode::Char('u') if ctrl => {
                    query.clear();
                    *sel = 0;
                }
                KeyCode::Char(c) if !ctrl && !alt => {
                    query.push(c);
                    *sel = 0;
                }
                _ => {}
            },
            Mode::Confirm { list, idx, yes } => {
                let delete = match code {
                    KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                        *yes = !*yes;
                        return false;
                    }
                    KeyCode::Char('h' | 'l') => {
                        *yes = !*yes;
                        return false;
                    }
                    KeyCode::Enter => *yes,
                    KeyCode::Char('y') => true,
                    KeyCode::Char('n') | KeyCode::Esc => false,
                    _ => return false,
                };
                if delete {
                    self.cfg.get_mut(&self.key).unwrap().list_mut(*list).remove(*idx);
                    if let Err(e) = save(&self.path, &self.cfg) {
                        self.status = format!("save failed: {e}");
                    }
                }
                self.mode = Mode::Normal;
            }
            Mode::Normal => return self.normal(code),
        }
        false
    }

    /// Opens the directory finder for adding a mount.
    pub(crate) fn add(&mut self) {
        self.mode = finder(env::var("HOME").unwrap_or_default());
    }

    fn normal(&mut self, code: KeyCode) -> bool {
        let at = self.at(self.sel);
        match code {
            KeyCode::Esc | KeyCode::Char('q') => return true,
            KeyCode::Down | KeyCode::Char('j') => self.sel += 1, // clamped in run
            KeyCode::Up | KeyCode::Char('k') => self.sel = self.sel.saturating_sub(1),
            KeyCode::Char('a') => self.add(),
            KeyCode::Char('h') => {
                let (prefix, buf) = (String::new(), String::new());
                self.mode = Mode::Input { list: 1, idx: None, prefix, buf, ro: false };
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                if let Some((list, i)) = at {
                    let s = &self.lists()[list][i];
                    let ((prefix, buf), ro) = match list {
                        0 => {
                            let (base, ro) = split_ro(s);
                            // validate() guarantees a colon in a stored mount.
                            let colon = base.find(':').map_or(0, |c| c + 1);
                            (base.split_at(colon), ro)
                        }
                        _ => (("", s.as_str()), false),
                    };
                    let (prefix, buf) = (prefix.to_string(), buf.to_string());
                    self.mode = Mode::Input { list, idx: Some(i), prefix, buf, ro };
                }
            }
            KeyCode::Tab => {
                if let Some((0, i)) = at {
                    let (base, ro) = split_ro(&self.lists()[0][i]);
                    let toggled = with_ro(base, !ro);
                    if let Err(e) = self.put(0, Some(i), toggled) {
                        self.status = e;
                    }
                }
            }
            KeyCode::Char('d') => {
                if let Some((list, idx)) = at {
                    self.mode = Mode::Confirm { list, idx, yes: false }; // No first: Enter is safe
                }
            }
            _ => {}
        }
        false
    }

    /// The finder's selected dir, or the typed path when nothing matches and
    /// it looks like one.
    fn picked(&self) -> Option<String> {
        let Mode::Finder { root, dirs, query, sel } = &self.mode else {
            return None;
        };
        let m = matches(dirs, root, query);
        match m.get((*sel).min(m.len().saturating_sub(1))) {
            Some((d, _)) => Some(d.to_string()),
            None if query.starts_with(['/', '~', '.', '$']) => Some(resolve_key(query.trim(), true)),
            None => None,
        }
    }

    fn submit(&mut self) {
        let Mode::Input { list, idx, prefix, buf, ro } = &self.mode else {
            return;
        };
        let (list, idx) = (*list, *idx);
        let input = buf.trim();
        if input.is_empty() {
            self.mode = Mode::Normal;
            return;
        }
        let value = if list == 0 {
            let (base, typed_ro) = split_ro(input);
            with_ro(&format!("{prefix}{base}"), *ro || typed_ro)
        } else {
            format!("{prefix}{input}")
        };
        match self.put(list, idx, value) {
            Ok(()) => self.mode = Mode::Normal,
            Err(e) => self.status = e, // stay in the popup so it can be fixed
        }
    }

    fn draw(&self, f: &mut Frame) {
        let mut area = f.area();
        if self.border {
            let block = Block::bordered().border_type(BorderType::Rounded);
            area = block.inner(area);
            f.render_widget(block, f.area());
        }
        let [head, body, bottom] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(area);

        f.render_widget(Paragraph::new(tilde(&self.key).white().bold()), head);

        let rows: Vec<Row> = (0..self.rows())
            .map(|r| {
                let (list, i) = self.at(r).unwrap();
                let s = self.lists()[list][i].as_str();
                let (entry, mode) = match list {
                    0 => {
                        let (base, ro) = split_ro(s);
                        let entry = if mount_missing(s) {
                            Line::from(vec![
                                Span::raw(base).red(),
                                Span::raw("  missing, skipped").dark_gray(),
                            ])
                        } else {
                            Line::from(base)
                        };
                        (entry, {
                            // "ro" styled exactly like the m label. "rw" is white; the
                            // highlight reverses, so on the selected row it gets the
                            // label's grey background with white text.
                            let style = match (ro, r == self.sel) {
                                (true, _) => Style::new().dark_gray(),
                                (false, true) => Style::new().dark_gray().on_white(),
                                (false, false) => Style::new().white(),
                            };
                            Cell::from(if ro { " ro " } else { " rw " }).style(style)
                        })
                    }
                    _ => (Line::from(s), Cell::from("")),
                };
                Row::new([Cell::from([" m", " h"][list].dark_gray()), mode, Cell::from(entry)])
            })
            .collect();
        let block = Block::new().borders(Borders::TOP);
        if rows.is_empty() {
            f.render_widget(Paragraph::new("no entries".dark_gray()).block(block), body);
        } else {
            let mut state = TableState::default().with_selected(Some(self.sel));
            let widths = [Constraint::Length(2), Constraint::Length(4), Constraint::Fill(1)];
            let table = Table::new(rows, widths)
                .column_spacing(0) // " ro " pads itself
                .block(block)
                .row_highlight_style(Style::new().reversed());
            f.render_stateful_widget(table, body, &mut state);
        }
        if self.border {
            // Over the TOP border above, reaching the outer one: ├───┤
            let full = f.area();
            let sep = "├".to_string() + &"─".repeat(full.width.saturating_sub(2) as usize) + "┤";
            f.render_widget(Paragraph::new(sep), Rect { x: full.x, width: full.width, height: 1, ..body });
        }

        let help = match &self.mode {
            _ if !self.status.is_empty() => self.status.as_str().red(),
            Mode::Normal => {
                "a add mount  h add host  e/enter edit  tab ro/rw  d delete  ↑↓/jk move  q quit".dark_gray()
            }
            Mode::Input { .. } => "enter save  ctrl+u clear  esc cancel".dark_gray(),
            Mode::Confirm { .. } => "tab switch  enter pick  y yes  n/esc no".dark_gray(),
            Mode::Finder { .. } => {
                "type to filter  ↑↓/alt+j/k move  enter open  backspace up  tab add  esc cancel"
                    .dark_gray()
            }
        };
        f.render_widget(Paragraph::new(help), bottom);

        match &self.mode {
            Mode::Normal => {}
            Mode::Confirm { yes, .. } => {
                // Like the `--test` dialog: caption, gap, Yes/No buttons.
                let text = "Are you sure?";
                let w = 30.min(body.width);
                let area = body.centered(Constraint::Length(w), Constraint::Length(5));
                let block = Block::bordered().border_type(BorderType::Rounded);
                let inner = block.inner(area);
                f.render_widget(Clear, area);
                f.render_widget(block, area);
                let [caption, _, row] = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                ])
                .areas(inner);
                f.render_widget(Paragraph::new(text.white()).centered(), caption);
                dialog::buttons(f, row, &["Yes", "No"], if *yes { 0 } else { 1 });
            }
            Mode::Input { buf, .. } => {
                let area = body.centered(Constraint::Percentage(80), Constraint::Length(3));
                let block = Block::bordered().border_type(BorderType::Rounded);
                let inner = block.inner(area);
                f.render_widget(Clear, area);
                f.render_widget(block, area);
                // Keep the end (and the cursor) in view when it's too long.
                let n = buf.chars().count();
                let skip = n.saturating_sub(inner.width.saturating_sub(1) as usize);
                let shown: String = buf.chars().skip(skip).collect();
                f.render_widget(Paragraph::new(shown), inner);
                f.set_cursor_position((inner.x + (n - skip) as u16, inner.y));
            }
            Mode::Finder { root, dirs, query, sel } => {
                let area = body.centered(Constraint::Percentage(80), Constraint::Percentage(80));
                let m = matches(dirs, root, query);
                let block = Block::bordered()
                    .border_type(BorderType::Rounded)
                    // Trailing ─ keeps one dash between the count and the corner.
                    .title(
                        Line::from(vec![
                            " ".into(),
                            format!("{}/{}", m.len(), dirs.len()).white(),
                            " ─".into(),
                        ])
                        .right_aligned(),
                    );
                let inner = block.inner(area);
                f.render_widget(Clear, area);
                f.render_widget(block, area);
                let [q, sep, list] = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(0),
                ])
                .areas(inner);
                // Drawn over the side borders so it joins them: ├───┤
                let sep_line = format!("├{}┤", "─".repeat(area.width.saturating_sub(2) as usize));
                f.render_widget(Paragraph::new(sep_line), Rect { x: area.x, width: area.width, ..sep });
                let prompt = format!("{}/ ", tilde(root).trim_end_matches('/'));
                let x = q.x + (prompt.chars().count() + query.chars().count()) as u16;
                f.render_widget(Paragraph::new(Line::from(vec![prompt.white().bold(), query.as_str().white()])), q);
                f.set_cursor_position((x.min(q.right().saturating_sub(1)), q.y));
                let picked = (*sel).min(m.len().saturating_sub(1));
                let items: Vec<ListItem> = m
                    .iter()
                    .enumerate()
                    .map(|(i, (d, hits))| {
                        // Default colour on the selected row, so the reversed
                        // highlight matches the main list's, not a white bar.
                        let base = if i == picked { Style::new() } else { Style::new().white() };
                        ListItem::new(Line::from(highlight(rel(root, d), hits, base)))
                    })
                    .collect();
                let n = items.len();
                let mut state = ListState::default().with_selected((n > 0).then_some(picked));
                let list_w = List::new(items).highlight_style(Style::new().reversed());
                f.render_stateful_widget(list_w, list, &mut state);
            }
        }
    }
}

/// `d` relative to `root`, "." for `root` itself.
fn rel<'a>(root: &str, d: &'a str) -> &'a str {
    match d.strip_prefix(root) {
        Some("") => ".",
        Some(r) => r.trim_start_matches('/'),
        None => d,
    }
}

/// Dirs matching `query` (against their path relative to `root`), best first.
// ponytail: rescored on every key/frame and capped at 500 shown; cache if it lags.
fn matches<'a>(dirs: &'a [String], root: &str, query: &str) -> Vec<(&'a str, Vec<usize>)> {
    if query.trim().is_empty() {
        return dirs.iter().take(500).map(|d| (d.as_str(), vec![])).collect();
    }
    let mut v: Vec<_> = dirs
        .iter()
        .filter_map(|d| fuzzy(query, rel(root, d)).map(|(s, hits)| (s, d.as_str(), hits)))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.len().cmp(&b.1.len())));
    v.into_iter().take(500).map(|(_, d, hits)| (d, hits)).collect()
}

/// Dirs under `root`, shallowest first. Skips SKIP, git internals and
/// symlinks (no loops). Other hidden dirs stay: projects live in e.g. ~/.projects.
// ponytail: a fixed deny list of the usual huge dirs; extend it when one bloats the finder.
const SKIP: [&str; 9] = [
    ".git", ".cache", ".cargo", ".rustup", ".npm", ".local", ".mozilla", "node_modules", "target",
];

// ponytail: depth 6 / 100k dirs, scanned synchronously on open. Typing an
// absolute path covers anything outside; move to a thread if the scan lags.
fn dirs(root: &Path) -> Vec<String> {
    let mut out = vec![];
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if SKIP.contains(&&*name) {
                continue;
            }
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let p = e.path();
            out.push(p.to_string_lossy().into_owned());
            // A dir with HEAD is a bare repo: its insides are noise.
            if depth < 6 && out.len() < 100_000 && !p.join("HEAD").is_file() {
                stack.push((p, depth + 1));
            }
        }
    }
    out.sort_by(|a, b| a.matches('/').count().cmp(&b.matches('/').count()).then(a.cmp(b)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks() {
        let tmp = env::temp_dir().join(format!("cdc-test-{}", std::process::id()));
        for d in ["a/b", ".cache/x", "node_modules/x", "bare.git/refs"] {
            fs::create_dir_all(tmp.join(d)).unwrap();
        }
        fs::write(tmp.join("bare.git/HEAD"), "").unwrap();
        let found: Vec<String> = dirs(&tmp)
            .iter()
            .map(|d| d.strip_prefix(tmp.to_str().unwrap()).unwrap().to_string())
            .collect();
        fs::remove_dir_all(&tmp).unwrap();
        assert_eq!(found, ["/a", "/bare.git", "/a/b"]);

        let ds = ["/x/foo".to_string(), "/x/foobar".to_string(), "/y".to_string()];
        let m: Vec<&str> = matches(&ds, "/x", "foo").into_iter().map(|(d, _)| d).collect();
        assert_eq!(m, ["/x/foo", "/x/foobar"]);
        assert!(matches(&ds, "/x", "x").is_empty()); // the root part doesn't match
        assert_eq!(rel("/x", "/x"), ".");
        assert_eq!(rel("/", "/y"), "y");

        assert_eq!(split_ro("/a:/b:ro"), ("/a:/b", true));
        assert_eq!(split_ro("/a:/b:rw"), ("/a:/b", false));
        assert_eq!(with_ro("/a:/b", true), "/a:/b:ro");
    }
}
