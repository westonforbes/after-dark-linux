//! Background idle monitor (`after-dark daemon`) — the Linux counterpart of the resident
//! Windows After Dark process.
//!
//! - Starts the saver after `TimeOut=` minutes without input (unless the cursor rests in the
//!   "Never Sleep" corner, or the session has an idle inhibitor such as a playing video).
//! - Hot corners: resting 600 ms in the "Sleep Now" corner starts the saver immediately.
//! - Global Sleep Now hotkey (`AD_PREFS.INI` hotkey, default Ctrl+Shift+S) on X11.
//! - Re-reads `AD_PREFS.INI` whenever the Control Panel saves it.

use std::process::{Command, ExitCode};
use std::time::{Duration, Instant, SystemTime};

use ad_sdk::prefs::AdPrefs;

use crate::common::{self, SaverChild};
use crate::platform::{self, X11};

const TICK: Duration = Duration::from_millis(250);
const CORNER_DWELL: Duration = Duration::from_millis(600);

/// Windows virtual-key code (as stored in `AD_PREFS.INI`) → X keysym.
fn vk_to_keysym(vk: u32) -> Option<u32> {
    match vk {
        0x30..=0x39 => Some(vk),                  // '0'..'9'
        0x41..=0x5A => Some(vk + 0x20),           // 'A'..'Z' → XK_a..XK_z
        0x70..=0x87 => Some(0xFFBE + (vk - 0x70)), // F1..F24
        _ => None,
    }
}

/// Does the desktop session currently inhibit idling (video playback, presentations, ...)?
fn session_idle_inhibited() -> bool {
    // GNOME: org.gnome.SessionManager.IsInhibited(8 = GSM_INHIBITOR_FLAG_IDLE).
    Command::new("gdbus")
        .args([
            "call", "--session", "--dest", "org.gnome.SessionManager", "--object-path",
            "/org/gnome/SessionManager", "--method", "org.gnome.SessionManager.IsInhibited", "8",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains("true"))
}

struct Daemon {
    prefs: AdPrefs,
    prefs_mtime: Option<SystemTime>,
    x11: Option<X11>,
    hotkey: Option<u8>,
    saver: Option<SaverChild>,
    sleep_corner_since: Option<Instant>,
    /// The Sleep Now corner re-arms only after the pointer has left it, so waking the saver with
    /// a click while still parked in the corner doesn't immediately restart it.
    corner_armed: bool,
    idle_triggered: bool,
    last_dbus_idle: Instant,
    dbus_idle_ms: u64,
}

impl Daemon {
    fn new() -> Self {
        let mut d = Self {
            prefs: AdPrefs::default(),
            prefs_mtime: None,
            x11: X11::connect(),
            hotkey: None,
            saver: None,
            sleep_corner_since: None,
            corner_armed: true,
            idle_triggered: false,
            last_dbus_idle: Instant::now() - Duration::from_secs(60),
            dbus_idle_ms: 0,
        };
        d.reload_prefs(true);
        d
    }

    fn reload_prefs(&mut self, force: bool) {
        let path = &common::paths().prefs;
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if !force && mtime == self.prefs_mtime {
            return;
        }
        self.prefs_mtime = mtime;
        self.prefs = AdPrefs::load_from_file(path);
        self.sync_hotkey();
    }

    fn sync_hotkey(&mut self) {
        let Some(x) = &self.x11 else { return };
        x.ungrab_all_keys();
        self.hotkey = None;
        if self.prefs.hotkey_enabled
            && let Some(sym) = vk_to_keysym(self.prefs.hotkey_vk) {
                self.hotkey = x.grab_hotkey(sym, self.prefs.hotkey_ctrl, self.prefs.hotkey_shift);
            }
    }

    fn saver_active(&mut self) -> bool {
        if let Some(s) = self.saver.as_mut() {
            if s.is_running() {
                return true;
            }
            self.saver = None;
        }
        // Started by someone else (Control Panel's Sleep Now, `after-dark sleep-now`)?
        common::is_locked(common::SAVER_LOCK)
    }

    fn start_saver(&mut self) {
        if self.saver_active() {
            return;
        }
        self.saver = SaverChild::spawn(false, None).ok();
        self.sleep_corner_since = None;
        self.corner_armed = false;
    }

    fn idle_ms(&mut self) -> u64 {
        if !platform::is_wayland_session()
            && let Some(ms) = self.x11.as_ref().and_then(|x| x.idle_ms()) {
                return ms;
            }
        // D-Bus is comparatively slow; once a second is plenty for minute-scale timeouts.
        if self.last_dbus_idle.elapsed() >= Duration::from_secs(1) {
            self.last_dbus_idle = Instant::now();
            self.dbus_idle_ms = platform::dbus_idle_ms().unwrap_or(0);
        }
        self.dbus_idle_ms
    }

    fn tick(&mut self) {
        self.reload_prefs(false);

        let hotkey_hit = match (&self.x11, self.hotkey) {
            (Some(x), Some(code)) => x.poll_hotkey(code),
            _ => false,
        };
        if self.saver_active() {
            self.sleep_corner_since = None;
            return;
        }
        if hotkey_hit {
            self.start_saver();
            return;
        }
        if !self.prefs.screen_saver_on {
            self.sleep_corner_since = None;
            return;
        }

        // 1. Hot corners ("Sleep Now" / "Never Sleep") — X11 only.
        let mut in_wake = false;
        if !self.prefs.no_corners
            && let Some(x) = &self.x11 {
                let monitors = x.monitors();
                let primary = monitors.iter().find(|m| m.primary).or(monitors.first()).copied();
                if let (Some(p), Some((pos, _))) = (primary, x.pointer()) {
                    in_wake = platform::point_in_corner(&p, pos, self.prefs.wake_corner);
                    let in_sleep = platform::point_in_corner(&p, pos, self.prefs.sleep_corner);
                    if !in_sleep {
                        self.corner_armed = true;
                    }
                    if in_sleep && self.corner_armed {
                        let since = *self.sleep_corner_since.get_or_insert_with(Instant::now);
                        if since.elapsed() >= CORNER_DWELL {
                            self.start_saver();
                            return;
                        }
                    } else {
                        self.sleep_corner_since = None;
                    }
                }
            }

        // 2. Idle timeout (suppressed in the Never Sleep corner or while the session inhibits idle).
        let idle = self.idle_ms();
        if std::env::var_os("AFTER_DARK_DEBUG").is_some() {
            eprintln!(
                "after-dark daemon: idle={idle}ms timeout={}min triggered={} in_wake_corner={in_wake}",
                self.prefs.timeout_minutes, self.idle_triggered
            );
        }
        let threshold = u64::from(self.prefs.timeout_minutes.max(1)) * 60_000;
        if idle < 2000 {
            self.idle_triggered = false;
        } else if !in_wake && !self.idle_triggered && idle >= threshold {
            self.idle_triggered = true;
            if !session_idle_inhibited() {
                self.start_saver();
            }
        }
    }
}

pub fn run() -> ExitCode {
    let Some(_lock) = common::try_lock(common::DAEMON_LOCK) else {
        eprintln!("after-dark: daemon already running");
        return ExitCode::SUCCESS;
    };
    let mut d = Daemon::new();
    if d.x11.is_none() && platform::dbus_idle_ms().is_none() {
        eprintln!("after-dark: no way to read idle time (need X11, or GNOME/KDE on Wayland)");
        return ExitCode::FAILURE;
    }
    loop {
        d.tick();
        std::thread::sleep(TICK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_mapping() {
        assert_eq!(vk_to_keysym(83), Some(0x73)); // 'S' → XK_s
        assert_eq!(vk_to_keysym(0x31), Some(0x31));
        assert_eq!(vk_to_keysym(0x70), Some(0xFFBE)); // F1
        assert_eq!(vk_to_keysym(0x0D), None);
    }
}
