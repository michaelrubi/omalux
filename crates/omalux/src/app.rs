//! The window. For now (M0) an empty one: the theme, the keys, the status
//! bar and a folder to open, the shell M1 builds the develop view in. Its
//! patterns (a `Command` a key or a script runs, `build` for tests, the
//! `Harness`) are Omacull's.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Align2, FontId, RichText, Ui};

use crate::commands::Command;
use crate::hotkeys::{self, format_shortcut};
use crate::theme::{self, Theme};

pub struct App {
    theme: Theme,
    theme_rx: Receiver<Theme>,
    /// Problems found at startup, shown in the status bar.
    warnings: Vec<String>,
    /// Commands to run, one a frame, from `OMALUX_SCRIPT` (comma-separated
    /// command names, e.g. `Open,Quit`). For testing without a keyboard.
    script: VecDeque<Command>,
    /// The shoot open, its folder.
    folder: Option<PathBuf>,
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
            folder: None,
            picking: None,
            message: None,
            title: String::new(),
        }
    }

    /// Open a shoot: its folder, or a raw in it.
    fn open(&mut self, path: PathBuf) {
        let dir = if path.is_dir() { path } else { path.parent().map(Path::to_path_buf).unwrap_or(path) };
        if !dir.is_dir() {
            self.message = Some((format!("Couldn't open {}: there's no such folder", dir.display()), true));
            return;
        }
        self.folder = Some(dir);
        self.message = None;
    }

    fn pick(&mut self, ctx: &egui::Context) {
        if self.picking.is_some() {
            return;
        }
        let mut dialog = rfd::FileDialog::new().set_title("Open Folder");
        if let Some(dir) = &self.folder {
            dialog = dialog.set_directory(dir);
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
        let title = match &self.folder {
            Some(dir) => format!("{} — OmaLux", file_name(dir)),
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
        let pasteboard = egui::Frame::new().fill(self.theme.pasteboard());
        egui::CentralPanel::no_frame().frame(pasteboard).show(ui, |ui| {
            if self.folder.is_none() {
                let hint = match hotkeys::current().command(Command::Open) {
                    Some(s) => format!("{} to open a shoot", format_shortcut(&s)),
                    None => "Open a shoot".to_owned(),
                };
                let rect = ui.max_rect();
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, hint, FontId::proportional(16.0), self.theme.dark_foreground);
            }
        });
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
                    let (text, colour) = match &self.folder {
                        Some(dir) => (dir.display().to_string(), self.theme.foreground),
                        None => ("No folder open".to_owned(), self.theme.dark_foreground),
                    };
                    ui.add(egui::Label::new(RichText::new(text).color(colour)).truncate());
                });
            });
        });
    }
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

    #[test]
    fn a_shoot_opens_by_its_folder_or_a_raw_in_it() {
        let dir = std::env::temp_dir().join(format!("omalux-app-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = Harness::new(&[]);
        assert_eq!(titles(&h.frame(vec![])), ["OmaLux"]);
        h.app.open(dir.join("DSC01234.ARW"));
        assert_eq!(h.app.folder.as_deref(), Some(dir.as_path()));
        let name = dir.file_name().unwrap().to_string_lossy();
        assert_eq!(titles(&h.frame(vec![])), [format!("{name} — OmaLux")]);

        h.app.open(dir.join("gone/DSC01234.ARW"));
        assert_eq!(h.app.folder.as_deref(), Some(dir.as_path()));
        assert!(h.app.message.as_ref().is_some_and(|(text, problem)| *problem && text.contains("no such folder")));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
