//! State and process plumbing shared by the control panel, the idle daemon and the saver runtime.
//!
//! Process model (each is the same `after-dark` binary):
//! - `after-dark` / `after-dark panel` — the Control Panel (eframe).
//! - `after-dark daemon` — background idle watcher: idle timeout, hot corners, Sleep Now hotkey.
//! - `after-dark run [--demo] [--module ID] [--window-id XID]` — the fullscreen saver itself.
//!
//! The panel and daemon start the saver as a child process ([`SaverChild`]). A running demo accepts
//! live control changes as text lines on stdin (see [`SaverChild::send_controls`]).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

use ad_modules::{ModuleCatalogEntry, build_catalog, instantiate_entry};
use ad_sdk::AdModule;
use ad_sdk::prefs::{AdPrefs, RandomizerEntry, load_random_ads, save_random_ads};

/// Filesystem locations (XDG base directories).
pub struct Paths {
    /// `$XDG_CONFIG_HOME/after-dark`: `AD_PREFS.INI`, `RANDOM.ADS`, `MESSAGES.ADS`, `GRAFSTAT.TXT`.
    pub config_dir: PathBuf,
    pub prefs: PathBuf,
    pub random_ads: PathBuf,
    /// `$XDG_DATA_HOME/after-dark/modules`: external `.so` plugin modules.
    pub plugin_dir: PathBuf,
    /// `$XDG_RUNTIME_DIR` (or the config dir): lock files.
    pub runtime_dir: PathBuf,
    /// `$XDG_CONFIG_HOME/autostart/after-dark-daemon.desktop`.
    pub autostart_file: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

pub fn paths() -> &'static Paths {
    static P: OnceLock<Paths> = OnceLock::new();
    P.get_or_init(|| {
        let config_home = xdg("XDG_CONFIG_HOME", ".config");
        let config_dir = config_home.join("after-dark");
        let plugin_dir = xdg("XDG_DATA_HOME", ".local/share").join("after-dark/modules");
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| config_dir.clone());
        let _ = fs::create_dir_all(&config_dir);
        Paths {
            prefs: config_dir.join("AD_PREFS.INI"),
            random_ads: config_dir.join("RANDOM.ADS"),
            autostart_file: config_home.join("autostart/after-dark-daemon.desktop"),
            config_dir,
            plugin_dir,
            runtime_dir,
        }
    })
}

/// Everything the panel/daemon/saver need from disk.
#[derive(Clone)]
pub struct AppData {
    pub prefs: AdPrefs,
    pub catalog: Vec<ModuleCatalogEntry>,
    /// Module ids in the Randomizer pool (`RANDOM.ADS`).
    pub randomizer_pool: Vec<String>,
}

const DEFAULT_POOL: &[&str] = &[
    "toasters", "starry", "clocks", "nocturne", "satori", "mountain", "stained", "aqua", "globe",
    "warp", "laser", "sgyra", "3dbounce",
];

impl AppData {
    pub fn load() -> Self {
        let p = paths();
        let mut prefs = AdPrefs::load_from_file(&p.prefs);
        // `Path=` is where modules read MESSAGES.ADS / GRAFSTAT.TXT. Windows-era values (or a
        // missing directory) are replaced by the config dir.
        if prefs.path.trim().is_empty() || !Path::new(&prefs.path).is_dir() {
            prefs.path = p.config_dir.to_string_lossy().into_owned();
        }
        let catalog = build_catalog(Some(&p.plugin_dir));
        let mut randomizer_pool: Vec<String> = Vec::new();
        for entry in load_random_ads(&p.random_ads) {
            if let Some(found) = catalog.iter().find(|c| {
                c.meta.id != "randomizer"
                    && (c.meta.name.eq_ignore_ascii_case(&entry.module_name)
                        || (!entry.filename.eq_ignore_ascii_case("AD.EXE")
                            && c.meta.filename.eq_ignore_ascii_case(&entry.filename)))
            })
                && !randomizer_pool.contains(&found.meta.id) {
                    randomizer_pool.push(found.meta.id.clone());
                }
        }
        if randomizer_pool.is_empty() {
            randomizer_pool = DEFAULT_POOL.iter().map(|s| s.to_string()).collect();
        }
        Self {
            prefs,
            catalog,
            randomizer_pool,
        }
    }

    /// Write `AD_PREFS.INI` and `RANDOM.ADS`.
    pub fn save(&self) -> io::Result<()> {
        let p = paths();
        self.prefs.save_to_file(&p.prefs)?;
        let entries: Vec<RandomizerEntry> = self
            .randomizer_pool
            .iter()
            .filter_map(|id| self.entry_by_id(id))
            .map(|e| RandomizerEntry {
                module_name: e.meta.name.clone(),
                filename: e.meta.filename.clone(),
            })
            .collect();
        save_random_ads(&p.random_ads, &entries)
    }

    pub fn entry_by_id(&self, id: &str) -> Option<&ModuleCatalogEntry> {
        self.catalog.iter().find(|e| e.meta.id == id)
    }

    /// Index of the module selected in prefs (`ModuleName=` / `ModuleFile=`), default Flying Toasters.
    pub fn selected_index(&self) -> usize {
        self.catalog
            .iter()
            .position(|e| {
                e.meta.name.eq_ignore_ascii_case(&self.prefs.module_name)
                    || e.meta.filename.eq_ignore_ascii_case(&self.prefs.module_file)
            })
            .or_else(|| self.catalog.iter().position(|e| e.meta.id == "toasters"))
            .unwrap_or(0)
    }

    /// Record `entry` as the selected module in prefs.
    pub fn select(&mut self, idx: usize) {
        if let Some(e) = self.catalog.get(idx) {
            self.prefs.module_name = e.meta.name.clone();
            self.prefs.module_file = e.meta.filename.clone();
        }
    }

    /// Saved raw control values for a module, or its defaults.
    pub fn raw_controls(&self, entry: &ModuleCatalogEntry) -> [i32; 4] {
        self.prefs
            .module_controls
            .get(&entry.meta.id)
            .copied()
            .unwrap_or_else(|| std::array::from_fn(|i| entry.meta.controls[i].default_raw()))
    }

    pub fn instantiate(&self, entry: &ModuleCatalogEntry) -> Box<dyn AdModule> {
        instantiate_entry(entry, &self.randomizer_pool, &self.prefs.module_controls)
            .unwrap_or_else(|| Box::new(ad_modules::toasters::ToastersModule::new()))
    }
}

/// Collection a catalog entry belongs to (for filters and Randomizer presets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Collection {
    Classic,
    Simpsons,
    StarTrek,
    LooneyTunes,
}

impl Collection {
    pub fn of(id: &str) -> Self {
        if id.starts_with("simpsons_") {
            Self::Simpsons
        } else if id.starts_with("tng_") {
            Self::StarTrek
        } else if id.starts_with("looney_") {
            Self::LooneyTunes
        } else {
            Self::Classic
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Simpsons => "Simpsons",
            Self::StarTrek => "Star Trek",
            Self::LooneyTunes => "Looney Tunes",
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Locks

/// An exclusive advisory lock held for the lifetime of the value.
pub struct LockGuard {
    _file: File,
}

/// Try to take the named lock in the runtime dir without blocking.
pub fn try_lock(name: &str) -> Option<LockGuard> {
    let path = paths().runtime_dir.join(format!("after-dark-{name}.lock"));
    let file = OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
    // SAFETY: flock on a valid, owned fd.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    (rc == 0).then_some(LockGuard { _file: file })
}

/// Is some process holding the named lock?
pub fn is_locked(name: &str) -> bool {
    try_lock(name).is_none()
}

pub const SAVER_LOCK: &str = "saver";
pub const DAEMON_LOCK: &str = "daemon";

// ---------------------------------------------------------------------------------------------
// Child processes

fn self_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("after-dark"))
}

/// A running `after-dark run` child.
pub struct SaverChild {
    child: Child,
}

impl SaverChild {
    /// Start the saver. `demo` runs it as the Control Panel's live Demo (exits on Escape/click,
    /// accepts control updates); otherwise it is the real screen saver (exits on any input).
    pub fn spawn(demo: bool, module_id: Option<&str>) -> io::Result<Self> {
        let mut cmd = Command::new(self_exe());
        cmd.arg("run");
        if demo {
            cmd.arg("--demo");
        }
        if let Some(id) = module_id {
            cmd.args(["--module", id]);
        }
        cmd.stdin(if demo { Stdio::piped() } else { Stdio::null() });
        Ok(Self { child: cmd.spawn()? })
    }

    /// Live-update a demo's module controls (the slider/menu that changed is `slot`).
    pub fn send_controls(&mut self, slot: usize, raw: [i32; 4]) {
        if let Some(stdin) = self.child.stdin.as_mut() {
            let _ = writeln!(stdin, "controls {slot} {} {} {} {}", raw[0], raw[1], raw[2], raw[3]);
            let _ = stdin.flush();
        }
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn stop(mut self) {
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = writeln!(stdin, "stop");
        }
        // Give it a moment to close its module cleanly, then make sure.
        for _ in 0..20 {
            if !self.is_running() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for SaverChild {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Daemon management

pub fn daemon_running() -> bool {
    is_locked(DAEMON_LOCK)
}

/// Start the idle daemon in the background (no-op if one is already running).
pub fn start_daemon() -> io::Result<()> {
    if daemon_running() {
        return Ok(());
    }
    Command::new(self_exe())
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

pub fn autostart_enabled() -> bool {
    paths().autostart_file.is_file()
}

/// Install or remove the XDG autostart entry that launches the daemon at login.
pub fn set_autostart(enabled: bool) -> io::Result<()> {
    let file = &paths().autostart_file;
    if !enabled {
        return match fs::remove_file(file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(
        file,
        format!(
            "[Desktop Entry]\nType=Application\nName=After Dark\nComment=After Dark screen saver idle monitor\nExec=\"{}\" daemon\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n",
            self_exe().display()
        ),
    )
}
