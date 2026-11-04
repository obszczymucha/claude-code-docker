//! Edits claude-docker.sh's per-project extra mounts/hosts config.
//!   claude-docker-config                  open the TUI
//!   claude-docker-config add <path>       add an entry for <path>
//!   claude-docker-config add .            add the current repo (bare repo dir / repo root / $PWD)
//!   claude-docker-config add . --force    add $PWD even inside a repo
//!   claude-docker-config --test           a "Test" box with OK/Cancel buttons, prints the pick

mod app;
mod config;
mod dialog;
mod fuzzy;
mod paths;

use std::{env, process::exit};

use app::{App, Mode};
use config::{config_path, load, save, Entry};
use paths::resolve_key;

const USAGE: &str = "usage: claude-docker-config [add <path> [--force] | --test]";

fn die(msg: &str) -> ! {
    eprintln!("claude-docker-config: {msg}");
    exit(1)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args == ["--test"] {
        // Drawn on stderr, so stdout carries only the pick (`> file`, `$(...)`).
        let picked = dialog::run().unwrap_or_else(|e| die(&e.to_string()));
        return println!("{picked}");
    }
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
