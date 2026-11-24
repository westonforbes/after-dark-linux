//! Small OS shims replacing the Win32 calls a few modules made directly.

use std::sync::atomic::{AtomicBool, Ordering};

/// Win32 `SYSTEMTIME` layout (field names kept so ported module code reads the same).
#[allow(non_snake_case)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

/// `GetLocalTime` equivalent using the C library's local time zone.
pub fn local_time() -> SYSTEMTIME {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as libc::time_t;
    // SAFETY: `localtime_r` writes only into the provided `tm`.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return SYSTEMTIME::default();
    }
    SYSTEMTIME {
        wYear: (tm.tm_year + 1900) as u16,
        wMonth: (tm.tm_mon + 1) as u16,
        wDayOfWeek: tm.tm_wday as u16,
        wDay: tm.tm_mday as u16,
        wHour: tm.tm_hour as u16,
        wMinute: tm.tm_min as u16,
        wSecond: tm.tm_sec as u16,
        wMilliseconds: now.subsec_millis() as u16,
    }
}

static CAPS_LOCK: AtomicBool = AtomicBool::new(false);

/// Host hook: report the current Caps Lock state (polled from the display server).
pub fn set_caps_lock(on: bool) {
    CAPS_LOCK.store(on, Ordering::Relaxed);
}

/// `GetKeyState(VK_CAPITAL) & 1` equivalent.
pub fn caps_lock_on() -> bool {
    CAPS_LOCK.load(Ordering::Relaxed)
}
