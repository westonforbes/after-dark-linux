//! Exact 1:1 decompilation of After Dark 1.0a's **Clocks** module
//! (`CLOCKS.AD`, `SEG 1`, `0x0000..0x0DEB`, by Bill Stewart & Jim Stewart, ©1990 Berkeley Systems Inc.).

use ad_sdk::canvas::{AdBitmap, AdSurface, ROP_SRCCOPY};
use ad_sdk::palette::rgb;
use ad_sdk::sound::Tone;
use ad_sdk::{AdContext, AdControl, AdMetadata, AdModule, AdResult, SaverMode};

// Exact original bitmaps extracted from CLOCKS.AD (RT_BITMAP #1..#14)
const BMP_MODERN: &[u8] = include_bytes!("../../../assets/clocks/RT_BITMAP_#1.bmp");
const BMP_ANTIQUE: &[u8] = include_bytes!("../../../assets/clocks/RT_BITMAP_#2.bmp");
const BMP_BWMODERN: &[u8] = include_bytes!("../../../assets/clocks/RT_BITMAP_#3.bmp");
const BMP_BWANTIQU: &[u8] = include_bytes!("../../../assets/clocks/RT_BITMAP_#4.bmp");
const BMP_DIGITS: [&[u8]; 10] = [
    include_bytes!("../../../assets/clocks/RT_BITMAP_#5.bmp"),  // ZERO
    include_bytes!("../../../assets/clocks/RT_BITMAP_#6.bmp"),  // ONE
    include_bytes!("../../../assets/clocks/RT_BITMAP_#7.bmp"),  // TWO
    include_bytes!("../../../assets/clocks/RT_BITMAP_#8.bmp"),  // THREE
    include_bytes!("../../../assets/clocks/RT_BITMAP_#9.bmp"),  // FOUR
    include_bytes!("../../../assets/clocks/RT_BITMAP_#10.bmp"), // FIVE
    include_bytes!("../../../assets/clocks/RT_BITMAP_#11.bmp"), // SIX
    include_bytes!("../../../assets/clocks/RT_BITMAP_#12.bmp"), // SEVEN
    include_bytes!("../../../assets/clocks/RT_BITMAP_#13.bmp"), // EIGHT
    include_bytes!("../../../assets/clocks/RT_BITMAP_#14.bmp"), // NINE
];

// Exact hand-tuned integer sine/cosine tables from CLOCKS.AD DS:001C..0075
const ANT_SIN: [i32; 15] = [0, 3, 5, 8, 11, 14, 17, 18, 20, 22, 24, 25, 26, 27, 28];
const ANT_COS: [i32; 15] = [28, 28, 27, 26, 25, 24, 22, 20, 18, 17, 14, 11, 8, 5, 3];

const MOD_SIN: [i32; 15] = [0, 3, 9, 12, 17, 20, 23, 27, 30, 32, 34, 36, 38, 39, 40];
const MOD_COS: [i32; 15] = [40, 40, 39, 38, 36, 34, 32, 30, 27, 23, 20, 17, 12, 9, 3];

const HOUR_SIN: [i32; 15] = [0, 2, 4, 6, 8, 10, 12, 13, 15, 16, 17, 18, 19, 20, 20];
const HOUR_COS: [i32; 15] = [20, 20, 20, 19, 18, 17, 16, 15, 13, 12, 10, 8, 6, 4, 2];

struct ClockArt {
    modern: AdBitmap,
    antique: AdBitmap,
    digits: [AdBitmap; 10],
}

impl ClockArt {
    fn load(monochrome: bool) -> Option<Self> {
        let modern = AdBitmap::from_bmp_bytes(if monochrome { BMP_BWMODERN } else { BMP_MODERN })?;
        let antique =
            AdBitmap::from_bmp_bytes(if monochrome { BMP_BWANTIQU } else { BMP_ANTIQUE })?;
        let mut digits_vec = Vec::with_capacity(10);
        for d in BMP_DIGITS {
            digits_vec.push(AdBitmap::from_bmp_bytes(d)?);
        }
        let digits: [AdBitmap; 10] = digits_vec.try_into().ok()?;
        Some(Self {
            modern,
            antique,
            digits,
        })
    }
}

pub struct ClocksModule {
    clock_type: i32,
    show_seconds: bool,
    step_x: i32,
    step_y: i32,
    dir_x: bool,
    dir_y: bool,
    move_delay: i32,
    move_counter: i32,
    pos_x: i32,
    pos_y: i32,
    clock_w: i32,
    clock_h: i32,
    disp_w: i32,
    disp_h: i32,
    scale_f: f32,
    last_second: i32,
    art: Option<ClockArt>,
    offscreen: Option<AdSurface>,
}

impl ClocksModule {
    pub fn new() -> Self {
        Self {
            clock_type: 2,
            show_seconds: true,
            step_x: 2,
            step_y: 1, // Permanently 1 in CLOCKS.AD DS:0016
            dir_x: true,
            dir_y: true,
            move_delay: 0,
            move_counter: 0,
            pos_x: 0,
            pos_y: 0,
            clock_w: 324,
            clock_h: 76,
            disp_w: 324,
            disp_h: 76,
            scale_f: 1.0,
            last_second: 61,
            art: None,
            offscreen: None,
        }
    }

    pub fn static_metadata() -> AdMetadata {
        AdMetadata {
            id: "clocks".to_string(),
            filename: "CLOCKS.AD".to_string(),
            name: "Clocks".to_string(),
            credit: "Clocks by Bill Stewart & Jim Stewart.\r\n  ©1990 Berkeley Systems Inc."
                .to_string(),
            about: "CLOCKS (tm)\r\n\r\nBased on Clock by Rob Vaterlaus.\r\n\r\nYou can choose from 3 different styles of clocks: Modern, Antique, or Digital.\r\n\r\nYou can adjust the floating speed, toggle seconds display, and enable authentic ticking and hourly chime sounds."
                .to_string(),
            controls: [
                AdControl::Menu {
                    title: "Type:".to_string(),
                    default_index: 2, // Digital (matches CLOCKS.AD TYPE_1000 #1)
                    choices: vec!["Modern".into(), "Antique".into(), "Digital".into()],
                },
                AdControl::LabeledSlider {
                    title: "Float Speed:".to_string(),
                    default_raw: 50,
                    labels: vec![
                        "Slowest".into(),
                        "Slow".into(),
                        "Medium".into(),
                        "Fast".into(),
                        "Fastest".into(),
                    ],
                    thresholds: vec![20, 40, 60, 80, 100],
                },
                AdControl::Checkbox {
                    title: "Show Seconds".to_string(),
                    default_checked: true,
                },
                AdControl::Checkbox {
                    title: "Sound".to_string(),
                    default_checked: false,
                },
            ],
            // ~30-40 fps makes Medium float speed (~1px/2 frames = ~15-20 px/sec) match 1991 386 speed
            target_frame_ms: 28,
        }
    }

    fn apply_float_speed(&mut self, speed_val: i32) {
        // Exact CLOCKS.AD switch (1:002E..1:00C3)
        match speed_val {
            0 => {
                self.step_x = 1;
                self.move_delay = 200;
            }
            20 => {
                self.step_x = 1;
                self.move_delay = 50;
            }
            40 => {
                self.step_x = 1;
                self.move_delay = 1;
            }
            60 => {
                self.step_x = 2;
                self.move_delay = 1;
            }
            80 | 100 => {
                self.step_x = 3;
                self.move_delay = 1;
            }
            _ => {}
        }
    }

    fn local_hms() -> (i32, i32, i32) {
        let st = ad_sdk::sys::local_time();
        (st.wHour as i32, st.wMinute as i32, st.wSecond as i32)
    }

    fn draw_hand(
        offscreen: &mut AdSurface,
        cx: i32,
        cy: i32,
        quadrant: i32,
        tick: usize,
        sin_tbl: &[i32; 15],
        cos_tbl: &[i32; 15],
        width: i32,
    ) {
        let s = sin_tbl[tick];
        let c = cos_tbl[tick];
        let (ex, ey) = match quadrant {
            0 => (cx + s, cy - c),
            1 => (cx + c, cy + s),
            2 => (cx - s, cy + c),
            _ => (cx - c, cy - s),
        };
        offscreen.line(cx, cy, ex, ey, width, rgb(0, 0, 0));
    }

    fn do_sound(&mut self, ctx: &mut AdContext<'_>, cur_min: i32, cur_sec: i32) {
        if ctx.system.sound_enabled
            && ctx.module.control_value[3] != 0
            && cur_sec != self.last_second
        {
            if cur_sec == 0 && cur_min == 0 {
                // Top of hour (XX:00:00): 3 consecutive system chimes
                ctx.sound.play_tones(vec![
                    Tone {
                        freq_hz: 880.0,
                        duration_ms: 110,
                        volume: 0.22,
                    },
                    Tone {
                        freq_hz: 0.0,
                        duration_ms: 70,
                        volume: 0.0,
                    },
                    Tone {
                        freq_hz: 880.0,
                        duration_ms: 110,
                        volume: 0.22,
                    },
                    Tone {
                        freq_hz: 0.0,
                        duration_ms: 70,
                        volume: 0.0,
                    },
                    Tone {
                        freq_hz: 880.0,
                        duration_ms: 150,
                        volume: 0.22,
                    },
                ]);
            } else {
                // Exact SetVoiceNote(1, 80, 1000, 10): G7 (~3136 Hz) crisp ~8ms PC-speaker tick
                ctx.sound.play_tones(vec![Tone {
                    freq_hz: 3135.96,
                    duration_ms: 8,
                    volume: 0.14,
                }]);
            }
        }
        self.last_second = cur_sec;
    }

    fn draw_digital_clock(&mut self, ctx: &mut AdContext<'_>) {
        let (raw_hour, min, sec) = Self::local_hms();
        let mut hour = raw_hour;
        if hour > 12 {
            hour -= 12;
        }
        if hour == 0 {
            hour = 12;
        }

        let Some(art) = self.art.as_ref() else {
            return;
        };
        let Some(offscreen) = self.offscreen.as_mut() else {
            return;
        };

        offscreen.fill_black();
        offscreen.set_text_color(rgb(0, 0, 0));
        offscreen.set_bk_color(rgb(255, 255, 255));

        // Hour tens digit (blank for 1..9; '1' at (3, 3) for 10..12)
        if hour > 9 {
            let d1 = &art.digits[1];
            offscreen.bit_blt_bitmap(3, 3, d1.width, d1.height, d1, 0, 0, ROP_SRCCOPY);
            hour -= 10;
        }

        // Hour ones — centered in 47px slot at x=34
        let dh = &art.digits[(hour as usize) % 10];
        offscreen.bit_blt_bitmap(
            34 + (47 - dh.width) / 2,
            3,
            dh.width,
            dh.height,
            dh,
            0,
            0,
            ROP_SRCCOPY,
        );

        // Colon 1: 9x9 white circles with black border at [87..96) x [23..32) and [53..62)
        let white = Some(rgb(255, 255, 255));
        let black = Some(rgb(0, 0, 0));
        offscreen.ellipse(87, 23, 96, 32, black, white);
        offscreen.ellipse(87, 53, 96, 62, black, white);

        // Minute tens — x=100
        let dm1 = &art.digits[(min / 10) as usize % 10];
        offscreen.bit_blt_bitmap(
            100 + (47 - dm1.width) / 2,
            3,
            dm1.width,
            dm1.height,
            dm1,
            0,
            0,
            ROP_SRCCOPY,
        );

        // Minute ones — x=154
        let dm2 = &art.digits[(min % 10) as usize % 10];
        offscreen.bit_blt_bitmap(
            154 + (47 - dm2.width) / 2,
            3,
            dm2.width,
            dm2.height,
            dm2,
            0,
            0,
            ROP_SRCCOPY,
        );

        // Colon 2 + Seconds (clipped naturally when offscreen width == 205!)
        if self.show_seconds {
            offscreen.ellipse(207, 23, 216, 32, black, white);
            offscreen.ellipse(207, 53, 216, 62, black, white);

            let ds1 = &art.digits[(sec / 10) as usize % 10];
            offscreen.bit_blt_bitmap(
                220 + (47 - ds1.width) / 2,
                3,
                ds1.width,
                ds1.height,
                ds1,
                0,
                0,
                ROP_SRCCOPY,
            );

            let ds2 = &art.digits[(sec % 10) as usize % 10];
            offscreen.bit_blt_bitmap(
                274 + (47 - ds2.width) / 2,
                3,
                ds2.width,
                ds2.height,
                ds2,
                0,
                0,
                ROP_SRCCOPY,
            );
        }

        // Blit to screen BEFORE moving (exact Digital order, scaled cleanly for HD/4K)
        let prev_x = self.pos_x;
        let prev_y = self.pos_y;
        if self.disp_w == self.clock_w && self.disp_h == self.clock_h {
            ctx.surface.bit_blt_surface(
                self.pos_x,
                self.pos_y,
                self.clock_w,
                self.clock_h,
                offscreen,
                0,
                0,
                ROP_SRCCOPY,
            );
        } else {
            ctx.surface.stretch_blt_surface(
                self.pos_x,
                self.pos_y,
                self.disp_w,
                self.disp_h,
                offscreen,
                0,
                0,
                self.clock_w,
                self.clock_h,
                ROP_SRCCOPY,
            );
        }

        self.do_sound(ctx, min, sec);

        // Floating motion & bounce (erase previous border strip cleanly at HD/4K scales)
        if self.move_counter > self.move_delay {
            self.move_counter = 0;
            let sp = self.scale_f.round().max(1.0) as i32;
            let sx = self.step_x * sp;
            let sy = self.step_y * sp;
            let max_x = (ctx.module.rgn_width - self.disp_w).max(0);
            let max_y = (ctx.module.rgn_height - self.disp_h).max(0);
            if self.dir_x {
                self.pos_x += sx;
                if self.pos_x >= max_x {
                    self.pos_x = max_x;
                    self.dir_x = false;
                }
            } else {
                self.pos_x -= sx;
                if self.pos_x <= 0 {
                    self.pos_x = 0;
                    self.dir_x = true;
                }
            }
            if self.dir_y {
                self.pos_y += sy;
                if self.pos_y >= max_y {
                    self.pos_y = max_y;
                    self.dir_y = false;
                }
            } else {
                self.pos_y -= sy;
                if self.pos_y <= 0 {
                    self.pos_y = 0;
                    self.dir_y = true;
                }
            }
            if self.pos_x > prev_x {
                ctx.surface.pat_blt_black(prev_x, prev_y, self.pos_x - prev_x, self.disp_h);
            } else if self.pos_x < prev_x {
                ctx.surface.pat_blt_black(self.pos_x + self.disp_w, prev_y, prev_x - self.pos_x, self.disp_h);
            }
            if self.pos_y > prev_y {
                ctx.surface.pat_blt_black(prev_x, prev_y, self.disp_w, self.pos_y - prev_y);
            } else if self.pos_y < prev_y {
                ctx.surface.pat_blt_black(prev_x, self.pos_y + self.disp_h, self.disp_w, prev_y - self.pos_y);
            }
        }
    }

    fn draw_analog_clock(&mut self, ctx: &mut AdContext<'_>) {
        let (raw_hour, min, sec) = Self::local_hms();

        let (face_w, face_h) = {
            let Some(art) = self.art.as_ref() else {
                return;
            };
            let face = if self.clock_type == 0 {
                &art.modern
            } else {
                &art.antique
            };
            let Some(offscreen) = self.offscreen.as_mut() else {
                return;
            };
            offscreen.set_text_color(rgb(0, 0, 0));
            offscreen.set_bk_color(rgb(255, 255, 255));
            offscreen.bit_blt_bitmap(2, 2, face.width, face.height, face, 0, 0, ROP_SRCCOPY);
            (face.width, face.height)
        };

        // Floating motion & bounce BEFORE blitting (exact Analog order)
        let prev_x = self.pos_x;
        let prev_y = self.pos_y;
        if self.move_counter > self.move_delay {
            self.move_counter = 0;
            let sp = self.scale_f.round().max(1.0) as i32;
            let sx = self.step_x * sp;
            let sy = self.step_y * sp;
            let max_x = (ctx.module.rgn_width - self.disp_w).max(0);
            let max_y = (ctx.module.rgn_height - self.disp_h).max(0);
            if self.dir_x {
                self.pos_x += sx;
                if self.pos_x >= max_x {
                    self.pos_x = max_x;
                    self.dir_x = false;
                }
            } else {
                self.pos_x -= sx;
                if self.pos_x <= 0 {
                    self.pos_x = 0;
                    self.dir_x = true;
                }
            }
            if self.dir_y {
                self.pos_y += sy;
                if self.pos_y >= max_y {
                    self.pos_y = max_y;
                    self.dir_y = false;
                }
            } else {
                self.pos_y -= sy;
                if self.pos_y <= 0 {
                    self.pos_y = 0;
                    self.dir_y = true;
                }
            }
            if self.pos_x > prev_x {
                ctx.surface.pat_blt_black(prev_x, prev_y, self.pos_x - prev_x, self.disp_h);
            } else if self.pos_x < prev_x {
                ctx.surface.pat_blt_black(self.pos_x + self.disp_w, prev_y, prev_x - self.pos_x, self.disp_h);
            }
            if self.pos_y > prev_y {
                ctx.surface.pat_blt_black(prev_x, prev_y, self.disp_w, self.pos_y - prev_y);
            } else if self.pos_y < prev_y {
                ctx.surface.pat_blt_black(prev_x, self.pos_y + self.disp_h, self.disp_w, prev_y - self.pos_y);
            }
        }

        let (cx, cy) = if self.clock_type == 0 {
            (self.clock_w / 2, self.clock_h / 2) // (66, 66)
        } else {
            (face_w / 2, face_h / 2 - 10) // (64, 54)
        };

        if let Some(offscreen) = self.offscreen.as_mut() {
            // Hour hand (2px black, radius 20)
            let mut h12 = raw_hour;
            if h12 > 12 {
                h12 -= 12;
            }
            if h12 == 12 {
                h12 = 0;
            }
            let h_quad = h12 / 3;
            let h_tick = (((5 * h12) % 15) + (min / 12)) as usize;
            Self::draw_hand(offscreen, cx, cy, h_quad, h_tick, &HOUR_SIN, &HOUR_COS, 2);

            // Minute hand (2px black; radius 40 Modern, 28 Antique)
            let m_tick = (min % 15) as usize;
            let m_quad = min / 15;
            if self.clock_type == 0 {
                Self::draw_hand(offscreen, cx, cy, m_quad, m_tick, &MOD_SIN, &MOD_COS, 2);
            } else {
                Self::draw_hand(offscreen, cx, cy, m_quad, m_tick, &ANT_SIN, &ANT_COS, 2);
            }

            // Second hand (1px solid black — authentic 1991 CLOCKS.AD!)
            if self.show_seconds {
                let s_tick = (sec % 15) as usize;
                let s_quad = sec / 15;
                if self.clock_type == 0 {
                    Self::draw_hand(offscreen, cx, cy, s_quad, s_tick, &MOD_SIN, &MOD_COS, 1);
                } else {
                    Self::draw_hand(offscreen, cx, cy, s_quad, s_tick, &ANT_SIN, &ANT_COS, 1);
                }
            }

            if self.disp_w == face_w + 4 && self.disp_h == face_h + 4 {
                ctx.surface.bit_blt_surface(
                    self.pos_x,
                    self.pos_y,
                    face_w + 4,
                    face_h + 4,
                    offscreen,
                    0,
                    0,
                    ROP_SRCCOPY,
                );
            } else {
                ctx.surface.stretch_blt_surface(
                    self.pos_x,
                    self.pos_y,
                    self.disp_w,
                    self.disp_h,
                    offscreen,
                    0,
                    0,
                    face_w + 4,
                    face_h + 4,
                    ROP_SRCCOPY,
                );
            }
        }

        self.do_sound(ctx, min, sec);
    }
}

impl Default for ClocksModule {
    fn default() -> Self {
        Self::new()
    }
}

impl AdModule for ClocksModule {
    fn metadata(&self) -> AdMetadata {
        Self::static_metadata()
    }

    fn do_initialize(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.clock_type = ctx.module.control_value[0].clamp(0, 2);
        self.show_seconds = ctx.module.control_value[2] != 0;
        self.apply_float_speed(ctx.module.control_value[1]);
        self.move_counter = 2000;

        let mono = ctx.system.bits_per_pixel == 1;
        let Some(art) = ClockArt::load(mono) else {
            return AdResult::MemError;
        };

        if self.clock_type < 2 {
            let face = if self.clock_type == 0 {
                &art.modern
            } else {
                &art.antique
            };
            self.clock_w = face.width + 4;
            self.clock_h = face.height + 4;
        } else {
            self.clock_w = if self.show_seconds { 324 } else { 205 };
            self.clock_h = 76;
        }

        // Scale clock smoothly for modern HD/4K screens (clamping so digital clock fits narrow preview windows!)
        let base_sf = (ctx.module.rgn_height as f32 / 480.0).clamp(1.0, 4.0);
        let fit_w = (ctx.module.rgn_width as f32 * 0.88) / self.clock_w.max(1) as f32;
        let fit_h = (ctx.module.rgn_height as f32 * 0.85) / self.clock_h.max(1) as f32;
        self.scale_f = base_sf.min(fit_w).min(fit_h).max(0.5);
        self.disp_w = (self.clock_w as f32 * self.scale_f).round().max(16.0) as i32;
        self.disp_h = (self.clock_h as f32 * self.scale_f).round().max(16.0) as i32;

        // Clamp starting position inside viewport in case monitor/demo size changed
        let max_x = (ctx.module.rgn_width - self.disp_w).max(0);
        let max_y = (ctx.module.rgn_height - self.disp_h).max(0);
        self.pos_x = self.pos_x.clamp(0, max_x);
        self.pos_y = self.pos_y.clamp(0, max_y);

        let Some(mut offscreen) = AdSurface::new(self.clock_w, self.clock_h) else {
            return AdResult::MemError;
        };
        offscreen.fill_black();

        self.art = Some(art);
        self.offscreen = Some(offscreen);
        self.last_second = 61;
        AdResult::NoError
    }

    fn do_blank(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        ctx.surface.fill_black();
        AdResult::NoError
    }

    fn do_draw_frame(&mut self, ctx: &mut AdContext<'_>) -> AdResult {
        self.move_counter += 1;
        if self.clock_type < 2 {
            self.draw_analog_clock(ctx);
        } else {
            self.draw_digital_clock(ctx);
        }
        AdResult::NoError
    }

    fn do_close(&mut self, _ctx: &mut AdContext<'_>) -> AdResult {
        self.art = None;
        self.offscreen = None;
        AdResult::NoError
    }
}

impl ClocksModule {
    pub fn on_live_control_change(
        &mut self,
        slot: usize,
        mapped_val: i32,
        mode: SaverMode,
    ) -> bool {
        if mode != SaverMode::Demo {
            return false;
        }
        match slot {
            0 => true, // Type changed -> RESTART_ME
            1 => {
                self.move_counter = 2000;
                self.apply_float_speed(mapped_val);
                false
            }
            2 => {
                if self.clock_type > 1 {
                    true // Digital requires offscreen resize
                } else {
                    self.show_seconds = mapped_val != 0;
                    false
                }
            }
            _ => false,
        }
    }
}
