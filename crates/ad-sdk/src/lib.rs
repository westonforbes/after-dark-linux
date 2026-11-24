//! Linux Rust SDK for After Dark 1.0a (1991).
//!
//! Preserves 1:1 semantics with Berkeley Systems' original `ADMODULE.SDK`:
//! - `MODULE.H` (`AD_SYSTEM`, `AD_MODULE`, control messages, error/palette codes)
//! - `RES_IDS.H` (`AD_CONTROL` types 0..5, `AD_STRING` name/credit/about)
//! - Microsoft C 6.0 `srand`/`rand` LCG (`MsvcRand`)
//! - Exact 235-entry palettes (`HSV_PAL`, `RGB_PAL`, `WHITE_PAL`, `PRIMARY_PAL`) from `AD.EXE` SEG 13
//! - Pure-Rust software GDI canvas (`AdSurface`): ROP2/ROP3, regions, shapes, text, nearest-neighbor scaling
//! - Win16 `SOUND.DRV` tones, PCM voices and SoundFont MIDI over PulseAudio/ALSA (`AdSound`)
//! - 1:1 `AD_PREFS.INI` and `RANDOM.ADS` compatibility (`prefs`)
//! - C-ABI dynamic `.so` plugin support (`ffi`)
//! - Local time and keyboard-lock state shims replacing `GetLocalTime` / `GetKeyState` (`sys`)

pub mod canvas;
pub mod ffi;
pub mod palette;
pub mod prefs;
pub mod sound;
pub mod sys;

use canvas::AdSurface;
use palette::{AdPalette, AdPaletteKind};

/// 1:1 replica of Microsoft C 6.0 `srand()` / `rand()` LCG used by all 1991 After Dark modules.
#[derive(Debug, Clone, Copy)]
pub struct MsvcRand {
    state: u32,
}

impl MsvcRand {
    pub const fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    pub fn from_system_time() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as u32 ^ (d.subsec_nanos() >> 8))
            .unwrap_or(1);
        // Original MODULE.C calls `time(&lTime); srand((unsigned)lTime);` (16-bit unsigned seed).
        Self::new(secs & 0xFFFF)
    }

    pub fn srand(&mut self, seed: u16) {
        self.state = seed as u32;
    }

    /// Returns a 15-bit non-negative integer in `0..=32767` (`0x7FFF`), identical to MSC 6.0 `rand()`.
    #[inline]
    pub fn rand(&mut self) -> i32 {
        self.state = self.state.wrapping_mul(214_013).wrapping_add(2_531_011);
        ((self.state >> 16) & 0x7FFF) as i32
    }

    /// Convenience helper matching `(rand() % n)` for `n > 0`.
    #[inline]
    pub fn rand_mod(&mut self, n: i32) -> i32 {
        if n <= 1 {
            0
        } else {
            self.rand() % n
        }
    }
}

impl Default for MsvcRand {
    fn default() -> Self {
        Self::new(1)
    }
}

/// Screensaver operating mode (`MODULE.H`: `SAVER = 0`, `DEMO = 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum SaverMode {
    Saver = 0,
    Demo = 1,
}

/// Module runner (`MODULE.H`: `AFTERDARK = 0`, `RANDOMIZER = 1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ModuleRunner {
    AfterDark = 0,
    Randomizer = 1,
}

/// 1:1 equivalent of `AD_SYSTEM` in `MODULE.H`.
#[derive(Debug, Clone)]
pub struct AdSystem {
    pub operating_mode: i32,  // ENHANCED = 2
    pub processor_type: i32,  // CPU_486 = 4
    pub coprocessor: bool,    // TRUE
    pub screen_width: i32,    // ptScreenSize.x
    pub screen_height: i32,   // ptScreenSize.y
    pub bits_per_pixel: i32,  // iBitsPerPixel (4/8/24/32)
    pub aspect_x: i32,        // ptAspect.x
    pub aspect_y: i32,        // ptAspect.y
    pub dpi_x: i32,           // ptDotsPerInch.x
    pub dpi_y: i32,           // ptDotsPerInch.y
    pub ad_version: i32,      // 0x0100 (1.0a)
    pub demo_rect: [i32; 4],  // left, top, right, bottom
    pub saver_mode: SaverMode,
    pub runner: ModuleRunner,
    pub sound_enabled: bool,
    pub authentic_timing: bool,
    /// Directory containing user-editable `AD_PREFS.INI`, `MESSAGES.ADS`, `GRAFSTAT.TXT`, `RANDOM.ADS`.
    pub data_dir: String,
    /// User-selected `.bmp` path for `Logo` (`AD_PREFS.INI` `LogoFile=`).
    pub logo_file: String,
    /// User-selected `.bmp` path for `Globe` (`AD_PREFS.INI` `GlobeFile=`).
    pub globe_file: String,
    /// User-selected directory of `.bmp` files for `Slide Show` (`AD_PREFS.INI` `SlideShowDir=`).
    pub slide_show_dir: String,
}

impl AdSystem {
    pub fn new(width: i32, height: i32, saver_mode: SaverMode) -> Self {
        Self {
            operating_mode: 2, // ENHANCED
            processor_type: 4, // CPU_486
            coprocessor: true,
            screen_width: width,
            screen_height: height,
            bits_per_pixel: 32,
            aspect_x: 36,
            aspect_y: 36,
            dpi_x: 96,
            dpi_y: 96,
            ad_version: 0x0100,
            demo_rect: [0, 0, 0, 0],
            saver_mode,
            runner: ModuleRunner::AfterDark,
            sound_enabled: true,
            authentic_timing: true,
            data_dir: String::new(),
            logo_file: String::new(),
            globe_file: String::new(),
            slide_show_dir: String::new(),
        }
    }
}

/// 1:1 equivalent of `AD_MODULE` in `MODULE.H`.
#[derive(Debug, Clone)]
pub struct AdModuleParams {
    pub rgn_width: i32,
    pub rgn_height: i32,
    /// Raw slider/control values (0..=100 for sliders, index for menu, 0/1 for checkbox) stored in prefs.
    pub raw_control_value: [i32; 4],
    /// Mapped values passed into `lpModule->iControlValue[0..4]`, computed identically to `AD.EXE` SEG 13.
    pub control_value: [i32; 4],
    pub control_id: [i32; 4],
    pub palette: Option<AdPalette>,
}

impl AdModuleParams {
    pub fn new(width: i32, height: i32, controls: &[AdControl; 4]) -> Self {
        let mut raw = [0i32; 4];
        let mut mapped = [0i32; 4];
        for i in 0..4 {
            raw[i] = controls[i].default_raw();
            mapped[i] = controls[i].map_raw_to_module_value(raw[i]);
        }
        Self {
            rgn_width: width,
            rgn_height: height,
            raw_control_value: raw,
            control_value: mapped,
            control_id: [1, 2, 3, 4],
            palette: None,
        }
    }

    pub fn set_raw_value(&mut self, slot: usize, raw_val: i32, controls: &[AdControl; 4]) {
        if slot < 4 {
            self.raw_control_value[slot] = raw_val;
            self.control_value[slot] = controls[slot].map_raw_to_module_value(raw_val);
        }
    }
}

/// Return codes from `MODULE.H`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdResult {
    NoError,
    MemError,
    RestartMe,
    WakeUp,
    UserError(String),
    PaletteRequest(AdPaletteKind),
}

/// Decoded 1:1 representation of After Dark's `TYPE_1000` (`AD_CONTROL`) resources.
#[derive(Debug, Clone)]
pub enum AdControl {
    /// Type 0: Unused control slot.
    None,
    /// Type 1: Slider (raw 0..=100) with discrete text labels & upper-bound thresholds.
    LabeledSlider {
        title: String,
        default_raw: i32,
        labels: Vec<String>,
        thresholds: Vec<i32>,
    },
    /// Type 2: Slider (raw 0..=100) mapped linearly onto `min..=max` with optional unit suffix.
    NumericSlider {
        title: String,
        default_raw: i32,
        unit: String,
        min: i32,
        max: i32,
    },
    /// Type 3: Discrete choice / radio / popup menu (`0..choices.len()`).
    Menu {
        title: String,
        default_index: i32,
        choices: Vec<String>,
    },
    /// Type 4: Push button (sends `BUTTONMESSAGE + slot`).
    Button { title: String },
    /// Type 5: Checkbox (`0` or `1`).
    Checkbox { title: String, default_checked: bool },
}

impl AdControl {
    pub fn title(&self) -> &str {
        match self {
            AdControl::None => "",
            AdControl::LabeledSlider { title, .. }
            | AdControl::NumericSlider { title, .. }
            | AdControl::Menu { title, .. }
            | AdControl::Button { title }
            | AdControl::Checkbox { title, .. } => title,
        }
    }

    pub fn default_raw(&self) -> i32 {
        match self {
            AdControl::None | AdControl::Button { .. } => 0,
            AdControl::LabeledSlider { default_raw, .. } => *default_raw,
            AdControl::NumericSlider { default_raw, .. } => *default_raw,
            AdControl::Menu { default_index, .. } => *default_index,
            AdControl::Checkbox {
                default_checked, ..
            } => i32::from(*default_checked),
        }
    }

    /// Exact 1:1 mapping from UI slider/control value to `lpModule->iControlValue[slot]`
    /// reverse-engineered from `AD.EXE` SEG 13 (`13:0DFD..13:0F3C`).
    pub fn map_raw_to_module_value(&self, raw: i32) -> i32 {
        match self {
            AdControl::None | AdControl::Button { .. } => 0,
            AdControl::LabeledSlider { thresholds, .. } => {
                // AD.EXE 13:0E16..13:0E58:
                // val = 0; for t in thresholds { if t <= raw { val = t; } else { break; } }
                let raw = raw.clamp(0, 100);
                let mut val = 0;
                for &t in thresholds {
                    if t <= raw {
                        val = t;
                    } else {
                        break;
                    }
                }
                val
            }
            AdControl::NumericSlider { min, max, .. } => {
                // AD.EXE 13:0ECD..13:0F1C:
                // if raw == 100 { max } else { min + ((max - min + 1) * raw) / 101 }
                let raw = raw.clamp(0, 100);
                if raw >= 100 {
                    *max
                } else {
                    *min + ((*max - *min + 1) * raw) / 101
                }
            }
            AdControl::Menu { choices, .. } => {
                if choices.is_empty() {
                    0
                } else {
                    raw.clamp(0, (choices.len() - 1) as i32)
                }
            }
            AdControl::Checkbox { .. } => i32::from(raw != 0),
        }
    }

    /// Human-readable display label for the current raw slider value (as shown in the Control Panel).
    pub fn format_display_value(&self, raw: i32) -> String {
        match self {
            AdControl::LabeledSlider {
                labels, thresholds, ..
            } => {
                if labels.is_empty() {
                    return format!("{raw}");
                }
                let raw = raw.clamp(0, 100);
                let mut idx = 0;
                for (i, &t) in thresholds.iter().enumerate() {
                    if raw <= t {
                        idx = i;
                        break;
                    }
                    idx = i;
                }
                labels
                    .get(idx)
                    .cloned()
                    .unwrap_or_else(|| format!("{raw}"))
            }
            AdControl::NumericSlider { unit, .. } => {
                let mapped = self.map_raw_to_module_value(raw);
                if unit.is_empty() {
                    format!("{mapped}")
                } else {
                    format!("{mapped}{unit}")
                }
            }
            AdControl::Menu { choices, .. } => choices
                .get(raw.max(0) as usize)
                .cloned()
                .unwrap_or_default(),
            AdControl::Checkbox { .. } => {
                if raw != 0 {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }
            AdControl::Button { title } => title.clone(),
            AdControl::None => String::new(),
        }
    }
}

/// Complete metadata for an After Dark module (`TYPE_2000` strings + `TYPE_1000` controls).
#[derive(Debug, Clone)]
pub struct AdMetadata {
    /// Stable ID (e.g. `"toasters"`, `"starry"`, `"clocks"`, `"nocturne"`, `"randomizer"`).
    pub id: String,
    /// Original 1991 filename (e.g. `"TOASTERS.AD"`, `"AD.EXE"`).
    pub filename: String,
    /// `RES_NAME` (`TYPE_2000 #20`).
    pub name: String,
    /// `RES_CREDIT` (`TYPE_2000 #10`).
    pub credit: String,
    /// `RES_ABOUT` (`TYPE_2000 #30`).
    pub about: String,
    /// `CONTROL1..CONTROL4` (`TYPE_1000 #1..#4`).
    pub controls: [AdControl; 4],
    /// Authentic target frame interval in milliseconds for 1991 pacing on modern CPUs.
    pub target_frame_ms: u32,
}

/// Context passed into every `AdModule` lifecycle callback.
pub struct AdContext<'a> {
    pub system: &'a mut AdSystem,
    pub module: &'a mut AdModuleParams,
    pub surface: &'a mut AdSurface,
    pub sound: &'a sound::AdSound,
    pub rng: &'a mut MsvcRand,
    /// Monotonic elapsed milliseconds since module initialization.
    pub elapsed_ms: u64,
}

/// Core trait implemented by every After Dark screensaver module (1:1 mapping of `MODULE.C`).
pub trait AdModule: Send {
    fn metadata(&self) -> AdMetadata;

    fn do_pre_initialize(&mut self, _ctx: &mut AdContext<'_>) -> AdResult {
        AdResult::NoError
    }

    fn do_initialize(&mut self, _ctx: &mut AdContext<'_>) -> AdResult {
        AdResult::NoError
    }

    fn do_blank(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        ctx.surface.fill_black();
        AdResult::NoError
    }

    fn do_draw_frame(&mut self, ctx: &mut AdContext<'_>) -> AdResult;

    fn do_close(&mut self, _ctx: &mut AdContext<'_>) -> AdResult {
        AdResult::NoError
    }

    fn do_selected(&mut self) -> AdResult {
        AdResult::NoError
    }

    fn do_about(&mut self) -> AdResult {
        AdResult::NoError
    }

    fn do_button_message(&mut self, _control_index: usize) -> AdResult {
        AdResult::NoError
    }
}
