//! Desktop-session services: display geometry, idle time, pointer/keyboard state, desktop
//! capture and the global "Sleep Now" hotkey.
//!
//! X11 is used when available (pure-Rust `x11rb`: RandR, MIT-SCREEN-SAVER, core `GetImage`,
//! `GrabKey`). Under a Wayland session, idle time comes from GNOME's
//! `org.gnome.Mutter.IdleMonitor` (or the freedesktop ScreenSaver interface) over D-Bus; capture,
//! hot corners and the global hotkey are unavailable there, and degrade gracefully.

use std::process::Command;

use ad_sdk::canvas::DesktopSnapshot;
use ad_sdk::prefs::ScreenCorner;
use x11rb::connection::Connection;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::screensaver::ConnectionExt as _;
use x11rb::protocol::xproto::{
    ConnectionExt as _, GrabMode, ImageFormat, KeyButMask, ModMask, Window,
};
use x11rb::rust_connection::RustConnection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub caps_lock: bool,
}

pub fn is_wayland_session() -> bool {
    // An empty variable means "unset" (e.g. `WAYLAND_DISPLAY=` to force X11).
    let set = |v: &str| std::env::var_os(v).is_some_and(|x| !x.is_empty());
    if set("WAYLAND_DISPLAY") {
        return true;
    }
    !set("DISPLAY") && std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"))
}

/// A connection to the X server (also works against XWayland, with reduced fidelity).
pub struct X11 {
    conn: RustConnection,
    root: Window,
    root_depth: u8,
    screen_w: i32,
    screen_h: i32,
}

impl X11 {
    pub fn connect() -> Option<Self> {
        std::env::var_os("DISPLAY")?;
        let (conn, screen_num) = x11rb::connect(None).ok()?;
        let screen = &conn.setup().roots[screen_num];
        Some(Self {
            root: screen.root,
            root_depth: screen.root_depth,
            screen_w: screen.width_in_pixels as i32,
            screen_h: screen.height_in_pixels as i32,
            conn,
        })
    }

    pub fn monitors(&self) -> Vec<MonitorRect> {
        let mut out: Vec<MonitorRect> = self
            .conn
            .randr_get_monitors(self.root, true)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| {
                r.monitors
                    .iter()
                    .map(|m| MonitorRect {
                        x: m.x as i32,
                        y: m.y as i32,
                        width: m.width as i32,
                        height: m.height as i32,
                        primary: m.primary,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if out.is_empty() {
            out.push(MonitorRect {
                x: 0,
                y: 0,
                width: self.screen_w,
                height: self.screen_h,
                primary: true,
            });
        }
        if !out.iter().any(|m| m.primary) {
            out[0].primary = true;
        }
        out
    }

    /// Milliseconds since the last user input (MIT-SCREEN-SAVER extension).
    pub fn idle_ms(&self) -> Option<u64> {
        let r = self.conn.screensaver_query_info(self.root).ok()?.reply().ok()?;
        Some(r.ms_since_user_input as u64)
    }

    /// Pointer position (root coordinates) and modifier state.
    pub fn pointer(&self) -> Option<((i32, i32), Modifiers)> {
        let r = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        let mask = r.mask;
        Some((
            (r.root_x as i32, r.root_y as i32),
            Modifiers {
                shift: mask.contains(KeyButMask::SHIFT),
                ctrl: mask.contains(KeyButMask::CONTROL),
                caps_lock: mask.contains(KeyButMask::LOCK),
            },
        ))
    }

    /// Snapshot the whole root window (all monitors) as `0x00RRGGBB`.
    pub fn capture_root(&self) -> Option<DesktopSnapshot> {
        if self.root_depth != 24 && self.root_depth != 32 {
            return None;
        }
        let (w, h) = (self.screen_w, self.screen_h);
        let mut pixels = Vec::with_capacity((w * h) as usize);
        // Fetch in bands to stay well within request/reply size limits.
        const BAND: i32 = 128;
        let mut y = 0;
        while y < h {
            let bh = BAND.min(h - y);
            let reply = self
                .conn
                .get_image(ImageFormat::Z_PIXMAP, self.root, 0, y as i16, w as u16, bh as u16, !0)
                .ok()?
                .reply()
                .ok()?;
            // 24/32-bit TrueColor ZPixmaps are 4 bytes per pixel, little-endian BGRX.
            let stride = reply.data.len() / bh as usize;
            if stride < (w as usize) * 4 {
                return None;
            }
            for row in reply.data.chunks_exact(stride) {
                pixels.extend(
                    row[..w as usize * 4]
                        .as_chunks::<4>().0.iter()
                        .map(|p| u32::from_le_bytes([p[0], p[1], p[2], 0])),
                );
            }
            y += bh;
        }
        Some(DesktopSnapshot {
            x: 0,
            y: 0,
            width: w,
            height: h,
            pixels,
        })
    }

    /// Grab `keysym` with the given modifiers on the root window (also with Caps/Num Lock on).
    /// Returns the keycode to match in `KeyPress` events.
    pub fn grab_hotkey(&self, keysym: u32, ctrl: bool, shift: bool) -> Option<u8> {
        let setup = self.conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = self
            .conn
            .get_keyboard_mapping(min, max - min + 1)
            .ok()?
            .reply()
            .ok()?;
        let per = map.keysyms_per_keycode as usize;
        let idx = map
            .keysyms
            .chunks(per.max(1))
            .position(|syms| syms.contains(&keysym))?;
        let keycode = min + idx as u8;
        let mut mods = ModMask::from(0u16);
        if ctrl {
            mods |= ModMask::CONTROL;
        }
        if shift {
            mods |= ModMask::SHIFT;
        }
        for extra in [0u16, u16::from(ModMask::LOCK), u16::from(ModMask::M2), u16::from(ModMask::LOCK) | u16::from(ModMask::M2)] {
            let m = ModMask::from(u16::from(mods) | extra);
            self.conn
                .grab_key(false, self.root, m, keycode, GrabMode::ASYNC, GrabMode::ASYNC)
                .ok()?;
        }
        self.conn.flush().ok()?;
        Some(keycode)
    }

    pub fn ungrab_all_keys(&self) {
        let _ = self.conn.ungrab_key(0, self.root, ModMask::ANY);
        let _ = self.conn.flush();
    }

    /// Drain pending events; true if a `KeyPress` of `keycode` arrived.
    pub fn poll_hotkey(&self, keycode: u8) -> bool {
        let mut hit = false;
        while let Ok(Some(ev)) = self.conn.poll_for_event() {
            if let x11rb::protocol::Event::KeyPress(k) = ev {
                hit |= k.detail == keycode;
            }
        }
        hit
    }
}

/// Idle time via D-Bus for Wayland sessions (GNOME Mutter, then the freedesktop ScreenSaver API).
pub fn dbus_idle_ms() -> Option<u64> {
    let attempts: [&[&str]; 2] = [
        &[
            "call", "--session", "--dest", "org.gnome.Mutter.IdleMonitor", "--object-path",
            "/org/gnome/Mutter/IdleMonitor/Core", "--method",
            "org.gnome.Mutter.IdleMonitor.GetIdletime",
        ],
        &[
            "call", "--session", "--dest", "org.freedesktop.ScreenSaver", "--object-path",
            "/org/freedesktop/ScreenSaver", "--method",
            "org.freedesktop.ScreenSaver.GetSessionIdleTime",
        ],
    ];
    for args in attempts {
        let Ok(out) = Command::new("gdbus").args(args).output() else {
            return None; // gdbus not installed
        };
        if !out.status.success() {
            continue;
        }
        // Replies look like `(uint64 12345,)` or `(uint32 12,)`.
        let text = String::from_utf8_lossy(&out.stdout);
        let digits: String = text
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(v) = digits.parse::<u64>() {
            // GetSessionIdleTime is in seconds; Mutter's GetIdletime in milliseconds.
            return Some(if args[4] == "org.freedesktop.ScreenSaver" { v * 1000 } else { v });
        }
    }
    None
}

/// Hot-corner hit test: 8x8 pixel zone on the primary monitor (`AD_LIB.DLL` `CHECKSLEEPRECT`).
pub fn point_in_corner(primary: &MonitorRect, (cx, cy): (i32, i32), corner: ScreenCorner) -> bool {
    const C: i32 = 8;
    let (l, t) = (primary.x, primary.y);
    let (r, b) = (l + primary.width, t + primary.height);
    let in_x_left = cx >= l && cx < l + C;
    let in_x_right = cx >= r - C && cx < r;
    let in_y_top = cy >= t && cy < t + C;
    let in_y_bottom = cy >= b - C && cy < b;
    match corner {
        ScreenCorner::TopLeft => in_x_left && in_y_top,
        ScreenCorner::TopRight => in_x_right && in_y_top,
        ScreenCorner::BottomLeft => in_x_left && in_y_bottom,
        ScreenCorner::BottomRight => in_x_right && in_y_bottom,
    }
}

/// Modifier state right now, when it can be known (X11 only).
pub fn current_modifiers() -> Modifiers {
    X11::connect()
        .and_then(|x| x.pointer())
        .map(|(_, m)| m)
        .unwrap_or_default()
}
