//! The config editor TUI.

use std::{
    io,
    path::{Path, PathBuf},
};

use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
};

use crate::{
    config::{Config, mount_missing, save, validate},
    fuzzy::{fuzzy, highlight},
    paths::{resolve_key, tilde},
};

// Panes: 0 = projects, then one per list.
const LISTS: [&str; 2] = ["mounts", "hosts"];

pub(crate) enum Mode {
    Normal,
    Input {
        prompt: &'static str,
        buf: String,
        edit: bool,
    },
    ConfirmDelete,
}

pub(crate) struct App {
    pub(crate) path: PathBuf,
    pub(crate) cfg: Config,
    pub(crate) focus: usize,
    pub(crate) sel: [usize; 3],
    pub(crate) mode: Mode,
    pub(crate) status: String,
    pub(crate) filter: String,
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

    pub(crate) fn run(&mut self, t: &mut DefaultTerminal) -> io::Result<()> {
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
                if code == KeyCode::Char('y')
                    && let Some(k) = self.key()
                {
                    self.cfg.remove(&k);
                    self.save();
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
                    self.cfg
                        .get_mut(&k)
                        .unwrap()
                        .list_mut(f - 1)
                        .remove(self.sel[f]);
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
        let Mode::Input { buf, edit, .. } = &self.mode else {
            return;
        };
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
            if list
                .iter()
                .enumerate()
                .any(|(j, s)| *s == input && !(edit && j == i))
            {
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
        let [main, bottom] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(f.area());
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                .areas(main);
        let [top, bot] =
            Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).areas(right);

        let [filter_box, left] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).areas(left);
        let visible = self.visible();
        let title = format!(" filter {}/{} ", visible.len(), self.cfg.len());
        f.render_widget(
            Paragraph::new(self.filter.as_str()).block(Block::bordered().title(title)),
            filter_box,
        );
        if matches!(self.mode, Mode::Normal) {
            let x = filter_box.x + 1 + self.filter.chars().count() as u16;
            f.set_cursor_position((
                x.min(filter_box.right().saturating_sub(2)),
                filter_box.y + 1,
            ));
        }

        let projects = visible.iter().map(|(k, hits)| {
            let e = &self.cfg[*k];
            let base = if Path::new(k).exists() {
                Style::new()
            } else {
                Style::new().red()
            };
            let mut spans = highlight(&tilde(k), hits, base);
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
        let border = if focused {
            Style::new().fg(Color::Yellow)
        } else {
            Style::new()
        };
        let highlight = if focused {
            Style::new().add_modifier(Modifier::REVERSED)
        } else {
            Style::new().bold()
        };
        let list = List::new(items)
            .block(
                Block::bordered()
                    .title(format!(" {title} "))
                    .border_style(border),
            )
            .highlight_style(highlight);
        let mut state =
            ListState::default().with_selected((n > 0).then(|| self.sel[pane].min(n - 1)));
        f.render_stateful_widget(list, area, &mut state);
    }
}
