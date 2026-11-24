//! SATORI.AD ("Satori") — 1:1 decompiled from After Dark 1.0a (1991).
//!
//! Original by Tracy Diane Elmore (original design and concept by Ben Haller).
//!
//! Exact 16-bit NE disassembly fidelity (`docs/disasm/satori_seg1.asm`, `satori_data.hex`):
//! - 5 mathematical potential/angle interference fields (`Fields`, `Pools`, `Rays`, `Waves`, `Leaves`),
//!   plus dual-field checkerboard `Mix` (with two independent 117-color looping palettes) and `Random`.
//! - Exact 13 named color themes (`Arizona` through `Oz`) from `DS:0x001C..0x031F`, with their
//!   exact RGB anchor tables, transition weights, step bounds, and structural flags.
//! - Exact 118-entry `SIN_TABLE`, 65-entry `ASIN_TABLE`, and 64-entry `LOG2_TABLE` from `DS:0x0364..0x05D3`.
//! - Progressive clockwise spiral-out refinement (`16×16 -> 8×8 -> 4×4 -> 2×2 -> 1×1`) down to
//!   `End Clarity`, backed by an indexed shadow buffer so live 234-color `AnimatePalette` rotation
//!   runs smoothly at ~33 FPS during both refinement and the post-completion hold.
//! - Supports the original 1991 Caps Lock Easter egg (toggling Caps Lock flips palettes live).

use ad_sdk::{AdContext, AdControl, AdMetadata, AdModule, AdResult};

const PAL_LEN: usize = 234; // 0xEA
const HALF_PAL: usize = 117; // 0x75
const MAX_KNOTS: usize = 20;

const SIN_TABLE: [u16; 118] = [
    0, 1760, 3518, 5273, 7025, 8772, 10513, 12246, 13970, 15684,
    17387, 19077, 20753, 22415, 24060, 25688, 27297, 28887, 30456, 32003,
    33527, 35027, 36501, 37949, 39370, 40763, 42126, 43458, 44760, 46029,
    47265, 48467, 49633, 50765, 51859, 52916, 53935, 54915, 55856, 56756,
    57615, 58433, 59209, 59941, 60631, 61277, 61879, 62436, 62949, 63415,
    63836, 64212, 64540, 64823, 65058, 65247, 65388, 65483, 65530, 65530,
    65483, 65388, 65247, 65058, 64823, 64540, 64212, 63836, 63415, 62949,
    62436, 61879, 61277, 60631, 59941, 59209, 58433, 57615, 56756, 55856,
    54915, 53935, 52916, 51859, 50765, 49633, 48467, 47265, 46029, 44760,
    43458, 42126, 40763, 39370, 37949, 36501, 35027, 33527, 32003, 30456,
    28887, 27297, 25688, 24060, 22415, 20753, 19077, 17387, 15684, 13970,
    12246, 10513, 8772, 7025, 5273, 3518, 1760, 0,
];

const ASIN_TABLE: [u32; 65] = [
    0x000000, 0x0094FA, 0x0129FD, 0x01BF12, 0x025444, 0x02E99B, 0x037F21, 0x0414E0,
    0x04AAE1, 0x05412E, 0x05D7D2, 0x066ED6, 0x070646, 0x079E2B, 0x083691, 0x08CF84,
    0x09690F, 0x0A033E, 0x0A9E1D, 0x0B39BB, 0x0BD623, 0x0C7365, 0x0D118F, 0x0DB0B0,
    0x0E50D8, 0x0EF21A, 0x0F9486, 0x10382F, 0x10DD29, 0x11838B, 0x122B69, 0x12D4DD,
    0x138000, 0x142CEE, 0x14DBC5, 0x158CA4, 0x163FAF, 0x16F50B, 0x17ACE2, 0x186760,
    0x1924B7, 0x19E51E, 0x1AA8D0, 0x1B7011, 0x1C3B2D, 0x1D0A77, 0x1DDE4F, 0x1EB723,
    0x1F9570, 0x2079C8, 0x2164D4, 0x22575D, 0x235254, 0x2456DC, 0x25665E, 0x26829D,
    0x27ADE2, 0x28EB32, 0x2A3EB2, 0x2BAE5F, 0x2D4368, 0x2F0D4A, 0x312A3F, 0x33E868,
    0x3A8000,
];

const LOG2_TABLE: [u16; 64] = [
    0x0000, 0x05BA, 0x0B5D, 0x10EB, 0x1664, 0x1BC8, 0x2119, 0x2656,
    0x2B80, 0x3098, 0x359F, 0x3A94, 0x3F78, 0x444C, 0x4910, 0x4DC5,
    0x526A, 0x5700, 0x5B89, 0x6003, 0x646F, 0x68CE, 0x6D20, 0x7165,
    0x759D, 0x79CA, 0x7DEA, 0x81FF, 0x8608, 0x8A06, 0x8DFA, 0x91E2,
    0x95C0, 0x9994, 0x9D5E, 0xA11E, 0xA4D4, 0xA881, 0xAC24, 0xAFBE,
    0xB350, 0xB6D9, 0xBA59, 0xBDD1, 0xC140, 0xC4A8, 0xC807, 0xCB5F,
    0xCEAF, 0xD1F7, 0xD538, 0xD872, 0xDBA5, 0xDED0, 0xE1F5, 0xE513,
    0xE82A, 0xEB3B, 0xEE45, 0xF149, 0xF446, 0xF73E, 0xFA2F, 0xFD1A,
];

struct ThemeDef {
    colors: &'static [[u8; 3]],
    weights: [u8; 4], // [w_non0_to_other, w_non0_to_0, w_0_to_0, w_0_to_other]
    min_step: i32,
    max_step: i32,
    pow2_steps: bool,
    fixed_step: bool,
    two_color_bias: bool,
    allow_early_wrap: bool,
}

const THEMES: [ThemeDef; 13] = [
    // 0: Arizona
    ThemeDef {
        colors: &[
            [1, 1, 1], [102, 51, 0], [255, 153, 102], [0, 34, 0],
            [102, 102, 51], [102, 51, 51], [51, 0, 0], [204, 204, 153],
        ],
        weights: [1, 1, 0, 1],
        min_step: 16, max_step: 64,
        pow2_steps: false, fixed_step: false, two_color_bias: false, allow_early_wrap: true,
    },
    // 1: Siberia
    ThemeDef {
        colors: &[
            [204, 204, 255], [255, 153, 204], [204, 153, 255], [153, 204, 204],
            [153, 153, 255], [102, 102, 255], [51, 51, 255], [51, 0, 102],
        ],
        weights: [1, 1, 1, 1],
        min_step: 16, max_step: 32,
        pow2_steps: false, fixed_step: false, two_color_bias: false, allow_early_wrap: true,
    },
    // 2: Tijuana
    ThemeDef {
        colors: &[
            [1, 1, 1], [127, 0, 0], [127, 63, 0], [255, 0, 0],
            [255, 0, 0], [255, 127, 0], [255, 255, 0],
        ],
        weights: [1, 2, 3, 1],
        min_step: 4, max_step: 16,
        pow2_steps: true, fixed_step: true, two_color_bias: false, allow_early_wrap: false,
    },
    // 3: Pacific
    ThemeDef {
        colors: &[
            [0, 0, 51], [153, 204, 255], [153, 255, 153], [0, 0, 153],
            [51, 0, 102], [0, 0, 255], [0, 153, 153], [0, 255, 153],
        ],
        weights: [1, 3, 1, 1],
        min_step: 8, max_step: 32,
        pow2_steps: false, fixed_step: false, two_color_bias: false, allow_early_wrap: true,
    },
    // 4: Hawaii
    ThemeDef {
        colors: &[
            [51, 204, 153], [0, 85, 0], [102, 255, 102], [255, 255, 51],
            [255, 0, 170], [255, 221, 0], [255, 102, 102], [1, 1, 1],
        ],
        weights: [2, 1, 0, 1],
        min_step: 4, max_step: 16,
        pow2_steps: false, fixed_step: false, two_color_bias: false, allow_early_wrap: true,
    },
    // 5: Nile
    ThemeDef {
        colors: &[
            [1, 1, 1], [0, 119, 0], [127, 63, 0], [102, 0, 102],
            [0, 0, 170], [255, 221, 0],
        ],
        weights: [0, 1, 1, 2],
        min_step: 8, max_step: 32,
        pow2_steps: true, fixed_step: true, two_color_bias: false, allow_early_wrap: false,
    },
    // 6: Louisiana
    ThemeDef {
        colors: &[
            [1, 1, 1], [0, 119, 0], [255, 153, 51], [102, 0, 51],
        ],
        weights: [0, 1, 1, 1],
        min_step: 4, max_step: 16,
        pow2_steps: true, fixed_step: false, two_color_bias: true, allow_early_wrap: true,
    },
    // 7: Camelot
    ThemeDef {
        colors: &[
            [1, 1, 1], [255, 0, 0], [255, 127, 0], [255, 255, 0],
            [51, 204, 0], [0, 102, 255], [153, 0, 153],
        ],
        weights: [0, 1, 1, 2],
        min_step: 8, max_step: 16,
        pow2_steps: false, fixed_step: true, two_color_bias: true, allow_early_wrap: false,
    },
    // 8: Colorado
    ThemeDef {
        colors: &[
            [51, 102, 17], [85, 85, 85], [34, 34, 170], [51, 51, 0],
            [51, 0, 0], [153, 102, 51],
        ],
        weights: [1, 1, 0, 1],
        min_step: 16, max_step: 64,
        pow2_steps: false, fixed_step: false, two_color_bias: false, allow_early_wrap: true,
    },
    // 9: Atlantis
    ThemeDef {
        colors: &[
            [0, 0, 51], [255, 255, 0], [51, 255, 102], [0, 0, 153],
            [51, 0, 102], [1, 1, 1],
        ],
        weights: [1, 2, 0, 1],
        min_step: 2, max_step: 8,
        pow2_steps: false, fixed_step: false, two_color_bias: true, allow_early_wrap: false,
    },
    // 10: Outback
    ThemeDef {
        colors: &[
            [255, 204, 102], [0, 85, 0], [102, 51, 0], [102, 85, 51],
            [204, 204, 153], [153, 102, 51], [255, 255, 204], [153, 51, 51],
        ],
        weights: [3, 1, 3, 1],
        min_step: 4, max_step: 8,
        pow2_steps: false, fixed_step: false, two_color_bias: false, allow_early_wrap: false,
    },
    // 11: Ithaca
    ThemeDef {
        colors: &[
            [64, 64, 64], [254, 254, 254], [1, 1, 1], [128, 128, 128], [192, 192, 192],
        ],
        weights: [2, 1, 0, 1],
        min_step: 2, max_step: 32,
        pow2_steps: true, fixed_step: false, two_color_bias: false, allow_early_wrap: true,
    },
    // 12: Oz
    ThemeDef {
        colors: &[
            [1, 1, 1], [255, 0, 0], [255, 127, 0], [255, 255, 0],
            [127, 0, 0], [0, 255, 0], [0, 255, 127], [0, 127, 255],
            [0, 0, 255], [127, 0, 255], [255, 0, 255], [255, 0, 127],
        ],
        weights: [1, 2, 1, 2],
        min_step: 8, max_step: 64,
        pow2_steps: true, fixed_step: true, two_color_bias: true, allow_early_wrap: true,
    },
];

#[derive(Clone, Copy, Default)]
struct Knot {
    x: i32,
    y: i32,
    weight: i32,
    ray_arms: i32,
    spiral_twist: i32,
    wave_freq: i32,
    wave_amp: i32,
    sign: i32,
}

pub struct SatoriModule {
    width: i32,
    height: i32,

    // Active controls
    display_ctrl: i32,  // 1..=7 (ctrl0 + 1)
    color_ctrl: i32,    // 0..=13 (ctrl1)
    end_clarity: i32,   // 1, 2, 4, 8, or 16
    num_knots: usize,   // 1..=20

    // Active field parameters (even pixels use field1, odd pixels use field2)
    field1: i32, // 1..=5
    field2: i32, // 1..=5
    quantize1: bool,
    quantize2: bool,
    knots: [Knot; MAX_KNOTS],

    // Spiral refinement state
    block_size: i32,
    cur_x: i32,
    cur_y: i32,
    dir: i32, // 0=right, 1=down, 2=left, 3=up
    bound_left: i32,
    bound_top: i32,
    bound_right: i32,
    bound_bottom: i32,

    // Post-completion animation hold
    holding: bool,
    hold_end_ms: u64,

    // Live 234-color cycling palette & indexed shadow buffer (0 = black, 1..=234 = palette index + 1)
    master_pal: [[u8; 3]; PAL_LEN],
    pal_offset: usize,
    indices: Vec<u8>,
    dirty_pixels: bool,

    // Caps Lock Easter egg toggle state
    caps_lock_state: i16,
}

impl Default for SatoriModule {
    fn default() -> Self {
        Self::new()
    }
}

impl SatoriModule {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            display_ctrl: 7,
            color_ctrl: 13,
            end_clarity: 1,
            num_knots: 6,
            field1: 1,
            field2: 3,
            quantize1: false,
            quantize2: false,
            knots: [Knot::default(); MAX_KNOTS],
            block_size: 16,
            cur_x: 0,
            cur_y: 0,
            dir: 2,
            bound_left: 0,
            bound_top: 0,
            bound_right: 0,
            bound_bottom: 0,
            holding: false,
            hold_end_ms: 0,
            master_pal: [[0; 3]; PAL_LEN],
            pal_offset: 0,
            indices: Vec::new(),
            dirty_pixels: false,
            caps_lock_state: 0,
        }
    }

    pub fn static_metadata() -> AdMetadata {
        AdMetadata {
            id: "satori".to_string(),
            filename: "SATORI.AD".to_string(),
            name: "Satori".to_string(),
            credit: "Satori by Tracy Diane Elmore.\n  ©1991 Berkeley Systems Inc.".to_string(),
            about: "SATORI (tm)  uses a technique called \"color animation\" which changes the colors of an image without having to redraw the image.  This happens only if your system has 256 colors.  If your system does not have 256 colors then a single frame of the Satori light show is built.\n\nIf your machine is in 256 colors:\nPressing CAPS LOCK key will change your palette, so that you can flip through different palettes without waking After Dark.\n\nTry the \"Mix\" display option -- it is well worth the wait.\n\nOriginal design and concept by Ben Haller.".to_string(),
            controls: [
                AdControl::Menu {
                    title: "Display:".to_string(),
                    default_index: 6,
                    choices: vec![
                        "Fields".to_string(),
                        "Pools".to_string(),
                        "Rays".to_string(),
                        "Waves".to_string(),
                        "Leaves".to_string(),
                        "Mix".to_string(),
                        "Random".to_string(),
                    ],
                },
                AdControl::Menu {
                    title: "Colors:".to_string(),
                    default_index: 13,
                    choices: vec![
                        "Arizona".to_string(),
                        "Siberia".to_string(),
                        "Tijuana".to_string(),
                        "Pacific".to_string(),
                        "Hawaii".to_string(),
                        "Nile".to_string(),
                        "Louisiana".to_string(),
                        "Camelot".to_string(),
                        "Colorado".to_string(),
                        "Atlantis".to_string(),
                        "Outback".to_string(),
                        "Ithaca".to_string(),
                        "Oz".to_string(),
                        "Random".to_string(),
                    ],
                },
                AdControl::LabeledSlider {
                    title: "End Clarity:".to_string(),
                    default_raw: 0,
                    labels: vec![
                        "1X1".to_string(),
                        "2X2".to_string(),
                        "4X4".to_string(),
                        "8X8".to_string(),
                        "16X16".to_string(),
                    ],
                    thresholds: vec![20, 40, 60, 80, 100],
                },
                AdControl::NumericSlider {
                    title: "Knots:".to_string(),
                    default_raw: 31,
                    unit: String::new(),
                    min: 1,
                    max: 20,
                },
            ],
            target_frame_ms: 30,
        }
    }

    fn read_end_clarity(raw: i32) -> i32 {
        if raw < 20 {
            1
        } else if raw < 40 {
            2
        } else if raw < 60 {
            4
        } else if raw < 80 {
            8
        } else {
            16
        }
    }

    fn hold_duration_ms(clarity: i32) -> u64 {
        match clarity {
            16 => 5_000,   // 0x1388
            8 => 30_000,   // 0x7530
            4 => 60_000,   // 0xEA60
            2 => 120_000,  // 0x1D4C0
            1 => 300_000,  // 0x493E0
            _ => 30_000,
        }
    }

    #[inline]
    fn rand_inclusive(ctx: &mut AdContext<'_>, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        let span = hi - lo + 1;
        lo + (ctx.rng.rand() % span)
    }

    /// Exact 16.16 fixed-point `log2(n)` (`ord_10`, `1:2930..29B1`).
    #[inline]
    fn fix_log2(n: i64) -> i64 {
        if n <= 0 {
            return 0;
        }
        let u = n as u64;
        let bit = 63 - u.leading_zeros() as i64;
        // Extract 6 bits immediately below the leading 1-bit
        let shifted = if bit >= 6 {
            ((u >> (bit - 6)) & 0x3F) as usize
        } else {
            ((u << (6 - bit)) & 0x3F) as usize
        };
        (bit << 16) + LOG2_TABLE[shifted] as i64
    }

    /// Exact 16.16 fixed-point `sin(angle)` with period `234.0 = 0xEA0000` (`ord_11`, `1:29B2..2A75`).
    #[inline]
    fn fix_sin(angle: i64) -> i64 {
        const PERIOD: i64 = (PAL_LEN as i64) << 16; // 0x00EA_0000
        const HALF: i64 = (HALF_PAL as i64) << 16; // 0x0075_0000
        let mut a = angle.rem_euclid(PERIOD);
        let neg = a > HALF;
        if neg {
            a -= HALF;
        }
        let idx = ((a >> 16) as usize).min(HALF_PAL - 1);
        let frac = a & 0xFFFF;
        let v0 = SIN_TABLE[idx] as i64;
        let v1 = SIN_TABLE[idx + 1] as i64;
        let val = v0 + (((v1 - v0) * frac) >> 16);
        if neg { -val } else { val }
    }

    /// Exact 16.16 fixed-point `asin(x)` for `x in [0, 0x10000]` returning `[0, 0x3A8000]` (`ord_13`, `1:2A76..2AF5`).
    #[inline]
    fn fix_asin(x: i64) -> i64 {
        if x <= 0 {
            return 0;
        }
        if x >= 0x10000 {
            return ASIN_TABLE[64] as i64;
        }
        let idx = (x >> 10) as usize;
        let frac = x & 0x3FF;
        let v0 = ASIN_TABLE[idx] as i64;
        let v1 = ASIN_TABLE[idx + 1] as i64;
        v0 + (((v1 - v0) * frac) >> 10)
    }

    /// Exact 16.16 fixed-point `sqrt(n)` (`ord_7`, `1:2AF6..2BD3`), widened to 64-bit for 4K monitors.
    #[inline]
    fn fix_sqrt(n: i64) -> i64 {
        if n <= 0 {
            return 0;
        }
        ((n as f64).sqrt() * 65536.0 + 0.5) as i64
    }

    /// Exact `ord_8(num_int, den_16_16)` (`1:2BD4..2C7C`): computes `num_int / (den / 65536)` in 16.16 fixed-point.
    #[inline]
    fn fix_div_int(num: i64, den: i64) -> i64 {
        if den <= 0 {
            return 0;
        }
        let neg = num < 0;
        let a = num.unsigned_abs() as u128;
        let d = den as u128;
        let q = (((a << 33) / d) + 1) >> 1;
        let res = q as i64;
        if neg { -res } else { res }
    }

    /// Interpolates linearly between `pal[start]` and `pal[end % 234]` in steps of 2 (`1:02AA..03FA`).
    fn interpolate_band(pal: &mut [[u8; 3]; PAL_LEN], start: usize, end: usize) {
        let span = (end as i32) - (start as i32);
        if span <= 2 {
            return;
        }
        let c0 = pal[start % PAL_LEN];
        let c1 = pal[end % PAL_LEN];
        let dr = c1[0] as i32 - c0[0] as i32;
        let dg = c1[1] as i32 - c0[1] as i32;
        let db = c1[2] as i32 - c0[2] as i32;
        let mut step = 2;
        while step < span {
            let idx = (start + step as usize) % PAL_LEN;
            pal[idx] = [
                (c0[0] as i32 + (dr * step) / span).clamp(0, 255) as u8,
                (c0[1] as i32 + (dg * step) / span).clamp(0, 255) as u8,
                (c0[2] as i32 + (db * step) / span).clamp(0, 255) as u8,
            ];
            step += 2;
        }
    }

    /// Builds a 117-color looping gradient into either even (`parity=0`) or odd (`parity=1`) entries (`1:03FB..07CA`).
    fn build_half_palette(
        ctx: &mut AdContext<'_>,
        pal: &mut [[u8; 3]; PAL_LEN],
        theme_idx: usize,
        parity: usize,
    ) {
        let t = &THEMES[theme_idx % THEMES.len()];
        let n_colors = t.colors.len() as i32;

        // Fixed-step seed (used in disassembly for fixed_step themes)
        let _fixed_step_val = if t.fixed_step {
            let mut v = 8;
            for _ in 0..32 {
                v = match Self::rand_inclusive(ctx, 0, 2) {
                    0 => 8,
                    1 => 16,
                    _ => 32,
                };
                if v >= t.min_step && v <= t.max_step {
                    break;
                }
            }
            v
        } else {
            -1
        };

        let (bias_c1, bias_c2) = if t.two_color_bias && n_colors > 1 {
            (
                Self::rand_inclusive(ctx, 1, n_colors - 1),
                Self::rand_inclusive(ctx, 1, n_colors - 1),
            )
        } else {
            (0, 0)
        };

        // Pick initial anchor color (`1:0492..04EE`)
        let mut cur_color: i32 = if t.weights[0] == 0 {
            0
        } else if t.two_color_bias {
            bias_c1
        } else if t.weights[1] == 0 {
            Self::rand_inclusive(ctx, 1, (n_colors - 1).max(1))
        } else {
            Self::rand_inclusive(ctx, 0, n_colors - 1)
        };

        let mut pos: usize = 0; // in units of half-palette slots (0..117)
        pal[parity] = t.colors[cur_color as usize];

        let mut hit_end = false;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 256 {
                break;
            }
            // Pick step length (`1:054D..05BC`)
            let step: i32 = if t.pow2_steps {
                let mut s = t.min_step;
                for _ in 0..64 {
                    let cand = match Self::rand_inclusive(ctx, 0, 5) {
                        0 => 8,
                        1 | 2 => 16,
                        3 | 4 => 32,
                        _ => 64,
                    };
                    if cand >= t.min_step && cand <= t.max_step {
                        s = cand;
                        break;
                    }
                }
                s.max(1)
            } else {
                Self::rand_inclusive(ctx, t.min_step, t.max_step).max(1)
            };

            if pos + step as usize >= HALF_PAL {
                hit_end = true;
            } else {
                // Pick next color (`1:05CA..06A3`)
                let mut next_color: i32 = if cur_color == 0 {
                    let sum = t.weights[2] as i32 + t.weights[3] as i32;
                    let r = Self::rand_inclusive(ctx, 0, (sum - 1).max(0));
                    if r < t.weights[2] as i32 && pos != 0 {
                        0
                    } else {
                        -1
                    }
                } else {
                    let sum = t.weights[0] as i32 + t.weights[1] as i32;
                    let r = Self::rand_inclusive(ctx, 0, (sum - 1).max(0));
                    if r >= t.weights[0] as i32 {
                        0
                    } else {
                        -1
                    }
                };

                let mut pick_guard = 0;
                while next_color == -1 || next_color == cur_color {
                    pick_guard += 1;
                    if pick_guard > 32 {
                        next_color = (cur_color + 1) % n_colors;
                        break;
                    }
                    if t.two_color_bias && Self::rand_inclusive(ctx, 0, 1) == 0 {
                        next_color = if Self::rand_inclusive(ctx, 0, 1) == 0 {
                            bias_c1
                        } else {
                            bias_c2
                        };
                    } else {
                        next_color = Self::rand_inclusive(ctx, 1, (n_colors - 1).max(1));
                    }
                    if next_color == cur_color {
                        next_color = -1;
                    }
                }

                cur_color = next_color.clamp(0, n_colors - 1);
                let next_pos = pos + step as usize;
                pal[next_pos * 2 + parity] = t.colors[cur_color as usize];
                Self::interpolate_band(pal, pos * 2 + parity, next_pos * 2 + parity);
                pos = next_pos;
            }

            // Early wrap check (`1:0700..0730`)
            let r_wrap = Self::rand_inclusive(ctx, 0, HALF_PAL as i32);
            if r_wrap < pos as i32 && (hit_end || t.allow_early_wrap) && pos >= 0x20 {
                break;
            }
            if hit_end {
                break;
            }
        }

        // Smooth wrap from last anchor back to entry 0 (`1:0731..0744`)
        Self::interpolate_band(pal, pos * 2 + parity, PAL_LEN + parity);
    }

    fn rebuild_palette(&mut self, ctx: &mut AdContext<'_>) {
        self.pal_offset = 0;
        let theme1 = if self.color_ctrl < 13 {
            self.color_ctrl.max(0) as usize
        } else {
            Self::rand_inclusive(ctx, 0, 12) as usize
        };
        Self::build_half_palette(ctx, &mut self.master_pal, theme1, 0);

        if self.display_ctrl == 6 {
            // Mix mode: build an independent second palette in the odd entries (`1:0878..08A1`)
            let theme2 = if self.color_ctrl < 13 {
                self.color_ctrl.max(0) as usize
            } else {
                Self::rand_inclusive(ctx, 0, 12) as usize
            };
            Self::build_half_palette(ctx, &mut self.master_pal, theme2, 1);
        } else {
            // Non-mix: mirror even entries to odd entries (`1:081A..0876`)
            for k in 0..HALF_PAL {
                self.master_pal[k * 2 + 1] = self.master_pal[k * 2];
            }
        }
        self.dirty_pixels = true;
    }

    fn init_spiral_for_block(&mut self, block: i32) {
        self.block_size = block;
        let mask = !(block - 1);
        let cx = (self.width / 2) & mask;
        let cy = (self.height / 2) & mask;
        self.cur_x = cx;
        self.cur_y = cy;
        self.dir = 2;
        self.bound_left = cx - block;
        self.bound_top = cy - block;
        self.bound_right = cx + block;
        self.bound_bottom = cy + block;
    }

    fn setup_field(&mut self, ctx: &mut AdContext<'_>, first_init: bool) {
        self.display_ctrl = ctx.module.control_value[0].clamp(0, 6) + 1;
        self.color_ctrl = ctx.module.control_value[1].clamp(0, 13);
        self.end_clarity = Self::read_end_clarity(ctx.module.control_value[2]);
        let knots_val = ctx.module.control_value[3].clamp(1, MAX_KNOTS as i32);
        self.num_knots = knots_val as usize;

        // Initial 16x16 spiral (`1:0AD0..0B10`)
        self.block_size = 16;
        let cx = (self.width & !0x1E) >> 1;
        let cy = (self.height & !0x1E) >> 1;
        self.cur_x = cx;
        self.cur_y = cy;
        self.dir = 2;
        self.bound_left = cx - 16;
        self.bound_top = cy - 16;
        self.bound_right = cx + 16;
        self.bound_bottom = cy + 16;

        // Mode & quantize selection (`1:0B39..0BE7`)
        if self.display_ctrl <= 5 {
            self.field1 = self.display_ctrl;
            self.field2 = self.display_ctrl;
            let q = Self::rand_inclusive(ctx, 1, 4) == 1;
            self.quantize1 = q;
            self.quantize2 = q;
        } else {
            if first_init {
                loop {
                    let f1 = Self::rand_inclusive(ctx, 2, 5);
                    if f1 != 3 {
                        self.field1 = f1;
                        self.field2 = 3;
                        break;
                    }
                }
            } else {
                loop {
                    let f1 = Self::rand_inclusive(ctx, 1, 5);
                    let f2 = Self::rand_inclusive(ctx, 1, 5);
                    if f1 != f2 {
                        self.field1 = f1;
                        self.field2 = f2;
                        break;
                    }
                }
            }
            self.quantize1 = Self::rand_inclusive(ctx, 1, 4) == 1;
            self.quantize2 = Self::rand_inclusive(ctx, 1, 4) == 1;
        }

        // Generate 20 knots (`1:0BEA..0E2B`)
        for i in 0..MAX_KNOTS {
            let x = Self::rand_inclusive(ctx, 0, self.width.max(1));
            let y = Self::rand_inclusive(ctx, 0, self.height.max(1));
            let weight = match Self::rand_inclusive(ctx, 0, 4) {
                0 => 0x40,
                1 => 0x80,
                2 => 0xC0,
                3 => 0x100,
                _ => 0x140,
            };
            let ray_arms = match Self::rand_inclusive(ctx, 0, 2) {
                0 => 2,
                1 => 3,
                2 => 4,
                3 => 6,
                _ => 8,
            };
            let spiral_twist = match Self::rand_inclusive(ctx, 0, 5) {
                0 | 1 => 0,
                2 => 1000,
                3 => 2000,
                4 => 3000,
                _ => 5000,
            };
            let wave_freq = match Self::rand_inclusive(ctx, 0, 5) {
                0 | 1 | 2 => 0,
                3 => 2,
                4 => 4,
                _ => 8,
            };
            let wave_amp = Self::rand_inclusive(ctx, 0, 3) + 1;
            let sign = if Self::rand_inclusive(ctx, 0, 1) != 0 { -1 } else { 1 };
            self.knots[i] = Knot {
                x,
                y,
                weight,
                ray_arms,
                spiral_twist,
                wave_freq,
                wave_amp,
                sign,
            };
        }

        self.holding = false;
    }

    /// Evaluates one of the 5 Satori fields at `(x, y)` (`1:1D67..2301`).
    fn eval_field(&self, x: i32, y: i32, mode: i32, quantize: bool) -> i32 {
        let mut sum: i64 = 0;
        let n = self.num_knots.min(MAX_KNOTS);
        for i in 0..n {
            let k = &self.knots[i];
            match mode {
                1 => {
                    // Fields: polar vortex/dipole with spiral twist + optional radial sine wave (`1:206F..2155`, `1:1D74..1EC8`)
                    let mut dx = (x - k.x) as i64;
                    let mut dy = (y - k.y) as i64;
                    let mut quad = 0;
                    if dx < 0 {
                        dx = -dx;
                        quad = 1;
                    }
                    if dy < 0 {
                        dy = -dy;
                        quad += 2;
                    }
                    let r2 = dx * dx + dy * dy;
                    if r2 > 0 {
                        let d = Self::fix_sqrt(r2);
                        let mut ang = if dy >= dx {
                            0x3A8000 - Self::fix_asin(Self::fix_div_int(dx, d))
                        } else {
                            Self::fix_asin(Self::fix_div_int(dy, d))
                        };
                        ang = match quad {
                            1 => 0x750000 - ang,
                            2 => 0xEA0000 - ang,
                            3 => 0x750000 + ang,
                            _ => ang,
                        };
                        if k.sign < 0 {
                            ang = 0xEA0000 - ang;
                        }
                        ang = ang.wrapping_mul(k.ray_arms as i64);
                        let twist = Self::fix_div_int(k.spiral_twist as i64, d + (10 << 16));
                        if k.sign < 0 {
                            ang -= twist;
                        } else {
                            ang += twist;
                        }
                        if k.wave_freq != 0 {
                            let arg = (d / k.wave_freq as i64) * 40;
                            let s = Self::fix_sin(arg) * k.wave_amp as i64;
                            let ripple = (s / (r2 + 200)) * 20_000;
                            ang += ripple;
                        }
                        sum += ang;
                    }
                }
                2 => {
                    // Pools: logarithmic equipotentials (`1:1ED4..1F65`)
                    let dx = (x - k.x) as i64;
                    let dy = (y - k.y) as i64;
                    let r2 = dx * dx + dy * dy + 1;
                    let lg = -Self::fix_log2(r2);
                    let term = (lg * (k.sign * k.weight) as i64) >> 3;
                    sum += term;
                }
                3 => {
                    // Rays: starburst angular maxima (`1:1F71..204A`)
                    let ax = (x - k.x).abs() as i64;
                    let ay = (y - k.y).abs() as i64;
                    let r2 = ax * ax + ay * ay;
                    if r2 > 0 {
                        let d = Self::fix_sqrt(r2);
                        let m = ax.max(ay);
                        let term = Self::fix_div_int(m * k.weight as i64, d);
                        sum += term;
                    }
                }
                4 => {
                    // Waves: decaying circular ripples (`1:2161..2229`)
                    let ax = (x - k.x).abs() as i64;
                    let ay = (y - k.y).abs() as i64;
                    let r2 = ax * ax + ay * ay;
                    if r2 > 0 {
                        let d = Self::fix_sqrt(r2);
                        let s = Self::fix_sin(d);
                        let prod = (s * (40 * k.weight) as i64) >> 16;
                        let term = Self::fix_div_int(prod, d + 0x10000);
                        sum += term;
                    }
                }
                5 => {
                    // Leaves: 4-petal angular minima (`1:2235..22FF`)
                    let ax = (x - k.x).abs() as i64;
                    let ay = (y - k.y).abs() as i64;
                    let r2 = ax * ax + ay * ay;
                    if r2 > 0 {
                        let d = Self::fix_sqrt((r2 >> 2).max(1));
                        let m = ax.min(ay);
                        let term = Self::fix_div_int(m * k.weight as i64, d);
                        sum += term;
                    }
                }
                _ => {}
            }
            if quantize {
                // `1:204D..205B`: snap partial sum to multiple of 4 integer steps
                sum &= !0x03FFFF;
            }
        }
        // Integer part mod 234 (`1:2302..231E`)
        let int_part = (sum >> 16) as i32;
        int_part.rem_euclid(PAL_LEN as i32)
    }

    #[inline]
    fn map_even_index(raw_idx: i32) -> u8 {
        // `1:232C..239E` for field1 (even slot): (idx | 1) - 1 -> 0, 2, ..., 232 -> stored as +1
        let mut v = raw_idx | 1;
        if v == PAL_LEN as i32 {
            v = 2;
        }
        (v - 1).clamp(0, (PAL_LEN - 1) as i32) as u8
    }

    #[inline]
    fn map_odd_index(raw_idx: i32) -> u8 {
        // `1:234B..239E` for field2 (odd slot): ((idx + 2) & 0xFE) wrap 234->2, -1 -> 1, 3, ..., 233
        let mut v = (raw_idx + 2) & 0xFE;
        if v == PAL_LEN as i32 {
            v = 2;
        }
        (v - 1).clamp(0, (PAL_LEN - 1) as i32) as u8
    }

    /// Evaluates & paints the current spiral block, then advances the clockwise spiral cursor (`1:1C8D..2678`).
    /// Returns `true` if the current block_size pass just finished.
    fn step_one_block(&mut self) -> bool {
        let w = self.width;
        let h = self.height;
        let bs = self.block_size;
        let x = self.cur_x;
        let y = self.cur_y;

        if bs == 1 {
            if x >= 0 && x < w && y >= 0 && y < h {
                let is_even = ((x ^ y) & 1) != 0;
                let idx = if is_even {
                    Self::map_even_index(self.eval_field(x, y, self.field1, self.quantize1))
                } else {
                    Self::map_odd_index(self.eval_field(x, y, self.field2, self.quantize2))
                };
                self.indices[(y * w + x) as usize] = idx + 1;
                self.dirty_pixels = true;
            }
        } else if x >= 0 && y >= 0 && x < w && y < h {
            let c_even = Self::map_even_index(self.eval_field(x, y, self.field1, self.quantize1)) + 1;
            let c_odd = if self.field1 == self.field2 && self.quantize1 == self.quantize2 && self.display_ctrl != 6 {
                c_even
            } else {
                Self::map_odd_index(self.eval_field(x, y, self.field2, self.quantize2)) + 1
            };
            let x_end = (x + bs).min(w);
            let y_end = (y + bs).min(h);
            if c_even == c_odd {
                for py in y..y_end {
                    let row = (py * w) as usize;
                    for px in x..x_end {
                        self.indices[row + px as usize] = c_even;
                    }
                }
            } else {
                for py in y..y_end {
                    let row = (py * w) as usize;
                    for px in x..x_end {
                        self.indices[row + px as usize] = if ((px ^ py) & 1) != 0 { c_even } else { c_odd };
                    }
                }
            }
            self.dirty_pixels = true;
        }

        // Advance clockwise spiral cursor (`1:2402..25D5`)
        let mut finished_pass = false;
        match self.dir {
            0 => {
                self.cur_x += bs;
                if self.cur_x >= self.bound_right {
                    self.dir = 1;
                    if self.cur_x >= w
                        && -bs >= self.bound_left
                        && w + bs <= self.bound_right
                        && -bs >= self.bound_top
                        && h + bs <= self.bound_bottom
                    {
                        finished_pass = true;
                    }
                    self.bound_right += bs;
                }
            }
            1 => {
                self.cur_y += bs;
                if self.cur_y >= self.bound_bottom {
                    self.dir = 2;
                    if self.cur_y >= h
                        && -bs >= self.bound_left
                        && w + bs <= self.bound_right
                        && -bs >= self.bound_top
                        && h + bs <= self.bound_bottom
                    {
                        finished_pass = true;
                    }
                    self.bound_bottom += bs;
                }
            }
            2 => {
                self.cur_x -= bs;
                if self.cur_x <= self.bound_left {
                    self.dir = 3;
                    if self.cur_x < 0
                        && -bs >= self.bound_left
                        && w + bs <= self.bound_right
                        && -bs >= self.bound_top
                        && h + bs <= self.bound_bottom
                    {
                        finished_pass = true;
                    }
                    self.bound_left -= bs;
                }
            }
            _ => {
                self.cur_y -= bs;
                if self.cur_y <= self.bound_top {
                    self.dir = 0;
                    if self.cur_y < 0
                        && -bs >= self.bound_left
                        && w + bs <= self.bound_right
                        && -bs >= self.bound_top
                        && h + bs <= self.bound_bottom
                    {
                        finished_pass = true;
                    }
                    self.bound_top -= bs;
                }
            }
        }
        finished_pass
    }

    fn flush_indexed_surface(&mut self, ctx: &mut AdContext<'_>) {
        let mut lut = [0u32; PAL_LEN + 1];
        for i in 0..PAL_LEN {
            let [r, g, b] = self.master_pal[(i + self.pal_offset) % PAL_LEN];
            lut[i + 1] = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
        let dst = ctx.surface.pixels_mut();
        let len = dst.len().min(self.indices.len());
        for i in 0..len {
            let code = self.indices[i] as usize;
            if code != 0 {
                dst[i] = lut[code];
            }
        }
        self.dirty_pixels = false;
    }

    fn check_caps_lock_flip(&mut self, ctx: &mut AdContext<'_>) {
        let state = i16::from(ad_sdk::sys::caps_lock_on());
        if state != self.caps_lock_state {
            self.caps_lock_state = state;
            self.rebuild_palette(ctx);
        }
    }
}

impl AdModule for SatoriModule {
    fn metadata(&self) -> AdMetadata {
        Self::static_metadata()
    }

    fn do_initialize(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.width = ctx.module.rgn_width.max(1);
        self.height = ctx.module.rgn_height.max(1);
        self.indices.clear();
        self.indices.resize((self.width * self.height) as usize, 0);
        self.caps_lock_state = i16::from(ad_sdk::sys::caps_lock_on());

        ctx.surface.fill_black();
        self.setup_field(ctx, true);
        self.rebuild_palette(ctx);
        AdResult::NoError
    }

    fn do_blank(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        ctx.surface.fill_black();
        AdResult::NoError
    }

    fn do_draw_frame(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.end_clarity = Self::read_end_clarity(ctx.module.control_value[2]);
        // Live Colors change without resetting the field (`ord_20`, `1:28A2..2906`)
        let new_color = ctx.module.control_value[1].clamp(0, 13);
        if new_color != self.color_ctrl {
            self.color_ctrl = new_color;
            self.rebuild_palette(ctx);
        }
        self.check_caps_lock_flip(ctx);

        // Live color animation: advance palette ring by +2 (`1:016C..026A`)
        self.pal_offset = (self.pal_offset + 2) % PAL_LEN;

        if self.holding {
            if ctx.elapsed_ms >= self.hold_end_ms {
                self.holding = false;
                self.setup_field(ctx, false);
                self.rebuild_palette(ctx);
            }
            self.flush_indexed_surface(ctx);
            return AdResult::NoError;
        }

        // Compute adaptive block budget per 30ms frame so progressive spiral refinement
        // feels smooth and hypnotic at every screen resolution (320×240 up to 4K).
        let total_blocks = ((self.width + self.block_size - 1) / self.block_size)
            * ((self.height + self.block_size - 1) / self.block_size);
        let target_frames = match self.block_size {
            16 => 18,
            8 => 30,
            4 => 45,
            2 => 70,
            _ => 110,
        };
        let budget = (total_blocks / target_frames).clamp(16, 16_384);

        let mut done_blocks = 0;
        let mut iter_guard = 0;
        let max_iters = (budget * 6).max(2048);
        while done_blocks < budget && iter_guard < max_iters {
            iter_guard += 1;
            let on_screen = self.cur_x >= 0
                && self.cur_y >= 0
                && self.cur_x < self.width
                && self.cur_y < self.height;
            let pass_done = self.step_one_block();
            if on_screen {
                done_blocks += 1;
            }
            if pass_done {
                if self.block_size > self.end_clarity {
                    self.init_spiral_for_block(self.block_size / 2);
                } else {
                    self.holding = true;
                    self.hold_end_ms = ctx.elapsed_ms + Self::hold_duration_ms(self.end_clarity);
                }
                break;
            }
        }

        self.flush_indexed_surface(ctx);
        AdResult::NoError
    }

    fn do_button_message(&mut self, slot: usize) -> AdResult {
        // Exact `ord_20`: Colors (slot 1) & End Clarity (slot 2) update live without full reinit
        if slot == 1 || slot == 2 {
            AdResult::NoError
        } else {
            AdResult::RestartMe
        }
    }
}
