//! The window: a shoot's frames in the loupe, the filmstrip under it and
//! the develop panel on the right, as Lightroom's Develop module has them.
//! Its patterns (a `Command` a key or a script runs, `build` for tests,
//! the `Harness`) are Omacull's.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Align2, FontId, RichText, Ui};
use omalux_engine::shoot::{Filter, Shoot, Step};

use crate::commands::Command;
use crate::filmstrip::Filmstrip;
use crate::hotkeys::{self, format_shortcut};
use crate::theme::{self, Theme};

/// The filmstrip's height, as Omacull's.
const STRIP_HEIGHT: f32 = 104.0;
/// The develop panel's width.
const PANEL_WIDTH: f32 = 300.0;

pub struct App {
    theme: Theme,
    theme_rx: Receiver<Theme>,
    /// Problems found at startup, shown in the status bar.
    warnings: Vec<String>,
    /// Commands to run, one a frame, from `OMALUX_SCRIPT` (comma-separated
    /// command names, e.g. `Open,Quit`). For testing without a keyboard.
    script: VecDeque<Command>,
    shoot: Option<Shoot>,
    /// The folder picker, while it's open.
    picking: Option<Receiver<Option<PathBuf>>>,
    /// The last thing to tell the user, and whether it's a problem.
    message: Option<(String, bool)>,
    title: String,
    filmstrip: Filmstrip,
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
        let mut app = Self::build(theme, theme::watch(ctx.clone()), warnings, script);
        if let Some(path) = open {
            app.open(path);
        }
        app
    }

    fn build(theme: Theme, theme_rx: Receiver<Theme>, warnings: Vec<String>, script: VecDeque<Command>) -> Self {
        Self {
            theme,
            theme_rx,
            warnings,
            script,
            shoot: None,
            picking: None,
            message: None,
            title: String::new(),
            filmstrip: Filmstrip::new(),
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
        }
    }

    fn step_to(&mut self, step: Step) {
        if let Some(shoot) = &mut self.shoot {
            shoot.step(step);
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
        if !ctx.egui_wants_keyboard_input() {
            for command in Command::pressed(ctx) {
                self.run(command, ctx);
            }
        }
        self.run_script(ctx);
        if let Some(shoot) = &self.shoot {
            self.filmstrip.sync_shoot(shoot, ctx);
            self.filmstrip.collect(ctx, shoot.current());
        }
        let title = match &self.shoot {
            Some(shoot) => format!("{} — OmaLux", file_name(shoot.dir())),
            None => "OmaLux".into(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
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
            Some(_) => self.loupe(ui),
            None => self.empty_state(ui),
        });
    }

    /// The shown frames' thumbnails, under the loupe.
    fn filmstrip(&mut self, ui: &mut Ui) {
        let Some(shoot) = &mut self.shoot else { return };
        if let Some(clicked) = self.filmstrip.show(ui, shoot, &self.theme) {
            shoot.go_to(clicked);
        }
    }

    /// The histogram, and the panels under it: White Balance, Light,
    /// Presence.
    fn develop_panel(&mut self, _ui: &mut Ui) {}

    /// The current frame.
    fn loupe(&mut self, _ui: &mut Ui) {}

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
            let app = App::build(Theme::default(), rx, Vec::new(), script.iter().copied().collect());
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

        /// A key pressed and let go in one frame.
        fn press(&mut self, modifiers: Modifiers, key: Key) -> egui::FullOutput {
            let mut events = key_events(modifiers, key, true);
            events.extend(key_events(modifiers, key, false));
            self.frame(events)
        }

        fn click(&mut self, at: egui::Pos2) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at), button(true)]);
            self.frame(vec![button(false)]);
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

    #[test]
    fn clicking_a_thumbnail_goes_to_it() {
        let folder = Folder::new("click", &[1, 1, 1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.frame(vec![]);
        let cells = h.app.filmstrip.cells.clone();
        assert!(!cells.is_empty(), "cells are present");
        let (index, rect) = cells.iter().copied().find(|&(i, _)| i == 2).unwrap();
        h.click(rect.center());
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), index);
    }

    #[test]
    fn only_the_shown_frames_are_in_the_strip() {
        let folder = Folder::new("strip-filter", &[1, 0, 1, 0, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.frame(vec![]);
        let shown: Vec<usize> = h.app.filmstrip.cells.iter().map(|&(i, _)| i).collect();
        assert_eq!(shown, vec![0, 2, 4], "picks only");
        h.press(Modifiers::COMMAND, Key::L);
        let all: Vec<usize> = h.app.filmstrip.cells.iter().map(|&(i, _)| i).collect();
        assert_eq!(all, vec![0, 1, 2, 3, 4], "all frames");
    }
}
