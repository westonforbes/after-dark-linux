//! *Simpsons Clocks* — 1994 *The Simpsons* After Dark 3.0 Module.
//!
//! Three animated Simpsons wall clocks showing your real system time:
//! - **Duff Brewery Cuckoo Clock** (`RLEP #19000`) — 3-stage double swinging doors,
//!   Barney, Lenny & Homer in his recliner inside the brick archways, flickering purple
//!   neon Duff sign with electric sparks, 7-frame scurrying rat, and belch chimes!
//! - **Krusty Pop-Eye Clock** (`RLEP #19100`) — forehead clock dial, roving eyeballs
//!   that spring-pop during chimes, pendulum pink tongue ticking left-center-right,
//!   and cartoon chime explosions!
//! - **Maggie Wall Clock** (`RLEP #19200` + `#30000`) — pixel-aligned blinking eyes,
//!   sucking pacifier, and toddling/tripping Maggie cameo beneath the clock!

use ad_sdk::{AdContext, AdControl, AdMetadata, AdModule, AdResult};
use ad_sdk::sys::SYSTEMTIME;

use super::{
    MOD_SIMPSONS_CLOCKS, RlepBank, SND_BARNEY_YOHO, SND_DETONATE, SND_DOOR, SND_DUFF_BELCH,
    SND_HORN_HONK, SND_KRUSTY_LAGH, SND_MAGGIE_FALL, SND_MAGGIE_SUCK, SND_NEON_SPARK, SND_RAT,
    SND_TICK, SimpsonsPack, draw_line_px, fill_circle_px, fill_rect_px, restore_or_clear,
};

pub struct SimpsonsClocksModule {
    duff_bank: RlepBank,
    krusty_bank: RlepBank,
    maggie_bank: RlepBank,
    maggie_walk_bank: RlepBank,
    saved_bg: Vec<u32>,
    clear_screen: bool,
    clock_setting: i32, // 0=Duff, 1=Krusty, 2=Maggie, 3=Random
    active_clock: i32,
    sound_mode: i32, // 0=Chimes&Ticks, 1=ChimesOnly, 2=TicksOnly, 3=Neither
    redraw_ticks: usize,
    clock_cx: i32,
    clock_cy: i32,
    last_sec: u16,
    chime_ticks: usize,
    chime_max: usize,
    chime_variant: u8,
    spark_ticks: usize,
    rat_x: f32,
    rat_dir: f32,
    rat_active: bool,
    maggie_x: f32,
    maggie_dir: f32,
    maggie_fall_step: usize, // 0 = walking, 1..=14 = trip/fall/getup sequence
    tick: usize,
    scale_f: f32,
}

impl SimpsonsClocksModule {
    pub fn new() -> Self {
        Self {
            duff_bank: RlepBank {
                frames: Vec::new(),
                palette: Vec::new(),
            },
            krusty_bank: RlepBank {
                frames: Vec::new(),
                palette: Vec::new(),
            },
            maggie_bank: RlepBank {
                frames: Vec::new(),
                palette: Vec::new(),
            },
            maggie_walk_bank: RlepBank {
                frames: Vec::new(),
                palette: Vec::new(),
            },
            saved_bg: Vec::new(),
            clear_screen: true,
            clock_setting: 3,
            active_clock: 0,
            sound_mode: 0,
            redraw_ticks: 900,
            clock_cx: 320,
            clock_cy: 240,
            last_sec: 99,
            chime_ticks: 0,
            chime_max: 28,
            chime_variant: 0,
            spark_ticks: 0,
            rat_x: 0.0,
            rat_dir: 1.0,
            rat_active: false,
            maggie_x: -60.0,
            maggie_dir: 1.0,
            maggie_fall_step: 0,
            tick: 0,
            scale_f: 1.6,
        }
    }

    fn compute_clock_scale(&self, w: i32, h: i32) -> f32 {
        let (native_w, native_h) = match self.active_clock {
            0 => (272.0_f32, 238.0_f32),
            // Krusty head (124x125) + hanging pendulum tongue (~48px below) + explosion halo
            1 => (196.0_f32, 186.0_f32),
            // Maggie clock (215x215) + toddler shelf beneath
            _ => (224.0_f32, 258.0_f32),
        };
        ((h as f32 * 0.78) / native_h)
            .min((w as f32 * 0.76) / native_w)
            .clamp(0.95, 4.4)
    }

    fn draw_backdrop(&self, surface: &mut ad_sdk::canvas::AdSurface) {
        if !self.clear_screen && self.saved_bg.len() == surface.pixels().len() {
            restore_or_clear(surface, &self.saved_bg, false, 0x000A_0E1A);
            return;
        }
        let w = surface.width;
        let h = surface.height;
        let cx = self.clock_cx as f32;
        let cy = self.clock_cy as f32;
        let rx = (w as f32 * 0.45).max(160.0);
        let ry = (h as f32 * 0.55).max(160.0);
        let (tr, tg, tb) = match self.active_clock {
            0 => (40.0, 26.0, 18.0), // Duff Brewery warm amber pub glow
            1 => (38.0, 18.0, 44.0), // Krusty purple/crimson studio wall
            _ => (18.0, 34.0, 58.0), // Maggie nursery twilight blue
        };
        let dst = surface.pixels_mut();
        for y in 0..h {
            let row = (y * w) as usize;
            let dy = (y as f32 - cy) / ry;
            let dy2 = dy * dy;
            for x in 0..w {
                let dx = (x as f32 - cx) / rx;
                let g = (1.0 - (dx * dx + dy2)).clamp(0.0, 1.0);
                let g2 = g * g;
                let r = (10.0 + tr * g2) as u32;
                let gr = (12.0 + tg * g2) as u32;
                let b = (22.0 + tb * g2) as u32;
                dst[row + x as usize] = (r << 16) | (gr << 8) | b;
            }
        }
    }

    pub fn static_metadata() -> AdMetadata {
        AdMetadata {
            id: "simpsons_clocks".to_string(),
            name: "Simpsons Clocks".to_string(),
            credit: "By Jarir Maani and Jeff Thomas.\nSoftware © 1994 Berkeley Systems, Inc.\nThe Simpsons ™ & © Twentieth Century Fox.".to_string(),
            about: "TICK TOCK TICK TOCK! Choose between the Duff Brewery Cuckoo Clock, the Krusty Pop-Eye Clock, and the Maggie Wall Clock — complete with real system time, ticks, and authentic Springfield chimes!".to_string(),
            controls: [
                AdControl::Menu {
                    title: "Clock:".to_string(),
                    default_index: 3,
                    choices: vec![
                        "Duff".into(),
                        "Krusty".into(),
                        "Maggie".into(),
                        "Random".into(),
                    ],
                },
                AdControl::LabeledSlider {
                    title: "Redraw Every:".to_string(),
                    default_raw: 30,
                    labels: vec![
                        "5 min.".into(),
                        "15 min.".into(),
                        "30 min.".into(),
                        "45 min.".into(),
                        "60 min.".into(),
                    ],
                    thresholds: vec![0, 20, 40, 60, 80],
                },
                AdControl::Menu {
                    title: "Sounds:".to_string(),
                    default_index: 0,
                    choices: vec![
                        "Chimes & Ticks".into(),
                        "Chimes Only".into(),
                        "Ticks Only".into(),
                        "Neither".into(),
                    ],
                },
                AdControl::Checkbox {
                    title: "Clear Screen First".to_string(),
                    default_checked: true,
                },
            ],
            target_frame_ms: 80,
            filename: "SIMPCLK.AD".to_string(),
        }
    }

    fn ensure_loaded(&mut self) {
        if !self.duff_bank.frames.is_empty() {
            return;
        }
        self.duff_bank = RlepBank::load(MOD_SIMPSONS_CLOCKS, 19000);
        self.krusty_bank = RlepBank::load(MOD_SIMPSONS_CLOCKS, 19100);
        self.maggie_bank = RlepBank::load(MOD_SIMPSONS_CLOCKS, 19200);
        self.maggie_walk_bank = RlepBank::load(MOD_SIMPSONS_CLOCKS, 30000);
    }

    fn draw_hands(
        &self,
        surface: &mut ad_sdk::canvas::AdSurface,
        cx: i32,
        cy: i32,
        r: i32,
        hour: u16,
        min: u16,
        sec: u16,
        hand_rgb: u32,
        sec_rgb: u32,
        thin: bool,
    ) {
        use std::f32::consts::TAU;
        let hr_ang = (((hour % 12) as f32 + (min as f32 / 60.0)) / 12.0) * TAU - TAU * 0.25;
        let mn_ang = (((min as f32) + (sec as f32 / 60.0)) / 60.0) * TAU - TAU * 0.25;
        let sc_ang = (sec as f32 / 60.0) * TAU - TAU * 0.25;

        let hr_len = (r as f32) * 0.56;
        let mn_len = (r as f32) * 0.82;
        let sc_len = (r as f32) * 0.90;

        let sf = self.scale_f;
        let hr_w = if thin {
            (1.4 * sf).round().max(2.0) as i32
        } else {
            (2.8 * sf).round().max(2.0) as i32
        };
        let mn_w = if thin {
            (1.0 * sf).round().max(1.0) as i32
        } else {
            (2.0 * sf).round().max(2.0) as i32
        };
        let sc_w = if thin {
            (0.75 * sf).round().max(1.0) as i32
        } else {
            (1.1 * sf).round().max(1.0) as i32
        };
        let hub_r = if thin {
            (1.4 * sf).round().max(2.0) as i32
        } else {
            (2.8 * sf).round().max(3.0) as i32
        };

        draw_line_px(
            surface,
            cx,
            cy,
            cx + (hr_ang.cos() * hr_len).round() as i32,
            cy + (hr_ang.sin() * hr_len).round() as i32,
            hand_rgb,
            hr_w,
        );
        draw_line_px(
            surface,
            cx,
            cy,
            cx + (mn_ang.cos() * mn_len).round() as i32,
            cy + (mn_ang.sin() * mn_len).round() as i32,
            hand_rgb,
            mn_w,
        );
        draw_line_px(
            surface,
            cx,
            cy,
            cx + (sc_ang.cos() * sc_len).round() as i32,
            cy + (sc_ang.sin() * sc_len).round() as i32,
            sec_rgb,
            sc_w,
        );
        fill_circle_px(surface, cx, cy, hub_r, sec_rgb);
    }

    fn render_clock(&self, surface: &mut ad_sdk::canvas::AdSurface, st: &SYSTEMTIME) {
        let sf = self.scale_f;
        let cx = self.clock_cx;
        let cy = self.clock_cy;
        let sc = |v: i32| (v as f32 * sf).round() as i32;

        match self.active_clock {
            0 => {
                // =========================================================================
                // DUFF BREWERY CUCKOO CLOCK (#19000)
                // Measured exact D:4 (264x228) geometry:
                //   - Orange clock dial: center (131, 56), radius 49
                //   - Left archway hole:  x=[43..93]  y=[108..183] (51x76)
                //   - Right archway hole: x=[170..220] y=[108..183] (51x76)
                //   - Double doors D:5 (27x78) -> D:6 (18x89) -> D:7 (8x90)
                // =========================================================================
                if let Some(body) = self.duff_bank.frames.get(4) {
                    let bw = sc(body.w);
                    let bh = sc(body.h);
                    let bx = cx - bw / 2;
                    let by = cy - bh / 2;

                    // 1. Dark interior backing inside both archways so transparent holes look deep
                    fill_rect_px(
                        surface,
                        bx + sc(41),
                        by + sc(106),
                        bx + sc(95),
                        by + sc(184),
                        0x001A_1014,
                    );
                    fill_rect_px(
                        surface,
                        bx + sc(168),
                        by + sc(106),
                        bx + sc(222),
                        by + sc(184),
                        0x001A_1014,
                    );

                    // Compute smooth 3-stage door swing (5=closed, 6=half-open, 7=wide-open)
                    let door_idx = if self.chime_ticks == 0 {
                        5
                    } else {
                        let elapsed = self.chime_max.saturating_sub(self.chime_ticks);
                        if elapsed < 2 || self.chime_ticks <= 2 {
                            6
                        } else {
                            7
                        }
                    };

                    // 2. Characters standing INSIDE the open brick archways (behind brick arch trim)
                    if door_idx >= 6 {
                        // Left archway: Barney waving/belching (#0..3, 38x40)
                        let b_fi = (self.tick / 3) % 4;
                        if let Some(barney) = self.duff_bank.frames.get(b_fi) {
                            barney.blit_transformed_f(
                                surface,
                                bx + sc(53),
                                by + sc(142),
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                        // Right archway: Lenny peeking (#9, 21x28) when variant == 0
                        if self.chime_variant % 2 == 0 {
                            if let Some(lenny) = self.duff_bank.frames.get(9) {
                                lenny.blit_transformed_f(
                                    surface,
                                    bx + sc(184),
                                    by + sc(154),
                                    sf,
                                    false,
                                    false,
                                    false,
                                    false,
                                );
                            }
                        }
                    }

                    // 3. Main Duff Brewery brick facade (#4, 264x228)
                    body.blit_transformed_f(surface, bx, by, sf, false, false, false, false);

                    // 4. Double wooden swinging doors hinged at left & right jambs of each arch
                    if let Some(door) = self.duff_bank.frames.get(door_idx) {
                        // Left arch: left leaf + mirrored right leaf
                        door.blit_transformed_f(
                            surface,
                            bx + sc(41),
                            by + sc(106),
                            sf,
                            false,
                            false,
                            false,
                            false,
                        );
                        door.blit_transformed_f(
                            surface,
                            bx + sc(95 - door.w),
                            by + sc(106),
                            sf,
                            true,
                            false,
                            false,
                            false,
                        );
                        // Right arch: left leaf + mirrored right leaf
                        door.blit_transformed_f(
                            surface,
                            bx + sc(168),
                            by + sc(106),
                            sf,
                            false,
                            false,
                            false,
                            false,
                        );
                        door.blit_transformed_f(
                            surface,
                            bx + sc(222 - door.w),
                            by + sc(106),
                            sf,
                            true,
                            false,
                            false,
                            false,
                        );
                    }

                    // On alternate chimes, Homer in his pink recliner (#8, 64x56) rolls out onto the right terrace!
                    if door_idx == 7 && self.chime_variant % 2 == 1 {
                        if let Some(homer_chair) = self.duff_bank.frames.get(8) {
                            let elapsed = self.chime_max.saturating_sub(self.chime_ticks);
                            let slide = (elapsed.min(6) as i32 * 2).min(12);
                            homer_chair.blit_transformed_f(
                                surface,
                                bx + sc(164),
                                by + sc(126 + slide),
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                    }

                    // 5. Purple neon "Duff" sign (#10, 79x58) mounted cleanly above the left roof slope
                    let neon_off = self.spark_ticks > 0 && (self.spark_ticks % 2 == 0);
                    if !neon_off {
                        if let Some(neon) = self.duff_bank.frames.get(10) {
                            neon.blit_transformed_f(
                                surface,
                                bx + sc(6),
                                by + sc(8),
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                    }
                    // Electric neon sparks (#18..20) when short-circuiting
                    if self.spark_ticks > 0 {
                        let sp_fi = 18 + ((6 - self.spark_ticks.min(6)) / 2).min(2);
                        if let Some(spark) = self.duff_bank.frames.get(sp_fi) {
                            spark.blit_transformed_f(
                                surface,
                                bx + sc(44 - spark.w / 2),
                                by + sc(34 - spark.h / 2),
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                    }

                    // 6. 7-frame scurrying pink rat (#11..17) across the bottom cobblestone terrace
                    if self.rat_active {
                        let r_fi = 11 + ((self.tick / 2) % 7);
                        if let Some(rat) = self.duff_bank.frames.get(r_fi) {
                            let flip_left = self.rat_dir < 0.0;
                            rat.blit_transformed_f(
                                surface,
                                bx + (self.rat_x * sf).round() as i32,
                                by + sc(204 - rat.h),
                                sf,
                                flip_left,
                                false,
                                false,
                                false,
                            );
                        }
                    }

                    // 7. Crisp clock hands centered on the exact orange Duff dial (131, 56)
                    self.draw_hands(
                        surface,
                        bx + sc(131),
                        by + sc(56),
                        sc(43),
                        st.wHour,
                        st.wMinute,
                        st.wSecond,
                        0x0010_1018,
                        0x00D8_1818,
                        false,
                    );
                }
            }
            1 => {
                // =========================================================================
                // KRUSTY POP-EYE CLOCK (#19100)
                // Measured exact K:4 (124x125) geometry:
                //   - Forehead white clock dial: center (61, 33), radius 14
                //   - Left eye socket:  center (46, 66)  (K:3 22x21 eyeball + K:5 4x4 pupil)
                //   - Right eye socket: center (77, 66)
                //   - Mouth interior:   center (62, 102) (K:13..16 swinging pendulum tongue)
                //   - Chime explosion:  K:2 -> K:1 -> K:0 behind Krusty's head + sparks K:6..8
                // =========================================================================
                if let Some(face) = self.krusty_bank.frames.get(4) {
                    let fx = cx - sc(face.w) / 2;
                    let fy = cy - sc(face.h + 36) / 2;

                    // 1. Cartoon explosion burst ONLY during chimes (not flashing every tick!)
                    if self.chime_ticks > 0 {
                        let elapsed = self.chime_max.saturating_sub(self.chime_ticks);
                        let exp_idx = if elapsed < 2 || self.chime_ticks <= 2 {
                            2 // small puff
                        } else if elapsed < 5 || self.chime_ticks <= 5 {
                            1 // medium yellow/blue burst
                        } else {
                            0 // huge bright starburst explosion
                        };
                        if let Some(burst) = self.krusty_bank.frames.get(exp_idx) {
                            burst.blit_transformed_f(
                                surface,
                                fx + sc(62 - burst.w / 2),
                                fy + sc(62 - burst.h / 2),
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                    }

                    // 2. Krusty's head & forehead watch face (#4, 124x125)
                    face.blit_transformed_f(surface, fx, fy, sf, false, false, false, false);

                    // 3. Swinging pendulum pink tongue (#13=left, #14=mid-long, #15=right, #16=mid)
                    // Synchronized smoothly to wall-clock seconds like a classic Kit-Cat pendulum!
                    let swing_phase = ((self.tick / 3) % 4) as usize;
                    let (t_fi, pupil_dx) = if self.chime_ticks > 0 {
                        const CHIME_T: [usize; 4] = [13, 17, 15, 14];
                        (CHIME_T[(self.tick / 2) % 4], 0)
                    } else {
                        match swing_phase {
                            0 => (13, -4), // tongue left, pupils look left
                            1 => (14, 0),  // tongue center-down, pupils center
                            2 => (15, 4),  // tongue right, pupils look right
                            _ => (16, 0),  // tongue center, pupils center
                        }
                    };
                    if let Some(tongue) = self.krusty_bank.frames.get(t_fi) {
                        let tx = match t_fi {
                            13 => fx + sc(62 - tongue.w + 6),
                            15 => fx + sc(62 - 6),
                            _ => fx + sc(62 - tongue.w / 2),
                        };
                        tongue.blit_transformed_f(
                            surface,
                            tx,
                            fy + sc(96),
                            sf,
                            false,
                            false,
                            false,
                            false,
                        );
                    }

                    // 4. Eyes: normal roving eyeballs (K:3 + K:5) OR spring-popping eyeballs (K:9..12) on chimes!
                    let pop_eyes = self.chime_ticks > 0 || (st.wSecond % 10 == 0);
                    if pop_eyes {
                        // Bounce smoothly through spring extension frames 9 -> 10 -> 11 -> 12 -> 11 -> 10
                        const BOING: [usize; 6] = [9, 10, 11, 12, 11, 10];
                        let eye_fi = BOING[(self.tick / 2) % BOING.len()];
                        if let Some(eye) = self.krusty_bank.frames.get(eye_fi) {
                            // Left eye pops outward-left (mirrored spring), right eye pops outward-right
                            eye.blit_transformed_f(
                                surface,
                                fx + sc(46 - eye.w + 11),
                                fy + sc(54),
                                sf,
                                true,
                                false,
                                false,
                                false,
                            );
                            eye.blit_transformed_f(
                                surface,
                                fx + sc(77 - 11),
                                fy + sc(54),
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                    } else if let Some(eyeball) = self.krusty_bank.frames.get(3) {
                        let lx = fx + sc(46 - eyeball.w / 2);
                        let rx = fx + sc(77 - eyeball.w / 2);
                        let ey = fy + sc(66 - eyeball.h / 2);
                        eyeball.blit_transformed_f(surface, lx, ey, sf, false, false, false, false);
                        eyeball.blit_transformed_f(surface, rx, ey, sf, false, false, false, false);
                        if let Some(pupil) = self.krusty_bank.frames.get(5) {
                            let py = fy + sc(67 - pupil.h / 2);
                            pupil.blit_transformed_f(
                                surface,
                                fx + sc(46 + pupil_dx - pupil.w / 2),
                                py,
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                            pupil.blit_transformed_f(
                                surface,
                                fx + sc(77 + pupil_dx - pupil.w / 2),
                                py,
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                    }

                    // Chime sparks (#6..8) around Krusty's hair during explosion
                    if self.chime_ticks > 4 && self.chime_ticks < self.chime_max.saturating_sub(2) {
                        let sp_fi = 6 + ((self.tick / 2) % 3);
                        if let Some(sp) = self.krusty_bank.frames.get(sp_fi) {
                            sp.blit_transformed_f(
                                surface,
                                fx + sc(16 - sp.w / 2),
                                fy + sc(42 - sp.h / 2),
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                            sp.blit_transformed_f(
                                surface,
                                fx + sc(108 - sp.w / 2),
                                fy + sc(42 - sp.h / 2),
                                sf,
                                true,
                                false,
                                false,
                                false,
                            );
                        }
                    }

                    // 5. Forehead white clock dial at exact center (61, 33), radius 14:
                    //    12 subtle dark hour ticks + crisp hour/minute/second hands!
                    let dial_cx = fx + sc(61);
                    let dial_cy = fy + sc(33);
                    let dot_r = (0.75 * sf).round().max(1.0) as i32;
                    for h in 0..12 {
                        let ang = (h as f32 / 12.0) * std::f32::consts::TAU
                            - std::f32::consts::FRAC_PI_2;
                        let mx = dial_cx + (ang.cos() * 11.8 * sf).round() as i32;
                        let my = dial_cy + (ang.sin() * 11.8 * sf).round() as i32;
                        fill_circle_px(surface, mx, my, dot_r, 0x0024_2430);
                    }
                    self.draw_hands(
                        surface,
                        dial_cx,
                        dial_cy,
                        sc(12),
                        st.wHour,
                        st.wMinute,
                        st.wSecond,
                        0x0014_1420,
                        0x00E0_1818,
                        true,
                    );
                }
            }
            _ => {
                // =========================================================================
                // MAGGIE WALL CLOCK (#19200) + TODDLING MAGGIE (#30000)
                // Exact pixel-matched offsets inside M:2 (215x215):
                //   - M:0/M:1 Eyes (92x46):    (61, 78)
                //   - M:3 Pacifier (38x43):    (93, 136)
                //   - Clock dial center:       (108, 108), r=80
                // =========================================================================
                if let Some(dial) = self.maggie_bank.frames.get(2) {
                    let dx = cx - sc(dial.w) / 2;
                    let dy = cy - sc(dial.h + 36) / 2;
                    dial.blit_transformed_f(surface, dx, dy, sf, false, false, false, false);

                    // Pixel-aligned blinking eyes every ~4 seconds (#19200[1] closed)
                    let blink = (self.tick % 48) >= 44;
                    if blink {
                        if let Some(closed) = self.maggie_bank.frames.get(1) {
                            closed.blit_transformed_f(
                                surface,
                                dx + sc(61),
                                dy + sc(78),
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                    }
                    // Pixel-aligned sucking pacifier pulse (#19200[3] at (93, 136))
                    let suck = ((self.tick / 3) % 4) == 0 || self.chime_ticks > 0 && (self.tick % 4 < 2);
                    if suck {
                        if let Some(pac) = self.maggie_bank.frames.get(3) {
                            pac.blit_transformed_f(
                                surface,
                                dx + sc(93),
                                dy + sc(136),
                                sf,
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                    }

                    // Crisp clock hands centered on Maggie's dial (108, 108)
                    self.draw_hands(
                        surface,
                        dx + sc(108),
                        dy + sc(108),
                        sc(82),
                        st.wHour,
                        st.wMinute,
                        st.wSecond,
                        0x0018_2458,
                        0x00E4_2424,
                        false,
                    );

                    // Authentic toddling / tripping Maggie (#30000, X:0..9) along the shelf beneath!
                    if !self.maggie_walk_bank.frames.is_empty() {
                        let m_fi = if self.maggie_fall_step > 0 {
                            // Trip -> faceplant -> push up -> stand up!
                            const FALL_SEQ: [usize; 14] = [0, 1, 2, 2, 2, 2, 2, 3, 3, 4, 4, 5, 5, 0];
                            FALL_SEQ[(self.maggie_fall_step - 1).min(FALL_SEQ.len() - 1)]
                        } else {
                            6 + ((self.tick / 2) % 4)
                        };
                        if let Some(maggie_sp) = self.maggie_walk_bank.frames.get(m_fi) {
                            let floor_y = dy + sc(dial.h + 44);
                            let mx = dx + sc(dial.w / 2) + (self.maggie_x * sf).round() as i32
                                - sc(maggie_sp.w) / 2;
                            let my = floor_y - sc(maggie_sp.h);
                            // Faces LEFT natively in X:0..9 -> flip_x when walking RIGHT!
                            maggie_sp.blit_transformed_f(
                                surface,
                                mx,
                                my,
                                sf,
                                self.maggie_dir > 0.0,
                                false,
                                false,
                                false,
                            );
                        }
                    }
                }
            }
        }
    }
}

impl Default for SimpsonsClocksModule {
    fn default() -> Self {
        Self::new()
    }
}

impl AdModule for SimpsonsClocksModule {
    fn metadata(&self) -> AdMetadata {
        Self::static_metadata()
    }

    fn do_initialize(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.ensure_loaded();
        self.saved_bg = ctx.surface.pixels().to_vec();
        self.clock_setting = ctx.module.raw_control_value[0].clamp(0, 3);
        let redraw_raw = ctx.module.raw_control_value[1].clamp(0, 100);
        self.redraw_ticks = (360 + redraw_raw * 18) as usize;
        self.sound_mode = ctx.module.raw_control_value[2].clamp(0, 3);
        self.clear_screen = ctx.module.raw_control_value[3] != 0;

        self.active_clock = if self.clock_setting == 3 {
            ctx.rng.rand_mod(3)
        } else {
            self.clock_setting
        };
        self.scale_f = self.compute_clock_scale(ctx.surface.width, ctx.surface.height);
        self.clock_cx = ctx.surface.width / 2;
        self.clock_cy = ctx.surface.height / 2;
        self.last_sec = 99;
        self.chime_max = 28;
        self.chime_ticks = 22;
        self.chime_variant = 0;
        self.spark_ticks = 0;
        self.rat_active = false;
        self.rat_x = 12.0;
        self.rat_dir = 1.0;
        self.maggie_x = -80.0;
        self.maggie_dir = 1.0;
        self.maggie_fall_step = 0;
        self.tick = 0;

        AdResult::NoError
    }

    fn do_blank(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.draw_backdrop(ctx.surface);
        let st = ad_sdk::sys::local_time();
        self.render_clock(ctx.surface, &st);
        AdResult::NoError
    }

    fn do_draw_frame(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.tick = self.tick.wrapping_add(1);
        let pack = SimpsonsPack::global();

        let st = ad_sdk::sys::local_time();

        let do_ticks = self.sound_mode == 0 || self.sound_mode == 2;
        let do_chimes = self.sound_mode == 0 || self.sound_mode == 1;

        if st.wSecond != self.last_sec {
            self.last_sec = st.wSecond;
            if do_ticks {
                let snd = if self.active_clock == 2 && (st.wSecond % 3 == 0) {
                    SND_MAGGIE_SUCK
                } else {
                    SND_TICK
                };
                pack.play_voice(ctx.sound, ctx.system.sound_enabled, snd);
            }
            // Trigger authentic animated chime every 15 seconds (and on top of the minute!)
            if st.wSecond % 15 == 0 {
                self.chime_ticks = self.chime_max;
                self.chime_variant = self.chime_variant.wrapping_add(1);
                if do_chimes {
                    let chime = match self.active_clock {
                        0 => {
                            if self.chime_variant % 2 == 0 {
                                SND_DUFF_BELCH
                            } else {
                                SND_BARNEY_YOHO
                            }
                        }
                        1 => {
                            if self.chime_variant % 2 == 0 {
                                SND_DETONATE
                            } else if ctx.rng.rand_mod(2) == 0 {
                                SND_KRUSTY_LAGH
                            } else {
                                SND_HORN_HONK
                            }
                        }
                        _ => SND_MAGGIE_SUCK,
                    };
                    pack.play_voice(ctx.sound, ctx.system.sound_enabled, chime);
                }
            }
        }

        if self.chime_ticks > 0 {
            if self.active_clock == 0 && (self.chime_ticks == self.chime_max || self.chime_ticks == 2) && do_chimes {
                pack.play_voice(ctx.sound, ctx.system.sound_enabled, SND_DOOR);
            }
            self.chime_ticks -= 1;
        }
        if self.spark_ticks > 0 {
            self.spark_ticks -= 1;
        }

        // Duff rat scurry (alternating directions!) & neon spark short-circuit
        if self.active_clock == 0 {
            if !self.rat_active && self.tick % 95 == 28 {
                self.rat_active = true;
                if ctx.rng.rand_mod(2) == 0 {
                    self.rat_dir = 1.0;
                    self.rat_x = 10.0;
                } else {
                    self.rat_dir = -1.0;
                    self.rat_x = 198.0;
                }
                if do_chimes {
                    pack.play_voice(ctx.sound, ctx.system.sound_enabled, SND_RAT);
                }
            }
            if self.rat_active {
                self.rat_x += self.rat_dir * 4.2;
                if self.rat_x < -4.0 || self.rat_x > 214.0 {
                    self.rat_active = false;
                }
            }
            if self.tick % 110 == 55 {
                self.spark_ticks = 6;
                if do_chimes {
                    pack.play_voice(ctx.sound, ctx.system.sound_enabled, SND_NEON_SPARK);
                }
            }
        }

        // Maggie toddler walk & comical faceplant beneath the Maggie Wall Clock
        if self.active_clock == 2 {
            if self.maggie_fall_step > 0 {
                if self.tick % 2 == 0 {
                    self.maggie_fall_step += 1;
                    if self.maggie_fall_step > 14 {
                        self.maggie_fall_step = 0;
                    }
                }
            } else {
                self.maggie_x += self.maggie_dir * 2.1;
                if self.maggie_x > 88.0 {
                    self.maggie_x = 88.0;
                    self.maggie_dir = -1.0;
                } else if self.maggie_x < -88.0 {
                    self.maggie_x = -88.0;
                    self.maggie_dir = 1.0;
                }
                if self.tick % 85 == 42 && self.maggie_x.abs() < 60.0 {
                    self.maggie_fall_step = 1;
                    if do_chimes {
                        pack.play_voice(ctx.sound, ctx.system.sound_enabled, SND_MAGGIE_FALL);
                    }
                }
            }
        }

        // Periodically reposition / rotate clock (preventing burn-in, just like original!)
        if self.tick % self.redraw_ticks == 0 {
            if self.clock_setting == 3 {
                self.active_clock = (self.active_clock + 1) % 3;
                self.scale_f = self.compute_clock_scale(ctx.surface.width, ctx.surface.height);
            }
            let pad_x = ((140.0 * self.scale_f) as i32).min(ctx.surface.width / 3);
            let pad_y = ((120.0 * self.scale_f) as i32).min(ctx.surface.height / 3);
            self.clock_cx =
                pad_x + ctx.rng.rand_mod((ctx.surface.width - pad_x * 2).max(1));
            self.clock_cy =
                pad_y + ctx.rng.rand_mod((ctx.surface.height - pad_y * 2).max(1));
        }

        self.draw_backdrop(ctx.surface);
        self.render_clock(ctx.surface, &st);
        AdResult::NoError
    }

    fn do_close(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        ctx.sound.stop_all();
        self.saved_bg.clear();
        AdResult::NoError
    }
}
