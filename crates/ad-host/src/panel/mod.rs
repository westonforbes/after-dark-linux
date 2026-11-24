//! Control Panel (eframe/egui): the Linux counterpart of the Windows After Dark Control Panel.
//!
//! - `view.rs`     main window layout (logo, power switch, module list, control slots, buttons)
//! - `dialogs.rs`  Setup, Randomizer chooser, Messages / GraphStat editors, About boxes
//! - `theme.rs`    "Retro Windows 3.1" and "Modern" looks
//! - `art.rs`      original `AD.EXE` bitmaps as textures
//! - `pickers.rs`  zenity / kdialog file and folder choosers
//!
//! Unlike the Windows build, the panel is not the resident idle watcher: that is
//! `after-dark daemon`, which the panel starts (and can install for login via XDG autostart).
//! The panel and daemon communicate only through `AD_PREFS.INI` / `RANDOM.ADS`, which the panel
//! rewrites whenever something changes.

mod art;
mod dialogs;
mod pickers;
mod theme;
mod view;

use std::process::ExitCode;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use ad_sdk::sound::AdSound;
use ad_sdk::{AdControl, AdResult};
use eframe::egui::{self, ViewportCommand, WindowLevel};

use crate::common::{self, AppData, SaverChild};
use dialogs::{GraphStatDlg, MessageBox, MessagesDlg, Outcome, PathPrompt, RandomizerDlg, SetupDlg};
use pickers::{PickResult, PickTarget};
use theme::Look;

/// Client size of the main window in points.
pub(crate) const WINDOW_SIZE: [f32; 2] = [view::CLIENT_W, view::CLIENT_H];

pub fn run() -> ExitCode {
    let mut data = AppData::load();
    // 1991 README: holding Ctrl while After Dark starts brings it up switched Off.
    if crate::platform::current_modifiers().ctrl && data.prefs.screen_saver_on {
        data.prefs.screen_saver_on = false;
        let _ = data.save();
    }
    if data.prefs.screen_saver_on
        && let Err(e) = common::start_daemon()
    {
        eprintln!("after-dark: could not start the idle monitor: {e}");
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("After Dark")
        .with_app_id("after-dark")
        .with_inner_size(WINDOW_SIZE)
        .with_min_inner_size(WINDOW_SIZE)
        .with_resizable(false)
        .with_maximize_button(false);
    if let Some(icon) = art::app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    match eframe::run_native("After Dark", options, Box::new(|cc| Ok(Box::new(PanelApp::new(cc, data))))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("after-dark: cannot open the Control Panel window: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Module list filter (collection combo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filter {
    All,
    Only(common::Collection),
    Favorites,
}

enum Dialog {
    Setup(Box<SetupDlg>),
    Randomizer(RandomizerDlg),
    Messages(MessagesDlg),
    GraphStat(GraphStatDlg),
    Message(MessageBox),
    Path(PathPrompt),
}

/// Things the main view asks for; applied after drawing.
pub(crate) enum Action {
    Select { idx: usize, demo: bool },
    ToggleFavorite,
    TogglePower,
    Setup,
    Demo,
    SleepNow,
    ToggleTheme,
    ModuleAbout,
    AppAbout,
    Control { slot: usize, raw: i32 },
    ModuleButton { slot: usize },
    StartDaemon,
    Autostart(bool),
}

struct PendingPick {
    target: PickTarget,
    rx: Receiver<PickResult>,
    resume_demo: bool,
}

pub(crate) struct PanelApp {
    data: AppData,
    selected: usize,
    filter: Filter,
    search: String,
    scroll_to_selected: bool,
    /// Scroll to the selection on the next frame (after a pending selection change applies).
    scroll_next: bool,
    art: Option<art::Art>,
    look: Look,
    sound: AdSound,
    demo: Option<SaverChild>,
    sleeper: Option<SaverChild>,
    on_top: bool,
    dialog: Option<Dialog>,
    picker: Option<PendingPick>,
    daemon_up: bool,
    daemon_checked: Instant,
    autostart: bool,
    dirty: bool,
    last_save: Instant,
    notice: Option<(String, Instant)>,
}

impl PanelApp {
    fn new(cc: &eframe::CreationContext<'_>, data: AppData) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let look = Look::of(data.prefs.ui_theme);
        theme::apply(&cc.egui_ctx, look);
        let sound = AdSound::new();
        sound.set_volumes(data.prefs.sfx_volume, data.prefs.midi_volume);
        Self {
            selected: data.selected_index(),
            data,
            filter: Filter::All,
            search: String::new(),
            scroll_to_selected: true,
            scroll_next: false,
            art: art::Art::load(&cc.egui_ctx),
            look,
            sound,
            demo: None,
            sleeper: None,
            on_top: false,
            dialog: None,
            picker: None,
            daemon_up: common::daemon_running(),
            daemon_checked: Instant::now(),
            autostart: common::autostart_enabled(),
            dirty: false,
            last_save: Instant::now(),
            notice: None,
        }
    }

    fn entry(&self) -> &ad_modules::ModuleCatalogEntry {
        &self.data.catalog[self.selected.min(self.data.catalog.len().saturating_sub(1))]
    }

    fn is_favorite(&self, id: &str) -> bool {
        self.data.prefs.favorites.iter().any(|f| f == id)
    }

    /// Catalog indices passing the collection filter and search box.
    fn filtered(&self) -> Vec<usize> {
        let q = self.search.trim().to_lowercase();
        self.data
            .catalog
            .iter()
            .enumerate()
            .filter(|(_, e)| match self.filter {
                Filter::All => true,
                Filter::Only(c) => common::Collection::of(&e.meta.id) == c,
                Filter::Favorites => self.is_favorite(&e.meta.id),
            })
            .filter(|(_, e)| q.is_empty() || e.meta.name.to_lowercase().contains(&q) || e.meta.id.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    // --- persistence ---------------------------------------------------------------------

    fn save(&mut self) {
        self.dirty = false;
        self.last_save = Instant::now();
        if let Err(e) = self.data.save() {
            self.notify(format!("Could not save settings: {e}"));
        }
    }

    /// Save soon (coalesces slider drags).
    fn touch(&mut self) {
        self.dirty = true;
    }

    fn flush(&mut self) {
        if self.dirty {
            self.save();
        }
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.notice = Some((msg.into(), Instant::now()));
    }

    // --- saver children ------------------------------------------------------------------

    fn demo_running(&self) -> bool {
        self.demo.is_some()
    }

    fn start_demo(&mut self) {
        self.stop_demo();
        self.flush();
        let id = self.entry().meta.id.clone();
        match SaverChild::spawn(true, Some(&id)) {
            Ok(child) => self.demo = Some(child),
            Err(e) => self.notify(format!("Could not start the demo: {e}")),
        }
    }

    fn stop_demo(&mut self) {
        if let Some(child) = self.demo.take() {
            child.stop();
        }
    }

    fn restart_demo_if_running(&mut self) {
        if self.demo_running() {
            self.start_demo();
        }
    }

    fn sleep_now(&mut self) {
        self.stop_demo();
        self.save();
        if self.sleeper.as_mut().is_some_and(SaverChild::is_running) {
            return;
        }
        match SaverChild::spawn(false, None) {
            Ok(child) => self.sleeper = Some(child),
            Err(e) => self.notify(format!("Could not start the screen saver: {e}")),
        }
    }

    /// Reap exited children and keep the panel above a running demo.
    fn poll_children(&mut self, ctx: &egui::Context) {
        if self.demo.as_mut().is_some_and(|c| !c.is_running()) {
            self.demo = None;
        }
        if self.sleeper.as_mut().is_some_and(|c| !c.is_running()) {
            self.sleeper = None;
        }
        let want_top = self.demo.is_some();
        if want_top != self.on_top {
            self.on_top = want_top;
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if want_top {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            }));
            if want_top {
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            }
        }
        if self.daemon_checked.elapsed() >= Duration::from_secs(1) {
            self.daemon_checked = Instant::now();
            self.daemon_up = common::daemon_running();
        }
    }

    // --- actions -------------------------------------------------------------------------

    fn select(&mut self, idx: usize) {
        if idx != self.selected && idx < self.data.catalog.len() {
            self.selected = idx;
            self.data.select(idx);
            self.save();
        }
    }

    fn apply(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Select { idx, demo } => {
                let changed = idx != self.selected;
                self.select(idx);
                if demo {
                    self.start_demo();
                } else if changed {
                    self.restart_demo_if_running();
                }
            }
            Action::ToggleFavorite => {
                let id = self.entry().meta.id.clone();
                let favs = &mut self.data.prefs.favorites;
                match favs.iter().position(|f| *f == id) {
                    Some(pos) => {
                        favs.remove(pos);
                    }
                    None => favs.push(id),
                }
                self.save();
            }
            Action::TogglePower => {
                self.data.prefs.screen_saver_on = !self.data.prefs.screen_saver_on;
                self.save();
                if self.data.prefs.screen_saver_on {
                    self.start_daemon();
                }
            }
            Action::Setup => self.dialog = Some(Dialog::Setup(Box::new(SetupDlg::new(&self.data.prefs, self.sound.clone())))),
            Action::Demo => {
                if self.demo_running() {
                    self.stop_demo();
                } else {
                    self.start_demo();
                }
            }
            Action::SleepNow => self.sleep_now(),
            Action::ToggleTheme => {
                self.data.prefs.ui_theme = match self.data.prefs.ui_theme {
                    ad_sdk::prefs::UiTheme::RetroWin31 => ad_sdk::prefs::UiTheme::ModernWin11,
                    ad_sdk::prefs::UiTheme::ModernWin11 => ad_sdk::prefs::UiTheme::RetroWin31,
                };
                self.save();
            }
            Action::ModuleAbout => {
                let meta = &self.entry().meta;
                let body = if meta.about.trim().is_empty() { meta.credit.clone() } else { meta.about.clone() };
                self.dialog = Some(Dialog::Message(MessageBox { title: format!("About {}", meta.name), body }));
            }
            Action::AppAbout => {
                self.dialog = Some(Dialog::Message(MessageBox {
                    title: "About After Dark".into(),
                    body: dialogs::about_body(self.daemon_up),
                }));
            }
            Action::Control { slot, raw } => {
                let entry = self.entry();
                let id = entry.meta.id.clone();
                let mut vals = self.data.raw_controls(entry);
                vals[slot] = raw;
                self.data.prefs.module_controls.insert(id, vals);
                if let Some(demo) = self.demo.as_mut() {
                    demo.send_controls(slot, vals);
                }
                self.touch();
            }
            Action::ModuleButton { slot } => self.module_button(slot, ctx),
            Action::StartDaemon => self.start_daemon(),
            Action::Autostart(on) => match common::set_autostart(on) {
                Ok(()) => self.autostart = on,
                Err(e) => {
                    self.autostart = common::autostart_enabled();
                    self.notify(format!("Could not change the login item: {e}"));
                }
            },
        }
    }

    fn start_daemon(&mut self) {
        if let Err(e) = common::start_daemon() {
            self.notify(format!("Could not start the idle monitor: {e}"));
        }
        // The new process needs a moment to take its lock; re-check shortly.
        self.daemon_checked = Instant::now() - Duration::from_millis(700);
    }

    /// A module's push-button control: the Windows panel's per-module editors and pickers.
    fn module_button(&mut self, slot: usize, ctx: &egui::Context) {
        let entry = self.entry().clone();
        let data_dir = self.data.prefs.path.clone();
        match entry.meta.id.as_str() {
            "randomizer" => {
                self.dialog = Some(Dialog::Randomizer(RandomizerDlg::new(
                    &self.data.catalog,
                    &self.data.randomizer_pool,
                    &self.data.prefs.favorites,
                )))
            }
            "message" => self.dialog = Some(Dialog::Messages(MessagesDlg::new(&data_dir))),
            "grafstat" => self.dialog = Some(Dialog::GraphStat(GraphStatDlg::new(&data_dir))),
            "logo" => self.pick(PickTarget::LogoFile, ctx),
            "globe" => self.pick(PickTarget::GlobeFile, ctx),
            "slidesho" => self.pick(PickTarget::SlideShowDir, ctx),
            _ => {
                // External plug-ins: let the module handle its own button.
                let mut module = self.data.instantiate(&entry);
                match module.do_button_message(slot) {
                    AdResult::UserError(msg) => {
                        self.dialog = Some(Dialog::Message(MessageBox { title: entry.meta.name.clone(), body: msg }))
                    }
                    AdResult::RestartMe => self.restart_demo_if_running(),
                    _ => {}
                }
            }
        }
    }

    fn pick_value(&mut self, target: PickTarget) -> &mut String {
        let p = &mut self.data.prefs;
        match target {
            PickTarget::LogoFile => &mut p.logo_file,
            PickTarget::GlobeFile => &mut p.globe_file,
            PickTarget::SlideShowDir => &mut p.slide_show_dir,
        }
    }

    fn pick(&mut self, target: PickTarget, ctx: &egui::Context) {
        let initial = self.pick_value(target).clone();
        // The chooser would open underneath a fullscreen demo; pause it and resume afterwards.
        let resume_demo = self.demo_running();
        self.stop_demo();
        match pickers::spawn(target, &initial, ctx) {
            Some(rx) => self.picker = Some(PendingPick { target, rx, resume_demo }),
            None => {
                self.dialog = Some(Dialog::Path(PathPrompt { target, text: initial, chooser_failed: false }));
                if resume_demo {
                    self.start_demo();
                }
            }
        }
    }

    fn set_picked(&mut self, target: PickTarget, path: String) {
        *self.pick_value(target) = path;
        self.save();
    }

    fn poll_picker(&mut self) {
        let Some(pending) = self.picker.as_ref() else { return };
        let result = match pending.rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => PickResult::Cancelled,
        };
        let PendingPick { target, resume_demo, .. } = self.picker.take().expect("pending picker");
        match result {
            PickResult::Picked(path) => self.set_picked(target, path),
            PickResult::Cancelled => {}
            PickResult::Failed(err) => {
                // A broken chooser: fall back to typing the path.
                let text = self.pick_value(target).clone();
                self.dialog = Some(Dialog::Path(PathPrompt { target, text, chooser_failed: true }));
                if !err.is_empty() {
                    self.notify(format!("File chooser failed: {err}"));
                }
            }
        }
        if resume_demo {
            self.start_demo();
        }
    }

    fn show_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let look = self.look;
        let outcome = match &mut dialog {
            Dialog::Setup(d) => d.show(ctx, look),
            Dialog::Randomizer(d) => d.show(ctx, look),
            Dialog::Messages(d) => d.show(ctx, look),
            Dialog::GraphStat(d) => d.show(ctx, look),
            Dialog::Message(d) => d.show(ctx, look),
            Dialog::Path(d) => d.show(ctx, look),
        };
        match (outcome, dialog) {
            (Outcome::Open, dialog) => self.dialog = Some(dialog),
            (Outcome::Cancel, _) => {}
            (Outcome::Accept, Dialog::Setup(d)) => {
                self.data.prefs = d.prefs;
                self.sound.set_volumes(self.data.prefs.sfx_volume, self.data.prefs.midi_volume);
                self.save();
            }
            (Outcome::Accept, Dialog::Randomizer(d)) => {
                self.data.randomizer_pool = d.pool();
                self.save();
                self.restart_demo_if_running();
            }
            (Outcome::Accept, Dialog::Messages(mut d)) => {
                match d.save(&self.data.prefs.path) {
                    Ok(()) => self.restart_demo_if_running(),
                    Err(e) => self.notify(format!("Could not save MESSAGES.ADS: {e}")),
                }
            }
            (Outcome::Accept, Dialog::GraphStat(d)) => match d.save(&self.data.prefs.path) {
                Ok(()) => self.restart_demo_if_running(),
                Err(e) => self.notify(format!("Could not save GRAFSTAT.TXT: {e}")),
            },
            (Outcome::Accept, Dialog::Message(_)) => {}
            (Outcome::Accept, Dialog::Path(d)) => {
                self.set_picked(d.target, d.text.trim().to_string());
                self.restart_demo_if_running();
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) -> Vec<Action> {
        if self.dialog.is_some() || self.picker.is_some() {
            return Vec::new();
        }
        ctx.input_mut(|i| {
            let mut out = Vec::new();
            for (key, action) in [(egui::Key::D, Action::Demo), (egui::Key::S, Action::SleepNow), (egui::Key::W, Action::Setup)] {
                if i.consume_key(egui::Modifiers::ALT, key) {
                    out.push(action);
                }
            }
            out
        })
    }
}

impl eframe::App for PanelApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let look = Look::of(self.data.prefs.ui_theme);
        if look != self.look {
            self.look = look;
            theme::apply(&ctx, look);
        }
        self.poll_children(&ctx);
        self.poll_picker();

        let mut actions = self.shortcuts(&ctx);
        actions.extend(self.draw(ui));
        self.show_dialog(&ctx);
        if let Some(p) = &self.picker {
            view::waiting_overlay(&ctx, look, p.target);
        }
        for action in actions {
            self.apply(action, &ctx);
        }

        if self.dirty && self.last_save.elapsed() >= Duration::from_millis(300) {
            self.save();
        }
        if self.notice.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(8)) {
            self.notice = None;
        }
        // Poll the demo/saver/daemon/picker state and flush pending saves.
        let fast = self.demo.is_some() || self.sleeper.is_some() || self.picker.is_some() || self.dirty;
        ctx.request_repaint_after(Duration::from_millis(if fast { 200 } else { 1000 }));
    }
}

impl Drop for PanelApp {
    fn drop(&mut self) {
        self.flush();
        self.stop_demo();
        // A running "Sleep Now" saver outlives the panel (dropping the handle would kill it).
        std::mem::forget(self.sleeper.take());
    }
}

/// Is `ctrl` a control the panel shows in a slot?
fn visible(ctrl: &AdControl) -> bool {
    !matches!(ctrl, AdControl::None)
}
