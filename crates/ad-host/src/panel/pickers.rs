//! File/folder choosers for Logo, Globe and Slide Show, via `zenity` or `kdialog` subprocesses
//! (no GUI toolkit dependency). When neither tool exists the panel falls back to a path field.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};

use eframe::egui;

/// What a picked path is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickTarget {
    LogoFile,
    GlobeFile,
    SlideShowDir,
}

impl PickTarget {
    pub fn title(self) -> &'static str {
        match self {
            Self::LogoFile => "Logo — Select Picture (.BMP)",
            Self::GlobeFile => "Globe — Select World Map (.BMP)",
            Self::SlideShowDir => "Slide Show — Select Folder of .BMP Pictures",
        }
    }

    pub fn is_dir(self) -> bool {
        self == Self::SlideShowDir
    }
}

pub enum PickResult {
    Picked(String),
    Cancelled,
    Failed(String),
}

fn find_in_path(exe: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(exe))
        .find(|p| p.is_file())
}

/// Start a native chooser in the background. `None` if no chooser program is installed.
pub fn spawn(target: PickTarget, initial: &str, ctx: &egui::Context) -> Option<Receiver<PickResult>> {
    let start = start_path(initial);
    let mut cmd = if let Some(zenity) = find_in_path("zenity") {
        let mut c = Command::new(zenity);
        c.arg("--file-selection").arg(format!("--title={}", target.title()));
        if target.is_dir() {
            c.arg("--directory");
        } else {
            c.arg("--file-filter=Windows Bitmaps (*.bmp) | *.bmp *.BMP *.Bmp")
                .arg("--file-filter=All Files | *");
        }
        if !start.is_empty() {
            c.arg(format!("--filename={start}"));
        }
        c
    } else {
        let mut c = Command::new(find_in_path("kdialog")?);
        let start = if start.is_empty() { ".".to_string() } else { start };
        if target.is_dir() {
            c.arg("--getexistingdirectory").arg(start);
        } else {
            c.arg("--getopenfilename").arg(start).arg("Windows Bitmaps (*.bmp *.BMP)");
        }
        c.arg("--title").arg(target.title());
        c
    };
    let (tx, rx) = mpsc::channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let result = match cmd.output() {
            Ok(out) if out.status.success() => {
                let path = String::from_utf8_lossy(&out.stdout).trim_end_matches(['\n', '\r']).to_string();
                if path.is_empty() { PickResult::Cancelled } else { PickResult::Picked(path) }
            }
            // Both tools exit 1 on Cancel; a signal means someone closed it some other way.
            Ok(out) if matches!(out.status.code(), Some(1) | None) => PickResult::Cancelled,
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
                PickResult::Failed(if last.is_empty() { format!("exit status {}", out.status) } else { last.to_string() })
            }
            Err(e) => PickResult::Failed(e.to_string()),
        };
        let _ = tx.send(result);
        ctx.request_repaint();
    });
    Some(rx)
}

/// Where the chooser opens: the current file/folder, else its parent, else `$HOME/`.
fn start_path(initial: &str) -> String {
    let p = Path::new(initial.trim());
    if !initial.trim().is_empty() && (p.exists() || p.parent().is_some_and(Path::is_dir)) {
        let mut s = p.to_string_lossy().into_owned();
        if p.is_dir() && !s.ends_with('/') {
            s.push('/');
        }
        return s;
    }
    std::env::var("HOME").map(|h| format!("{h}/")).unwrap_or_default()
}
