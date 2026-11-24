//! Linux port of Berkeley Systems' `ADMODULE.SDK\BLANKER\BLANKER.C` (1991).
//!
//! Demonstrates BOTH:
//! 1. Implementing the Rust `ad_sdk::AdModule` trait
//! 2. Exporting a C-ABI `.so` plugin (`ad_module_entry_v2` / `ad_module_metadata_v2`)
//!    mirroring `MODULE.C`'s `Module(iMessage, hDrawDC, hADSystem)` entry point, with the
//!    draw target passed as an `AdCSurface` pixel buffer instead of an `HDC`.

use std::ffi::c_int;

use ad_sdk::ffi::{
    AD_ABI_VERSION, AdCMetadata, AdCModuleState, AdCSurface, MSG_BLANK, MSG_CLOSE, MSG_DRAWFRAME, MSG_INITIALIZE,
    MSG_PREINITIALIZE, RET_NOERROR,
};
use ad_sdk::{AdContext, AdControl, AdMetadata, AdModule, AdResult};

pub struct BlankerModule;

impl BlankerModule {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BlankerModule {
    fn default() -> Self {
        Self::new()
    }
}

impl AdModule for BlankerModule {
    fn metadata(&self) -> AdMetadata {
        AdMetadata {
            id: "blanker".to_string(),
            filename: "BLANKER.AD".to_string(),
            name: "Blanker".to_string(),
            credit: "Blanker by Bill Stewart.\r\n  ©1990 Berkeley Systems Inc.".to_string(),
            about: "BLANKER.\r\n\r\nNo snazzy graphics, all it does is blank your screen."
                .to_string(),
            controls: [
                AdControl::None,
                AdControl::None,
                AdControl::None,
                AdControl::None,
            ],
            target_frame_ms: 100,
        }
    }

    fn do_blank(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        ctx.surface.fill_black();
        AdResult::NoError
    }

    fn do_draw_frame(&mut self, _ctx: &mut AdContext<'_>) -> AdResult {
        AdResult::NoError
    }
}

// ============================================================================
// C-ABI shared-library exports (1:1 successor to MODULE.C / BLANKER.DEF)
// ============================================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ad_module_metadata_v2() -> AdCMetadata {
    AdCMetadata {
        abi_version: AD_ABI_VERSION,
        name: c"Blanker (SDK Sample Plugin)".as_ptr(),
        credit: c"Blanker by Bill Stewart.\r\n  \xA91990 Berkeley Systems Inc.".as_ptr(),
        about: c"BLANKER.\r\n\r\nNo snazzy graphics, all it does is blank your screen.".as_ptr(),
        target_frame_ms: 100,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ad_module_entry_v2(
    message: c_int,
    surface: *mut AdCSurface,
    state: *mut AdCModuleState,
) -> c_int {
    if state.is_null() {
        return RET_NOERROR;
    }
    match message {
        MSG_PREINITIALIZE | MSG_INITIALIZE | MSG_DRAWFRAME | MSG_CLOSE => RET_NOERROR,
        MSG_BLANK => {
            // SAFETY: the host passes a valid surface describing a live pixel buffer.
            if let Some(s) = unsafe { surface.as_ref() } {
                if !s.pixels.is_null() && s.width > 0 && s.height > 0 && s.stride >= s.width {
                    let len = (s.stride as usize) * (s.height as usize);
                    let px = unsafe { std::slice::from_raw_parts_mut(s.pixels, len) };
                    for row in px.chunks_mut(s.stride as usize) {
                        row[..s.width as usize].fill(0);
                    }
                }
            }
            RET_NOERROR
        }
        _ => RET_NOERROR,
    }
}
