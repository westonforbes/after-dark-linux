//! Fullscreen multi-monitor saver runtime and live Demo engine.
//!
//! - Per-monitor independent instances, a different module per monitor, a single canvas spanned
//!   across every monitor, or the primary monitor only.
//! - Logical-resolution rendering with crisp nearest-neighbor scaling (`Auto`/`1x`/`2x`/`3x`), the
//!   pillarboxed 640x480 classic stage, or CRT scanlines.
//! - Authentic 1991 frame pacing (`target_frame_ms`) or the smooth/high-refresh modes.
//! - Saver mode wakes on any key/click or a mouse move beyond 12 px (after a 500 ms grace period);
//!   holding Shift when it starts blanks to solid black instead (1991 `README.TXT`).
//! - Demo mode (from the Control Panel) exits on Escape/click and applies control changes live.
//! - `--window-id` renders into an existing X11 window for xscreensaver.

use std::num::NonZeroU32;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ad_modules::ModuleCatalogEntry;
use ad_sdk::canvas::{AdSurface, set_desktop_snapshot};
use ad_sdk::palette::AdPalette;
use ad_sdk::prefs::{AdPrefs, FrameRateMode, MultiMonitorMode, ScaleMode};
use ad_sdk::sound::AdSound;
use ad_sdk::{AdContext, AdModule, AdModuleParams, AdResult, AdSystem, MsvcRand, SaverMode};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::monitor::MonitorHandle;
use winit::window::{Fullscreen, Window, WindowId, WindowLevel};

use crate::common::{self, AppData};
use crate::platform::{self, X11};

#[derive(Debug, Default, Clone)]
pub struct RunOptions {
    pub demo: bool,
    pub module_id: Option<String>,
    pub window_id: Option<u32>,
}

const WAKE_GRACE_KEYS: Duration = Duration::from_millis(300);
const WAKE_GRACE_MOUSE: Duration = Duration::from_millis(500);
const MOUSE_JITTER_PX: f64 = 12.0;

// =============================================================================================
// Render core

/// One running module on one logical canvas.
struct Instance {
    surface: AdSurface,
    crt_scratch: Option<AdSurface>,
    scale_mode: ScaleMode,
    module: Box<dyn AdModule>,
    system: AdSystem,
    params: AdModuleParams,
    rng: MsvcRand,
    sound: AdSound,
    started_at: Instant,
    last_frame_at: Instant,
    frame_interval: Duration,
    solid_black_only: bool,
    /// Virtual-screen rectangle captured as this canvas' "desktop".
    desktop_rect: (i32, i32, i32, i32),
}

enum Step {
    Idle,
    Drew,
    WakeUp,
}

impl Instance {
    #[allow(clippy::too_many_arguments)]
    fn new(
        idx: usize,
        logical: (i32, i32),
        desktop_rect: (i32, i32, i32, i32),
        mode: SaverMode,
        prefs: &AdPrefs,
        module: Box<dyn AdModule>,
        raw_controls: [i32; 4],
        sound: &AdSound,
        sound_enabled: bool,
        solid_black_only: bool,
    ) -> Option<Self> {
        let (lw, lh) = logical;
        let mut surface = AdSurface::new(lw, lh)?;
        let crt_scratch = (prefs.scale_mode == ScaleMode::CrtScanlines)
            .then(|| AdSurface::new(lw, lh))
            .flatten();
        let (dx, dy, dw, dh) = desktop_rect;
        surface.capture_desktop(dx, dy, dw, dh);

        let meta = module.metadata();
        let mut system = AdSystem::new(lw, lh, mode);
        system.sound_enabled = sound_enabled;
        system.authentic_timing = prefs.frame_rate_mode == FrameRateMode::AuthenticRetro;
        system.data_dir = prefs.path.clone();
        system.logo_file = prefs.logo_file.clone();
        system.globe_file = prefs.globe_file.clone();
        system.slide_show_dir = prefs.slide_show_dir.clone();
        let mut params = AdModuleParams::new(lw, lh, &meta.controls);
        for (slot, &raw) in raw_controls.iter().enumerate() {
            params.set_raw_value(slot, raw, &meta.controls);
        }
        let mut rng = MsvcRand::from_system_time();
        // Salt per monitor so independent monitors don't look cloned.
        for _ in 0..(idx * 17) {
            rng.rand();
        }
        let now = Instant::now();
        let mut inst = Self {
            surface,
            crt_scratch,
            scale_mode: prefs.scale_mode,
            module,
            system,
            params,
            rng,
            sound: sound.clone(),
            started_at: now,
            last_frame_at: now,
            frame_interval: Duration::from_millis(
                prefs.frame_rate_mode.resolve_frame_ms(meta.target_frame_ms),
            ),
            solid_black_only,
            desktop_rect,
        };
        if solid_black_only {
            inst.surface.fill_black();
        } else {
            inst.with_ctx(|m, ctx| {
                let _ = m.do_pre_initialize(ctx);
                if let AdResult::PaletteRequest(kind) = m.do_initialize(ctx) {
                    ctx.module.palette = Some(AdPalette::build(kind));
                }
                if let AdResult::PaletteRequest(kind) = m.do_blank(ctx) {
                    ctx.module.palette = Some(AdPalette::build(kind));
                }
            });
        }
        Some(inst)
    }

    fn with_ctx<R>(&mut self, f: impl FnOnce(&mut dyn AdModule, &mut AdContext<'_>) -> R) -> R {
        let elapsed_ms = self.started_at.elapsed().as_millis() as u64;
        let mut ctx = AdContext {
            system: &mut self.system,
            module: &mut self.params,
            surface: &mut self.surface,
            sound: &self.sound,
            rng: &mut self.rng,
            elapsed_ms,
        };
        f(self.module.as_mut(), &mut ctx)
    }

    fn next_due(&self) -> Instant {
        self.last_frame_at + self.frame_interval
    }

    fn step(&mut self) -> Step {
        if self.solid_black_only || Instant::now() < self.next_due() {
            return Step::Idle;
        }
        self.last_frame_at = Instant::now();
        let result = self.with_ctx(|m, ctx| match m.do_draw_frame(ctx) {
            AdResult::WakeUp => true,
            AdResult::RestartMe => {
                let _ = m.do_close(ctx);
                let _ = m.do_initialize(ctx);
                let _ = m.do_blank(ctx);
                false
            }
            AdResult::PaletteRequest(kind) => {
                ctx.module.palette = Some(AdPalette::build(kind));
                false
            }
            _ => false,
        });
        if result { Step::WakeUp } else { Step::Drew }
    }

    /// Live control update (Demo). Mirrors the Windows engine: `do_button_message(slot)` first,
    /// and modules that handle it in place (Spotlight, Satori, Stained Glass) skip the restart.
    fn update_controls_live(&mut self, slot: usize, raw: [i32; 4]) {
        let meta = self.module.metadata();
        for (s, &v) in raw.iter().enumerate() {
            self.params.set_raw_value(s, v, &meta.controls);
        }
        let btn = self.module.do_button_message(slot);
        if btn == AdResult::NoError && matches!(meta.id.as_str(), "spotlite" | "satori" | "stained") {
            return;
        }
        self.sound.stop_all();
        // Give screen-effect modules the original desktop back before re-blanking.
        let (dx, dy, dw, dh) = self.desktop_rect;
        self.surface.capture_desktop(dx, dy, dw, dh);
        self.with_ctx(|m, ctx| {
            let _ = m.do_close(ctx);
            if let AdResult::PaletteRequest(kind) = m.do_initialize(ctx) {
                ctx.module.palette = Some(AdPalette::build(kind));
            }
            if let AdResult::PaletteRequest(kind) = m.do_blank(ctx) {
                ctx.module.palette = Some(AdPalette::build(kind));
            }
        });
    }

    fn close(&mut self) {
        self.sound.stop_all();
        self.with_ctx(|m, ctx| {
            let _ = m.do_close(ctx);
        });
    }

    /// The surface to show (with CRT scanlines applied when enabled).
    fn display_surface(&mut self) -> &AdSurface {
        if let Some(scratch) = self.crt_scratch.as_mut() {
            scratch.pixels_mut().copy_from_slice(self.surface.pixels());
            scratch.apply_crt_scanlines();
            return scratch;
        }
        &self.surface
    }
}

/// Scale `src_rect` (logical) of `src` into a `dw x dh` framebuffer, nearest-neighbor,
/// optionally pillarboxed to a centered 4:3 stage.
fn present_region(
    src: &AdSurface,
    (sx, sy, sw, sh): (i32, i32, i32, i32),
    dst: &mut [u32],
    dw: usize,
    dh: usize,
    pillarbox: bool,
) {
    if dw == 0 || dh == 0 || sw <= 0 || sh <= 0 {
        return;
    }
    let (stage_x, stage_w) = if pillarbox {
        let w = ((dh * 4) / 3).min(dw);
        ((dw - w) / 2, w)
    } else {
        (0, dw)
    };
    let px = src.pixels();
    let pitch = src.width as usize;
    let xmap: Vec<Option<usize>> = (0..stage_w)
        .map(|x| {
            let lx = sx + ((x as i64 * sw as i64) / stage_w as i64) as i32;
            (lx >= 0 && lx < src.width).then_some(lx as usize)
        })
        .collect();
    for (y, row) in dst.chunks_exact_mut(dw).take(dh).enumerate() {
        let ly = sy + ((y as i64 * sh as i64) / dh as i64) as i32;
        row[..stage_x].fill(0);
        row[stage_x + stage_w..].fill(0);
        let stage = &mut row[stage_x..stage_x + stage_w];
        if ly < 0 || ly >= src.height {
            stage.fill(0);
            continue;
        }
        let line = &px[ly as usize * pitch..(ly as usize + 1) * pitch];
        for (d, m) in stage.iter_mut().zip(&xmap) {
            *d = m.map_or(0, |i| line[i]);
        }
    }
}

/// Logical canvas size for a physical area, per the scale mode.
fn logical_size(prefs: &AdPrefs, phys_w: i32, phys_h: i32, scale_factor: f64) -> (i32, i32) {
    if prefs.scale_mode == ScaleMode::Classic640x480 {
        return (640, 480);
    }
    let dpi = (scale_factor * 96.0).round() as u32;
    let scale = prefs.scale_mode.resolve_factor(dpi).max(1);
    ((phys_w / scale).max(64), (phys_h / scale).max(64))
}

/// Pick module + controls for canvas `idx` (Different-Per-Monitor draws from the Randomizer pool).
fn choose_module(app: &AppData, primary: &ModuleCatalogEntry, idx: usize) -> (Box<dyn AdModule>, [i32; 4]) {
    if app.prefs.multi_monitor == MultiMonitorMode::DifferentPerMonitor && idx > 0 {
        let pool = &app.randomizer_pool;
        let candidates: Vec<&ModuleCatalogEntry> = app
            .catalog
            .iter()
            .filter(|e| {
                e.meta.id != "randomizer"
                    && e.meta.id != primary.meta.id
                    && (pool.is_empty() || pool.contains(&e.meta.id))
            })
            .collect();
        if !candidates.is_empty() {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as usize)
                .unwrap_or(0)
                .wrapping_add(idx.wrapping_mul(37));
            let chosen = candidates[seed % candidates.len()];
            return (app.instantiate(chosen), app.raw_controls(chosen));
        }
    }
    (app.instantiate(primary), app.raw_controls(primary))
}

// =============================================================================================
// Entry point

enum UserEvent {
    Controls(usize, [i32; 4]),
    Stop,
}

pub fn run(opts: RunOptions) -> ExitCode {
    let mode = if opts.demo { SaverMode::Demo } else { SaverMode::Saver };
    // Only one real saver at a time (the daemon, hot corner and Sleep Now may race).
    let _lock = if mode == SaverMode::Saver {
        match common::try_lock(common::SAVER_LOCK) {
            Some(l) => Some(l),
            None => return ExitCode::SUCCESS,
        }
    } else {
        None
    };

    let app = AppData::load();
    let primary_idx = opts
        .module_id
        .as_deref()
        .and_then(|id| app.catalog.iter().position(|e| e.meta.id == id))
        .unwrap_or_else(|| app.selected_index());
    let Some(primary) = app.catalog.get(primary_idx).cloned() else {
        eprintln!("after-dark: no modules available");
        return ExitCode::FAILURE;
    };

    // Under Wayland, XWayland's root window is not the real desktop: skip capture there.
    let x11 = if platform::is_wayland_session() { None } else { X11::connect() };
    let solid_black_only = mode == SaverMode::Saver
        && x11.as_ref().and_then(|x| x.pointer()).is_some_and(|(_, m)| m.shift);
    // Snapshot the desktop before any saver window covers it.
    set_desktop_snapshot(x11.as_ref().and_then(|x| x.capture_root()));

    let sound = AdSound::new();
    sound.set_volumes(app.prefs.sfx_volume, app.prefs.midi_volume);

    if let Some(xid) = opts.window_id {
        return run_in_x11_window(xid, &app, &primary, &sound, mode);
    }

    let event_loop = match EventLoop::<UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("after-dark: cannot open display: {e}");
            return ExitCode::FAILURE;
        }
    };
    if mode == SaverMode::Demo {
        spawn_stdin_reader(event_loop.create_proxy());
    }
    let mut runner = Runner {
        app,
        primary,
        sound,
        mode,
        solid_black_only,
        x11,
        windows: Vec::new(),
        instances: Vec::new(),
        started_at: Instant::now(),
        last_poll: Instant::now(),
        baseline_pointer: None,
        last_pointer: None,
        exit_code: ExitCode::SUCCESS,
        stopping: false,
    };
    if let Err(e) = event_loop.run_app(&mut runner) {
        eprintln!("after-dark: event loop error: {e}");
        return ExitCode::FAILURE;
    }
    runner.exit_code
}

/// Demo control channel: `controls SLOT R0 R1 R2 R3` and `stop`; EOF (panel gone) stops too.
fn spawn_stdin_reader(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            match stdin.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    let _ = proxy.send_event(UserEvent::Stop);
                    return;
                }
                Ok(_) => {}
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            match parts.as_slice() {
                ["stop"] => {
                    let _ = proxy.send_event(UserEvent::Stop);
                    return;
                }
                ["controls", slot, rest @ ..] if rest.len() == 4 => {
                    let vals: Vec<i32> = rest.iter().filter_map(|v| v.parse().ok()).collect();
                    if let (Ok(slot), Ok(raw)) = (slot.parse::<usize>(), <[i32; 4]>::try_from(vals)) {
                        let _ = proxy.send_event(UserEvent::Controls(slot.min(3), raw));
                    }
                }
                _ => {}
            }
        }
    });
}

// =============================================================================================
// winit fullscreen windows

struct SaverWindow {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    instance: usize,
    /// Logical sub-rectangle of the instance canvas shown in this window.
    src_rect: (i32, i32, i32, i32),
    first_cursor: Option<PhysicalPosition<f64>>,
}

struct Runner {
    app: AppData,
    primary: ModuleCatalogEntry,
    sound: AdSound,
    mode: SaverMode,
    solid_black_only: bool,
    x11: Option<X11>,
    windows: Vec<SaverWindow>,
    instances: Vec<Instance>,
    started_at: Instant,
    last_poll: Instant,
    /// Pointer position when the saver started, and at the previous poll (focus-independent wake).
    baseline_pointer: Option<(i32, i32)>,
    last_pointer: Option<(i32, i32)>,
    exit_code: ExitCode,
    stopping: bool,
}

struct Target {
    /// `None` when the display server reports no outputs (e.g. nested/headless X servers).
    monitor: Option<MonitorHandle>,
    pos: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
}

impl Runner {
    fn targets(&self, el: &ActiveEventLoop) -> Vec<Target> {
        let monitors: Vec<MonitorHandle> = el.available_monitors().collect();
        let primary = el.primary_monitor().or_else(|| monitors.first().cloned());
        let to_target = |m: &MonitorHandle| Target {
            monitor: Some(m.clone()),
            pos: m.position(),
            size: m.size(),
        };
        if monitors.is_empty() {
            let (w, h) = self
                .x11
                .as_ref()
                .and_then(|x| x.monitors().first().map(|m| (m.width, m.height)))
                .unwrap_or((1920, 1080));
            return vec![Target {
                monitor: None,
                pos: PhysicalPosition::new(0, 0),
                size: PhysicalSize::new(w as u32, h as u32),
            }];
        }
        match self.app.prefs.multi_monitor {
            MultiMonitorMode::PrimaryOnly => primary.iter().map(to_target).collect(),
            _ => monitors.iter().map(to_target).collect(),
        }
    }

    fn open_window(&self, el: &ActiveEventLoop, t: &Target) -> Option<Rc<Window>> {
        let attrs = Window::default_attributes()
            .with_title("After Dark")
            .with_decorations(false)
            .with_position(t.pos)
            .with_inner_size(t.size)
            .with_fullscreen(Some(Fullscreen::Borderless(t.monitor.clone())))
            .with_window_level(if self.mode == SaverMode::Saver {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            });
        let w = match el.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("after-dark: cannot create window: {e}");
                return None;
            }
        };
        if self.mode == SaverMode::Saver {
            w.set_cursor_visible(false);
        }
        w.focus_window();
        Some(w)
    }

    fn create_all(&mut self, el: &ActiveEventLoop) {
        let targets = self.targets(el);
        if targets.is_empty() {
            return;
        }
        let spanned = self.app.prefs.multi_monitor == MultiMonitorMode::Spanned && targets.len() > 1;
        if spanned {
            // One canvas over the virtual-desktop bounding box; each monitor window shows its part.
            let vx = targets.iter().map(|t| t.pos.x).min().unwrap_or(0);
            let vy = targets.iter().map(|t| t.pos.y).min().unwrap_or(0);
            let vr = targets.iter().map(|t| t.pos.x + t.size.width as i32).max().unwrap_or(1);
            let vb = targets.iter().map(|t| t.pos.y + t.size.height as i32).max().unwrap_or(1);
            let (vw, vh) = (vr - vx, vb - vy);
            let sf = targets[0].monitor.as_ref().map_or(1.0, |m| m.scale_factor());
            let mut prefs = self.app.prefs.clone();
            if prefs.scale_mode == ScaleMode::Classic640x480 {
                prefs.scale_mode = ScaleMode::Auto; // a 4:3 stage makes no sense spanned
            }
            let (lw, lh) = logical_size(&prefs, vw, vh, sf);
            let (module, raw) = choose_module(&self.app, &self.primary, 0);
            let Some(inst) = Instance::new(
                0, (lw, lh), (vx, vy, vw, vh), self.mode, &prefs, module, raw, &self.sound,
                self.app.prefs.sound_enabled, self.solid_black_only,
            ) else {
                return;
            };
            self.instances.push(inst);
            for t in &targets {
                let rect = (
                    ((t.pos.x - vx) as i64 * lw as i64 / vw as i64) as i32,
                    ((t.pos.y - vy) as i64 * lh as i64 / vh as i64) as i32,
                    (t.size.width as i64 * lw as i64 / vw as i64) as i32,
                    (t.size.height as i64 * lh as i64 / vh as i64) as i32,
                );
                self.add_window(el, t, 0, rect);
            }
        } else {
            for (idx, t) in targets.iter().enumerate() {
                let (pw, ph) = (t.size.width as i32, t.size.height as i32);
                let (lw, lh) = logical_size(&self.app.prefs, pw, ph, t.monitor.as_ref().map_or(1.0, |m| m.scale_factor()));
                // The classic stage only captures the centered 4:3 part of the desktop.
                let desktop = if self.app.prefs.scale_mode == ScaleMode::Classic640x480 {
                    let sw = ((ph * 4) / 3).min(pw);
                    (t.pos.x + (pw - sw) / 2, t.pos.y, sw, ph)
                } else {
                    (t.pos.x, t.pos.y, pw, ph)
                };
                let (module, raw) = choose_module(&self.app, &self.primary, idx);
                let Some(inst) = Instance::new(
                    idx, (lw, lh), desktop, self.mode, &self.app.prefs, module, raw, &self.sound,
                    self.app.prefs.sound_enabled && idx == 0, self.solid_black_only,
                ) else {
                    continue;
                };
                self.instances.push(inst);
                self.add_window(el, t, self.instances.len() - 1, (0, 0, lw, lh));
            }
        }
    }

    fn add_window(&mut self, el: &ActiveEventLoop, t: &Target, instance: usize, src_rect: (i32, i32, i32, i32)) {
        let Some(window) = self.open_window(el, t) else {
            return;
        };
        let context = match softbuffer::Context::new(window.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("after-dark: cannot initialise drawing: {e}");
                return;
            }
        };
        let surface = match softbuffer::Surface::new(&context, window.clone()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("after-dark: cannot create drawing surface: {e}");
                return;
            }
        };
        self.windows.push(SaverWindow {
            window,
            surface,
            instance,
            src_rect,
            first_cursor: None,
        });
    }

    fn present(&mut self, wi: usize) {
        let w = &mut self.windows[wi];
        let size = w.window.inner_size();
        let (Some(nw), Some(nh)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
            return;
        };
        if w.surface.resize(nw, nh).is_err() {
            return;
        }
        let inst = &mut self.instances[w.instance];
        let pillarbox = inst.scale_mode == ScaleMode::Classic640x480;
        let src = inst.display_surface();
        let Ok(mut buf) = w.surface.buffer_mut() else {
            return;
        };
        present_region(src, w.src_rect, &mut buf, size.width as usize, size.height as usize, pillarbox);
        let _ = buf.present();
    }

    fn stop(&mut self, el: &ActiveEventLoop) {
        if self.stopping {
            return;
        }
        self.stopping = true;
        for inst in &mut self.instances {
            inst.close();
        }
        self.windows.clear();
        el.exit();
    }

    fn wake_on_input(&mut self, el: &ActiveEventLoop) {
        let grace = if self.mode == SaverMode::Saver { WAKE_GRACE_KEYS } else { Duration::ZERO };
        if self.started_at.elapsed() >= grace {
            self.stop(el);
        }
    }

    /// Focus-independent wake check for the real saver: a fullscreen window may not hold keyboard
    /// focus, so watch the X server's idle counter, ignoring sub-threshold mouse jitter.
    fn poll_global_input(&mut self, el: &ActiveEventLoop) {
        if self.mode != SaverMode::Saver || self.last_poll.elapsed() < Duration::from_millis(100) {
            return;
        }
        let since_prev_poll = self.last_poll.elapsed();
        self.last_poll = Instant::now();
        let Some(x) = &self.x11 else { return };
        let Some((pos, mods)) = x.pointer() else { return };
        ad_sdk::sys::set_caps_lock(mods.caps_lock);
        let base = *self.baseline_pointer.get_or_insert(pos);
        let prev = self.last_pointer.replace(pos).unwrap_or(pos);
        if self.started_at.elapsed() < WAKE_GRACE_MOUSE {
            self.baseline_pointer = Some(pos);
            return;
        }
        // Real pointer travel beyond the jitter threshold since the saver started.
        let moved = ((pos.0 - base.0) as f64).hypot((pos.1 - base.1) as f64) > MOUSE_JITTER_PX;
        // Input that happened after the previous poll while the pointer stayed put = a key press
        // or click. (Motion before the previous poll is already reflected in `prev`.)
        let recent_input = x
            .idle_ms()
            .is_some_and(|ms| Duration::from_millis(ms) + Duration::from_millis(10) < since_prev_poll);
        if moved || (recent_input && pos == prev) {
            self.stop(el);
        }
    }
}

impl ApplicationHandler<UserEvent> for Runner {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.windows.is_empty() && !self.stopping {
            self.create_all(el);
            if self.windows.is_empty() {
                eprintln!("after-dark: could not create any saver window");
                self.exit_code = ExitCode::FAILURE;
                el.exit();
                return;
            }
            self.started_at = Instant::now();
            for i in 0..self.windows.len() {
                self.present(i);
            }
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Stop => self.stop(el),
            UserEvent::Controls(slot, raw) => {
                for inst in &mut self.instances {
                    inst.update_controls_live(slot, raw);
                }
                for i in 0..self.windows.len() {
                    self.present(i);
                }
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(wi) = self.windows.iter().position(|w| w.window.id() == id) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested | WindowEvent::Destroyed => self.stop(el),
            WindowEvent::RedrawRequested | WindowEvent::Resized(_) => self.present(wi),
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    state: ElementState::Pressed,
                    logical_key,
                    ..
                },
                ..
            } => {
                // The Demo only stops on Escape (or a click); any key wakes the real saver.
                if self.mode == SaverMode::Saver || logical_key == Key::Named(NamedKey::Escape) {
                    self.wake_on_input(el);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                ..
            } => self.wake_on_input(el),
            WindowEvent::CursorMoved { position, .. } if self.mode == SaverMode::Saver => {
                let w = &mut self.windows[wi];
                let first = *w.first_cursor.get_or_insert(position);
                if self.started_at.elapsed() < WAKE_GRACE_MOUSE {
                    w.first_cursor = Some(position);
                } else if (position.x - first.x).hypot(position.y - first.y) > MOUSE_JITTER_PX {
                    self.stop(el);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.stopping {
            return;
        }
        self.poll_global_input(el);
        if self.stopping {
            return;
        }
        let mut drew = vec![false; self.instances.len()];
        for (i, inst) in self.instances.iter_mut().enumerate() {
            match inst.step() {
                Step::WakeUp => {
                    self.stop(el);
                    return;
                }
                Step::Drew => drew[i] = true,
                Step::Idle => {}
            }
        }
        for wi in 0..self.windows.len() {
            if drew[self.windows[wi].instance] {
                self.present(wi);
            }
        }
        let mut next = self
            .instances
            .iter()
            .filter(|i| !i.solid_black_only)
            .map(Instance::next_due)
            .min()
            .unwrap_or_else(|| Instant::now() + Duration::from_millis(100));
        if self.mode == SaverMode::Saver {
            next = next.min(self.last_poll + Duration::from_millis(100));
        }
        el.set_control_flow(ControlFlow::WaitUntil(next));
    }
}

// =============================================================================================
// xscreensaver: draw into an existing X11 window

fn run_in_x11_window(xid: u32, app: &AppData, primary: &ModuleCatalogEntry, sound: &AdSound, mode: SaverMode) -> ExitCode {
    use x11rb::connection::{Connection, RequestConnection};
    use x11rb::protocol::xproto::{ConnectionExt as _, CreateGCAux, ImageFormat};

    let Ok((conn, _)) = x11rb::connect(None) else {
        eprintln!("after-dark: cannot connect to X display");
        return ExitCode::FAILURE;
    };
    let geom = |conn: &x11rb::rust_connection::RustConnection| -> Option<(u16, u16, u8)> {
        let g = conn.get_geometry(xid).ok()?.reply().ok()?;
        Some((g.width, g.height, g.depth))
    };
    let Some((mut ww, mut wh, depth)) = geom(&conn) else {
        eprintln!("after-dark: window {xid:#x} not found");
        return ExitCode::FAILURE;
    };
    if depth != 24 && depth != 32 {
        eprintln!("after-dark: unsupported window depth {depth}");
        return ExitCode::FAILURE;
    }
    let Ok(gc) = conn.generate_id() else { return ExitCode::FAILURE };
    if conn.create_gc(gc, xid, &CreateGCAux::new()).is_err() {
        return ExitCode::FAILURE;
    }
    let make = |w: u16, h: u16| {
        let (lw, lh) = logical_size(&app.prefs, w as i32, h as i32, 1.0);
        let (module, raw) = (app.instantiate(primary), app.raw_controls(primary));
        // Previews in xscreensaver's settings should stay quiet.
        Instance::new(0, (lw, lh), (0, 0, w as i32, h as i32), mode, &app.prefs, module, raw, sound, false, false)
    };
    let Some(mut inst) = make(ww, wh) else { return ExitCode::FAILURE };
    let mut frame = vec![0u32; ww as usize * wh as usize];
    let max_bytes = (conn.maximum_request_bytes().saturating_sub(64)).max(4096);
    loop {
        // Follow window resizes (xscreensaver's preview pane).
        match geom(&conn) {
            Some((w, h, _)) if (w, h) != (ww, wh) => {
                inst.close();
                (ww, wh) = (w, h);
                let Some(n) = make(ww, wh) else { return ExitCode::FAILURE };
                inst = n;
                frame = vec![0u32; ww as usize * wh as usize];
            }
            None => return ExitCode::SUCCESS, // window destroyed
            _ => {}
        }
        if let Step::WakeUp = inst.step() {
            inst.close();
            return ExitCode::SUCCESS;
        }
        let pillarbox = inst.scale_mode == ScaleMode::Classic640x480;
        let (sw, sh) = (inst.surface.width, inst.surface.height);
        present_region(inst.display_surface(), (0, 0, sw, sh), &mut frame, ww as usize, wh as usize, pillarbox);
        let rows_per_req = (max_bytes / (ww as usize * 4)).max(1);
        let mut y = 0usize;
        let mut bytes = Vec::new();
        while y < wh as usize {
            let rows = rows_per_req.min(wh as usize - y);
            bytes.clear();
            for &p in &frame[y * ww as usize..(y + rows) * ww as usize] {
                bytes.extend_from_slice(&p.to_le_bytes());
            }
            if conn
                .put_image(ImageFormat::Z_PIXMAP, xid, gc, ww, rows as u16, 0, y as i16, 0, depth, &bytes)
                .is_err()
            {
                return ExitCode::SUCCESS;
            }
            y += rows;
        }
        if conn.flush().is_err() {
            return ExitCode::SUCCESS;
        }
        let wait = inst.next_due().saturating_duration_since(Instant::now());
        std::thread::sleep(wait.max(Duration::from_millis(4)));
    }
}
