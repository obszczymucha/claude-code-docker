//! `--test`: a small OK/Cancel dialog, meant for a tmux floating pane, and
//! its buttons, shared with the delete popup.

use std::io;

use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    crossterm::{
        cursor,
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    },
    layout::{Constraint, Flex, Layout, Rect},
    style::Style,
    widgets::{Block, BorderType, Paragraph},
};

const BUTTONS: [&str; 2] = ["OK", "Cancel"];

/// Runs the dialog and returns the picked label.
pub(crate) fn run() -> io::Result<&'static str> {
    execute!(io::stderr(), cursor::Hide)?; // shown again on exit
    terminal::enable_raw_mode()?;
    execute!(io::stderr(), EnterAlternateScreen)?;
    let picked = ui(&mut Terminal::new(CrosstermBackend::new(io::stderr()))?);
    terminal::disable_raw_mode()?;
    execute!(io::stderr(), LeaveAlternateScreen, cursor::Show)?;
    picked
}

/// Buttons centred in `row`, the focused one yellow.
pub(crate) fn buttons(f: &mut Frame, row: Rect, labels: &[&str; 2], focus: usize) {
    // Two spaces either side of each label, so it centres evenly.
    let areas: [Rect; 2] = Layout::horizontal(labels.map(|l| Constraint::Length(l.len() as u16 + 4)))
        .flex(Flex::Center)
        .spacing(2)
        .areas(row);
    for (i, (label, area)) in labels.iter().zip(areas).enumerate() {
        let style = if i == focus {
            Style::new().black().on_yellow()
        } else {
            Style::new().reversed()
        };
        f.render_widget(Paragraph::new(*label).centered().style(style), area);
    }
}

/// Tab switches buttons, Enter picks the focused one, Esc/Ctrl+C mean Cancel.
fn ui(t: &mut Terminal<CrosstermBackend<io::Stderr>>) -> io::Result<&'static str> {
    let mut focus = 0;
    loop {
        t.draw(|f| {
            let outer = Block::bordered().border_type(BorderType::Rounded);
            let inner = outer.inner(f.area());
            f.render_widget(outer, f.area());
            let [caption, _, row] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .areas(inner);
            f.render_widget(Paragraph::new("Princess Kenny").centered(), caption);
            buttons(f, row, &BUTTONS, focus);
        })?;
        if let Event::Key(k) = event::read()? {
            match k.code {
                _ if k.kind != KeyEventKind::Press => {}
                KeyCode::Tab | KeyCode::BackTab => focus = 1 - focus,
                KeyCode::Enter => return Ok(BUTTONS[focus]),
                KeyCode::Esc => return Ok("Cancel"),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok("Cancel");
                }
                _ => {}
            }
        }
    }
}
