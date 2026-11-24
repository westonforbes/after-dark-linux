//! C-ABI plugin interface (`ad_module_v2`) for external shared-library (`.so`) modules,
//! preserving Berkeley Systems' original `ADMODULE.SDK` `Module(iMessage, hDrawDC, hADSystem)` design.
//!
//! On Windows the draw target was an `HDC`. On Linux there is no GDI device context, so the module
//! receives an [`AdCSurface`] describing the host's 32-bit `0x00RRGGBB` pixel buffer instead. The
//! ABI version is 2 so that Windows-era (v1) plugins are rejected rather than misinterpreted.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::Path;

use libloading::Library;

use crate::{AdContext, AdControl, AdMetadata, AdModule, AdResult};

/// Control messages matching `MODULE.H`.
pub const MSG_INITIALIZE: c_int = 0;
pub const MSG_BLANK: c_int = 1;
pub const MSG_DRAWFRAME: c_int = 2;
pub const MSG_CLOSE: c_int = 3;
pub const MSG_MODULESELECTED: c_int = 5;
pub const MSG_ABOUT: c_int = 6;
pub const MSG_BUTTONMESSAGE: c_int = 7;
pub const MSG_PREINITIALIZE: c_int = 12;

/// Return codes matching `MODULE.H`.
pub const RET_NOERROR: c_int = 0;
pub const RET_MEM_ERROR: c_int = 1;
pub const RET_RESTART_ME: c_int = 3;
pub const RET_WAKE_UP: c_int = 5;
pub const RET_USER_ERROR: c_int = 7;
pub const RET_HSV_PAL: c_int = 10;
pub const RET_RGB_PAL: c_int = 11;
pub const RET_WHITE_PAL: c_int = 12;
pub const RET_PRIMARY_PAL: c_int = 13;

/// Current plugin ABI version.
pub const AD_ABI_VERSION: u32 = 2;

/// Draw target passed to plugins in place of the Win16 `hDrawDC`.
/// `pixels[y * stride + x]` is the `0x00RRGGBB` pixel at `(x, y)`.
#[repr(C)]
pub struct AdCSurface {
    pub pixels: *mut u32,
    pub width: c_int,
    pub height: c_int,
    /// Row stride in pixels (not bytes).
    pub stride: c_int,
}

/// C-ABI system + module parameter block passed to external `.so` modules.
#[repr(C)]
pub struct AdCModuleState {
    pub abi_version: u32,
    pub screen_width: c_int,
    pub screen_height: c_int,
    pub rgn_width: c_int,
    pub rgn_height: c_int,
    pub bits_per_pixel: c_int,
    pub saver_mode: c_int,
    pub control_value: [c_int; 4],
    pub elapsed_ms: u64,
    pub user_data: *mut c_void,
}

/// C-ABI metadata returned by `ad_module_metadata_v2()`.
#[repr(C)]
pub struct AdCMetadata {
    pub abi_version: u32,
    pub name: *const c_char,
    pub credit: *const c_char,
    pub about: *const c_char,
    pub target_frame_ms: u32,
}

pub type AdModuleEntryFn = unsafe extern "C" fn(c_int, *mut AdCSurface, *mut AdCModuleState) -> c_int;
pub type AdModuleMetaFn = unsafe extern "C" fn() -> AdCMetadata;

/// Dynamically loaded shared-library module built against the `ad-sdk` C-ABI.
pub struct DynamicModule {
    // Keeps the library mapped for as long as `entry` may be called.
    _lib: Library,
    entry: AdModuleEntryFn,
    meta: AdMetadata,
    state: AdCModuleState,
}

/// Backwards-compatible name from the Windows port.
pub type DynamicDllModule = DynamicModule;

// SAFETY: the plugin contract requires modules to be callable from the host's render thread;
// all calls go through `&mut self`, so they are never concurrent.
unsafe impl Send for DynamicModule {}

impl DynamicModule {
    pub fn load(path: &Path) -> Option<Self> {
        // SAFETY: loading a plugin runs its initializers; plugins are trusted code the user
        // placed in their modules directory, exactly like the Windows `.dll` modules.
        unsafe {
            let lib = Library::new(path).ok()?;
            let entry: AdModuleEntryFn = *lib.get::<AdModuleEntryFn>(b"ad_module_entry_v2\0").ok()?;
            let meta_fn: AdModuleMetaFn = *lib.get::<AdModuleMetaFn>(b"ad_module_metadata_v2\0").ok()?;
            let cmeta = meta_fn();
            if cmeta.abi_version != AD_ABI_VERSION {
                return None;
            }
            let read_cstr = |p: *const c_char| -> String {
                if p.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(p).to_string_lossy().into_owned()
                }
            };
            let filename = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "plugin.so".to_string());
            let name = read_cstr(cmeta.name);
            let id = format!(
                "so_{}",
                filename
                    .to_ascii_lowercase()
                    .trim_start_matches("lib")
                    .trim_end_matches(".so")
            );
            let meta = AdMetadata {
                id,
                filename,
                name: if name.is_empty() {
                    "External Module".to_string()
                } else {
                    name
                },
                credit: read_cstr(cmeta.credit),
                about: read_cstr(cmeta.about),
                controls: [
                    AdControl::None,
                    AdControl::None,
                    AdControl::None,
                    AdControl::None,
                ],
                target_frame_ms: cmeta.target_frame_ms.max(16),
            };
            Some(Self {
                _lib: lib,
                entry,
                meta,
                state: AdCModuleState {
                    abi_version: AD_ABI_VERSION,
                    screen_width: 0,
                    screen_height: 0,
                    rgn_width: 0,
                    rgn_height: 0,
                    bits_per_pixel: 32,
                    saver_mode: 0,
                    control_value: [0; 4],
                    elapsed_ms: 0,
                    user_data: std::ptr::null_mut(),
                },
            })
        }
    }

    fn dispatch(&mut self, msg: c_int, ctx: &mut AdContext<'_>) -> AdResult {
        self.state.screen_width = ctx.system.screen_width;
        self.state.screen_height = ctx.system.screen_height;
        self.state.rgn_width = ctx.module.rgn_width;
        self.state.rgn_height = ctx.module.rgn_height;
        self.state.bits_per_pixel = ctx.system.bits_per_pixel;
        self.state.saver_mode = ctx.system.saver_mode as c_int;
        self.state.control_value = ctx.module.control_value;
        self.state.elapsed_ms = ctx.elapsed_ms;
        let (width, height) = (ctx.surface.width, ctx.surface.height);
        let mut target = AdCSurface {
            pixels: ctx.surface.pixels_mut().as_mut_ptr(),
            width,
            height,
            stride: width,
        };
        // SAFETY: `target` points at a live, correctly sized buffer for the duration of the call.
        let code = unsafe { (self.entry)(msg, &mut target, &mut self.state) };
        match code {
            RET_NOERROR => AdResult::NoError,
            RET_MEM_ERROR => AdResult::MemError,
            RET_RESTART_ME => AdResult::RestartMe,
            RET_WAKE_UP => AdResult::WakeUp,
            RET_HSV_PAL => AdResult::PaletteRequest(crate::palette::AdPaletteKind::Hsv),
            RET_RGB_PAL => AdResult::PaletteRequest(crate::palette::AdPaletteKind::Rgb),
            RET_WHITE_PAL => AdResult::PaletteRequest(crate::palette::AdPaletteKind::White),
            RET_PRIMARY_PAL => AdResult::PaletteRequest(crate::palette::AdPaletteKind::Primary),
            _ => AdResult::NoError,
        }
    }
}

impl AdModule for DynamicModule {
    fn metadata(&self) -> AdMetadata {
        self.meta.clone()
    }

    fn do_pre_initialize(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.dispatch(MSG_PREINITIALIZE, ctx)
    }

    fn do_initialize(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.dispatch(MSG_INITIALIZE, ctx)
    }

    fn do_blank(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.dispatch(MSG_BLANK, ctx)
    }

    fn do_draw_frame(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.dispatch(MSG_DRAWFRAME, ctx)
    }

    fn do_close(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.dispatch(MSG_CLOSE, ctx)
    }
}
