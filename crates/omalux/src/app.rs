//! The window: a shoot's frames in the loupe, the filmstrip under it and
//! the develop panel on the right, as Lightroom's Develop module has them.
//! Its patterns (a `Command` a key or a script runs, `build` for tests,
//! the `Harness`) are Omacull's.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Align2, FontId, RichText, Ui};
use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::SourceInfo;
use omalux_engine::edit::{self, History};
use omalux_engine::shoot::{Filter, Shoot, Step};
use omalux_engine::sidecar;

use crate::commands::Command;
use crate::develop::{self, PanelOut};
use crate::hotkeys::{self, format_shortcut};
use crate::loupe::{Loupe, News};
use crate::render::{Decoder, Renderer, View, Wanted};
use crate::theme::{self, Theme};

/// The filmstrip's height, as Omacull's.
const STRIP_HEIGHT: f32 = 104.0;
/// The develop panel's width.
const PANEL_WIDTH: f32 = 300.0;
/// How many frames are developed ahead of the cursor, the way it's
/// going, and behind it.
const AHEAD: usize = 2;
const BEHIND: usize = 1;

/// A frame whose raw has been decoded this session: its edit, and what
/// its settings are read against.
struct Editing {
    history: History,
    info: Arc<SourceInfo>,
    /// The history's settings, as the renderer is handed them.
    settings: Arc<DevelopSettings>,
}

impl Editing {
    /// Change the edit with `change`, which says whether it did.
    fn edit(&mut self, change: impl FnOnce(&mut History, &SourceInfo) -> bool) -> bool {
        let changed = change(&mut self.history, &self.info);
        if *self.settings != *self.history.settings() {
            self.settings = Arc::new(self.history.settings().clone());
        }
        changed
    }
}

pub struct App {
    theme: Theme,
    theme_rx: Receiver<Theme>,
    /// Problems found at startup, shown in the status bar.
    warnings: Vec<String>,
    /// Commands to run, one a frame, from `OMALUX_SCRIPT` (comma-separated
    /// command names, e.g. `Open,Quit`). For testing without a keyboard.
    script: VecDeque<Command>,
    shoot: Option<Shoot>,
    renderer: Renderer,
    loupe: Loupe,
    /// The edits their sidecars held, of frames not decoded yet: read
    /// when a frame is first wanted.
    saved: HashMap<PathBuf, Option<Arc<DevelopSettings>>>,
    /// The frames edited or looked at this session, each with its steps
    /// to undo.
    edits: HashMap<PathBuf, Editing>,
    /// The slider the keys act on, by its place in the panel.
    selected: usize,
    /// A slider is being dragged: the loupe gets drafts.
    dragging: bool,
    /// The folder picker, while it's open.
    picking: Option<Receiver<Option<PathBuf>>>,
    /// The last thing to tell the user, and whether it's a problem.
    message: Option<(String, bool)>,
    title: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        let ctx = &cc.egui_ctx;
        // Ctrl+= / Ctrl+- are for zooming the image, not the interface.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        theme::install_font(ctx);
        let theme = Theme::load();
        ctx.set_visuals(theme.visuals());

        let warnings = hotkeys::load();
        for w in &warnings {
            log::warn!("{w}");
        }
        let script = std::env::var("OMALUX_SCRIPT")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .filter_map(|step| {
                let parsed = Command::from_name(step.trim());
                if parsed.is_none() {
                    log::warn!("OMALUX_SCRIPT: can't understand {step:?}");
                }
                parsed
            })
            .collect();
        // OMALUX_GPU=0 develops on the CPU.
        let gpu = std::env::var("OMALUX_GPU").as_deref() != Ok("0");
        if gpu {
            lightcraft_gpu::warm_up();
        }
        let wake = ctx.clone();
        let renderer = Renderer::new(Decoder::raw(), gpu, move || wake.request_repaint());
        let mut app = Self::build(theme, theme::watch(ctx.clone()), warnings, script, renderer);
        if let Some(path) = open {
            app.open(path);
        }
        app
    }

    fn build(theme: Theme, theme_rx: Receiver<Theme>, warnings: Vec<String>, script: VecDeque<Command>, renderer: Renderer) -> Self {
        Self {
            theme,
            theme_rx,
            warnings,
            script,
            shoot: None,
            renderer,
            loupe: Loupe::default(),
            saved: HashMap::new(),
            edits: HashMap::new(),
            selected: develop::sliders().position(|spec| spec.id == "light.exposure").unwrap_or(0),
            dragging: false,
            picking: None,
            message: None,
            title: String::new(),
        }
    }

    /// Open a shoot: its folder, or a raw in it.
    fn open(&mut self, path: PathBuf) {
        let raw = (!path.is_dir()).then(|| path.clone());
        let dir = if path.is_dir() { path.clone() } else { path.parent().map(Path::to_path_buf).unwrap_or(path.clone()) };
        if !dir.is_dir() {
            self.message = Some((format!("Couldn't open {}: there's no such folder", dir.display()), true));
            return;
        }
        match Shoot::open(&dir) {
            Ok((mut shoot, unread)) => {
                if raw.is_some_and(|raw| !shoot.go_to_path(&raw)) {
                    log::warn!("{} isn't a raw in the shoot", path.display());
                }
                self.message = (!unread.is_empty()).then(|| (format!("Couldn't read {}", unread.join("; ")), true));
                self.shoot = Some(shoot);
                self.saved.clear();
                self.edits.clear();
                self.dragging = false;
            }
            Err(e) => self.message = Some((format!("Couldn't open {}: {e}", dir.display()), true)),
        }
    }

    fn pick(&mut self, ctx: &egui::Context) {
        if self.picking.is_some() {
            return;
        }
        let mut dialog = rfd::FileDialog::new().set_title("Open Folder");
        if let Some(shoot) = &self.shoot {
            dialog = dialog.set_directory(shoot.dir());
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(dialog.pick_folder());
            ctx.request_repaint();
        });
        self.picking = Some(rx);
    }

    fn finish_picking(&mut self) {
        let Some(picked) = &self.picking else { return };
        match picked.try_recv() {
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) | Ok(None) => self.picking = None,
            Ok(Some(dir)) => {
                self.picking = None;
                self.open(dir);
            }
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Open => self.pick(ctx),
            Command::Previous => self.step_to(Step::Previous),
            Command::Next => self.step_to(Step::Next),
            Command::First => self.step_to(Step::First),
            Command::Last => self.step_to(Step::Last),
            Command::Filter => {
                if let Some(shoot) = &mut self.shoot {
                    shoot.set_filter(if shoot.filter() == Filter::Picks { Filter::All } else { Filter::Picks });
                }
            }
            Command::Undo => self.edit(|history, _| history.undo()),
            Command::Redo => self.edit(|history, _| history.redo()),
            Command::Reset => self.edit(|history, _| history.reset()),
            Command::SliderPrevious => self.selected = self.selected.saturating_sub(1),
            Command::SliderNext => self.selected = (self.selected + 1).min(develop::sliders().count() - 1),
            Command::Increase => self.nudge(1.0),
            Command::Decrease => self.nudge(-1.0),
            Command::IncreaseMore => self.nudge(5.0),
            Command::DecreaseMore => self.nudge(-5.0),
        }
    }

    fn step_to(&mut self, step: Step) {
        if let Some(shoot) = &mut self.shoot {
            shoot.step(step);
        }
    }

    /// Change the current frame's edit in one step, and save it. `change`
    /// says whether it changed anything. Nothing happens until the frame's
    /// raw has been decoded.
    fn edit(&mut self, change: impl FnOnce(&mut History, &SourceInfo) -> bool) {
        let Some(path) = self.shoot.as_ref().map(|shoot| shoot.frame().path.clone()) else { return };
        if self.edits.get_mut(&path).is_some_and(|editing| editing.edit(change)) {
            self.save(&path);
        }
    }

    /// Write a frame's edit to its sidecar.
    fn save(&mut self, path: &Path) {
        let Some(editing) = self.edits.get(path) else { return };
        let saved = sidecar::Edit::new(editing.history.settings().clone());
        if let Err(e) = sidecar::update(path, |xmp| sidecar::with_edit(xmp, &saved)) {
            self.message = Some((format!("Couldn't save {}: {e}", file_name(&sidecar::path_for(path))), true));
        }
    }

    /// Move the selected slider by `steps` nudges.
    fn nudge(&mut self, steps: f64) {
        let Some(spec) = develop::sliders().nth(self.selected) else { return };
        self.edit(|history, info| {
            let value = lightcraft_develop::controls::get(history.settings(), spec.id).unwrap_or(spec.default);
            history.set(|settings| {
                edit::set_control(settings, info, spec.id, crate::slider::nudged(spec, value, steps));
            })
        });
    }

    /// What was done in the develop panel: a drag's changes are drafts
    /// in the loupe and one step once it ends, saved then.
    fn panel(&mut self, out: PanelOut) {
        let Some(path) = self.shoot.as_ref().map(|shoot| shoot.frame().path.clone()) else { return };
        let Some(editing) = self.edits.get_mut(&path) else { return };
        let mut save = false;
        if out.as_shot {
            save = editing.edit(|history, info| history.set(|settings| edit::as_shot(settings, info)));
        }
        if let Some((index, slid)) = out.slider {
            if slid.touched {
                self.selected = index;
            }
            if let (Some(value), Some(spec)) = (slid.value, develop::sliders().nth(index)) {
                editing.edit(|history, info| {
                    history.change(|settings| {
                        edit::set_control(settings, info, spec.id, value);
                    });
                    true
                });
            }
            self.dragging = (self.dragging || slid.drag_started) && !slid.drag_stopped;
            if slid.drag_stopped {
                save = editing.edit(|history, _| history.commit());
            }
        }
        if save {
            self.save(&path);
        }
    }

    fn run_script(&mut self, ctx: &egui::Context) {
        let Some(command) = self.script.pop_front() else {
            return;
        };
        log::info!("script: {command:?}");
        self.run(command, ctx);
        ctx.request_repaint();
    }

    /// A frame's work before anything is drawn.
    fn step(&mut self, ctx: &egui::Context) {
        if let Some(theme) = self.theme_rx.try_iter().last() {
            ctx.set_visuals(theme.visuals());
            self.theme = theme;
        }
        self.finish_picking();
        self.develop(ctx);
        if !ctx.egui_wants_keyboard_input() {
            for command in Command::pressed(ctx) {
                self.run(command, ctx);
            }
        }
        self.run_script(ctx);
        let title = match &self.shoot {
            Some(shoot) => format!("{} — OmaLux", file_name(shoot.dir())),
            None => "OmaLux".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    /// Ask for the current frame and its neighbours to be developed, and
    /// take in what's been finished.
    fn develop(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = self.shoot.as_ref().map_or_else(Vec::new, |shoot| {
            let frames = std::iter::once(shoot.current()).chain(shoot.neighbours(AHEAD, BEHIND));
            frames.map(|i| shoot.frames()[i].path.clone()).collect()
        });
        let wanted: Vec<Wanted> = paths.into_iter().map(|path| Wanted { settings: self.settings_of(&path), path }).collect();
        self.loupe.retain(|path| wanted.iter().any(|w| w.path == path));
        self.renderer.want(wanted, View { size: self.loupe.size(), dragging: self.dragging });
        for news in self.loupe.take(ctx, self.renderer.done()) {
            match news {
                News::Decoded(path, info) => {
                    // Its edit starts from what its sidecar held, or as a fresh raw.
                    let fresh = DevelopSettings::for_raw(info.as_shot_temp, info.as_shot_tint);
                    let saved = self.saved.remove(&path).flatten().map(|settings| (*settings).clone());
                    let history = History::new(fresh, saved);
                    let settings = Arc::new(history.settings().clone());
                    self.edits.entry(path).or_insert(Editing { history, info, settings });
                }
                News::Failed(path, error) => self.message = Some((format!("Couldn't develop {}: {error}", file_name(&path)), true)),
            }
        }
    }

    /// A frame's settings as they stand: its edit this session, or else
    /// the one in its sidecar, read the first time it's asked for. `None`
    /// for a frame with neither.
    fn settings_of(&mut self, path: &Path) -> Option<Arc<DevelopSettings>> {
        if let Some(editing) = self.edits.get(path) {
            return Some(editing.settings.clone());
        }
        if let Some(saved) = self.saved.get(path) {
            return saved.clone();
        }
        let read = sidecar::read(path).and_then(|xmp| xmp.as_deref().map(sidecar::edit).transpose());
        let saved = match read {
            Ok(saved) => saved.flatten(),
            Err(e) => {
                self.message = Some((format!("Couldn't read the edit in {}: {e}", file_name(&sidecar::path_for(path))), true));
                None
            }
        };
        if saved.as_ref().is_some_and(|edit| edit.process > sidecar::PROCESS_VERSION) {
            self.message = Some((format!("{} was edited with a newer OmaLux and may look different here", file_name(path)), true));
        }
        let settings = saved.map(|edit| Arc::new(edit.settings));
        self.saved.insert(path.to_path_buf(), settings.clone());
        settings
    }

    fn show(&mut self, ui: &mut Ui) {
        let bar = egui::Frame::new().fill(self.theme.dark_background).inner_margin(egui::Margin::symmetric(8, 4));
        egui::Panel::bottom("status").frame(bar).show(ui, |ui| self.status_bar(ui));
        if self.shoot.is_some() {
            let strip = egui::Frame::new().fill(self.theme.darker_background);
            egui::Panel::bottom("filmstrip").frame(strip).resizable(false).exact_size(STRIP_HEIGHT).show(ui, |ui| self.filmstrip(ui));
            let side = egui::Frame::new().fill(self.theme.dark_background).inner_margin(egui::Margin::same(6));
            egui::Panel::right("develop").frame(side).resizable(false).exact_size(PANEL_WIDTH).show(ui, |ui| self.develop_panel(ui));
        }
        let pasteboard = egui::Frame::new().fill(self.theme.pasteboard());
        egui::CentralPanel::no_frame().frame(pasteboard).show(ui, |ui| match &self.shoot {
            Some(shoot) => self.loupe.show(ui, &shoot.frame().path),
            None => self.empty_state(ui),
        });
    }

    /// The shown frames' thumbnails, under the loupe.
    fn filmstrip(&mut self, _ui: &mut Ui) {}

    /// The histogram, and the panels under it: White Balance, Light,
    /// Presence.
    fn develop_panel(&mut self, ui: &mut Ui) {
        let Some(editing) = self.shoot.as_ref().and_then(|shoot| self.edits.get(&shoot.frame().path)) else { return };
        let out = develop::show(ui, &self.theme, editing.history.settings(), editing.history.fresh(), self.selected);
        self.panel(out);
    }

    /// With no shoot open: how to open one.
    fn empty_state(&self, ui: &mut Ui) {
        let hint = match hotkeys::current().command(Command::Open) {
            Some(s) => format!("{} to open a shoot", format_shortcut(&s)),
            None => "Open a shoot".to_owned(),
        };
        let rect = ui.max_rect();
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, hint, FontId::proportional(16.0), self.theme.dark_foreground);
    }

    fn status_bar(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let warnings = (!self.warnings.is_empty()).then(|| (self.warnings.join("; "), true));
                if let Some((text, problem)) = self.message.clone().or(warnings) {
                    let colour = if problem { self.theme.red } else { self.theme.dark_foreground };
                    ui.add(egui::Label::new(RichText::new(text).color(colour)).truncate());
                }
                // The room that's left is the status line's, cut short in a
                // narrow window.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let (text, colour) = match &self.shoot {
                        Some(shoot) => (status(shoot), self.theme.foreground),
                        None => ("No folder open".to_owned(), self.theme.dark_foreground),
                    };
                    ui.add(egui::Label::new(RichText::new(text).color(colour)).truncate());
                });
            });
        });
    }
}

/// The current frame, its place among the frames shown, and the filter.
fn status(shoot: &Shoot) -> String {
    let shown = shoot.shown_indices();
    let place = shown.iter().position(|&i| i == shoot.current()).map_or(0, |p| p + 1);
    let of = match shoot.filter() {
        Filter::Picks => format!("{} picks of {} frames", shown.len(), shoot.frames().len()),
        Filter::All => format!("{} frames", shown.len()),
    };
    format!("{}   {place} / {of}", shoot.frame().name())
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.step(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use egui::{Event, Key, Modifiers};
    use omalux_engine::sidecar;

    /// The app driven with synthetic events, with no window or GPU.
    struct Harness {
        ctx: egui::Context,
        app: App,
        time: f64,
    }

    impl Harness {
        fn new(script: &[Command]) -> Self {
            let (_tx, rx) = channel();
            let ctx = egui::Context::default();
            let renderer = Renderer::new(crate::render::tests::decoder(), false, || {});
            let app = App::build(Theme::default(), rx, Vec::new(), script.iter().copied().collect(), renderer);
            Self { ctx, app, time: 0.0 }
        }

        fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
            self.time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                self.app.step(&ui.ctx().clone());
                self.app.show(ui);
            });
            // There's no renderer to upload textures to.
            output.textures_delta.clear();
            output
        }

        /// Frames until everything wanted has been developed and shown.
        fn settle(&mut self) {
            for _ in 0..2000 {
                // A frame can ask for more (the first one drawn tells the
                // renderer how much room the loupe has), so it's settled
                // when a frame leaves the renderer with nothing to do and
                // the next has taken in what was done.
                self.frame(vec![]);
                if !self.app.renderer.busy() {
                    self.frame(vec![]);
                    if !self.app.renderer.busy() {
                        return;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            panic!("the renderer never finished");
        }

        /// Where a slider's track is on screen.
        fn track(&self, id: &'static str) -> egui::Rect {
            self.ctx.data(|d| d.get_temp(egui::Id::new(id))).expect("the slider has been drawn")
        }

        fn pointer(&mut self, at: egui::Pos2, pressed: Option<bool>) {
            let mut events = vec![Event::PointerMoved(at)];
            if let Some(pressed) = pressed {
                events.push(Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            }
            self.frame(events);
        }

        /// The current frame's settings.
        fn settings(&self) -> &DevelopSettings {
            let path = &self.app.shoot.as_ref().unwrap().frame().path;
            self.app.edits[path].history.settings()
        }

        /// The edit in the current frame's sidecar.
        fn saved(&self) -> Option<DevelopSettings> {
            let path = &self.app.shoot.as_ref().unwrap().frame().path;
            sidecar::edit(&sidecar::read(path).unwrap()?).unwrap().map(|edit| edit.settings)
        }

        /// A key pressed and let go in one frame.
        fn press(&mut self, modifiers: Modifiers, key: Key) -> egui::FullOutput {
            let mut events = key_events(modifiers, key, true);
            events.extend(key_events(modifiers, key, false));
            self.frame(events)
        }
    }

    fn key_events(modifiers: Modifiers, key: Key, pressed: bool) -> Vec<Event> {
        vec![Event::ModifiersChanged(modifiers), Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }]
    }

    fn closes(output: &egui::FullOutput) -> bool {
        output.viewport_output.values().any(|v| v.commands.iter().any(|c| matches!(c, egui::ViewportCommand::Close)))
    }

    fn titles(output: &egui::FullOutput) -> Vec<String> {
        let commands = output.viewport_output.values().flat_map(|v| &v.commands);
        commands.filter_map(|c| if let egui::ViewportCommand::Title(t) = c { Some(t.clone()) } else { None }).collect()
    }

    #[test]
    fn ctrl_q_closes_the_window_and_q_doesnt() {
        let mut h = Harness::new(&[]);
        assert!(!closes(&h.frame(vec![])));
        assert!(!closes(&h.press(Modifiers::NONE, Key::Q)));
        assert!(closes(&h.press(Modifiers::COMMAND, Key::Q)));
    }

    #[test]
    fn a_script_runs_a_command_a_frame() {
        let mut h = Harness::new(&[Command::Quit]);
        assert!(closes(&h.frame(vec![])));
        assert!(!closes(&h.frame(vec![])));
    }

    /// A folder of raws (empty files: nothing here decodes them) rated as
    /// given, removed when dropped.
    struct Folder(PathBuf);

    impl Folder {
        fn new(name: &str, ratings: &[i32]) -> Self {
            let dir = std::env::temp_dir().join(format!("omalux-app-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            for (i, &rating) in ratings.iter().enumerate() {
                let raw = dir.join(format!("DSC{:05}.ARW", i + 1));
                std::fs::write(&raw, "").unwrap();
                if rating != 0 {
                    sidecar::update(&raw, |xmp| sidecar::with_rating(xmp, rating)).unwrap();
                }
            }
            Self(dir)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn current(h: &Harness) -> String {
        h.app.shoot.as_ref().unwrap().frame().name()
    }

    #[test]
    fn a_shoot_opens_by_its_folder_or_a_raw_in_it() {
        let folder = Folder::new("open", &[1, 0, 1]);
        let dir = &folder.0;
        let mut h = Harness::new(&[]);
        assert_eq!(titles(&h.frame(vec![])), ["OmaLux"]);
        h.app.open(dir.clone());
        assert_eq!(h.app.shoot.as_ref().map(Shoot::dir), Some(dir.as_path()));
        let name = dir.file_name().unwrap().to_string_lossy();
        assert_eq!(titles(&h.frame(vec![])), [format!("{name} — OmaLux")]);
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   1 / 2 picks of 3 frames");

        // A raw the filter leaves out is opened all the same.
        h.app.open(dir.join("DSC00002.ARW"));
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00002.ARW   2 / 3 frames");

        h.app.open(dir.join("gone/DSC01234.ARW"));
        assert_eq!(current(&h), "DSC00002.ARW", "the shoot that was open stays");
        assert!(h.app.message.as_ref().is_some_and(|(text, problem)| *problem && text.contains("no such folder")));
    }

    #[test]
    fn the_loupe_shows_the_cameras_jpeg_then_the_render_and_neighbours_are_ready() {
        let folder = Folder::new("loupe", &[1, 1, 1, 1, 1]);
        std::fs::write(folder.0.join("DSC00003.ARW"), "broken").unwrap();
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let size = |h: &Harness, name: &str| {
            let shown = h.app.loupe.shown(&folder.0.join(name))?;
            Some((shown.preview.as_ref().map(|t| t.size()), shown.render.as_ref().map(|r| (r.texture.size(), r.settings.wb.temp))))
        };
        // The test decoder's 12×8 JPEG and 48×32 raw, the raw developed as shot at its own size or the loupe's.
        let developed = Some((Some([12, 8]), Some(([48, 32], 5200.0))));
        assert_eq!(size(&h, "DSC00001.ARW"), developed);
        assert_eq!(size(&h, "DSC00002.ARW"), developed, "the next frame is ready before it's stepped to");
        assert_eq!(size(&h, "DSC00003.ARW"), Some((Some([12, 8]), None)), "a raw that can't be developed keeps its JPEG");
        assert!(h.app.message.as_ref().is_some_and(|(text, _)| text.contains("Couldn't develop DSC00003.ARW")), "{:?}", h.app.message);
        assert_eq!(size(&h, "DSC00004.ARW"), None, "two ahead and no further");

        h.press(Modifiers::NONE, Key::End);
        h.settle();
        assert_eq!(size(&h, "DSC00005.ARW"), developed);
        assert_eq!(size(&h, "DSC00001.ARW"), None, "frames the cursor has left are forgotten");
    }

    #[test]
    fn the_keys_nudge_the_selected_slider_and_every_step_is_saved_and_undone() {
        let folder = Folder::new("keys", &[1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        assert_eq!(h.saved(), None, "nothing is written on opening a frame");

        // Exposure is selected to begin with.
        h.press(Modifiers::NONE, Key::Equals);
        h.press(Modifiers::NONE, Key::Equals);
        assert!((h.settings().light.exposure - 0.10).abs() < 1e-9);
        h.press(Modifiers::SHIFT, Key::Minus);
        assert!((h.settings().light.exposure + 0.15).abs() < 1e-9, "Shift: five nudges");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        assert_eq!(sidecar::rating(&sidecar::read(&folder.0.join("DSC00001.ARW")).unwrap().unwrap()).unwrap(), Some(1), "the mark stays");

        // . and , move the selection: Contrast, then back past Exposure to Tint.
        h.press(Modifiers::NONE, Key::Period);
        h.press(Modifiers::NONE, Key::Equals);
        assert_eq!(h.settings().light.contrast, 1.0);
        h.press(Modifiers::NONE, Key::Comma);
        h.press(Modifiers::NONE, Key::Comma);
        h.press(Modifiers::NONE, Key::Minus);
        let wb = h.settings().wb;
        assert_eq!((wb.mode, wb.temp, wb.tint), (lightcraft_develop::WbMode::Custom, 5200.0, 1.0), "from as shot, 3");

        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!((h.settings().wb.mode, h.settings().light.contrast), (lightcraft_develop::WbMode::AsShot, 1.0));
        assert_eq!(h.saved().as_ref(), Some(h.settings()), "undo is saved too");
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        assert_eq!(h.settings().wb.tint, 1.0);
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::R);
        assert_eq!(h.settings(), &DevelopSettings::for_raw(5200.0, 3.0), "reset");
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().wb.tint, 1.0, "and reset is a step like any other");

        // Each frame has its own steps: the next one has none to undo.
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.settle();
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!((h.settings(), h.saved()), (&DevelopSettings::for_raw(5200.0, 3.0), None));
        h.press(Modifiers::NONE, Key::ArrowLeft);
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().wb.mode, lightcraft_develop::WbMode::AsShot, "and the first one's are still there");
    }

    #[test]
    fn a_drag_shows_drafts_and_is_one_step_saved_when_it_ends() {
        let folder = Folder::new("drag", &[1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let raw = folder.0.join("DSC00001.ARW");
        let render = |h: &Harness| h.app.loupe.shown(&raw).unwrap().render.as_ref().map(|r| (r.key.draft, r.settings.light.contrast)).unwrap();

        let track = h.track("light.contrast");
        h.pointer(track.center(), None);
        h.pointer(track.center(), Some(true));
        for step in 1..=4 {
            h.pointer(track.center() + egui::vec2(track.width() / 16.0 * step as f32, 0.0), None);
        }
        assert_eq!(h.settings().light.contrast, 50.0, "a quarter of the track right of the middle");
        assert!(h.app.dragging && h.app.selected == 3, "the slider dragged is the selected one");
        assert_eq!(h.saved(), None, "nothing is saved until the drag ends");
        h.settle();
        assert_eq!(render(&h), (true, 50.0), "a draft while it's dragged");

        h.pointer(track.center() + egui::vec2(track.width() / 4.0, 0.0), Some(false));
        assert!(!h.app.dragging);
        assert_eq!(h.saved().map(|s| s.light.contrast), Some(50.0));
        h.settle();
        assert_eq!(render(&h), (false, 50.0), "and the full render once it's let go");
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().light.contrast, 0.0, "the whole drag is one step");

        // A double-click on the track puts a slider back.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        for _ in 0..2 {
            h.pointer(track.center(), Some(true));
            h.pointer(track.center(), Some(false));
        }
        assert_eq!((h.settings().light.contrast, h.saved().map(|s| s.light.contrast)), (0.0, Some(0.0)));
    }

    #[test]
    fn a_frame_opens_with_the_edit_in_its_sidecar() {
        let folder = Folder::new("saved", &[1, 1]);
        let raw = folder.0.join("DSC00002.ARW");
        let mut edited = DevelopSettings::for_raw(5200.0, 3.0);
        edited.light.exposure = 0.7;
        sidecar::update(&raw, |xmp| sidecar::with_edit(xmp, &sidecar::Edit::new(edited.clone()))).unwrap();

        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let ahead = h.app.loupe.shown(&raw).unwrap().render.as_ref().unwrap();
        assert_eq!(ahead.settings.light.exposure, 0.7, "prepared ahead with its own edit");
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.settle();
        assert_eq!(h.settings(), &edited);
        // Reset goes back to a fresh raw, not to the saved edit.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::R);
        assert_eq!(h.settings().light.exposure, 0.0);
    }

    #[test]
    fn the_arrows_step_through_the_picks_and_ctrl_l_shows_everything() {
        let folder = Folder::new("step", &[1, 0, sidecar::REJECT, 2]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        assert_eq!(current(&h), "DSC00001.ARW");
        h.press(Modifiers::NONE, Key::ArrowRight);
        assert_eq!(current(&h), "DSC00004.ARW");
        h.press(Modifiers::NONE, Key::ArrowRight);
        assert_eq!(current(&h), "DSC00004.ARW", "the last pick");
        h.press(Modifiers::COMMAND, Key::L);
        h.press(Modifiers::NONE, Key::ArrowLeft);
        assert_eq!(current(&h), "DSC00003.ARW");
        h.press(Modifiers::NONE, Key::Home);
        assert_eq!(current(&h), "DSC00001.ARW");
        h.press(Modifiers::NONE, Key::End);
        assert_eq!(current(&h), "DSC00004.ARW");
    }
}
