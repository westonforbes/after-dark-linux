//! After Dark for Linux.
//!
//! One binary, several roles (see `common.rs` for how they talk to each other):
//!
//! ```text
//! after-dark                     Control Panel
//! after-dark daemon              idle monitor: timeout, hot corners, Sleep Now hotkey
//! after-dark run [options]       run the screen saver now
//!     --demo                     Control Panel demo: Escape/click exits, live control updates on stdin
//!     --module ID                module to run (default: the one selected in the Control Panel)
//!     --window-id XID            draw into an existing X11 window (xscreensaver hack mode;
//!                                $XSCREENSAVER_WINDOW is honoured too)
//! after-dark sleep-now           start the saver immediately (bind this to a keyboard shortcut)
//! after-dark list                list module ids
//! ```

mod common;
mod daemon;
mod panel;
mod platform;
mod saver;

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage: after-dark [panel | daemon | run [--demo] [--module ID] [--window-id XID] | sleep-now | list]"
    );
    ExitCode::from(2)
}

fn parse_xid(s: &str) -> Option<u32> {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None | Some("panel") => panel::run(),
        Some("daemon") => daemon::run(),
        Some("sleep-now") => saver::run(saver::RunOptions::default()),
        Some("list") => {
            for e in common::AppData::load().catalog {
                println!("{:28} {}", e.meta.id, e.meta.name);
            }
            ExitCode::SUCCESS
        }
        Some("run") => {
            let mut opts = saver::RunOptions::default();
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--demo" => opts.demo = true,
                    "--module" => opts.module_id = it.next().cloned(),
                    "--window-id" | "-window-id" => match it.next().and_then(|s| parse_xid(s)) {
                        Some(x) => opts.window_id = Some(x),
                        None => return usage(),
                    },
                    // xscreensaver passes these; they need no special handling.
                    "-root" | "--root" | "-install" | "-visual" => {}
                    _ => return usage(),
                }
            }
            if opts.window_id.is_none() {
                opts.window_id = std::env::var("XSCREENSAVER_WINDOW").ok().and_then(|s| parse_xid(&s));
            }
            saver::run(opts)
        }
        Some("-h" | "--help" | "help") => {
            println!("{}", include_str!("main.rs").lines().skip(4).take(11).map(|l| l.trim_start_matches("//! ")).collect::<Vec<_>>().join("\n"));
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}
