//! The window: a shoot's frames in the loupe, the filmstrip under it and
//! the develop panel on the right, as Lightroom's Develop module has them.
//! Its patterns (a `Command` a key or a script runs, `build` for tests,
//! the `Harness`) are Omacull's.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Align2, FontId, RichText, Ui};
use lightcraft_codecs::display::DisplayProfile;
use lightcraft_develop::DevelopSettings;
use lightcraft_pipeline::SourceInfo;
use omalux_engine::auto::{self, Auto};
use omalux_engine::copy::{self, Copied, Group};
use omalux_engine::crop;
use omalux_engine::disk::{Disk, Mark, Problem};
use omalux_engine::edit::{self, History};
use omalux_engine::look::{self, Camera, Profile};
use omalux_engine::shoot::{Filter, Shoot, Step, stars};
use omalux_engine::sidecar;

use crate::checklist::{Checklist, Outcome};
use crate::commands::Command;
use crate::config::Config;
use crate::develop::{self, PanelOut};
use crate::filmstrip::{Click, Filmstrip, STRIP_HEIGHT};
use crate::hotkeys::{self, format_shortcut};
use crate::crop_panel;
use crate::loupe::{Compare, Loupe, Mouse, News};
use crate::monitor;
use crate::render::{Decoder, Renderer, View, Wanted};
use crate::slider::SliderOut;
use crate::state::{Place, State};
use crate::theme::{self, Theme};

/// The develop panel's width.
const PANEL_WIDTH: f32 = 300.0;
/// How many frames are developed ahead of the cursor, the way it's
/// going, and behind it.
const AHEAD: usize = 2;
const BEHIND: usize = 1;

/// Lightroom's loupe info overlays: what each shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Info {
    /// The file name, when it was shot, and its size cropped.
    One,
    /// The file name, the exposure, and the lens.
    Two,
}

/// A frame whose raw has been decoded this session, or that's been pasted
/// onto: its edit, and what its settings are read against.
struct Editing {
    history: History,
    /// None for a frame pasted onto whose raw hasn't been decoded yet:
    /// only a paste and its undo reach it until it has.
    info: Option<Arc<SourceInfo>>,
    /// What the camera did to its own JPEG, once its raw is decoded.
    camera: Arc<Camera>,
    /// The history's settings, as the renderer is handed them.
    settings: Arc<DevelopSettings>,
}

impl Editing {
    fn new(history: History, decoded: Option<(Arc<SourceInfo>, Arc<Camera>)>) -> Self {
        let settings = Arc::new(history.settings().clone());
        let (info, camera) = decoded.unzip();
        Self { history, info, camera: camera.unwrap_or_default(), settings }
    }

    /// Change the edit with `change`, which says whether it did. Nothing
    /// happens until the frame's raw has been decoded.
    fn edit(&mut self, change: impl FnOnce(&mut History, &SourceInfo) -> bool) -> bool {
        let Some(info) = &self.info else { return false };
        let changed = change(&mut self.history, info);
        self.stepped();
        changed
    }

    /// Take up the history's settings after a step. True if they're not
    /// what they were: there's something to save.
    fn stepped(&mut self) -> bool {
        let changed = *self.settings != *self.history.settings();
        if changed {
            self.settings = Arc::new(self.history.settings().clone());
        }
        changed
    }
}

/// Where OmaLux keeps what it remembers; somewhere else in tests.
pub struct Paths {
    /// Where state is saved (`state.toml`); nowhere in most tests.
    pub state: Option<PathBuf>,
    /// Where the filmstrip's thumbnails are cached; nowhere in tests.
    pub thumbnails: Option<PathBuf>,
    /// Where chosen monitor profiles are kept (`monitors.toml`); nowhere in tests.
    pub monitors: Option<PathBuf>,
}

/// The threads behind the window: a test's renderer decodes what the
/// test gives it.
pub struct Workers {
    pub renderer: Renderer,
    pub disk: Disk,
}

/// Frames being exported to a folder, and how it's gone so far.
struct Exporting {
    to: PathBuf,
    /// The TIFF files that were asked for, including those already present.
    tiffs: Vec<PathBuf>,
    total: usize,
    exported: usize,
    /// There already, and left as they were.
    there: usize,
    /// Those that couldn't be exported, and why.
    failed: Vec<String>,
    /// Whether to open in the configured program once done.
    edit: bool,
    /// Whether each was exported, as it's dealt with.
    results: Receiver<Result<bool, String>>,
}

impl Exporting {
    /// How it went, and whether that's a problem.
    fn said(&self) -> (String, bool) {
        let to = file_name(&self.to);
        if let Some(first) = self.failed.first() {
            return (format!("Couldn't export {} of {} to {to}, e.g. {first}", self.failed.len(), self.total), true);
        }
        let there = match self.there {
            0 => String::new(),
            1 => "; 1 was there already".to_owned(),
            n => format!("; {n} were there already"),
        };
        (format!("Exported {} of {} to {to}{there}", self.exported, self.total), false)
    }
}

pub struct App {
    theme: Theme,
    theme_rx: Receiver<Theme>,
    config: Config,
    /// Problems found at startup, shown in the status bar.
    warnings: Vec<String>,
    /// Commands to run, one a frame, from `OMALUX_SCRIPT` (comma-separated
    /// command names, e.g. `Open,Quit`). For testing without a keyboard.
    script: VecDeque<Command>,
    state: State,
    shoot: Option<Shoot>,
    renderer: Renderer,
    loupe: Loupe,
    /// The edits their sidecars held, of frames not decoded yet: read
    /// when a frame is first wanted.
    saved: HashMap<PathBuf, Option<Arc<DevelopSettings>>>,
    /// Frames whose sidecar holds an edit this OmaLux can't read (a newer
    /// one's, say): shown as fresh raws and never written over.
    unreadable: HashSet<PathBuf>,
    /// The frames edited or looked at this session, each with its steps
    /// to undo.
    edits: HashMap<PathBuf, Editing>,
    /// The slider the keys act on, by its place in the panel.
    selected: usize,
    /// A slider is being dragged: the loupe gets drafts.
    dragging: bool,
    /// The White Balance Selector is out: a click on the frame picks.
    selector: bool,
    /// The loupe shows the current frame with no edit, until it's left.
    before: bool,
    /// A before/after view (Y, Alt+Y, Shift+Y), until it's put away: it
    /// stays from frame to frame, as Lightroom's does.
    compare: Option<Compare>,
    /// The keyboard shortcuts legend is open.
    legend: bool,
    /// The info overlay shown over the loupe, if one is (I), and the one
    /// Ctrl+I shows again.
    info: Option<Info>,
    info_last: Info,
    /// The groups of settings whose panels are switched off: the loupe
    /// shows every frame without them, until they're on again or another
    /// shoot is opened. Never saved.
    off: Vec<Group>,
    /// The folder picker, while it's open.
    picking: Option<Receiver<Option<PathBuf>>>,
    /// Frames being exported, while the export runs.
    exporting: Option<Exporting>,
    /// The last thing to tell the user, and whether it's a problem.
    message: Option<(String, bool)>,
    title: String,
    disk: Disk,
    filmstrip: Filmstrip,
    /// The clipping warnings are on: blown highlights and crushed
    /// shadows are coloured in the loupe.
    clipping: bool,
    watch: monitor::Watch,
    pub display: Option<Arc<DisplayProfile>>,
    /// Whether V is held, between frames: how Ctrl+V is told (see
    /// [`Command::pressed`]).
    v_down: bool,
    /// The settings last copied, for the session.
    clipboard: Option<Copied>,
    /// How many pastes there have been: each one's number is on its step
    /// in every frame it reached, so it's undone on all of them at once.
    pastes: u64,
    /// The frame the cursor is on, and the one it came from: whose
    /// settings Paste from Previous takes.
    at: Option<PathBuf>,
    previous: Option<PathBuf>,
    /// The checklist of what to copy, while it's open.
    choosing: Option<Checklist>,
    /// The Crop & Straighten tool, while it's open: the loupe shows the
    /// current frame whole, with its crop drawn over it.
    crop: Option<crate::crop::Tool>,
    /// Guided Upright's tool, while it's open: the loupe shows the current
    /// frame through its lens corrections alone, with its lines over it.
    guided: Option<crate::guided::Tool>,
    /// The crop keeps its shape when it's dragged (Lightroom's lock, A),
    /// from one frame and one opening of the tool to the next.
    crop_locked: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        let ctx = &cc.egui_ctx;
        // Ctrl+= / Ctrl+- are for zooming the image, not the interface.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        theme::install_font(ctx);
        let theme = Theme::load();
        ctx.set_visuals(theme.visuals());

        let (config, config_warnings) = Config::load();
        let mut warnings = hotkeys::load();
        warnings.extend(config_warnings);
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
        let wake = ctx.clone();
        let disk = Disk::new(move || wake.request_repaint());
        let paths = Paths {
            state: hotkeys::config_dir().map(|dir| dir.join("state.toml")),
            thumbnails: omalux_engine::thumbs::default_dir(),
            monitors: hotkeys::config_dir().map(|dir| dir.join("monitors.toml")),
        };
        let mut app = Self::build(theme, theme::watch(ctx.clone()), config, warnings, script, Workers { renderer, disk }, paths);
        if let Some(path) = open {
            app.open(path);
        }
        app
    }

    fn build(
        theme: Theme,
        theme_rx: Receiver<Theme>,
        config: Config,
        warnings: Vec<String>,
        script: VecDeque<Command>,
        workers: Workers,
        paths: Paths,
    ) -> Self {
        let state = paths.state.map_or_else(State::default, State::load_from);
        let watch = paths.monitors.map_or_else(monitor::Watch::off, |dir| monitor::Watch::new(Some(dir)));
        let display = watch.profile().cloned();
        let mut filmstrip = Filmstrip::new(paths.thumbnails);
        filmstrip.set_display(display.clone());
        workers.renderer.set_display(display.clone());
        Self {
            theme,
            theme_rx,
            config,
            warnings,
            script,
            state,
            shoot: None,
            renderer: workers.renderer,
            loupe: Loupe::default(),
            saved: HashMap::new(),
            unreadable: HashSet::new(),
            edits: HashMap::new(),
            selected: develop::sliders().position(|spec| spec.id == "light.exposure").unwrap_or(0),
            dragging: false,
            selector: false,
            before: false,
            compare: None,
            legend: false,
            info: None,
            info_last: Info::One,
            off: Vec::new(),
            picking: None,
            exporting: None,
            message: None,
            title: String::new(),
            disk: workers.disk,
            filmstrip,
            clipping: false,
            watch,
            display,
            v_down: false,
            clipboard: None,
            pastes: 0,
            at: None,
            previous: None,
            choosing: None,
            crop: None,
            guided: None,
            crop_locked: true,
        }
    }

    /// Remember where the open shoot was left, to come back to it.
    fn remember_place(&mut self) {
        if let Some(shoot) = &self.shoot {
            let frame = shoot.frame().name();
            self.state.remember(Place { dir: shoot.dir().to_path_buf(), frame, filter: shoot.filter() });
        }
    }

    /// Open a shoot: its folder, or a raw in it.
    fn open(&mut self, path: PathBuf) {
        let raw = (!path.is_dir()).then(|| path.clone());
        let dir = if path.is_dir() { path.clone() } else { path.parent().map(Path::to_path_buf).unwrap_or(path.clone()) };
        if !dir.is_dir() {
            self.state.remove_recent(&dir);
            self.message = Some((format!("Couldn't open {}: there's no such folder", dir.display()), true));
            return;
        }
        match Shoot::open(&dir) {
            Ok((mut shoot, unread)) => {
                self.remember_place();
                let place = self.state.place(shoot.dir()).cloned();
                if let Some(raw) = raw {
                    if !shoot.go_to_path(&raw) {
                        log::warn!("{} isn't a raw in the shoot", path.display());
                    }
                } else if let Some(place) = place {
                    shoot.set_filter(place.filter);
                    if let Some(index) = shoot.frames().iter().position(|f| f.name() == place.frame) {
                        shoot.go_to(index);
                    }
                }
                self.state.add_recent(shoot.dir());
                self.message = (!unread.is_empty()).then(|| (format!("Couldn't read {}", unread.join("; ")), true));
                self.shoot = Some(shoot);
                self.forget();
            }
            Err(e) => self.message = Some((format!("Couldn't open {}: {e}", dir.display()), true)),
        }
    }

    /// Close the open shoot, back to the empty state: its place is
    /// remembered, as when another is opened.
    fn close(&mut self) {
        self.remember_place();
        self.shoot = None;
        self.message = None;
        self.forget();
    }

    /// Forget what was held of the shoot that was open. What was copied
    /// stays: it can be pasted in the next shoot.
    fn forget(&mut self) {
        self.saved.clear();
        self.unreadable.clear();
        self.edits.clear();
        self.filmstrip.reset();
        (self.at, self.previous) = (None, None);
        self.choosing = None;
        (self.crop, self.guided) = (None, None);
        self.off.clear();
        self.leave();
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

    fn export(&mut self, edit: bool, ctx: &egui::Context) {
        let Some(shoot) = &self.shoot else { return };
        if self.exporting.is_some() {
            return;
        }
        let frames = targets(shoot);
        self.export_frames(frames, edit, ctx);
    }

    /// Copy `groups` of the current frame's settings. Nothing happens
    /// until the frame's raw has been decoded.
    fn copy(&mut self, groups: &[Group]) {
        let Some(shoot) = &self.shoot else { return };
        let Some(editing) = self.edits.get(&shoot.frame().path) else { return };
        self.clipboard = Some(Copied::new(editing.history.settings(), groups));
        let what = if groups == Group::ALL { "every setting".to_owned() } else { listed(groups) };
        self.message = Some((format!("Copied {what} of {}", shoot.frame().name()), false));
    }

    fn export_frames(&mut self, frames: Vec<PathBuf>, edit: bool, ctx: &egui::Context) {
        let Some(shoot) = &self.shoot else { return };
        if frames.is_empty() || self.exporting.is_some() {
            return;
        }
        let to = shoot.dir().join(&self.config.export_folder);
        let tiffs: Vec<PathBuf> = frames
            .iter()
            .map(|f| {
                let stem = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                to.join(format!("{stem}.tif"))
            })
            .collect();
        let (tx, rx) = channel();
        // Each with its edit as it stands, where it's been opened this
        // session; the rest are exported with what their sidecars hold.
        let to_export: Vec<(PathBuf, Option<Arc<DevelopSettings>>)> =
            frames.iter().map(|frame| (frame.clone(), self.edits.get(frame).map(|editing| editing.settings.clone()))).collect();
        let (ctx, target) = (ctx.clone(), to.clone());
        let total = frames.len();
        self.message = Some((format!("Exporting 1 of {total}…"), false));
        self.exporting = Some(Exporting {
            to,
            tiffs,
            total,
            exported: 0,
            there: 0,
            failed: Vec::new(),
            edit,
            results: rx,
        });
        std::thread::spawn(move || {
            for (frame, settings) in to_export {
                let res = omalux_engine::export::export(&frame, &target, settings.as_deref())
                    .map_err(|e| format!("{}: {e}", file_name(&frame)));
                if tx.send(res).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        });
    }

    fn finish_exporting(&mut self) {
        let Some(exporting) = &mut self.exporting else { return };
        let mut more = false;
        loop {
            match exporting.results.try_recv() {
                Ok(Ok(true)) => exporting.exported += 1,
                Ok(Ok(false)) => exporting.there += 1,
                Ok(Err(why)) => exporting.failed.push(why),
                Err(TryRecvError::Empty) if more => {
                    let done = exporting.exported + exporting.there + exporting.failed.len();
                    if done < exporting.total {
                        let text = format!("Exporting {} of {}…", done + 1, exporting.total);
                        return self.message = Some((text, false));
                    }
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => break,
            }
            more = true;
        }
        let Some(exporting) = self.exporting.take() else { return };
        // Only what's there: a frame that couldn't be exported has no TIFF.
        let tiffs: Vec<&PathBuf> = exporting.tiffs.iter().filter(|tiff| tiff.exists()).collect();
        if exporting.edit && !tiffs.is_empty() {
            let started = std::process::Command::new(&self.config.editor)
                .arg("--round-trip")
                .args(tiffs)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            match started {
                Ok(mut child) => {
                    std::thread::spawn(move || child.wait());
                    self.message = Some(exporting.said());
                }
                Err(e) => {
                    self.message = Some((format!("Couldn't start {}: {e}", self.config.editor), true));
                }
            }
        } else {
            self.message = Some(exporting.said());
        }
    }

    fn run(&mut self, command: Command, ctx: &egui::Context) {
        match command {
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Open => self.pick(ctx),
            Command::Close => self.close(),
            Command::Export => self.export(false, ctx),
            Command::ExportAndEdit => self.export(true, ctx),
            Command::Previous => self.step_to(Step::Previous),
            Command::Next => self.step_to(Step::Next),
            Command::First => self.step_to(Step::First),
            Command::Last => self.step_to(Step::Last),
            Command::Filter => {
                if let Some(shoot) = &mut self.shoot {
                    shoot.set_filter(if shoot.filter() == Filter::Picks { Filter::All } else { Filter::Picks });
                }
            }
            Command::Undo => self.undo(false),
            Command::Redo => self.undo(true),
            Command::Reset => self.edit(|history, _| history.reset()),
            Command::SliderPrevious => self.selected = self.selected.saturating_sub(1),
            Command::SliderNext => self.selected = (self.selected + 1).min(develop::sliders().count() - 1),
            Command::Increase => self.nudge(1.0),
            Command::Decrease => self.nudge(-1.0),
            Command::IncreaseMore => self.nudge(5.0),
            Command::DecreaseMore => self.nudge(-5.0),
            Command::WhiteBalance => {
                self.selector = !self.selector && self.shoot.is_some();
                (self.crop, self.guided, self.compare) = (None, None, None);
            }
            // In a before/after view, back to the loupe.
            Command::BeforeAfter if self.compare.is_some() => self.compare = None,
            Command::BeforeAfter => self.before = !self.before && self.shoot.is_some(),
            Command::CompareSideBySide | Command::CompareTopBottom | Command::CompareSplit => {
                let view = match command {
                    Command::CompareSideBySide => Compare::SideBySide,
                    Command::CompareTopBottom => Compare::TopBottom,
                    _ => Compare::Split,
                };
                self.compare = (self.compare != Some(view) && self.shoot.is_some()).then_some(view);
                if self.compare.is_some() {
                    (self.before, self.selector, self.crop, self.guided) = (false, false, None, None);
                }
            }
            Command::GuidedUpright => self.guided_tool(self.guided.is_none()),
            Command::Clipping => self.clipping = !self.clipping,
            Command::Shortcuts => self.legend = !self.legend,
            // Lightroom's I: Info 1, Info 2, none, and round again.
            Command::InfoCycle => {
                self.info = match self.info {
                    None => Some(Info::One),
                    Some(Info::One) => Some(Info::Two),
                    Some(Info::Two) => None,
                };
                self.info_last = self.info.unwrap_or(self.info_last);
            }
            Command::InfoShow => self.info = if self.info.is_some() { None } else { Some(self.info_last) },
            // The crop tool shows the frame whole.
            Command::Zoom if self.crop.is_some() => {}
            Command::Zoom => {
                let (pointer, time) = ctx.input(|i| (i.pointer.hover_pos(), i.time));
                self.loupe.key_down(pointer, time);
            }
            Command::Crop => {
                self.crop = (self.crop.is_none() && self.shoot.is_some()).then(Default::default);
                (self.selector, self.compare, self.guided) = (false, None, None);
            }
            Command::RotateLeft => self.edit(|history, _| history.set(|settings| crop::turn(settings, false))),
            Command::RotateRight => self.edit(|history, _| history.set(|settings| crop::turn(settings, true))),
            Command::ResetCrop => self.edit(|history, _| history.set(crop::reset)),
            Command::AutoTone => self.auto(Auto::Tone),
            Command::AutoWhiteBalance => self.auto(Auto::WhiteBalance),
            Command::AutoExposure => self.auto(Auto::Exposure),
            Command::AutoLevel => self.auto(Auto::Level),
            Command::Flat => self.edit(|history, _| history.set(|settings| Profile::of(settings).toggled().set(settings))),
            Command::SelectAll => {
                if let Some(shoot) = &mut self.shoot {
                    shoot.select_all();
                }
            }
            Command::SelectNone => {
                if let Some(shoot) = &mut self.shoot {
                    shoot.clear_selection();
                }
            }
            Command::SelectPrevious => self.extend_to(Step::Previous),
            Command::SelectNext => self.extend_to(Step::Next),
            Command::Copy => self.copy(Group::ALL),
            Command::CopyChoose => {
                // Once the frame's raw has been decoded: there's nothing to copy before.
                if let Some(shoot) = &self.shoot
                    && self.edits.contains_key(&shoot.frame().path)
                {
                    self.choosing = Some(Checklist::new(&self.state.copy_groups));
                }
            }
            Command::Paste => {
                if let Some(shoot) = &self.shoot {
                    self.paste(targets(shoot));
                }
            }
            Command::PastePrevious => self.paste_previous(),
            Command::PasteStack => {
                let Some(shoot) = &self.shoot else { return };
                match shoot.stack(shoot.current()) {
                    Some(stack) => self.paste(stack.into_iter().map(|i| shoot.frames()[i].path.clone()).collect()),
                    None => self.message = Some((format!("{} isn't in a stack", shoot.frame().name()), false)),
                }
            }
            Command::ToggleStack => {
                if let Some(shoot) = &mut self.shoot
                    && shoot.toggle_stack()
                {
                    self.leave();
                }
            }
            Command::Pick
            | Command::Reject
            | Command::Unmark
            | Command::Star1
            | Command::Star2
            | Command::Star3
            | Command::Star4
            | Command::Star5 => {
                if let Some(rating) = command.rating()
                    && let Some(shoot) = &mut self.shoot
                    && let Some(change) = shoot.mark(rating)
                {
                    self.disk.write(Mark {
                        path: shoot.frame().path.clone(),
                        rating: change.now,
                    });
                }
            }
        }
    }

    /// Marks that didn't reach their sidecar are put back as they are on
    /// disk; an edit that didn't is said.
    fn disk_problems(&mut self) {
        let problems: Vec<Problem> = self.disk.problems().collect();
        for problem in problems {
            let text = match problem {
                Problem::Sidecar { path, error, on_disk } => {
                    if let Some(shoot) = &mut self.shoot {
                        shoot.restore(&path, on_disk);
                    }
                    format!("Couldn't mark {}: {error}", file_name(&path))
                }
                Problem::Edit { path, error } => format!("Couldn't save {}: {error}", file_name(&sidecar::path_for(&path))),
            };
            self.message = Some((text, true));
        }
    }

    fn step_to(&mut self, step: Step) {
        if let Some(shoot) = &mut self.shoot
            && shoot.step(step)
        {
            self.leave();
        }
    }

    /// Ctrl+V: paste what was copied onto `frames`.
    fn paste(&mut self, frames: Vec<PathBuf>) {
        let Some(copied) = self.clipboard.clone() else {
            let how = hotkeys::current().command(Command::Copy).map(|s| format!(" ({})", format_shortcut(&s))).unwrap_or_default();
            return self.message = Some((format!("Nothing to paste: copy a frame's settings first{how}"), false));
        };
        let pasted = self.paste_onto(&copied, &frames);
        self.message = Some(pasted.said("Pasted onto"));
    }

    /// Ctrl+Alt+V, Lightroom's Paste from Previous: every setting of the
    /// frame the cursor came from, onto the current frame alone.
    fn paste_previous(&mut self) {
        let Some(to) = self.shoot.as_ref().map(|shoot| shoot.frame().path.clone()) else { return };
        let Some(from) = self.previous.clone() else {
            return self.message = Some(("There's no previous frame to paste from yet".to_owned(), false));
        };
        // A frame with no edit is pasted as a fresh raw.
        let settings = self.settings_of(&from).map_or_else(|| History::unseen(None).settings().clone(), |settings| (*settings).clone());
        let pasted = self.paste_onto(&Copied::new(&settings, Group::ALL), &[to]);
        self.message = Some(pasted.said(&format!("Pasted {}'s settings onto", file_name(&from))));
    }

    /// Paste `copied` onto `frames`, decoded yet or not, as one step to
    /// undo however many it reaches, and save each one it changed.
    fn paste_onto(&mut self, copied: &Copied, frames: &[PathBuf]) -> Pasted {
        let mut pasted = Pasted::default();
        let Some(current) = self.shoot.as_ref().map(|shoot| shoot.frame().path.clone()) else { return pasted };
        // What each frame becomes, before anything is changed: a paste
        // that changes none of them is no step.
        let mut changes = Vec::new();
        for path in frames {
            if !self.edits.contains_key(path) {
                // Not decoded this session: its edit is what its sidecar holds.
                let saved = self.settings_of(path).map(|settings| (*settings).clone());
                if !self.unreadable.contains(path) {
                    self.edits.insert(path.clone(), Editing::new(History::unseen(saved), None));
                }
            }
            match self.edits.get(path) {
                Some(editing) if !self.unreadable.contains(path) => {
                    let mut becomes = copied.paste(editing.history.settings(), editing.history.fresh());
                    // A crop from a frame of another shape, kept inside this
                    // one: where the frame's raw is decoded, so its shape is known.
                    if let Some(source) = self.renderer.source(path) {
                        let dims = crop::dims((source.width, source.height), &becomes);
                        crop::constrain(&mut becomes, dims);
                        // An automatic Upright, found on this frame.
                        if let Some(info) = editing.info.as_deref().filter(|_| becomes.geometry.upright_transform.is_none()) {
                            edit::refresh_upright(&mut becomes, info, &source);
                        }
                    }
                    if becomes != *editing.history.settings() {
                        changes.push((path.clone(), becomes));
                    }
                }
                _ => pasted.unreadable += 1,
            }
        }
        if changes.is_empty() {
            return pasted;
        }
        self.pastes += 1;
        // The frame it's made from holds the step too, changed or not, so
        // undo right after a paste takes it back.
        if !changes.iter().any(|(path, _)| *path == current)
            && let Some(editing) = self.edits.get_mut(&current)
        {
            let same = editing.history.settings().clone();
            editing.history.paste(self.pastes, same);
        }
        for (path, becomes) in changes {
            if let Some(editing) = self.edits.get_mut(&path) {
                editing.history.paste(self.pastes, becomes);
                editing.stepped();
                self.save(&path);
                pasted.frames.push(path);
            }
        }
        pasted
    }

    /// Ctrl+Z and Ctrl+Shift+Z: take back the current frame's last step,
    /// or make it again. A paste is one step however many frames it
    /// reached: it's undone on every frame where it's still the last.
    fn undo(&mut self, redo: bool) {
        let Some(current) = self.shoot.as_ref().map(|shoot| shoot.frame().path.clone()) else { return };
        if self.locked(&current) {
            return;
        }
        let step = |history: &mut History| if redo { history.redo() } else { history.undo() };
        let next = |history: &History| if redo { history.redoes() } else { history.undoes() };
        let Some(editing) = self.edits.get_mut(&current) else { return };
        let paste = next(&editing.history);
        step(&mut editing.history);
        let mut changed: Vec<PathBuf> = editing.stepped().then(|| current.clone()).into_iter().collect();
        if let Some(paste) = paste {
            for (path, editing) in &mut self.edits {
                if *path != current && next(&editing.history) == Some(paste) {
                    step(&mut editing.history);
                    if editing.stepped() {
                        changed.push(path.clone());
                    }
                }
            }
            if !changed.is_empty() {
                let what = if redo { "Pasted again onto" } else { "Took the paste back from" };
                self.message = Some((format!("{what} {}", counted(&changed)), false));
            }
        }
        for path in changed {
            self.save(&path);
        }
    }

    /// Shift+arrow: step, taking the frames passed into the selection.
    fn extend_to(&mut self, step: Step) {
        if let Some(shoot) = &mut self.shoot
            && shoot.extend(step)
        {
            self.leave();
        }
    }

    /// What ends when the cursor leaves a frame: a drag, the White Balance
    /// Selector, the look at Before.
    fn leave(&mut self) {
        (self.dragging, self.selector, self.before) = (false, false, false);
    }

    /// The White Balance Selector clicked the current frame at `at`, a
    /// share of its width and height: what's there becomes neutral, and
    /// the selector is put away, as Lightroom's is.
    fn pick_white_balance(&mut self, at: [f64; 2]) {
        self.selector = false;
        let Some(source) = self.shoot.as_ref().and_then(|shoot| self.renderer.source(&shoot.frame().path)) else { return };
        self.edit(|history, info| history.set(|settings| edit::pick_white_balance(settings, info, &source, at)));
    }

    /// Change the current frame's edit in one step, and save it. `change`
    /// says whether it changed anything. Nothing happens until the frame's
    /// raw has been decoded.
    fn edit(&mut self, change: impl FnOnce(&mut History, &SourceInfo) -> bool) {
        let Some(path) = self.shoot.as_ref().map(|shoot| shoot.frame().path.clone()) else { return };
        if self.locked(&path) {
            return;
        }
        if self.edits.get_mut(&path).is_some_and(|editing| editing.edit(change)) {
            self.save(&path);
        }
    }

    /// Whether a frame's edit is one this OmaLux can't read, and so mustn't
    /// change: says so if it is.
    fn locked(&mut self, path: &Path) -> bool {
        let locked = self.unreadable.contains(path);
        if locked {
            let text = format!("{}'s edit can't be read by this OmaLux, so it's left as it is", file_name(path));
            self.message = Some((text, true));
        }
        locked
    }

    /// Send a frame's edit to its sidecar, on the disk thread.
    fn save(&mut self, path: &Path) {
        let Some(editing) = self.edits.get(path) else { return };
        self.disk.save(path.to_path_buf(), sidecar::Edit::new(editing.history.settings().clone()));
    }

    /// Move the selected slider by `steps` nudges.
    fn nudge(&mut self, steps: f64) {
        let Some(spec) = develop::sliders().nth(self.selected) else { return };
        // As a touch of the slider would, it turns its panel back on.
        let groups = develop::switches_of(self.selected);
        self.off.retain(|group| !groups.contains(group));
        self.edit(|history, info| {
            let value = lightcraft_develop::controls::get(history.settings(), spec.id).unwrap_or(spec.default);
            history.set(|settings| {
                edit::set_control(settings, info, spec.id, crate::slider::nudged(&spec, value, steps));
            })
        });
        if spec.id.starts_with("optics.") {
            self.refresh_upright();
        }
    }

    /// Set `what` on the current frame from the frame itself, as one step.
    fn auto(&mut self, what: Auto) {
        let Some(path) = self.shoot.as_ref().map(|shoot| shoot.frame().path.clone()) else { return };
        let Some(source) = self.renderer.source(&path) else { return };
        let camera = self.edits.get(&path).map(|editing| editing.camera.clone()).unwrap_or_default();
        let (mut found, mut changed, started) = (true, false, std::time::Instant::now());
        self.edit(|history, info| {
            changed = history.set(|settings| found = auto::apply(settings, info, &camera, &source, what));
            changed
        });
        log::debug!("{what:?} in {:.0} ms", started.elapsed().as_secs_f64() * 1e3);
        // A press that changes nothing says why.
        let text = match what {
            _ if changed => return,
            Auto::Level if !found => "Couldn't find a horizon or uprights to level by",
            _ if !found => "There's no automatic value for that slider",
            Auto::Tone => "Nothing clips that Whites or Blacks could bring in",
            _ => "It's as Auto would set it already",
        };
        self.message = Some((text.to_owned(), false));
    }

    /// The current frame's size as it's shown uncropped, once its raw has
    /// been decoded: what its crop is read against.
    fn dims(&self) -> Option<(f64, f64)> {
        let path = &self.shoot.as_ref()?.frame().path;
        let source = self.renderer.source(path)?;
        Some(crop::dims((source.width, source.height), self.edits.get(path)?.history.settings()))
    }

    /// A change to the current frame's edit under a drag: every change
    /// until the drag has `stopped` is one step, saved then.
    fn dragged(&mut self, change: Option<impl FnOnce(&mut DevelopSettings, &SourceInfo)>, stopped: bool) {
        let Some(path) = self.shoot.as_ref().map(|shoot| shoot.frame().path.clone()) else { return };
        if (change.is_some() || stopped) && self.locked(&path) {
            return;
        }
        let Some(editing) = self.edits.get_mut(&path) else { return };
        if let Some(change) = change {
            editing.edit(|history, info| {
                history.change(|settings| change(settings, info));
                true
            });
        }
        if stopped && editing.edit(|history, _| history.commit()) {
            self.save(&path);
        }
    }

    /// A slider of the current frame's moved, `set` being how its value is
    /// written: a drag's changes are drafts in the loupe and one step once
    /// it ends.
    fn slide(&mut self, slid: SliderOut, set: impl FnOnce(&mut DevelopSettings, &SourceInfo, f64)) {
        self.dragging = (self.dragging || slid.drag_started) && !slid.drag_stopped;
        let change = slid.value.map(|value| move |settings: &mut DevelopSettings, info: &SourceInfo| set(settings, info, value));
        self.dragged(change, slid.drag_stopped);
    }

    /// What was done to the crop on the frame, with the mouse.
    fn crop_gesture(&mut self, out: crate::crop::Out) {
        use crate::crop::Act;
        if out.done {
            self.crop = None;
        }
        let Some(dims) = self.dims() else { return };
        let change = out.act.map(|act| {
            move |settings: &mut DevelopSettings, _: &SourceInfo| match act {
                Act::Drag { start, handle, from, to, keep_ratio } => crop::drag(settings, dims, start, handle, from, to, keep_ratio),
                Act::Turn(angle) => crop::straighten(settings, dims, angle),
                Act::Level { from, to } => crop::straighten_along(settings, dims, from, to),
            }
        });
        self.dragged(change, out.stopped);
    }

    /// Open Guided Upright's tool, which sets the frame's Upright to
    /// Guided, or put it away.
    fn guided_tool(&mut self, open: bool) {
        if !open || self.shoot.is_none() {
            self.guided = None;
            return;
        }
        // What Esc goes back to is the frame before the tool set Guided.
        let mut tool = crate::guided::Tool::default();
        if let Some(shoot) = &self.shoot
            && let Some(editing) = self.edits.get(&shoot.frame().path)
        {
            tool.on(&shoot.frame().path, editing.history.settings());
        }
        self.guided = Some(tool);
        (self.crop, self.selector, self.compare, self.before) = (None, false, None, false);
        self.edit(|history, _| history.set(|settings| settings.geometry.upright = lightcraft_develop::Upright::Guided));
    }

    /// Set the current frame's Upright to `mode`, found on its source, as
    /// one step. Guided opens its tool.
    fn upright(&mut self, mode: lightcraft_develop::Upright) {
        if mode == lightcraft_develop::Upright::Guided {
            return self.guided_tool(true);
        }
        self.guided = None;
        let Some(source) = self.shoot.as_ref().and_then(|shoot| self.renderer.source(&shoot.frame().path)) else { return };
        let started = std::time::Instant::now();
        self.edit(|history, info| history.set(|settings| edit::upright(settings, info, &source, mode)));
        log::debug!("Upright {mode:?} in {:.0} ms", started.elapsed().as_secs_f64() * 1e3);
    }

    /// An automatic Upright on the current frame found again, after the
    /// lens corrections it reads the frame through have changed: part of
    /// the change, not a step of its own.
    fn refresh_upright(&mut self) {
        let Some(path) = self.shoot.as_ref().map(|shoot| shoot.frame().path.clone()) else { return };
        let Some(source) = self.renderer.source(&path) else { return };
        let Some(editing) = self.edits.get_mut(&path) else { return };
        let Some(info) = editing.info.clone() else { return };
        if editing.history.amend(|settings| edit::refresh_upright(settings, &info, &source)) {
            editing.stepped();
            self.save(&path);
        }
    }

    /// An automatic Upright pasted onto a frame before its raw was
    /// decoded, found on it now it is: no step, as the paste was the step.
    fn fill_upright(&mut self, path: &Path) {
        let Some(source) = self.renderer.source(path) else { return };
        let Some(editing) = self.edits.get_mut(path) else { return };
        let geometry = &editing.history.settings().geometry;
        if !edit::upright_is_auto(geometry.upright) || geometry.upright_transform.is_some() || self.unreadable.contains(path) {
            return;
        }
        let Some(info) = editing.info.clone() else { return };
        if editing.history.amend(|settings| edit::refresh_upright(settings, &info, &source)) {
            editing.stepped();
            self.save(path);
        }
    }

    /// What was done in the develop panel.
    fn panel(&mut self, out: PanelOut) {
        if let Some((groups, on)) = out.switch {
            self.off.retain(|group| !groups.contains(group));
            if !on {
                self.off.extend_from_slice(groups);
            }
        }
        if let Some(profile) = out.profile {
            self.edit(|history, _| history.set(|settings| profile.set(settings)));
        }
        match out.wb {
            Some(lightcraft_develop::WbMode::AsShot) => self.edit(|history, info| history.set(|settings| edit::as_shot(settings, info))),
            Some(lightcraft_develop::WbMode::Auto) => self.auto(Auto::WhiteBalance),
            Some(preset) => self.edit(|history, info| history.set(|settings| _ = edit::preset_white_balance(settings, info, preset))),
            None => {}
        }
        if let Some(what) = out.auto {
            self.auto(what);
        }
        if let Some(on) = out.remove_ca {
            self.edit(|history, _| history.set(|settings| settings.optics.remove_ca = on));
            self.refresh_upright();
        }
        if let Some(on) = out.lens_profile {
            self.edit(|history, _| history.set(|settings| settings.optics.lens_profile = on));
            self.refresh_upright();
        }
        if let Some(mode) = out.upright {
            self.upright(mode);
        }
        if let Some(on) = out.constrain_crop {
            self.edit(|history, _| history.set(|settings| settings.geometry.constrain_crop = on));
        }
        let Some((index, slid)) = out.slider else { return };
        let Some(spec) = develop::sliders().nth(index) else { return };
        if slid.touched {
            self.selected = index;
        }
        if slid.auto {
            return self.auto(Auto::Control(spec.id));
        }
        self.slide(slid, |settings, info, value| {
            edit::set_control(settings, info, spec.id, value);
        });
        if spec.id.starts_with("optics.") && !self.dragging && (slid.value.is_some() || slid.drag_stopped) {
            self.refresh_upright();
        }
    }

    /// What was done in the crop tool's panel.
    fn crop_panel(&mut self, out: crop_panel::Out) {
        let Some(dims) = self.dims() else { return };
        if let Some(aspect) = out.aspect {
            self.edit(|history, _| history.set(|settings| crop::set_aspect(settings, dims, aspect)));
        }
        if out.swap {
            self.edit(|history, _| history.set(|settings| crop::swap_aspect(settings, dims)));
        }
        if out.lock {
            self.crop_locked = !self.crop_locked;
        }
        self.slide(out.angle, |settings, _, angle| crop::straighten(settings, dims, angle));
        if out.level {
            self.auto(Auto::Level);
        }
        if let Some(clockwise) = out.turn {
            self.edit(|history, _| history.set(|settings| crop::turn(settings, clockwise)));
        }
        if out.reset {
            self.edit(|history, _| history.set(crop::reset));
        }
        if out.done {
            self.crop = None;
        }
    }

    /// The keys that are the crop tool's while it's open, Lightroom's: X
    /// swaps the aspect, A locks it or lets it go, Return puts the tool
    /// away, and Esc puts it away with the crop as it was when the tool
    /// came to the frame.
    fn crop_keys(&mut self, ctx: &egui::Context) {
        let Some(dims) = self.dims() else { return };
        let pressed = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        if pressed(egui::Key::X) {
            self.edit(|history, _| history.set(|settings| crop::swap_aspect(settings, dims)));
        }
        if pressed(egui::Key::A) {
            self.crop_locked = !self.crop_locked;
        }
        if pressed(egui::Key::Enter) {
            self.crop = None;
        }
        if pressed(egui::Key::Escape) {
            let was = self.shoot.as_ref().zip(self.crop.take()).and_then(|(shoot, tool)| tool.was(&shoot.frame().path));
            if let Some((crop, orientation)) = was {
                self.edit(|history, _| history.set(|settings| (settings.crop, settings.orientation) = (crop, orientation)));
            }
        }
    }

    /// The keys that are Guided Upright's tool's while it's open: Delete
    /// takes the line held, or else the last, away; Return puts the tool
    /// away, and Esc puts it away with the lines as they were when the
    /// tool came to the frame.
    fn guided_keys(&mut self, ctx: &egui::Context) {
        let pressed = |key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key));
        if pressed(egui::Key::Delete) || pressed(egui::Key::Backspace) {
            let held = self.guided.as_mut().and_then(|tool| tool.held.take());
            self.edit(|history, _| {
                let count = history.settings().geometry.guides.len();
                let Some(index) = held.filter(|i| *i < count).or(count.checked_sub(1)) else { return false };
                history.set(|settings| _ = settings.geometry.guides.remove(index))
            });
        }
        if pressed(egui::Key::Enter) {
            self.guided = None;
        }
        if pressed(egui::Key::Escape) {
            let was = self.shoot.as_ref().zip(self.guided.take()).and_then(|(shoot, tool)| tool.was(&shoot.frame().path));
            if let Some(geometry) = was {
                self.edit(|history, _| history.set(|settings| settings.geometry = geometry));
            }
        }
    }

    /// The script's next command, once everything wanted has been
    /// developed: so a script sees what its last command led to, and
    /// `Next,Zoom,Quit` on real raws renders each step before the next.
    fn run_script(&mut self, ctx: &egui::Context) {
        if self.script.is_empty() {
            return;
        }
        if self.renderer.busy() {
            return ctx.request_repaint();
        }
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
        if self.watch.check(ctx) {
            self.display = self.watch.profile().cloned();
            self.filmstrip.set_display(self.display.clone());
            self.renderer.set_display(self.display.clone());
            // What the loupe holds was for the other monitor.
            self.loupe.retain(|_| false);
        }
        self.finish_picking();
        self.develop(ctx);
        self.finish_exporting();
        self.disk_problems();
        if !ctx.egui_wants_keyboard_input() && self.choosing.is_none() {
            // The crop tool's own keys first: its X isn't a reject.
            if self.crop.is_some() && !self.legend {
                self.crop_keys(ctx);
            }
            if self.guided.is_some() && !self.legend {
                self.guided_keys(ctx);
            }
            // Under the legend, only its own key, to put it away.
            for command in Command::pressed(ctx, &mut self.v_down) {
                if !self.legend || command == Command::Shortcuts {
                    self.run(command, ctx);
                }
            }
        }
        self.run_script(ctx);
        // The zoom key let go ends a look.
        if let Some(key) = hotkeys::current().command(Command::Zoom).map(|s| s.logical_key)
            && let (false, time) = ctx.input(|i| (i.key_down(key), i.time))
        {
            self.loupe.key_up(time);
        }
        if let Some(shoot) = &self.shoot {
            self.filmstrip.sync_shoot(shoot, ctx);
            self.filmstrip.collect(ctx, shoot.current());
            if self.at.as_ref() != Some(&shoot.frame().path) {
                self.previous = self.at.replace(shoot.frame().path.clone());
            }
            if let (Some(tool), Some(editing)) = (&mut self.crop, self.edits.get(&shoot.frame().path)) {
                tool.on(&shoot.frame().path, editing.history.settings());
            }
            if let (Some(tool), Some(editing)) = (&mut self.guided, self.edits.get(&shoot.frame().path)) {
                tool.on(&shoot.frame().path, editing.history.settings());
            }
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

    /// Ask for the current frame and its neighbours to be developed, and
    /// take in what's been finished.
    fn develop(&mut self, ctx: &egui::Context) {
        let wanted = self.wanted();
        let zoom = self.loupe.zoom().filter(|_| self.crop.is_none() && self.guided.is_none());
        self.loupe.retain(|path| wanted.iter().any(|w| w.path == path));
        self.loupe.set_compare(self.compare);
        let compare = self.compare.is_some();
        self.renderer.want(wanted, View { size: self.loupe.size(), dragging: self.dragging, clipping: self.clipping, zoom, compare });
        for news in self.loupe.take(ctx, self.renderer.done()) {
            match news {
                News::Decoded(path, info, camera) => {
                    // Its edit starts from what its sidecar held, or as a fresh raw.
                    let fresh = look::fresh(info.as_shot_temp, info.as_shot_tint);
                    let saved = self.saved.remove(&path).flatten().map(|settings| (*settings).clone());
                    match self.edits.get_mut(&path) {
                        // Pasted onto before it was decoded.
                        Some(editing) if editing.info.is_none() => {
                            editing.history.decoded(fresh);
                            (editing.info, editing.camera) = (Some(info), camera);
                            editing.stepped();
                        }
                        // Decoded again, for another profile's look.
                        Some(editing) => (editing.info, editing.camera) = (Some(info), camera),
                        None => _ = self.edits.insert(path.clone(), Editing::new(History::new(fresh, saved), Some((info, camera)))),
                    }
                    self.fill_upright(&path);
                }
                News::Failed(path, error) => self.message = Some((format!("Couldn't develop {}: {error}", file_name(&path)), true)),
            }
        }
    }

    /// The current frame and its neighbours, as the loupe is to show them.
    fn wanted(&mut self) -> Vec<Wanted> {
        let paths: Vec<PathBuf> = self.shoot.as_ref().map_or_else(Vec::new, |shoot| {
            let frames = std::iter::once(shoot.current()).chain(shoot.neighbours(AHEAD, BEHIND));
            frames.map(|i| shoot.frames()[i].path.clone()).collect()
        });
        let mut wanted: Vec<Wanted> = paths.into_iter().map(|path| Wanted { settings: self.settings_of(&path), path }).collect();
        // Panels switched off: each frame without them, once it's decoded
        // and its fresh settings are known.
        if !self.off.is_empty() {
            for frame in &mut wanted {
                if let Some(editing) = self.edits.get(&frame.path) {
                    frame.settings = Some(Arc::new(copy::switched_off(&editing.settings, editing.history.fresh(), &self.off)));
                }
            }
        }
        // Before: the current frame as a fresh raw (`None` is one).
        if let Some(current) = wanted.first_mut().filter(|_| self.before) {
            current.settings = None;
        }
        // The crop tool draws its crop over the current frame whole, so
        // a change to the crop there is nothing to render again.
        if self.crop.is_some()
            && let Some(current) = wanted.first_mut()
            && let Some(settings) = current.settings.as_mut().filter(|settings| settings.crop != Default::default())
        {
            Arc::make_mut(settings).crop = Default::default();
        }
        // Guided Upright's lines are drawn on the frame through its lens
        // corrections alone.
        if self.guided.is_some()
            && let Some(current) = wanted.first_mut()
            && let Some(settings) = current.settings.as_mut()
        {
            let settings = Arc::make_mut(settings);
            (settings.crop, settings.geometry) = (Default::default(), Default::default());
        }
        wanted
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
                self.unreadable.insert(path.to_path_buf());
                None
            }
        };
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
        let dims = self.dims();
        let mut picked = None;
        let mut cropped = None;
        let mut guided = None;
        let mut clicked = None;
        let context = self.menu_context();
        let mut chosen = None;
        egui::CentralPanel::no_frame().frame(pasteboard).show(ui, |ui| match &self.shoot {
            Some(shoot) => {
                let mouse = if self.crop.is_some() || self.guided.is_some() {
                    Mouse::Tool
                } else if self.selector {
                    Mouse::Pick
                } else {
                    Mouse::Zoom
                };
                let path = &shoot.frame().path;
                let response;
                (picked, response) = self.loupe.show(ui, path, mouse);
                if let Some(response) = &response {
                    chosen = crate::menus::loupe(response, &self.theme, &context);
                }
                if let (Some(tool), Some(editing), Some(dims)) = (&mut self.crop, self.edits.get(path), dims) {
                    let settings = editing.history.settings();
                    let frame = crate::crop::Frame { area: self.loupe.area(), whole: self.loupe.whole(path, settings.orientation), dims };
                    cropped = Some(tool.show(ui, &self.theme, frame, settings, self.crop_locked));
                }
                if let (Some(tool), Some(editing), Some(dims)) = (&mut self.guided, self.edits.get(path), dims) {
                    let settings = editing.history.settings();
                    let whole = self.loupe.untransformed(path, settings.orientation);
                    guided = tool.show(ui, &self.theme, self.loupe.area(), whole, dims, &settings.geometry.guides).added;
                }
                let mut corner = ui.max_rect().left_top() + egui::vec2(12.0, 8.0);
                if let Some(info) = self.info {
                    let editing = self.edits.get(path);
                    let lines = info_lines(info, &shoot.frame().name(), editing.map(|e| (&e.camera.capture, e.history.settings())));
                    corner = info_overlay(ui, &self.theme, corner, &lines);
                }
                // What the loupe leaves out, so it isn't forgotten.
                if self.before {
                    ui.painter().text(corner, Align2::LEFT_TOP, "Before", FontId::proportional(13.0), self.theme.foreground);
                } else if !self.off.is_empty() {
                    let off = Group::ALL.iter().filter(|group| self.off.contains(group)).map(|group| group.label()).collect::<Vec<_>>().join(", ");
                    ui.painter().text(corner, Align2::LEFT_TOP, format!("Off: {off}"), FontId::proportional(13.0), self.theme.foreground);
                }
            }
            None => clicked = self.empty_state(ui),
        });
        if let Some(at) = picked {
            self.pick_white_balance(at);
        }
        if let Some(cropped) = cropped {
            self.crop_gesture(cropped);
        }
        if let Some(line) = guided {
            self.edit(|history, _| {
                history.set(|settings| {
                    settings.geometry.upright = lightcraft_develop::Upright::Guided;
                    settings.geometry.guides.push(line);
                })
            });
        }
        if let Some(dir) = clicked {
            self.open(dir);
        }
        if let Some(command) = chosen {
            self.run(command, &ui.ctx().clone());
        }
        if self.legend {
            self.legend = crate::legend::show(ui.ctx(), &self.theme);
        }
        if let Some(mut dialog) = self.choosing.take() {
            match dialog.show(ui.ctx(), &self.theme) {
                Outcome::KeepOpen => self.choosing = Some(dialog),
                Outcome::Copied(chosen) => {
                    self.copy(&chosen);
                    self.state.remember_copy(&chosen);
                }
                Outcome::Cancelled => {}
            }
        }
    }

    /// The shown frames' thumbnails, under the loupe.
    fn filmstrip(&mut self, ui: &mut Ui) {
        let Some(shoot) = &mut self.shoot else { return };
        let (clicked, response) = self.filmstrip.show(ui, shoot, &self.theme);
        let was = shoot.current();
        match clicked {
            Some((clicked, Click::Toggle)) => shoot.toggle_selected(clicked),
            Some((clicked, Click::Range)) => shoot.select_to(clicked),
            Some((clicked, Click::Plain)) => {
                shoot.clear_selection();
                shoot.go_to(clicked);
            }
            // The menu acts on the frame right-clicked, and on the
            // selection only if that's in it, as in Lightroom.
            Some((clicked, Click::Menu)) if !shoot.is_selected(clicked) => {
                shoot.clear_selection();
                shoot.go_to(clicked);
            }
            Some((clicked, Click::Menu)) => shoot.go_to(clicked),
            None => {}
        }
        if shoot.current() != was {
            self.leave();
        }
        if let Some(command) = crate::menus::filmstrip(&response, &self.theme, &self.menu_context()) {
            self.run(command, &ui.ctx().clone());
        }
    }

    /// What the right-click menus are told about the current frame.
    fn menu_context(&self) -> crate::menus::Context {
        let Some(shoot) = &self.shoot else { return Default::default() };
        let current = shoot.current();
        let settings = self.edits.get(&shoot.frame().path).map(|editing| editing.history.settings());
        crate::menus::Context {
            rating: shoot.frame().rating,
            copied: self.clipboard.is_some(),
            previous: self.previous.is_some(),
            stack: shoot.stack(current).map(|_| shoot.expanded(current)),
            selection: shoot.selection().len() > 1,
            before: self.before,
            clipping: self.clipping,
            flat: settings.is_some_and(|settings| Profile::of(settings) == Profile::Flat),
        }
    }

    /// The histogram, and under it the crop tool's panel while the tool is
    /// open, then the develop panels, which scroll.
    fn develop_panel(&mut self, ui: &mut Ui) {
        // The histogram of the frame as the loupe shows it.
        let path = self.shoot.as_ref().map(|shoot| &shoot.frame().path);
        let histogram = path.and_then(|path| self.loupe.histogram(path));
        if crate::histogram::show(ui, &self.theme, histogram, self.clipping) {
            self.clipping = !self.clipping;
        }
        ui.add_space(6.0);
        // Not until the frame's raw has been decoded: the sliders are read against it.
        let editing = self.shoot.as_ref().and_then(|shoot| self.edits.get(&shoot.frame().path));
        let Some(editing) = editing.filter(|editing| editing.info.is_some()) else { return };
        let (settings, fresh) = (editing.history.settings(), editing.history.fresh());
        let dims = self.dims().filter(|_| self.crop.is_some());
        let scroll = egui::ScrollArea::vertical().auto_shrink(false).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
        let (cropped, out) = scroll
            .show(ui, |ui| {
                let cropped = dims.map(|dims| crop_panel::show(ui, &self.theme, settings, dims, self.crop_locked));
                (cropped, develop::show(ui, &self.theme, settings, fresh, self.selected, &editing.camera, &self.off))
            })
            .inner;
        if let Some(cropped) = cropped {
            self.crop_panel(cropped);
        }
        self.panel(out);
    }

    /// With no shoot open: how to open one, and the shoots opened recently.
    /// Returns the path clicked, if one was.
    fn empty_state(&self, ui: &mut Ui) -> Option<PathBuf> {
        let hint = match hotkeys::current().command(Command::Open) {
            Some(s) => format!("{} to open a shoot", format_shortcut(&s)),
            None => "Open a shoot".to_owned(),
        };
        let mut clicked = None;
        ui.vertical_centered(|ui| {
            let space = if self.state.recent.is_empty() { 0.45 } else { 0.2 };
            ui.add_space(ui.available_height() * space);
            ui.label(RichText::new(hint).size(16.0).color(self.theme.dark_foreground));
            if self.state.recent.is_empty() {
                return;
            }
            ui.add_space(20.0);
            ui.label(RichText::new("Recent shoots").strong().color(self.theme.foreground));
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(self.theme.dark_background)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_max_width(450.0);
                    let (big, small) = (FontId::proportional(13.0), FontId::proportional(11.0));
                    for dir in self.state.recent.iter().take(8) {
                        let size = egui::vec2(ui.available_width(), 26.0);
                        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 0.0, self.theme.lighter_background);
                        }
                        let painter = ui.painter();
                        let left = egui::pos2(rect.left() + 6.0, rect.center().y);
                        let (name, foreground) = (file_name(dir), self.theme.foreground);
                        let name = painter.text(left, Align2::LEFT_CENTER, name, big.clone(), foreground);
                        // The folder it's in, shortened from the left to fit.
                        let room = rect.right() - 6.0 - (name.right() + 24.0);
                        let parent = elided(painter, &home_relative(dir.parent().unwrap_or(dir)), &small, room);
                        let right = egui::pos2(rect.right() - 6.0, rect.center().y);
                        painter.text(right, Align2::RIGHT_CENTER, parent, small.clone(), self.theme.dark_foreground);
                        if response.clicked() {
                            clicked = Some(dir.clone());
                        }
                    }
                });
        });
        clicked
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

/// The current frame, its mark, its place among the frames shown, and the filter.
fn status(shoot: &Shoot) -> String {
    let shown = shoot.shown_indices();
    let place = shown.iter().position(|&i| i == shoot.current()).map_or(0, |p| p + 1);
    // Stacks closed up show one frame each: how many are shown isn't how many there are.
    let through = shoot.frames().iter().filter(|frame| shoot.filter().matches(frame)).count();
    let (count, closed) = if shown.len() < through { (through, format!("{} shown of ", shown.len())) } else { (shown.len(), String::new()) };
    let of = match shoot.filter() {
        Filter::Picks => format!("{closed}{count} picks of {} frames", shoot.frames().len()),
        Filter::All => format!("{closed}{count} frames"),
    };
    let mark = stars(shoot.frame().rating);
    let mark = if mark.is_empty() { String::new() } else { format!("{mark}   ") };
    let selected = match shoot.selection().len() {
        0 => String::new(),
        n => format!("   {n} selected"),
    };
    format!("{}   {mark}{place} / {of}{selected}", shoot.frame().name())
}

/// What an info overlay says of a frame: its name first, then what's
/// known of the rest. `shot` is how it was shot and its settings, once its
/// raw has been decoded.
fn info_lines(info: Info, name: &str, shot: Option<(&look::Capture, &DevelopSettings)>) -> Vec<String> {
    let rest = shot.map_or_else(Vec::new, |(capture, settings)| match info {
        Info::One => [capture.time.clone(), capture.cropped(settings)].into_iter().flatten().collect(),
        Info::Two => [capture.exposure(), capture.optics()].into_iter().flatten().collect(),
    });
    std::iter::once(name.to_owned()).chain(rest).collect()
}

/// The info overlay's lines at the loupe's corner, the first larger, on a
/// plate so they read over any frame. Where what goes under it starts.
fn info_overlay(ui: &Ui, theme: &Theme, corner: egui::Pos2, lines: &[String]) -> egui::Pos2 {
    let painter = ui.painter();
    let galleys: Vec<_> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let size = if i == 0 { 15.0 } else { 12.5 };
            let colour = if i == 0 { theme.foreground } else { theme.dark_foreground };
            painter.layout_no_wrap(line.clone(), FontId::proportional(size), colour)
        })
        .collect();
    let width = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
    let height: f32 = galleys.iter().map(|g| g.size().y + 2.0).sum();
    let plate = egui::Rect::from_min_size(corner - egui::vec2(6.0, 4.0), egui::vec2(width + 12.0, height + 6.0));
    painter.rect_filled(plate, 4.0, theme.dark_background.gamma_multiply(0.8));
    let mut at = corner;
    for galley in galleys {
        let h = galley.size().y;
        painter.galley(at, galley, theme.foreground);
        at.y += h + 2.0;
    }
    egui::pos2(corner.x, plate.bottom() + 6.0)
}

/// How a paste went.
#[derive(Default)]
struct Pasted {
    /// The frames it changed.
    frames: Vec<PathBuf>,
    /// How many it left alone because this OmaLux can't read their edits.
    unreadable: usize,
}

impl Pasted {
    /// What to say of it, starting "Pasted onto", and whether that's a
    /// problem.
    fn said(&self, pasted_onto: &str) -> (String, bool) {
        let left = match self.unreadable {
            0 => String::new(),
            1 => "; 1 has an edit this OmaLux can't read, and was left as it is".to_owned(),
            n => format!("; {n} have edits this OmaLux can't read, and were left as they are"),
        };
        match (self.frames.is_empty(), self.unreadable) {
            (true, 0) => ("Nothing to paste: these settings are there already".to_owned(), false),
            (true, _) => (format!("Pasted nothing{left}"), true),
            (false, unreadable) => (format!("{pasted_onto} {}{left}", counted(&self.frames)), unreadable > 0),
        }
    }
}

/// "DSC01234.ARW" for one frame, "12 frames" for more.
fn counted(frames: &[PathBuf]) -> String {
    match frames {
        [one] => file_name(one),
        frames => format!("{} frames", frames.len()),
    }
}

/// The frames a paste or an export reaches: the selection, or with none
/// the current frame.
fn targets(shoot: &Shoot) -> Vec<PathBuf> {
    let selection = shoot.selection();
    let frames = if selection.is_empty() { vec![shoot.current()] } else { selection };
    frames.into_iter().map(|i| shoot.frames()[i].path.clone()).collect()
}

/// "White Balance, Light and Presence".
fn listed(groups: &[Group]) -> String {
    let labels: Vec<&str> = groups.iter().map(|group| group.label()).collect();
    match labels.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => labels.concat(),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// A path with the home folder written as ~.
fn home_relative(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => Path::new("~").join(rest).display().to_string(),
        None => path.display().to_string(),
    }
}

/// `text` without as much of its start as it takes to fit `width`.
fn elided(painter: &egui::Painter, text: &str, font: &FontId, width: f32) -> String {
    let fits = |t: &str| painter.layout_no_wrap(t.to_owned(), font.clone(), egui::Color32::WHITE).size().x <= width;
    if fits(text) {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    (1..chars.len())
        .map(|skip| format!("…{}", chars[skip..].iter().collect::<String>()))
        .find(|t| fits(t))
        .unwrap_or_default()
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.step(ctx);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn on_exit(&mut self) {
        self.remember_place();
        self.disk.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use egui::{Event, Key, Modifiers};
    use omalux_engine::shoot::Frame;
    use omalux_engine::sidecar;

    /// The app driven with synthetic events, with no window or GPU.
    struct Harness {
        ctx: egui::Context,
        app: App,
        time: f64,
        /// The window's size: taller for a test of the panels that are
        /// below the fold at 600.
        screen: egui::Vec2,
    }

    impl Harness {
        fn new(script: &[Command]) -> Self {
            Self::with_paths(script, Paths { state: None, thumbnails: None, monitors: None })
        }

        fn with_paths(script: &[Command], paths: Paths) -> Self {
            let (_tx, rx) = channel();
            let ctx = egui::Context::default();
            let renderer = Renderer::new(crate::render::tests::decoder(), false, || {});
            let wake = ctx.clone();
            let disk = Disk::new(move || wake.request_repaint());
            let app = App::build(Theme::default(), rx, Config::default(), Vec::new(), script.iter().copied().collect(), Workers { renderer, disk }, paths);
            Self { ctx, app, time: 0.0, screen: egui::vec2(800.0, 600.0) }
        }

        fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
            self.time += 0.05;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.screen)),
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

        /// Where a slider's value is on screen.
        fn value(&self, id: &'static str) -> egui::Rect {
            self.ctx.data(|d| d.get_temp(egui::Id::new((id, "value")))).expect("the slider value has been drawn")
        }

        /// Where `text` was last drawn: in a dialog, if one is open over
        /// the panels.
        fn text(&mut self, text: &str) -> egui::Rect {
            let output = self.frame(vec![]);
            texts(&output).into_iter().rev().find(|(t, _)| t == text).map(|(_, rect)| rect).expect("the text has been drawn")
        }

        fn type_text(&mut self, text: &str) -> egui::FullOutput {
            self.frame(vec![Event::Text(text.to_owned())])
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
            self.app.disk.flush();
            let path = &self.app.shoot.as_ref().unwrap().frame().path;
            sidecar::edit(&sidecar::read(path).unwrap()?).unwrap().map(|edit| edit.settings)
        }

        /// A frame's settings as they stand, by its raw's name.
        fn settings_of(&mut self, name: &str) -> DevelopSettings {
            let path = self.app.shoot.as_ref().unwrap().dir().join(name);
            self.app.settings_of(&path).map_or_else(|| look::fresh(5200.0, 3.0), |settings| (*settings).clone())
        }

        /// The edit in a frame's sidecar, by its raw's name.
        fn saved_of(&self, name: &str) -> Option<DevelopSettings> {
            self.app.disk.flush();
            let path = self.app.shoot.as_ref().unwrap().dir().join(name);
            sidecar::edit(&sidecar::read(&path).unwrap()?).unwrap().map(|edit| edit.settings)
        }

        fn said(&self) -> &str {
            self.app.message.as_ref().map_or("", |(text, _)| text)
        }

        /// A key pressed and let go in one frame.
        fn press(&mut self, modifiers: Modifiers, key: Key) -> egui::FullOutput {
            let mut events = key_events(modifiers, key, true);
            events.extend(key_events(modifiers, key, false));
            // The modifiers are let go with the key: egui goes on taking
            // them for held until it's told otherwise.
            events.push(Event::ModifiersChanged(Modifiers::NONE));
            self.frame(events)
        }

        fn click(&mut self, at: egui::Pos2) {
            self.click_with(at, Modifiers::NONE);
        }

        fn click_with(&mut self, at: egui::Pos2, modifiers: Modifiers) {
            let button =
                |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers };
            self.frame(vec![Event::ModifiersChanged(modifiers), Event::PointerMoved(at), button(true)]);
            self.frame(vec![Event::ModifiersChanged(modifiers), button(false)]);
            if modifiers != Modifiers::NONE {
                self.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
            }
        }

        /// A right-click: what opens a menu.
        fn right_click(&mut self, at: egui::Pos2) {
            let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(at), button(true)]);
            self.frame(vec![button(false)]);
            self.frame(vec![]);
        }
    }

    fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        let mut out = Vec::new();
        for cs in &output.shapes {
            collect_texts(&cs.shape, &mut out);
        }
        out
    }

    fn collect_texts(shape: &egui::epaint::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::epaint::Shape::Text(t) => {
                let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
                out.push((t.galley.text().to_string(), rect));
            }
            egui::epaint::Shape::Vec(shapes) => {
                for s in shapes {
                    collect_texts(s, out);
                }
            }
            _ => {}
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
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   ★   1 / 2 picks of 3 frames");

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
        assert_eq!(h.settings(), &look::fresh(5200.0, 3.0), "reset");
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().wb.tint, 1.0, "and reset is a step like any other");

        // Each frame has its own steps: the next one has none to undo.
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.settle();
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!((h.settings(), h.saved()), (&look::fresh(5200.0, 3.0), None));
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
    fn typing_a_value_into_a_slider_applies_and_saves_it_as_one_step() {
        let folder = Folder::new("type-slider", &[0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        assert_eq!(h.settings().light.exposure, 0.0);
        assert_eq!(h.saved(), None);

        let value = h.value("light.exposure");
        h.click(value.center());
        h.type_text("0.5");
        h.press(Modifiers::NONE, Key::Enter);

        assert_eq!(h.settings().light.exposure, 0.5);
        assert_eq!(h.saved().map(|s| s.light.exposure), Some(0.5));

        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().light.exposure, 0.0, "one Ctrl+Z takes it back");
        assert_eq!(h.saved().map(|s| s.light.exposure), Some(0.0));
    }

    #[test]
    fn escape_keeps_the_old_slider_value() {
        let folder = Folder::new("type-esc", &[0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        assert_eq!(h.settings().light.exposure, 0.0);

        let value = h.value("light.exposure");
        h.click(value.center());
        h.type_text("1.5");
        h.press(Modifiers::NONE, Key::Escape);

        assert_eq!(h.settings().light.exposure, 0.0);
        assert_eq!(h.saved(), None, "nothing saved on escape");
    }

    #[test]
    fn typing_letters_does_not_fire_app_commands() {
        let folder = Folder::new("type-cmd", &[0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let raw = folder.0.join("DSC00001.ARW");
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 0);
        assert_eq!(sidecar::read(&raw).unwrap(), None, "no sidecar initially");

        let value = h.value("light.exposure");
        h.click(value.center());
        h.press(Modifiers::NONE, Key::P);
        h.type_text("p");
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 0, "p did not pick the frame");

        h.press(Modifiers::NONE, Key::Num5);
        h.type_text("5");
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 0, "5 did not rate the frame");
        assert_eq!(sidecar::read(&raw).unwrap(), None, "5 did not write to sidecar");

        h.press(Modifiers::NONE, Key::Escape);
        assert_eq!(h.settings().light.exposure, 0.0);
    }

    #[test]
    fn typed_value_past_the_range_is_clamped() {
        let folder = Folder::new("type-clamp", &[0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();

        let value = h.value("light.exposure");
        h.click(value.center());
        h.type_text("10");
        h.press(Modifiers::NONE, Key::Enter);

        assert_eq!(h.settings().light.exposure, 5.0, "clamped to max of 5.0");
        assert_eq!(h.saved().map(|s| s.light.exposure), Some(5.0));
    }

    #[test]
    fn non_numeric_typed_text_keeps_the_old_value() {
        let folder = Folder::new("type-bad", &[0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();

        let value = h.value("light.exposure");
        h.click(value.center());
        h.type_text("abc");
        h.press(Modifiers::NONE, Key::Enter);

        assert_eq!(h.settings().light.exposure, 0.0);
        assert_eq!(h.saved(), None);
    }

    #[test]
    fn tab_applies_the_typed_value() {
        let folder = Folder::new("type-tab", &[0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();

        let value = h.value("light.exposure");
        h.click(value.center());
        h.type_text("0.5");
        h.press(Modifiers::NONE, Key::Tab);

        assert_eq!(h.settings().light.exposure, 0.5);
        assert_eq!(h.saved().map(|s| s.light.exposure), Some(0.5));
    }

    #[test]
    fn clicking_away_applies_the_typed_value() {
        let folder = Folder::new("type-away", &[0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();

        let value = h.value("light.exposure");
        h.click(value.center());
        h.type_text("1.0");
        h.click(egui::pos2(100.0, 100.0));

        assert_eq!(h.settings().light.exposure, 1.0);
        assert_eq!(h.saved().map(|s| s.light.exposure), Some(1.0));
    }

    #[test]
    fn w_picks_the_white_balance_from_a_click_and_backslash_shows_before() {
        use lightcraft_develop::WbMode;
        let folder = Folder::new("selector", &[1, 1]);
        let raw = folder.0.join("DSC00001.ARW");
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let frame: egui::Rect = h.ctx.data(|d| d.get_temp(egui::Id::new("loupe frame"))).unwrap();

        // A click on the frame does nothing until the selector is out.
        h.pointer(frame.center(), Some(true));
        h.pointer(frame.center(), Some(false));
        assert_eq!(h.settings().wb.mode, WbMode::AsShot);
        h.press(Modifiers::NONE, Key::W);
        h.frame(vec![]);
        h.pointer(frame.center(), Some(true));
        h.pointer(frame.center(), Some(false));
        assert_eq!(h.settings().wb.mode, WbMode::Custom);
        assert!(!h.app.selector, "one click and the selector is put away");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        h.settle();
        let rendered = |h: &Harness| h.app.loupe.shown(&raw).unwrap().render.as_ref().unwrap().settings.wb.mode;
        assert_eq!(rendered(&h), WbMode::Custom);

        // Before: the frame with no edit, in the loupe only.
        h.press(Modifiers::NONE, Key::Backslash);
        h.settle();
        assert_eq!((rendered(&h), h.settings().wb.mode), (WbMode::AsShot, WbMode::Custom));
        h.press(Modifiers::NONE, Key::Backslash);
        h.settle();
        assert_eq!(rendered(&h), WbMode::Custom);
        // Leaving the frame ends it.
        h.press(Modifiers::NONE, Key::Backslash);
        h.press(Modifiers::NONE, Key::ArrowRight);
        assert!(!h.app.before);
    }

    #[test]
    fn r_opens_the_crop_tool_on_the_whole_frame_and_esc_puts_the_crop_back() {
        let folder = Folder::new("crop-tool", &[1, 1]);
        let raw = folder.0.join("DSC00001.ARW");
        let mut h = Harness::new(&[]);
        h.press(Modifiers::NONE, Key::R);
        assert!(h.app.crop.is_none(), "nothing to crop with no shoot open");
        h.app.open(folder.0.clone());
        h.settle();
        let dims = h.app.dims().unwrap();
        h.app.edit(|history, _| history.set(|settings| crop::set_aspect(settings, dims, crop::Aspect::Ratio(1, 1))));
        h.settle();
        let rendered = |h: &Harness| h.app.loupe.shown(&raw).unwrap().render.as_ref().unwrap().settings.crop.geometry;
        assert_eq!(rendered(&h), h.settings().crop.geometry, "cropped, in the loupe");

        // The tool shows the frame whole, with the crop still in its settings.
        h.press(Modifiers::NONE, Key::R);
        h.settle();
        assert!(h.app.crop.is_some());
        assert!(rendered(&h).is_identity() && !h.settings().crop.geometry.is_identity());
        assert!(h.app.loupe.whole(&raw, h.settings().orientation).is_some());
        // Z doesn't zoom while it's open.
        h.press(Modifiers::NONE, Key::Z);
        assert!(h.app.loupe.zoom().is_none());

        // X turns the crop on its side, and isn't a reject; A is the lock.
        let square = h.settings().crop;
        h.app.edit(|history, _| history.set(|settings| crop::set_aspect(settings, dims, crop::Aspect::Ratio(4, 5))));
        let landscape = h.settings().crop.geometry.rect_px(dims.0, dims.1);
        h.press(Modifiers::NONE, Key::X);
        let portrait = h.settings().crop.geometry.rect_px(dims.0, dims.1);
        assert!(landscape.width() > landscape.height() && portrait.width() < portrait.height(), "{landscape:?} {portrait:?}");
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 1, "not rejected");
        assert!(h.app.crop_locked);
        h.press(Modifiers::NONE, Key::A);
        assert!(!h.app.crop_locked);

        // Esc puts the tool away with the crop as it was when it was opened.
        h.press(Modifiers::NONE, Key::Escape);
        assert!(h.app.crop.is_none());
        assert_eq!(h.settings().crop, square);
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        h.settle();
        assert_eq!(rendered(&h), square.geometry, "cropped again");

        // Return keeps what was done, and the tool stays open from frame to frame.
        h.press(Modifiers::NONE, Key::R);
        h.press(Modifiers::NONE, Key::X);
        h.press(Modifiers::NONE, Key::ArrowRight);
        assert!(h.app.crop.is_some());
        h.press(Modifiers::NONE, Key::ArrowLeft);
        h.press(Modifiers::NONE, Key::Enter);
        assert!(h.app.crop.is_none());
        // (A square on its side is the same square.)
        assert_eq!(h.settings().crop.geometry, square.geometry);
        // X is a reject again.
        h.press(Modifiers::NONE, Key::X);
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, sidecar::REJECT);
    }

    /// Where the crop tool last drew the crop.
    fn crop_box(h: &Harness) -> egui::Rect {
        h.ctx.data(|d| d.get_temp(egui::Id::new("crop box"))).expect("the crop tool has drawn")
    }

    /// Press at `from`, move to `to` by way of the point halfway, and let
    /// go there, with `modifiers` held.
    fn drag(h: &mut Harness, from: egui::Pos2, to: egui::Pos2, modifiers: Modifiers) {
        let button = |pos, pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers };
        let held = Event::ModifiersChanged(modifiers);
        h.frame(vec![held.clone(), Event::PointerMoved(from)]);
        h.frame(vec![held.clone(), button(from, true)]);
        h.frame(vec![held.clone(), Event::PointerMoved(from + (to - from) / 2.0)]);
        h.frame(vec![held.clone(), Event::PointerMoved(to)]);
        h.frame(vec![held, button(to, false)]);
        h.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
    }

    /// A shoot of one frame with the crop tool open on it.
    fn cropping(name: &str) -> (Folder, Harness) {
        let folder = Folder::new(name, &[1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::R);
        h.settle();
        (folder, h)
    }

    #[test]
    fn the_crop_is_dragged_by_its_handles_and_moved_from_inside() {
        let (_folder, mut h) = cropping("crop-drag");
        let (w, h0) = h.app.dims().unwrap();
        let shape = |h: &Harness| h.settings().crop.geometry.rect_px(w, h0).aspect();
        let whole = crop_box(&h);
        assert!((whole.aspect_ratio() - (w / h0) as f32).abs() < 0.01, "the whole frame to start with: {whole:?}");

        // A corner, with the lock on: the crop keeps the frame's shape.
        let corner = whole.right_bottom();
        let button = |pos, pressed| Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        h.frame(vec![Event::PointerMoved(corner)]);
        h.frame(vec![button(corner, true)]);
        h.frame(vec![Event::PointerMoved(corner - whole.size() / 2.0)]);
        let rect = h.settings().crop.geometry.rect;
        assert!((rect.x1 - 0.5).abs() < 0.01 && (shape(&h) - w / h0).abs() < 0.01, "{rect:?}");
        assert_eq!((rect.x0, rect.y0), (0.0, 0.0), "the corner opposite stays");
        assert_eq!(h.saved(), None, "nothing is saved until it's let go");
        h.frame(vec![button(corner - whole.size() / 2.0, false)]);
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        assert!((crop_box(&h).size() - whole.size() / 2.0).length() < 0.5, "drawn where it is");

        // From inside, it's moved, as far as the frame's edge.
        let half = crop_box(&h);
        drag(&mut h, half.center(), half.center() + egui::vec2(whole.width(), 0.0), Modifiers::NONE);
        let rect = h.settings().crop.geometry.rect;
        assert!((rect.x0 - 0.5).abs() < 1e-6 && (rect.x1 - 1.0).abs() < 1e-6 && rect.y0 == 0.0, "{rect:?}");

        // With the lock off (A), an edge moves alone and the shape is its own.
        h.press(Modifiers::NONE, Key::A);
        let moved = crop_box(&h);
        drag(&mut h, moved.center_bottom(), moved.center_bottom() - egui::vec2(0.0, moved.height() / 2.0), Modifiers::NONE);
        let rect = h.settings().crop.geometry.rect;
        assert!((rect.y1 - 0.25).abs() < 0.01 && (rect.x0 - 0.5).abs() < 1e-6 && (rect.x1 - 1.0).abs() < 1e-6, "{rect:?}");
        assert_eq!(crop::aspect(h.settings(), (w, h0)), None);
        // Shift holds the shape for one drag.
        let wide = crop_box(&h);
        drag(&mut h, wide.left_top(), wide.left_top() + egui::vec2(wide.width() / 2.0, 0.0), Modifiers::SHIFT);
        assert!((shape(&h) - 2.0 * w / h0).abs() < 0.05, "{:?}", h.settings().crop.geometry.rect);

        // Each drag was a step.
        for _ in 0..4 {
            h.press(Modifiers::COMMAND, Key::Z);
        }
        assert!(h.settings().crop.geometry.is_identity());
        assert_eq!(crop_box(&h), whole);
    }

    #[test]
    fn a_drag_outside_the_crop_turns_the_frame_and_a_line_with_ctrl_levels_it() {
        let (_folder, mut h) = cropping("crop-turn");
        let (w, h0) = h.app.dims().unwrap();
        h.app.edit(|history, _| history.set(|settings| crop::set_aspect(settings, (w, h0), crop::Aspect::Ratio(1, 1))));
        h.frame(vec![]);
        let (on, area) = (crop_box(&h), h.app.loupe.area());
        // Right of the square crop, level with the middle of the frame,
        // and 5° round it, clockwise.
        let (centre, reach) = (area.center(), on.width() / 2.0 + 30.0);
        let round = |degrees: f32| centre + reach * egui::Vec2::angled(degrees.to_radians());
        drag(&mut h, round(0.0), round(5.0), Modifiers::NONE);
        let geometry = h.settings().crop.geometry;
        assert!((geometry.angle - 5.0).abs() < 0.05, "{}", geometry.angle);
        assert!(geometry.is_within_image(w, h0));
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().crop.geometry.angle, 0.0, "one step");

        // A line drawn with Ctrl, dropping 3° to the right, is made level.
        let from = centre - egui::vec2(100.0, 0.0);
        drag(&mut h, from, from + 200.0 * egui::Vec2::angled(3f32.to_radians()), Modifiers::COMMAND);
        assert!((h.settings().crop.geometry.angle + 3.0).abs() < 0.05, "{}", h.settings().crop.geometry.angle);
        assert_eq!(h.saved().as_ref(), Some(h.settings()));

        // A double-click inside the crop puts the tool away.
        let on = crop_box(&h);
        for _ in 0..2 {
            h.pointer(on.center(), Some(true));
            h.pointer(on.center(), Some(false));
        }
        assert!(h.app.crop.is_none());
        assert!((h.settings().crop.geometry.angle + 3.0).abs() < 0.05, "with the crop as it was");
    }

    #[test]
    fn quarter_turns_and_the_crops_reset_are_steps_with_or_without_the_tool() {
        let folder = Folder::new("turns", &[1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let (w, h0) = h.app.dims().unwrap();
        h.press(Modifiers::COMMAND, Key::CloseBracket);
        assert_eq!(h.app.dims(), Some((h0, w)), "on its side");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        h.press(Modifiers::COMMAND, Key::OpenBracket);
        assert_eq!(h.app.dims(), Some((w, h0)));
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.app.dims(), Some((h0, w)), "each turn is a step");

        let dims = h.app.dims().unwrap();
        h.app.edit(|history, _| history.set(|settings| crop::straighten(settings, dims, 5.0)));
        assert_eq!(h.settings().crop.geometry.angle, 5.0);
        h.press(Modifiers::COMMAND | Modifiers::ALT, Key::R);
        assert!(h.settings().crop.geometry.is_identity());
        assert_eq!(h.app.dims(), Some((h0, w)), "still on its side");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
    }

    #[test]
    fn ctrl_u_is_auto_tone_and_ctrl_shift_u_auto_white_balance_each_one_step() {
        use lightcraft_develop::WbMode;
        let folder = Folder::new("auto", &[1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let fresh = h.settings().clone();

        // The test's frame is one flat grey: nothing in it clips, so
        // there's nothing for Whites and Blacks to bring in.
        h.press(Modifiers::COMMAND, Key::U);
        assert_eq!(h.settings(), &fresh);
        assert_eq!(h.said(), "Nothing clips that Whites or Blacks could bring in");
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 1, "not unmarked");
        // Auto sets them again where they've been moved.
        h.app.edit(|history, _| history.set(|settings| (settings.light.whites, settings.light.blacks) = (30.0, -20.0)));
        h.press(Modifiers::COMMAND, Key::U);
        assert_eq!((h.settings().light.whites, h.settings().light.blacks), (0.0, 0.0));
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!((h.settings().light.whites, h.settings().light.blacks), (30.0, -20.0), "one step");
        h.press(Modifiers::COMMAND, Key::Z);

        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::U);
        assert_eq!(h.settings().wb.mode, WbMode::Auto);
        assert_eq!(h.settings().light, fresh.light, "and only the white balance");

        // Exposure and level have no key: a script, or their buttons.
        h.app.run(Command::AutoExposure, &h.ctx.clone());
        assert_ne!(h.settings().light.exposure, 0.0);
        let before = h.settings().clone();
        h.app.run(Command::AutoLevel, &h.ctx.clone());
        assert_eq!(h.settings(), &before);
        assert_eq!(h.said(), "Couldn't find a horizon or uprights to level by");
    }

    #[test]
    fn an_edit_from_a_newer_omalux_is_shown_fresh_and_never_written_over() {
        let folder = Folder::new("newer", &[1]);
        let raw = folder.0.join("DSC00001.ARW");
        let mut edited = look::fresh(5200.0, 3.0);
        edited.light.exposure = 0.7;
        sidecar::update(&raw, |xmp| {
            let with = sidecar::with_edit(xmp, &sidecar::Edit::new(edited.clone()))?;
            Ok(with.replace(&format!(">{}</omalux:processVersion>", sidecar::PROCESS_VERSION), ">99</omalux:processVersion>"))
        })
        .unwrap();
        let newer = std::fs::read_to_string(sidecar::path_for(&raw)).unwrap();
        assert!(newer.contains(">99<"));

        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        assert!(h.app.message.as_ref().is_some_and(|(text, _)| text.contains("newer OmaLux")), "{:?}", h.app.message);
        assert_eq!(h.settings().light.exposure, 0.0, "shown as a fresh raw");
        h.press(Modifiers::NONE, Key::Equals);
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::R);
        let track = h.track("light.contrast");
        h.pointer(track.center() + egui::vec2(20.0, 0.0), Some(true));
        h.pointer(track.center() + egui::vec2(20.0, 0.0), Some(false));
        assert_eq!((h.settings().light.exposure, h.settings().light.contrast), (0.0, 0.0), "and not changed");
        assert!(h.app.message.as_ref().is_some_and(|(text, _)| text.contains("left as it is")), "{:?}", h.app.message);
        h.app.disk.flush();
        assert_eq!(std::fs::read_to_string(sidecar::path_for(&raw)).unwrap(), newer);
        // The mark is still the frame's to change.
        h.press(Modifiers::NONE, Key::Num3);
        h.app.disk.flush();
        assert_eq!(std::fs::read_to_string(sidecar::path_for(&raw)).unwrap(), newer.replace("xmp:Rating=\"1\"", "xmp:Rating=\"3\""));
    }

    /// Ctrl+C and Ctrl+V as egui-winit sends them: a Copy event, and a V
    /// let go that was never pressed.
    fn copy(h: &mut Harness) {
        h.frame(vec![Event::ModifiersChanged(Modifiers::COMMAND), Event::Copy]);
    }

    fn paste(h: &mut Harness, modifiers: Modifiers) {
        h.frame(vec![
            Event::ModifiersChanged(modifiers),
            Event::Key { key: Key::V, physical_key: None, pressed: false, repeat: false, modifiers },
        ]);
    }

    /// The first frame of a shoot of `frames` picks, with Exposure at 0.10
    /// and Tint a nudge down from as shot, copied.
    fn copied(name: &str, frames: usize) -> (Folder, Harness) {
        let folder = Folder::new(name, &vec![1; frames]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::Equals);
        h.press(Modifiers::NONE, Key::Equals);
        h.press(Modifiers::NONE, Key::Comma);
        h.press(Modifiers::NONE, Key::Minus);
        copy(&mut h);
        (folder, h)
    }

    #[test]
    fn ctrl_c_copies_every_setting_and_ctrl_v_pastes_it_onto_the_next_frame() {
        let folder = Folder::new("copy", &[1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        paste(&mut h, Modifiers::COMMAND);
        assert!(h.said().contains("Nothing to paste: copy a frame's settings first (Ctrl+C)"), "{}", h.said());
        drop(h);

        let (_folder, mut h) = copied("copy", 2);
        assert_eq!(h.said(), "Copied every setting of DSC00001.ARW");
        let first = h.settings().clone();
        assert_eq!((first.light.exposure > 0.09, first.wb.mode, first.wb.tint), (true, lightcraft_develop::WbMode::Custom, 1.0));
        paste(&mut h, Modifiers::COMMAND);
        assert_eq!(h.said(), "Nothing to paste: these settings are there already");

        h.press(Modifiers::NONE, Key::ArrowRight);
        h.settle();
        paste(&mut h, Modifiers::COMMAND);
        assert_eq!(h.said(), "Pasted onto DSC00002.ARW");
        assert_eq!((h.settings(), h.saved().as_ref()), (&first, Some(&first)));
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &look::fresh(5200.0, 3.0), "one step");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        assert_eq!(h.said(), "Took the paste back from DSC00002.ARW");
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        assert_eq!((h.settings(), h.said()), (&first, "Pasted again onto DSC00002.ARW"));

        // What was copied can be pasted in the next shoot too.
        let other = Folder::new("copy-other", &[1]);
        h.app.open(other.0.clone());
        h.settle();
        paste(&mut h, Modifiers::COMMAND);
        assert_eq!(h.settings(), &first);
    }

    #[test]
    fn a_paste_onto_many_frames_is_one_step_to_undo_whatever_has_been_decoded() {
        let (folder, mut h) = copied("paste-many", 6);
        let first = h.settings().clone();
        let fresh = look::fresh(5200.0, 3.0);
        let names: Vec<String> = (1..=6).map(|i| format!("DSC{i:05}.ARW")).collect();
        let frames: Vec<PathBuf> = names.iter().map(|name| folder.0.join(name)).collect();
        // The frame they were copied from among them, as in its stack; the
        // last three haven't been decoded (two are prepared ahead).
        assert!(h.app.edits.contains_key(&frames[2]) && !h.app.edits.contains_key(&frames[3]));
        h.app.paste(frames.clone());
        assert_eq!(h.said(), "Pasted onto 5 frames");
        for name in &names[1..] {
            let saved = h.saved_of(name).unwrap();
            assert_eq!((saved.light.exposure, saved.wb.mode, saved.wb.tint), (first.light.exposure, first.wb.mode, 1.0), "{name}");
        }
        h.settle();
        let shown = h.app.loupe.shown(&frames[1]).unwrap().render.as_ref().map(|r| r.settings.light.exposure);
        assert_eq!(shown, Some(first.light.exposure), "the frames prepared ahead are developed again");

        // Undone from the frame it was made from, which it didn't change.
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.said(), "Took the paste back from 5 frames");
        assert_eq!(h.settings(), &first, "its own edit isn't what's undone");
        for name in &names[1..] {
            assert_eq!(h.saved_of(name).map(|s| (s.light.exposure, s.wb.mode)), Some((0.0, lightcraft_develop::WbMode::AsShot)), "{name}");
        }
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        assert_eq!(h.said(), "Pasted again onto 5 frames");
        assert_eq!(h.settings_of("DSC00006.ARW").light.exposure, first.light.exposure);

        // A frame edited since keeps its edit and the paste under it; the
        // rest are undone from any frame of the paste.
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.settle();
        h.press(Modifiers::NONE, Key::Period);
        h.press(Modifiers::NONE, Key::Period);
        h.press(Modifiers::NONE, Key::Equals);
        let second = h.settings().clone();
        assert_eq!(second.light.contrast, 1.0);
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.settle();
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.said(), "Took the paste back from 4 frames");
        assert_eq!(h.settings(), &fresh);
        assert_eq!(h.settings_of("DSC00002.ARW"), second);
        assert_eq!(h.settings_of("DSC00001.ARW"), first);
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings_of("DSC00002.ARW"), second, "and there's nothing more to undo on this one");
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        assert_eq!(h.said(), "Pasted again onto 4 frames");

        // A frame pasted onto before it was decoded shows its own as-shot
        // white balance once it is, and the sliders work on it.
        let mut unseen = look::fresh(4100.0, -7.0);
        unseen.light.exposure = -1.0;
        h.app.clipboard = Some(Copied::new(&unseen, &[Group::WhiteBalance, Group::Light]));
        h.app.paste(vec![frames[5].clone()]);
        assert_eq!(h.saved_of("DSC00006.ARW").map(|s| (s.wb.mode, s.light.exposure)), Some((lightcraft_develop::WbMode::AsShot, -1.0)));
        h.press(Modifiers::NONE, Key::End);
        h.settle();
        assert_eq!((h.settings().wb.temp, h.settings().wb.tint, h.settings().light.exposure), (5200.0, 3.0, -1.0));
        h.press(Modifiers::NONE, Key::Equals);
        assert_eq!(h.settings().light.contrast, 1.0);
        h.press(Modifiers::COMMAND, Key::Z);
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().light.exposure, first.light.exposure, "back to the first paste");
    }

    #[test]
    fn ctrl_v_pastes_onto_the_selection_and_ctrl_z_takes_it_back_from_all_of_it() {
        let (_folder, mut h) = copied("paste-selection", 5);
        let first = h.settings().clone();
        let fresh = look::fresh(5200.0, 3.0);
        // The second to the fourth, the cursor on the fourth.
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.press(Modifiers::SHIFT, Key::ArrowRight);
        h.press(Modifiers::SHIFT, Key::ArrowRight);
        paste(&mut h, Modifiers::COMMAND);
        assert_eq!(h.said(), "Pasted onto 3 frames");
        for name in ["DSC00002.ARW", "DSC00003.ARW", "DSC00004.ARW"] {
            assert_eq!((h.settings_of(name), h.saved_of(name)), (first.clone(), Some(first.clone())), "{name}");
        }
        assert_eq!((h.settings_of("DSC00005.ARW"), h.saved_of("DSC00005.ARW")), (fresh.clone(), None), "and no further");
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.said(), "Took the paste back from 3 frames");
        for name in ["DSC00002.ARW", "DSC00003.ARW", "DSC00004.ARW"] {
            assert_eq!(h.settings_of(name).light.exposure, 0.0, "{name}");
        }
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [1, 2, 3], "the selection stays, to paste something else");
    }

    #[test]
    fn a_paste_leaves_an_edit_it_cant_read_alone_and_says_so() {
        let (folder, mut h) = copied("paste-newer", 4);
        let raw = folder.0.join("DSC00004.ARW");
        sidecar::update(&raw, |xmp| {
            let with = sidecar::with_edit(xmp, &sidecar::Edit::new(look::fresh(5200.0, 3.0)))?;
            Ok(with.replace(&format!(">{}</omalux:processVersion>", sidecar::PROCESS_VERSION), ">99</omalux:processVersion>"))
        })
        .unwrap();
        let newer = std::fs::read_to_string(sidecar::path_for(&raw)).unwrap();
        h.app.paste(vec![folder.0.join("DSC00003.ARW"), raw.clone()]);
        assert_eq!(h.app.message.clone().unwrap(), ("Pasted onto DSC00003.ARW; 1 has an edit this OmaLux can't read, and was left as it is".to_owned(), true));
        h.app.paste(vec![raw.clone()]);
        assert_eq!(h.said(), "Pasted nothing; 1 has an edit this OmaLux can't read, and was left as it is");
        h.app.disk.flush();
        assert_eq!(std::fs::read_to_string(sidecar::path_for(&raw)).unwrap(), newer);
    }

    #[test]
    fn ctrl_alt_v_pastes_the_settings_of_the_frame_the_cursor_came_from() {
        let folder = Folder::new("previous", &[1, 1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        paste(&mut h, Modifiers::COMMAND | Modifiers::ALT);
        assert_eq!(h.said(), "There's no previous frame to paste from yet");
        h.press(Modifiers::NONE, Key::Equals);
        let first = h.settings().clone();

        // From the frame it came from, not the one before it in the strip.
        h.press(Modifiers::NONE, Key::End);
        h.settle();
        paste(&mut h, Modifiers::COMMAND | Modifiers::ALT);
        assert_eq!(h.said(), "Pasted DSC00001.ARW's settings onto DSC00003.ARW");
        assert_eq!((h.settings(), h.saved().as_ref()), (&first, Some(&first)));
        assert!(h.app.clipboard.is_none(), "what was copied is left alone");

        // From a frame with no edit: back to a fresh raw.
        h.press(Modifiers::NONE, Key::ArrowLeft);
        h.settle();
        h.press(Modifiers::NONE, Key::ArrowRight);
        paste(&mut h, Modifiers::COMMAND | Modifiers::ALT);
        assert_eq!(h.settings(), &look::fresh(5200.0, 3.0));
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &first);

        // Another shoot has no previous frame to begin with.
        h.app.open(folder.0.clone());
        h.settle();
        paste(&mut h, Modifiers::COMMAND | Modifiers::ALT);
        assert_eq!(h.said(), "There's no previous frame to paste from yet");
    }

    #[test]
    fn z_and_a_click_zoom_to_100_percent_where_the_full_size_is_rendered() {
        let folder = Folder::new("zoom", &[1, 1]);
        let raw = |name: &str| folder.0.join(name);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let window = |h: &Harness, name: &str| h.app.loupe.shown(&raw(name)).unwrap().zoomed.as_ref().map(|z| (z.key.window, z.key.full));
        let fitted: egui::Rect = h.ctx.data(|d| d.get_temp(egui::Id::new("loupe frame"))).unwrap();
        assert!(h.app.loupe.zoom().is_none() && window(&h, "DSC00001.ARW").is_none());

        // A tap of Z: at 100%, and the window on screen rendered from the full size
        // (the test decoder's is 192×128, smaller than the loupe: all of it).
        h.press(Modifiers::NONE, Key::Z);
        assert_eq!(h.app.loupe.zoom(), Some([0.5, 0.5]));
        h.settle();
        assert_eq!(window(&h, "DSC00001.ARW"), Some(([0, 0, 192, 128], (192, 128))));
        let zoomed: egui::Rect = h.ctx.data(|d| d.get_temp(egui::Id::new("loupe frame"))).unwrap();
        assert_eq!((zoomed.size(), zoomed.center()), (egui::vec2(192.0, 128.0), fitted.center()), "a pixel of the frame to a pixel of the screen");

        // Stepping stays at 100%, on the next frame's own full size.
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.settle();
        assert_eq!(window(&h, "DSC00002.ARW"), Some(([0, 0, 192, 128], (192, 128))));
        // Another tap: back to the whole frame.
        h.press(Modifiers::NONE, Key::Z);
        h.settle();
        assert!(h.app.loupe.zoom().is_none());

        // Held longer than a tap, Z is a look that ends when it's let go.
        h.frame(key_events(Modifiers::NONE, Key::Z, true));
        assert!(h.app.loupe.zoom().is_some());
        for _ in 0..10 {
            h.frame(vec![]);
        }
        h.frame(key_events(Modifiers::NONE, Key::Z, false));
        assert!(h.app.loupe.zoom().is_none());

        // A click zooms in, and a click again out.
        h.click(fitted.center());
        assert!(h.app.loupe.zoom().is_some());
        h.click(fitted.center());
        assert!(h.app.loupe.zoom().is_none());
    }

    #[test]
    fn a_frame_opens_with_the_edit_in_its_sidecar() {
        let folder = Folder::new("saved", &[1, 1]);
        let raw = folder.0.join("DSC00002.ARW");
        let mut edited = look::fresh(5200.0, 3.0);
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

    #[test]
    fn a_shoot_is_reopened_where_it_was_left_with_its_filter() {
        let folder = Folder::new("reopen", &[1, 0, 1, 2]);
        let dir = folder.0.clone();
        let mut h = Harness::new(&[]);
        h.app.open(dir.clone());
        assert_eq!(current(&h), "DSC00001.ARW");
        assert_eq!(h.app.shoot.as_ref().unwrap().filter(), Filter::Picks);

        // Step to DSC00004.ARW and toggle filter to All.
        h.press(Modifiers::NONE, Key::ArrowRight);
        assert_eq!(current(&h), "DSC00003.ARW");
        h.press(Modifiers::NONE, Key::ArrowRight);
        assert_eq!(current(&h), "DSC00004.ARW");
        h.press(Modifiers::COMMAND, Key::L);
        assert_eq!(h.app.shoot.as_ref().unwrap().filter(), Filter::All);

        // Close the shoot (Ctrl+W).
        h.press(Modifiers::COMMAND, Key::W);
        assert!(h.app.shoot.is_none());

        // Reopen the shoot by its folder: goes back to frame 4 and Filter::All.
        h.app.open(dir.clone());
        assert_eq!(current(&h), "DSC00004.ARW");
        assert_eq!(h.app.shoot.as_ref().unwrap().filter(), Filter::All);
    }

    #[test]
    fn naming_a_raw_wins_over_the_remembered_place() {
        let folder = Folder::new("named", &[1, 0, 1]);
        let dir = folder.0.clone();
        let mut h = Harness::new(&[]);
        h.app.open(dir.clone());
        h.press(Modifiers::NONE, Key::ArrowRight);
        assert_eq!(current(&h), "DSC00003.ARW");

        // Close the shoot.
        h.press(Modifiers::COMMAND, Key::W);
        assert!(h.app.shoot.is_none());

        // Open naming DSC00001.ARW specifically: opens at DSC00001.ARW rather than 3.
        h.app.open(dir.join("DSC00001.ARW"));
        assert_eq!(current(&h), "DSC00001.ARW");
    }

    #[test]
    fn ctrl_w_closes_the_shoot_and_keeps_its_place() {
        let folder = Folder::new("close", &[1, 1, 1]);
        let dir = folder.0.clone();
        let mut h = Harness::new(&[]);
        h.app.open(dir.clone());
        assert_eq!(current(&h), "DSC00001.ARW");
        h.press(Modifiers::NONE, Key::ArrowRight);
        assert_eq!(current(&h), "DSC00002.ARW");

        // Ctrl+W closes the shoot.
        let output = h.press(Modifiers::COMMAND, Key::W);
        assert!(h.app.shoot.is_none());
        assert_eq!(titles(&output), ["OmaLux"]);
        assert_eq!(h.app.title, "OmaLux");

        // The place was kept.
        let place = h.app.state.place(&dir);
        assert_eq!(place.map(|p| p.frame.as_str()), Some("DSC00002.ARW"));
    }

    #[test]
    fn recent_shoots_are_listed_and_clicking_one_opens_it() {
        let folder = Folder::new("recent", &[1, 0, 1]);
        let dir = folder.0.clone();
        let mut h = Harness::new(&[]);
        h.app.open(dir.clone());
        h.press(Modifiers::COMMAND, Key::W);
        assert!(h.app.shoot.is_none());

        // An empty-state frame lists the recent shoot by name.
        let output = h.frame(vec![]);
        let rendered = texts(&output);
        let name = file_name(&dir);
        let target = rendered.iter().find(|(t, _)| t == &name);
        assert!(target.is_some(), "expected recent list to show {name}, got {rendered:?}");

        // Clicking it opens the shoot.
        let rect = target.unwrap().1;
        h.click(rect.center());
        assert!(h.app.shoot.is_some());
        assert_eq!(h.app.shoot.as_ref().map(Shoot::dir), Some(dir.as_path()));
    }

    #[test]
    fn ctrl_shift_e_starts_export_and_status_bar_follows_it() {
        let folder = Folder::new("export-shift", &[1]);
        let out_dir = folder.0.join("omalux");
        std::fs::create_dir_all(&out_dir).unwrap();
        let tiff_path = out_dir.join("DSC00001.tif");
        std::fs::write(&tiff_path, b"existing tiff").unwrap();

        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());

        h.press(Modifiers { shift: true, ..Modifiers::COMMAND }, Key::E);
        assert_eq!(h.app.message.as_ref().map(|(t, p)| (t.as_str(), *p)), Some(("Exporting 1 of 1…", false)));

        let started = std::time::Instant::now();
        while h.app.exporting.is_some() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "export timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.frame(vec![]);
        }

        assert_eq!(
            h.app.message.as_ref().map(|(t, p)| (t.as_str(), *p)),
            Some(("Exported 0 of 1 to omalux; 1 was there already", false))
        );
    }

    #[test]
    fn ctrl_e_starts_the_configured_program_with_round_trip_and_every_tiff() {
        let folder = Folder::new("export-edit", &[1]);
        let out_dir = folder.0.join("omalux");
        std::fs::create_dir_all(&out_dir).unwrap();
        let tiff_path = out_dir.join("DSC00001.tif");
        std::fs::write(&tiff_path, b"existing tiff").unwrap();

        let script = folder.0.join("editor.sh");
        let said = folder.0.join("said");
        std::fs::write(&script, format!("#!/bin/sh\necho \"$@\" > '{}'\n", said.display())).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();

        let mut h = Harness::new(&[]);
        h.app.config.editor = script.display().to_string();
        h.app.open(folder.0.clone());

        h.press(Modifiers::COMMAND, Key::E);
        assert_eq!(h.app.message.as_ref().map(|(t, p)| (t.as_str(), *p)), Some(("Exporting 1 of 1…", false)));

        let started = std::time::Instant::now();
        while !said.exists() || h.app.exporting.is_some() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "editor wasn't started");
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.frame(vec![]);
        }

        let given = std::fs::read_to_string(&said).unwrap();
        let expected = format!("--round-trip {}", tiff_path.display());
        assert_eq!(given.trim(), expected);
        assert_eq!(
            h.app.message.as_ref().map(|(t, p)| (t.as_str(), *p)),
            Some(("Exported 0 of 1 to omalux; 1 was there already", false))
        );
    }

    #[test]
    fn failure_to_start_program_is_reported_in_status_bar() {
        let folder = Folder::new("export-fail", &[1]);
        let out_dir = folder.0.join("omalux");
        std::fs::create_dir_all(&out_dir).unwrap();
        let tiff_path = out_dir.join("DSC00001.tif");
        std::fs::write(&tiff_path, b"existing tiff").unwrap();

        let mut h = Harness::new(&[]);
        h.app.config.editor = "non_existent_program_xyz_123".into();
        h.app.open(folder.0.clone());

        h.press(Modifiers::COMMAND, Key::E);
        assert_eq!(h.app.message.as_ref().map(|(t, p)| (t.as_str(), *p)), Some(("Exporting 1 of 1…", false)));

        let started = std::time::Instant::now();
        while h.app.exporting.is_some() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "export timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.frame(vec![]);
        }

        assert!(h.app.message.as_ref().is_some_and(|(t, p)| *p && t.contains("Couldn't start non_existent_program_xyz_123")));
    }

    #[test]
    fn corpus_raw_exports_through_harness() {
        let dir = std::env::var_os("LIGHTCRAFT_CORPUS")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/lightcraft/corpus"))
            .join("raw");
        let Ok(bytes) = std::fs::read(dir.join("arw-sony-a7m3-compressed.arw")) else {
            eprintln!("skip: CC0 corpus raws absent");
            return;
        };

        let folder = Folder::new("corpus-export", &[]);
        let raw_path = folder.0.join("DSC00001.ARW");
        std::fs::write(&raw_path, &bytes).unwrap();
        sidecar::update(&raw_path, |xmp| sidecar::with_rating(xmp, 1)).unwrap();

        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.press(Modifiers { shift: true, ..Modifiers::COMMAND }, Key::E);

        let started = std::time::Instant::now();
        while h.app.exporting.is_some() {
            assert!(started.elapsed() < std::time::Duration::from_secs(30), "export timed out");
            std::thread::sleep(std::time::Duration::from_millis(50));
            h.frame(vec![]);
        }

        let target = folder.0.join("omalux/DSC00001.tif");
        assert!(target.exists(), "TIFF was not exported");
        assert_eq!(h.app.message.as_ref().map(|(t, p)| (t.as_str(), *p)), Some(("Exported 1 of 1 to omalux", false)));
    }

    #[test]
    fn keys_mark_and_reach_the_sidecars() {
        let folder = Folder::new("keys-mark", &[0, 0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   1 / 2 frames");

        // P marks pick
        h.press(Modifiers::NONE, Key::P);
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   ★   1 / 2 frames");
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 1);

        // 3 marks 3 stars
        h.press(Modifiers::NONE, Key::Num3);
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   ★★★   1 / 2 frames");
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 3);

        // X marks reject
        h.press(Modifiers::NONE, Key::X);
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   ✕   1 / 2 frames");
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, sidecar::REJECT);

        // 0 unmarks
        h.press(Modifiers::NONE, Key::Num0);
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   1 / 2 frames");
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 0);

        // Mark 4 stars and check it reached the sidecar
        h.press(Modifiers::NONE, Key::Num4);
        h.app.disk.flush();
        let raw1 = folder.0.join("DSC00001.ARW");
        let xmp = sidecar::read(&raw1).unwrap().unwrap();
        assert_eq!(sidecar::rating(&xmp).unwrap(), Some(4));
    }

    #[test]
    fn a_failed_write_is_taken_back() {
        use std::os::unix::fs::PermissionsExt;
        let folder = Folder::new("failed-write", &[2]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 2);

        // Make directory read-only so sidecar::update fails to create tmp file
        std::fs::set_permissions(&folder.0, std::fs::Permissions::from_mode(0o555)).unwrap();

        // Marking 5 stars will fail on disk
        h.press(Modifiers::NONE, Key::Num5);
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 5);
        h.app.disk.flush();
        // A new frame lets step() collect disk problems
        h.frame(vec![]);
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 2);
        assert!(h.app.message.as_ref().is_some_and(|(msg, prob)| *prob && msg.contains("Couldn't mark DSC00001.ARW")));

        // Restore directory permissions for cleanup
        std::fs::set_permissions(&folder.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_pick_unmarked_while_the_picks_are_shown_stays_until_the_cursor_leaves_it() {
        let folder = Folder::new("unmark-shown", &[1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        assert_eq!(current(&h), "DSC00001.ARW");
        assert_eq!(h.app.shoot.as_ref().unwrap().filter(), Filter::Picks);
        assert!(h.app.shoot.as_ref().unwrap().shown(0));
        assert!(h.app.shoot.as_ref().unwrap().shown(1));

        // Unmark DSC00001
        h.press(Modifiers::NONE, Key::U);
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 0);
        // It stays in the filmstrip while current
        assert_eq!(current(&h), "DSC00001.ARW");
        assert!(h.app.shoot.as_ref().unwrap().shown(0));

        // Stepping to the next frame moves the cursor
        h.press(Modifiers::NONE, Key::ArrowRight);
        assert_eq!(current(&h), "DSC00002.ARW");
        // Now that the cursor left it, it's no longer shown
        assert!(!h.app.shoot.as_ref().unwrap().shown(0));
    }

    #[test]
    fn marks_are_flushed_on_exit() {
        let folder = Folder::new("on-exit", &[0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.press(Modifiers::NONE, Key::P);
        eframe::App::on_exit(&mut h.app);
        let raw = folder.0.join("DSC00001.ARW");
        let xmp = sidecar::read(&raw).unwrap().unwrap();
        assert_eq!(sidecar::rating(&xmp).unwrap(), Some(1));
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

    #[test]
    fn j_toggles_clipping_warnings() {
        let mut h = Harness::new(&[]);
        assert!(!h.app.clipping);
        h.press(Modifiers::NONE, Key::J);
        assert!(h.app.clipping);
        h.press(Modifiers::NONE, Key::J);
        assert!(!h.app.clipping);
    }

    #[test]
    fn the_histogram_is_the_renders_and_its_triangle_turns_the_warnings_on() {
        let folder = Folder::new("histogram", &[1]);
        let raw = folder.0.join("DSC00001.ARW");
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.frame(vec![]);
        assert!(h.app.loupe.histogram(&raw).is_none(), "an empty plate until the frame is rendered");
        h.settle();
        assert_eq!(h.app.loupe.histogram(&raw).map(|histogram| histogram.total), Some(48 * 32));
        let clipping = |h: &Harness| h.app.loupe.shown(&raw).unwrap().render.as_ref().unwrap().key.clipping;
        assert!(!clipping(&h));

        // A click on a triangle, like J, colours the clipped pixels in the loupe.
        let triangle: egui::Rect = h.ctx.data(|d| d.get_temp(egui::Id::new("highlight clipping"))).unwrap();
        h.click(triangle.center());
        assert!(h.app.clipping);
        h.settle();
        assert!(clipping(&h));
        h.press(Modifiers::NONE, Key::J);
        h.settle();
        assert!(!clipping(&h));
    }

    #[test]
    fn detail_and_lens_corrections_sliders_are_drawn_and_drag_changes_setting() {
        let folder = Folder::new("detail-lens", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();

        // The panels and subheadings are drawn.
        let _ = h.text("Sharpening");
        let _ = h.text("Noise Reduction");
        let _ = h.text("Defringe");

        // Detail sliders labelled as Lightroom's.
        let _ = h.text("Amount");
        let _ = h.text("Radius");
        let _ = h.text("Detail");
        let _ = h.text("Masking");
        let _ = h.text("Luminance");
        let _ = h.text("Color");

        // Lens Corrections sliders labelled under Lightcraft's own names.
        let _ = h.text("Purple Amount");
        let _ = h.text("Purple Hue Low");
        let _ = h.text("Purple Hue High");
        let _ = h.text("Green Amount");
        let _ = h.text("Green Hue Low");
        let _ = h.text("Green Hue High");

        // A drag on Amount changes that setting, is one step to undo, and is saved.
        assert_eq!(h.settings().detail.sharpen_amount, 40.0);
        let track = h.track("detail.sharpenAmount");
        h.pointer(track.center(), None);
        h.pointer(track.center(), Some(true));
        h.pointer(track.center() + egui::vec2(track.width() / 4.0, 0.0), None);
        h.pointer(track.center() + egui::vec2(track.width() / 4.0, 0.0), Some(false));
        let amount = h.settings().detail.sharpen_amount;
        assert!(amount > 40.0);
        assert_eq!(h.saved().map(|s| s.detail.sharpen_amount), Some(amount));
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().detail.sharpen_amount, 40.0, "one step to undo");
        assert_eq!(h.saved().map(|s| s.detail.sharpen_amount), Some(40.0));

        // A drag on Luminance changes that setting, is one step to undo, and is saved.
        assert_eq!(h.settings().detail.nr_luminance, 0.0);
        let track = h.track("detail.nrLuminance");
        h.pointer(track.center(), None);
        h.pointer(track.center(), Some(true));
        h.pointer(track.center() + egui::vec2(track.width() / 4.0, 0.0), None);
        h.pointer(track.center() + egui::vec2(track.width() / 4.0, 0.0), Some(false));
        let luma = h.settings().detail.nr_luminance;
        assert!(luma > 0.0);
        assert_eq!(h.saved().map(|s| s.detail.nr_luminance), Some(luma));
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().detail.nr_luminance, 0.0, "one step to undo");
        assert_eq!(h.saved().map(|s| s.detail.nr_luminance), Some(0.0));
    }

    #[test]
    fn double_click_on_amount_resets_to_fresh_raw_value_of_40() {
        let folder = Folder::new("reset-amount", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();

        assert_eq!(h.settings().detail.sharpen_amount, 40.0);
        // Change Amount away from 40.
        let track = h.track("detail.sharpenAmount");
        h.pointer(track.center() + egui::vec2(20.0, 0.0), Some(true));
        h.pointer(track.center() + egui::vec2(20.0, 0.0), Some(false));
        assert_ne!(h.settings().detail.sharpen_amount, 40.0);

        for _ in 0..10 {
            h.frame(vec![]);
        }

        // A plain double-click goes back to the fresh raw's 40, not to 0.
        for _ in 0..2 {
            h.pointer(track.center(), Some(true));
            h.pointer(track.center(), Some(false));
        }
        assert_eq!(h.settings().detail.sharpen_amount, 40.0);
    }

    #[test]
    fn comma_and_period_step_through_sliders_and_equals_nudges() {
        let folder = Folder::new("step-sliders", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();

        // Selected begins on Exposure (index 2).
        assert_eq!(h.app.selected, 2);

        // Step back to index 0 (Temp).
        h.press(Modifiers::NONE, Key::Comma);
        h.press(Modifiers::NONE, Key::Comma);
        assert_eq!(h.app.selected, 0);

        // Step forward through Light (2..7), HDR (8..10), Presence (11..15), to Amount in Detail (16).
        for _ in 0..16 {
            h.press(Modifiers::NONE, Key::Period);
        }
        assert_eq!(h.app.selected, 16);
        assert_eq!(h.settings().detail.sharpen_amount, 40.0);
        h.press(Modifiers::NONE, Key::Equals);
        assert_eq!(h.settings().detail.sharpen_amount, 41.0, "nudged Amount");

        // Step forward to Luminance (index 20).
        for _ in 0..4 {
            h.press(Modifiers::NONE, Key::Period);
        }
        assert_eq!(h.app.selected, 20);
        assert_eq!(h.settings().detail.nr_luminance, 0.0);
        h.press(Modifiers::NONE, Key::Equals);
        assert_eq!(h.settings().detail.nr_luminance, 1.0, "nudged Luminance");

        // Step forward past the profile's Distortion and Vignetting to
        // Purple Amount in Lens Corrections (index 24).
        for _ in 0..4 {
            h.press(Modifiers::NONE, Key::Period);
        }
        assert_eq!(h.app.selected, 24);
        assert_eq!(h.settings().optics.defringe_purple_amount, 0.0);
        h.press(Modifiers::NONE, Key::Equals);
        assert_eq!(h.settings().optics.defringe_purple_amount, 1.0, "nudged Purple Amount");

        // Step back with comma.
        h.press(Modifiers::NONE, Key::Comma);
        assert_eq!(h.app.selected, 23);
    }

    #[test]
    fn clicking_remove_chromatic_aberration_and_profile_corrections_saves_and_ctrl_z_undoes() {
        let folder = Folder::new("remove-ca", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();

        assert!(!h.settings().optics.remove_ca);
        assert!(h.settings().optics.lens_profile, "a fresh raw has its profile's corrections on");
        let _ = h.text("No profile for this lens");
        let ca_rect = h.text("Remove Chromatic Aberration");
        h.click(ca_rect.center());
        assert!(h.settings().optics.remove_ca);
        assert_eq!(h.saved().map(|s| s.optics.remove_ca), Some(true));

        h.press(Modifiers::COMMAND, Key::Z);
        assert!(!h.settings().optics.remove_ca, "Ctrl+Z turns it off");
        assert_eq!(h.saved().map(|s| s.optics.remove_ca), Some(false));

        // Enable Profile Corrections likewise, the other way.
        let profile_rect = h.text("Enable Profile Corrections");
        h.click(profile_rect.center());
        assert!(!h.settings().optics.lens_profile);
        assert_eq!(h.saved().map(|s| s.optics.lens_profile), Some(false));
        h.press(Modifiers::COMMAND, Key::Z);
        assert!(h.settings().optics.lens_profile, "Ctrl+Z turns it back on");
    }

    #[test]
    fn f_goes_to_flat_and_back_to_camera_match_and_so_does_a_click_on_the_profile() {
        let folder = Folder::new("flat", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();

        assert_eq!(Profile::of(h.settings()), Profile::Camera, "a fresh raw opens under Camera match");
        assert_eq!(h.saved(), None);
        h.press(Modifiers::NONE, Key::F);
        assert_eq!(Profile::of(h.settings()), Profile::Flat);
        assert_eq!(h.saved().as_ref().map(Profile::of), Some(Profile::Flat), "it's part of the edit");
        h.press(Modifiers::NONE, Key::F);
        assert_eq!(Profile::of(h.settings()), Profile::Camera);

        // The panel names the profile after the camera's Creative Style, and a click swaps it.
        let named = h.text("Camera Standard");
        h.click(named.center());
        assert_eq!(Profile::of(h.settings()), Profile::Flat);
        let flat = h.text("Flat");
        h.click(flat.center());
        assert_eq!(Profile::of(h.settings()), Profile::Camera);
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(Profile::of(h.settings()), Profile::Flat, "each is a step");
    }

    #[test]
    fn an_edit_from_before_keeps_its_fitted_look_until_its_profile_is_changed() {
        let folder = Folder::new("fitted", &[1]);
        let raw = folder.0.join("DSC00001.ARW");
        // As M1 to M3 saved an edit: Lightcraft's own profile id, process version 1.
        let mut before = DevelopSettings::for_raw(5200.0, 3.0);
        before.light.exposure = 0.5;
        sidecar::update(&raw, |xmp| {
            let with = sidecar::with_edit(xmp, &sidecar::Edit::new(before.clone()))?;
            Ok(with.replace(&format!(">{}</omalux:processVersion>", sidecar::PROCESS_VERSION), ">1</omalux:processVersion>"))
        })
        .unwrap();
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();

        assert_eq!(h.settings(), &before);
        assert_eq!(Profile::of(h.settings()), Profile::Fitted);
        h.press(Modifiers::NONE, Key::Equals);
        assert_eq!(Profile::of(h.settings()), Profile::Fitted, "an edit to it leaves its look alone");
        // Reset is a fresh raw: Camera match.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::R);
        h.settle();
        assert_eq!(Profile::of(h.settings()), Profile::Camera);
        assert_eq!(h.settings(), &look::fresh(5200.0, 3.0));
    }

    #[test]
    fn a_panels_switch_shows_the_frame_without_it_and_saves_nothing() {
        let folder = Folder::new("switches", &[1, 1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        let specs: Vec<_> = develop::sliders().collect();
        h.app.selected = specs.iter().position(|spec| spec.id == "light.hdr").unwrap();
        for _ in 0..30 {
            h.press(Modifiers::NONE, Key::Equals);
        }
        let saved = h.saved();
        // The switch is just left of the panel's title.
        let output = h.frame(vec![]);
        let title = texts(&output).into_iter().find(|(text, _)| text == "HDR").map(|(_, rect)| rect).expect("the HDR panel");
        let switch = title.left_center() - egui::vec2(10.0, 0.0);
        let shown = |h: &mut Harness| h.app.wanted()[0].settings.as_ref().map(|settings| settings.light.hdr);
        h.click(switch);
        assert_eq!(h.app.off, [Group::Hdr]);
        assert_eq!((shown(&mut h), h.settings().light.hdr), (Some(0.0), 30.0), "the loupe without it, the edit as it was");
        assert_eq!(h.saved(), saved);
        assert!(texts(&h.frame(vec![])).iter().any(|(text, _)| text == "Off: HDR"));
        // Onto the next frame, and back: still off.
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.settle();
        h.press(Modifiers::NONE, Key::ArrowLeft);
        assert_eq!(h.app.off, [Group::Hdr]);
        // A nudge of one of its sliders turns it back on.
        h.press(Modifiers::NONE, Key::Equals);
        assert!(h.app.off.is_empty());
        assert_eq!(shown(&mut h), Some(31.0));
        h.click(switch);
        h.app.open(folder.0.clone());
        assert!(h.app.off.is_empty(), "a shoot opens with every panel on");
    }

        #[test]
    fn hdr_is_a_slider_between_light_and_presence() {
        let folder = Folder::new("hdr", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();

        let specs: Vec<_> = develop::sliders().collect();
        let at = specs.iter().position(|spec| spec.id == "light.hdr").unwrap();
        let ids = |range: std::ops::RangeInclusive<usize>| specs[range].iter().map(|spec| spec.id).collect::<Vec<_>>();
        assert_eq!(ids(at - 1..=at + 3), ["light.blacks", "light.hdr", "light.hdrHighlights", "light.hdrShadows", "effects.texture"]);
        h.app.selected = at;
        h.press(Modifiers::NONE, Key::Equals);
        assert_eq!(h.settings().light.hdr, 1.0);
        assert_eq!(h.saved().map(|s| s.light.hdr), Some(1.0));
        h.press(Modifiers::NONE, Key::Minus);
        h.press(Modifiers::NONE, Key::Minus);
        assert_eq!(h.settings().light.hdr, 0.0, "it stops at 0");
    }

    #[test]
    fn y_alt_y_and_shift_y_are_lightrooms_before_after_views_with_one_zoom() {
        let folder = Folder::new("compare", &[1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let panes = |h: &mut Harness| {
            h.frame(vec![]);
            h.ctx.data(|d| d.get_temp::<[egui::Rect; 2]>(egui::Id::new("loupe panes")))
        };
        assert_eq!(panes(&mut h), None, "the loupe alone");
        h.press(Modifiers::NONE, Key::Y);
        h.settle();
        let [before, after] = panes(&mut h).expect("side by side");
        assert!(before.right() < after.left() && before.height() == after.height(), "{before:?} {after:?}");
        let path = h.app.shoot.as_ref().unwrap().frame().path.clone();
        assert!(h.app.loupe.shown(&path).is_some_and(|shown| shown.before.is_some()), "before is rendered");
        assert_eq!(h.app.renderer.shared_view().size, ((after.width() * h.ctx.pixels_per_point()) as usize, (after.height() * h.ctx.pixels_per_point()) as usize), "each the size of its room");

        h.press(Modifiers::ALT, Key::Y);
        let [before, after] = panes(&mut h).expect("one above the other");
        assert!(before.bottom() < after.top() && before.width() == after.width());
        h.press(Modifiers::SHIFT, Key::Y);
        let [before, after] = panes(&mut h).expect("split");
        assert_eq!(before, after);

        // One zoom for both: Z, and each gets its window at 100%.
        h.press(Modifiers::NONE, Key::Y);
        h.settle();
        h.press(Modifiers::NONE, Key::Z);
        h.frame(key_events(Modifiers::NONE, Key::Z, false));
        h.settle();
        let shown = h.app.loupe.shown(&path).unwrap();
        let (after, before) = (shown.zoomed.as_ref().map(|z| z.key.window), shown.zoomed_before.as_ref().map(|z| z.key.window));
        assert!(after.is_some() && after == before, "{after:?} {before:?}");

        // Y again is the loupe; so are \, R and W.
        h.press(Modifiers::NONE, Key::Y);
        assert_eq!(panes(&mut h), None);
        for key in [Key::Backslash, Key::R, Key::W] {
            h.press(Modifiers::NONE, Key::Y);
            assert!(panes(&mut h).is_some());
            h.press(Modifiers::NONE, key);
            assert_eq!(h.app.compare, None, "{key:?}");
            assert!(!h.app.before, "{key:?} isn't Before as well");
            (h.app.crop, h.app.selector) = (None, false);
        }
    }

    #[test]
    fn upright_is_a_step_found_on_the_frame_and_pasted_it_is_found_again() {
        use lightcraft_develop::Upright;
        let folder = Folder::new("upright", &[1, 1, 1, 1, 1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 3000.0);
        h.app.open(folder.0.clone());
        h.settle();
        let fresh = h.settings().clone();
        // Upright's button, not the slider of the same name below it.
        let output = h.frame(vec![]);
        let vertical = texts(&output).into_iter().find(|(text, _)| text == "Vertical").unwrap().1;
        h.click(vertical.center());
        assert_eq!(h.settings().geometry.upright, Upright::Vertical);
        assert!(h.settings().geometry.upright_transform.is_some(), "found and kept");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        let constrain = h.text("Constrain Crop");
        h.click(constrain.center());
        assert!(h.settings().geometry.constrain_crop);
        h.press(Modifiers::COMMAND, Key::Z);
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &fresh, "two steps");
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        assert_eq!(h.settings().geometry.upright, Upright::Vertical);

        // Pasted onto a frame not decoded yet (past the neighbours
        // prepared ahead): found once it is.
        copy(&mut h);
        let last = h.app.shoot.as_ref().unwrap().frames()[4].path.clone();
        assert!(h.app.renderer.source(&last).is_none());
        h.app.paste_onto(&h.app.clipboard.clone().unwrap(), std::slice::from_ref(&last));
        assert_eq!(h.app.edits[&last].history.settings().geometry.upright_transform, None, "not this frame's yet");
        h.press(Modifiers::NONE, Key::End);
        h.settle();
        assert_eq!(h.settings().geometry.upright, Upright::Vertical);
        assert!(h.settings().geometry.upright_transform.is_some(), "found on its own frame");
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().geometry.upright, Upright::Off, "the paste was the one step");
    }

    #[test]
    fn shift_t_draws_guided_uprights_lines_on_the_frame_and_esc_takes_them_back() {
        use lightcraft_develop::Upright;
        let folder = Folder::new("guided", &[1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let fresh = h.settings().geometry.clone();
        h.press(Modifiers::SHIFT, Key::T);
        h.settle();
        assert!(h.app.guided.is_some());
        assert_eq!(h.settings().geometry.upright, Upright::Guided);
        let frame = h.ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new("guided frame"))).expect("the frame is drawn");
        let at = |u: f32, v: f32| frame.min + frame.size() * egui::vec2(u, v);
        drag(&mut h, at(0.2, 0.1), at(0.25, 0.9), Modifiers::NONE);
        drag(&mut h, at(0.8, 0.1), at(0.75, 0.9), Modifiers::NONE);
        let guides = h.settings().geometry.guides.clone();
        assert_eq!(guides.len(), 2);
        assert!((guides[0].0.x - 0.2).abs() < 0.01 && (guides[0].1.y - 0.9).abs() < 0.01, "{guides:?}");
        assert_eq!(h.saved().map(|s| s.geometry.guides.len()), Some(2));
        // The frame shown is through its lens corrections alone.
        let wanted = h.app.wanted();
        assert_eq!(wanted[0].settings.as_ref().unwrap().geometry, Default::default());
        // Delete takes the last away; Esc, all of what the tool did.
        h.press(Modifiers::NONE, Key::Delete);
        assert_eq!(h.settings().geometry.guides.len(), 1);
        h.press(Modifiers::NONE, Key::Escape);
        assert!(h.app.guided.is_none());
        assert_eq!(h.settings().geometry, fresh);
        // Return keeps them.
        h.press(Modifiers::SHIFT, Key::T);
        h.settle();
        drag(&mut h, at(0.2, 0.1), at(0.25, 0.9), Modifiers::NONE);
        h.press(Modifiers::NONE, Key::Enter);
        assert!(h.app.guided.is_none());
        assert_eq!(h.settings().geometry.guides.len(), 1);
    }

    #[test]
    fn i_cycles_lightrooms_info_overlays_and_ctrl_i_shows_or_hides_the_last() {
        let folder = Folder::new("info", &[1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();
        let path = h.app.shoot.as_ref().unwrap().frame().path.clone();
        let capture = look::Capture {
            time: Some("2026-10-04 15:43:16".into()),
            iso: Some(100),
            shutter: Some(1.0 / 250.0),
            aperture: Some(1.8),
            focal: Some(85.0),
            lens: Some("FE 85mm F1.8".into()),
            size: Some((6000, 4000)),
        };
        h.app.edits.get_mut(&path).unwrap().camera = Arc::new(Camera { capture, ..Camera::default() });
        let shown = |output: &egui::FullOutput| texts(output).into_iter().map(|(text, _)| text).collect::<Vec<_>>();
        let has = |output: &egui::FullOutput, text: &str| shown(output).iter().any(|t| t == text);
        let output = h.frame(vec![]);
        assert!(!has(&output, "2026-10-04 15:43:16"), "none to begin with");
        let output = h.press(Modifiers::NONE, Key::I);
        let output = if has(&output, "6000 × 4000") { output } else { h.frame(vec![]) };
        assert!(has(&output, "2026-10-04 15:43:16") && has(&output, "6000 × 4000"), "Info 1: {:?}", shown(&output));
        h.press(Modifiers::NONE, Key::I);
        let output = h.frame(vec![]);
        assert!(has(&output, "1/250 sec at f/1.8, ISO 100") && has(&output, "FE 85mm F1.8 at 85 mm"), "Info 2: {:?}", shown(&output));
        assert!(!has(&output, "6000 × 4000"));
        h.press(Modifiers::NONE, Key::I);
        let output = h.frame(vec![]);
        assert!(!has(&output, "1/250 sec at f/1.8, ISO 100"), "and none");
        h.press(Modifiers::COMMAND, Key::I);
        let output = h.frame(vec![]);
        assert!(has(&output, "1/250 sec at f/1.8, ISO 100"), "Ctrl+I shows the last one again");
        h.press(Modifiers::COMMAND, Key::I);
        let output = h.frame(vec![]);
        assert!(!has(&output, "1/250 sec at f/1.8, ISO 100"), "and hides it");
    }

    #[test]
    fn auto_in_white_balance_sets_auto_mode_and_clicking_returns_to_as_shot() {
        use lightcraft_develop::WbMode;
        let folder = Folder::new("wb-auto", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();

        assert_eq!(h.settings().wb.mode, WbMode::AsShot);
        // Initially state word is "As Shot", and to its left is "Auto"
        // (the first: Upright has an Auto too).
        let output = h.frame(vec![]);
        let auto_btn = texts(&output).into_iter().find(|(text, _)| text == "Auto").unwrap().1;
        h.click(auto_btn.center());
        assert_eq!(h.settings().wb.mode, WbMode::Auto);

        // State word is now "Auto", a click on it is the WB menu, and As
        // Shot in it goes back.
        let auto_state = h.text("Auto ▾");
        h.click(auto_state.center());
        let as_shot = h.text("As Shot");
        h.click(as_shot.center());
        assert_eq!(h.settings().wb.mode, WbMode::AsShot);
    }

    #[test]
    fn the_wb_menu_sets_a_preset_as_one_step_and_a_paste_keeps_it() {
        use lightcraft_develop::WbMode;
        let folder = Folder::new("wb-preset", &[1, 1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        let fresh = h.settings().clone();
        let menu = h.text("As Shot ▾");
        h.click(menu.center());
        let tungsten = h.text("Tungsten");
        h.click(tungsten.center());
        assert_eq!((h.settings().wb.mode, h.settings().wb.temp, h.settings().wb.tint), (WbMode::Tungsten, 2850.0, 0.0));
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        // Pasted as the preset, not as the frame's own as-shot white balance.
        copy(&mut h);
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.settle();
        paste(&mut h, Modifiers::COMMAND);
        assert_eq!((h.settings().wb.mode, h.settings().wb.temp), (WbMode::Tungsten, 2850.0));
        h.press(Modifiers::NONE, Key::ArrowLeft);
        h.settle();
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &fresh, "one step to undo");
    }

    #[test]
    fn auto_tone_and_auto_exposure_buttons_set_only_their_settings() {
        let folder = Folder::new("auto-buttons", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        let fresh = h.settings().clone();

        // "Auto Tone" sets Whites and Blacks and nothing else, in one step.
        // Nothing clips in the tests' flat grey frame, so they're set to 0.
        h.app.edit(|history, _| history.set(|settings| (settings.light.whites, settings.light.blacks, settings.light.contrast) = (30.0, -20.0, 10.0)));
        let moved = h.settings().clone();
        let tone_btn = h.text("Auto Tone");
        h.click(tone_btn.center());
        assert_eq!((h.settings().light.whites, h.settings().light.blacks), (0.0, 0.0));
        assert_eq!(h.settings().light.contrast, 10.0);
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &moved, "one step to undo Auto Tone");
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &fresh);

        // "Auto Exposure" sets Exposure and nothing else; is one step to undo.
        let exp_btn = h.text("Auto Exposure");
        h.click(exp_btn.center());
        assert_ne!(h.settings().light.exposure, 0.0);
        assert_eq!(h.settings().light.whites, fresh.light.whites);
        assert_eq!(h.settings().light.blacks, fresh.light.blacks);
        assert_eq!(h.settings().light.contrast, fresh.light.contrast);
        assert_eq!(h.settings().light.highlights, fresh.light.highlights);
        assert_eq!(h.settings().light.shadows, fresh.light.shadows);
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &fresh, "one step to undo Auto Exposure");
    }

    #[test]
    fn shift_double_click_sets_control_alone_or_shows_no_automatic_value() {
        let folder = Folder::new("shift-dc", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        let fresh = h.settings().clone();

        // Shift+double-click on Exposure's name sets Exposure alone.
        let exp_name = h.text("Exposure");
        for _ in 0..2 {
            h.click_with(exp_name.center(), Modifiers::SHIFT);
        }
        assert_ne!(h.settings().light.exposure, fresh.light.exposure);
        assert_eq!(h.settings().light.contrast, fresh.light.contrast);
        assert_eq!(h.settings().light.highlights, fresh.light.highlights);
        assert_eq!(h.settings().light.shadows, fresh.light.shadows);
        assert_eq!(h.settings().light.whites, fresh.light.whites);
        assert_eq!(h.settings().light.blacks, fresh.light.blacks);
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &fresh);
        for _ in 0..10 {
            h.frame(vec![]);
        }

        // Shift+double-click on Exposure's thumb also sets Exposure alone.
        let track = h.track("light.exposure");
        for _ in 0..2 {
            h.click_with(track.center(), Modifiers::SHIFT);
        }
        assert_ne!(h.settings().light.exposure, fresh.light.exposure);
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &fresh);
        for _ in 0..10 {
            h.frame(vec![]);
        }

        // Shift+double-click on Whites' name sets Whites alone: to 0, on
        // a frame where nothing clips.
        h.app.edit(|history, _| history.set(|settings| (settings.light.whites, settings.light.blacks) = (30.0, -20.0)));
        let whites_name = h.text("Whites");
        for _ in 0..2 {
            h.click_with(whites_name.center(), Modifiers::SHIFT);
        }
        assert_eq!((h.settings().light.whites, h.settings().light.blacks), (0.0, -20.0));
        assert_eq!(h.settings().light.exposure, fresh.light.exposure);
        h.press(Modifiers::COMMAND, Key::Z);
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings(), &fresh);
        for _ in 0..10 {
            h.frame(vec![]);
        }

        // Shift+double-click on Masking changes nothing and status bar says no automatic value.
        let masking_name = h.text("Masking");
        for _ in 0..2 {
            h.click_with(masking_name.center(), Modifiers::SHIFT);
        }
        assert_eq!(h.settings(), &fresh);
        assert_eq!(h.said(), "There's no automatic value for that slider");
    }

    #[test]
    fn the_app_runs_with_no_monitor_found() {
        let mut h = Harness::new(&[]);
        h.frame(vec![]);
        assert!(h.app.display.is_none());
    }

    #[test]
    fn a_right_click_in_the_filmstrip_goes_to_the_frame_and_keeps_a_selection_its_in() {
        let folder = Folder::new("right clicks", &[1, 1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.frame(vec![]);
        let cell = |h: &Harness, index: usize| h.app.filmstrip.cells.iter().find(|(i, _)| *i == index).map(|(_, r)| r.center()).expect("filmstrip cell");
        h.right_click(cell(&h, 1));
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 1);
        // Each right-click opens the menu: Esc puts it away (and it's
        // gone from under the pointer a frame later).
        h.press(Modifiers::NONE, Key::Escape);
        h.frame(vec![]);
        h.click_with(cell(&h, 2), Modifiers::COMMAND);
        h.right_click(cell(&h, 1));
        assert_eq!((h.app.shoot.as_ref().unwrap().current(), h.app.shoot.as_ref().unwrap().selection()), (1, vec![1, 2]));
        h.press(Modifiers::NONE, Key::Escape);
        h.frame(vec![]);
        h.right_click(cell(&h, 0));
        assert_eq!((h.app.shoot.as_ref().unwrap().current(), h.app.shoot.as_ref().unwrap().selection()), (0, vec![]));
    }

    #[test]
    fn filmstrip_ctrl_click_and_shift_click_selects_frames() {
        let folder = Folder::new("clicks", &[1, 1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.frame(vec![]);

        let cell = |h: &Harness, index: usize| {
            h.app.filmstrip.cells.iter().find(|(i, _)| *i == index).map(|(_, r)| *r).expect("filmstrip cell")
        };

        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 0);
        assert!(h.app.shoot.as_ref().unwrap().selection().is_empty());

        // Ctrl+click on cell 1: selects 0 and 1, goes to 1.
        h.click_with(cell(&h, 1).center(), Modifiers::COMMAND);
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 1);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [0, 1]);
        assert!(h.app.shoot.as_ref().unwrap().is_selected(0));
        assert!(h.app.shoot.as_ref().unwrap().is_selected(1));

        // Ctrl+click on cell 0: deselects 0, goes to 0.
        h.click_with(cell(&h, 0).center(), Modifiers::COMMAND);
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 0);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [1]);
        assert!(!h.app.shoot.as_ref().unwrap().is_selected(0));
        assert!(h.app.shoot.as_ref().unwrap().is_selected(1));

        // Shift+click on cell 2: selects from current (0) to 2, goes to 2.
        h.click_with(cell(&h, 2).center(), Modifiers::SHIFT);
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 2);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [0, 1, 2]);

        // Plain click on cell 1: clears selection, goes to 1.
        h.click(cell(&h, 1).center());
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 1);
        assert!(h.app.shoot.as_ref().unwrap().selection().is_empty());
    }

    #[test]
    fn shift_arrows_extend_the_selection() {
        let folder = Folder::new("shift-arrows", &[1, 1, 1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 0);
        assert!(h.app.shoot.as_ref().unwrap().selection().is_empty());

        // Shift+Right: extends from 0 to 1.
        h.press(Modifiers::SHIFT, Key::ArrowRight);
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 1);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [0, 1]);

        // Shift+Right again: extends to 2.
        h.press(Modifiers::SHIFT, Key::ArrowRight);
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 2);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [0, 1, 2]);

        // Shift+Left: extends to 1 (keeps 2 in selection).
        h.press(Modifiers::SHIFT, Key::ArrowLeft);
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 1);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [0, 1, 2]);
    }

    #[test]
    fn ctrl_a_selects_all_and_ctrl_d_clears_selection() {
        let folder = Folder::new("ctrl-a-d", &[1, 1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        assert!(h.app.shoot.as_ref().unwrap().selection().is_empty());

        h.press(Modifiers::COMMAND, Key::A);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [0, 1, 2]);

        h.press(Modifiers::COMMAND, Key::D);
        assert!(h.app.shoot.as_ref().unwrap().selection().is_empty());
    }

    #[test]
    fn a_plain_arrow_clears_the_selection() {
        let folder = Folder::new("plain-arrow-clears", &[1, 1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());

        h.press(Modifiers::COMMAND, Key::A);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [0, 1, 2]);

        h.press(Modifiers::NONE, Key::ArrowRight);
        assert!(h.app.shoot.as_ref().unwrap().selection().is_empty());
        assert_eq!(h.app.shoot.as_ref().unwrap().current(), 1);
    }

    #[test]
    fn opening_another_shoot_starts_with_no_selection() {
        let folder1 = Folder::new("open-first", &[1, 1]);
        let folder2 = Folder::new("open-second", &[1, 1]);
        let mut h = Harness::new(&[]);

        h.app.open(folder1.0.clone());
        h.press(Modifiers::COMMAND, Key::A);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [0, 1]);

        h.app.open(folder2.0.clone());
        assert!(h.app.shoot.as_ref().unwrap().selection().is_empty());
    }

    #[test]
    fn status_bar_shows_selection_count() {
        let folder = Folder::new("status-count", &[1, 1, 1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());

        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   ★   1 / 4 picks of 4 frames");

        h.press(Modifiers::SHIFT, Key::ArrowRight);
        h.press(Modifiers::SHIFT, Key::ArrowRight);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection().len(), 3);
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00003.ARW   ★   3 / 4 picks of 4 frames   3 selected");

        h.press(Modifiers::COMMAND, Key::D);
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00003.ARW   ★   3 / 4 picks of 4 frames");
    }

    #[test]
    fn ctrl_shift_e_exports_the_selection() {
        let folder = Folder::new("export-selection", &[1, 1, 1, 1]);
        let out_dir = folder.0.join("omalux");
        std::fs::create_dir_all(&out_dir).unwrap();
        for name in ["DSC00001.tif", "DSC00002.tif", "DSC00003.tif"] {
            std::fs::write(out_dir.join(name), b"existing tiff").unwrap();
        }

        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());

        // Select three frames (0, 1, 2)
        h.press(Modifiers::SHIFT, Key::ArrowRight);
        h.press(Modifiers::SHIFT, Key::ArrowRight);
        assert_eq!(h.app.shoot.as_ref().unwrap().selection(), [0, 1, 2]);

        h.press(Modifiers { shift: true, ..Modifiers::COMMAND }, Key::E);
        assert_eq!(h.app.message.as_ref().map(|(t, p)| (t.as_str(), *p)), Some(("Exporting 1 of 3…", false)));

        let started = std::time::Instant::now();
        while h.app.exporting.is_some() {
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "export timed out");
            std::thread::sleep(std::time::Duration::from_millis(10));
            h.frame(vec![]);
        }

        assert_eq!(
            h.app.message.as_ref().map(|(t, p)| (t.as_str(), *p)),
            Some(("Exported 0 of 3 to omalux; 3 were there already", false))
        );
    }

    #[test]
    fn copy_settings_dialog_copies_chosen_groups_and_remembers_choice() {
        let folder = Folder::new("checklist", &[0]);
        let mut h = Harness::new(&[]);

        // Does not open with no shoot.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
        assert!(h.app.choosing.is_none());

        h.app.open(folder.0.clone());
        // Does not open before current frame has been decoded (which can
        // be done by the frame the keys are read in, keys after).
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
        let decoded = h.app.edits.contains_key(&h.app.shoot.as_ref().unwrap().frame().path);
        assert_eq!(h.app.choosing.is_some(), decoded);
        h.app.choosing = None;

        h.settle();

        // Ctrl+Shift+C opens it with everything checked.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
        assert!(h.app.choosing.is_some());
        assert_eq!(h.app.choosing.as_ref().unwrap().chosen(), Group::ALL);

        // A key pressed while it's open doesn't reach the app (P doesn't mark).
        h.press(Modifiers::NONE, Key::P);
        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, 0);
        assert!(h.app.choosing.is_some(), "dialog stays open");

        // Unchecking White Balance, then Light.
        let wb_rect = h.text("White Balance");
        h.click(wb_rect.center());
        assert_eq!(h.app.choosing.as_ref().unwrap().chosen(), &Group::ALL[1..]);

        let light_rect = h.text("Light");
        h.click(light_rect.center());
        let rest: Vec<Group> = Group::ALL.iter().copied().filter(|group| !matches!(group, Group::WhiteBalance | Group::Light)).collect();
        assert_eq!(h.app.choosing.as_ref().unwrap().chosen(), rest);

        // Return copies only the rest into clipboard.
        h.press(Modifiers::NONE, Key::Enter);
        assert!(h.app.choosing.is_none());
        assert_eq!(h.app.clipboard.as_ref().unwrap().groups(), rest);

        // The next time it opens, the last choice is checked.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
        assert!(h.app.choosing.is_some());
        assert_eq!(h.app.choosing.as_ref().unwrap().chosen(), rest);

        // Check All selects everything again.
        let all_rect = h.text("Check All");
        h.click(all_rect.center());
        assert_eq!(h.app.choosing.as_ref().unwrap().chosen(), Group::ALL);

        // Esc copies nothing and leaves the clipboard as it was.
        h.press(Modifiers::NONE, Key::Escape);
        assert!(h.app.choosing.is_none());
        assert_eq!(h.app.clipboard.as_ref().unwrap().groups(), rest);

        // Since Esc cancelled, the next time it opens, the previously remembered choice is still checked.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
        assert_eq!(h.app.choosing.as_ref().unwrap().chosen(), rest);

        // Closing the shoot closes the dialog.
        h.app.close();
        assert!(h.app.choosing.is_none());
    }

    #[test]
    fn copy_settings_dialog_click_outside_and_copy_none() {
        let folder = Folder::new("checklist-cancel", &[0]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();

        // Open dialog.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
        assert!(h.app.choosing.is_some());

        // A click outside cancels the dialog, once it's been laid out
        // (egui sizes it unseen in its first frame).
        h.frame(vec![]);
        h.click(egui::pos2(10.0, 10.0));
        assert!(h.app.choosing.is_none());
        assert!(h.app.clipboard.is_none());

        // Reopen dialog and click Check None.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
        assert!(h.app.choosing.is_some());
        let none_rect = h.text("Check None");
        h.click(none_rect.center());
        assert!(h.app.choosing.as_ref().unwrap().chosen().is_empty());

        // Copy with nothing checked does what Cancel does (closes without copying).
        h.press(Modifiers::NONE, Key::Enter);
        assert!(h.app.choosing.is_none());
        assert!(h.app.clipboard.is_none());

        // Opening another shoot closes the dialog.
        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::C);
        assert!(h.app.choosing.is_some());
        h.app.open(folder.0.clone());
        assert!(h.app.choosing.is_none());
    }

    /// Open `folder` as if its frames had been shot at these times, in
    /// seconds: the tests' raws are empty files, which hold none.
    fn open_shot_at(h: &mut Harness, folder: &Folder, times: &[f64]) {
        h.app.open(folder.0.clone());
        let shoot = h.app.shoot.as_ref().unwrap();
        let frames = shoot.frames().iter().zip(times).map(|(frame, &at)| Frame { captured: Some(at), ..frame.clone() });
        h.app.shoot = Some(Shoot::new(shoot.dir().to_path_buf(), frames.collect()));
    }

    #[test]
    fn g_opens_the_current_frames_stack_out_and_closes_it_up() {
        let folder = Folder::new("stack-toggle", &[1, 1, 1, 1]);
        let mut h = Harness::new(&[]);
        open_shot_at(&mut h, &folder, &[0.0, 0.5, 1.0, 60.0]);
        h.settle();
        let shown = |h: &Harness| h.app.shoot.as_ref().unwrap().shown_indices();
        assert_eq!(shown(&h), [0, 3], "stacks start closed up");
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   ★   1 / 2 shown of 4 picks of 4 frames");
        h.press(Modifiers::NONE, Key::G);
        assert_eq!(shown(&h), [0, 1, 2, 3]);
        assert_eq!(status(h.app.shoot.as_ref().unwrap()), "DSC00001.ARW   ★   1 / 4 picks of 4 frames");
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.press(Modifiers::NONE, Key::G);
        assert_eq!((shown(&h), current(&h).as_str()), (vec![0, 3], "DSC00001.ARW"), "closed from any frame in it");
        h.press(Modifiers::NONE, Key::ArrowRight);
        h.press(Modifiers::NONE, Key::G);
        assert_eq!((shown(&h), current(&h).as_str()), (vec![0, 3], "DSC00004.ARW"), "a frame on its own has nothing to open");

        // Another shoot's stacks are its own, closed up.
        h.press(Modifiers::NONE, Key::Home);
        h.press(Modifiers::NONE, Key::G);
        let other = Folder::new("stack-toggle-other", &[1, 1, 1]);
        open_shot_at(&mut h, &other, &[0.0, 0.5, 1.0]);
        assert_eq!(shown(&h), [0]);
    }

    #[test]
    fn ctrl_shift_v_pastes_onto_the_whole_stack_and_nothing_the_filter_hides() {
        // Four frames shot together, the second not a pick, and one on its own.
        let folder = Folder::new("paste-stack", &[1, 0, 1, 1, 1]);
        let mut h = Harness::new(&[]);
        open_shot_at(&mut h, &folder, &[0.0, 0.5, 1.0, 1.5, 60.0]);
        h.settle();
        assert_eq!(h.app.shoot.as_ref().unwrap().shown_indices(), [0, 4]);
        h.press(Modifiers::NONE, Key::Equals);
        h.press(Modifiers::NONE, Key::Equals);
        let first = h.settings().clone();
        copy(&mut h);
        paste(&mut h, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(h.said(), "Pasted onto 2 frames");
        // Closed up, they haven't been decoded: as shot is each one's own all the same.
        for name in ["DSC00003.ARW", "DSC00004.ARW"] {
            let saved = h.saved_of(name).unwrap();
            assert_eq!((saved.light.exposure, saved.wb.mode), (first.light.exposure, lightcraft_develop::WbMode::AsShot), "{name}");
        }
        assert_eq!((h.saved_of("DSC00002.ARW"), h.saved_of("DSC00005.ARW")), (None, None), "not the frame the filter hides, nor the next stack");

        // Undone from the frame that stands for the stack, which it didn't change.
        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.said(), "Took the paste back from 2 frames");
        assert_eq!((h.settings_of("DSC00003.ARW").light.exposure, h.settings()), (0.0, &first));

        h.press(Modifiers::NONE, Key::End);
        h.settle();
        paste(&mut h, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(h.said(), "DSC00005.ARW isn't in a stack");
        assert_eq!(h.saved(), None);
    }

    #[test]
    fn crop_panel_is_drawn_only_while_the_tool_is_open() {
        let folder = Folder::new("crop-panel-open", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();

        let output = h.frame(vec![]);
        assert!(!texts(&output).iter().any(|(t, _)| t == "Crop & Straighten"));

        h.press(Modifiers::NONE, Key::R);
        let output = h.frame(vec![]);
        assert!(texts(&output).iter().any(|(t, _)| t == "Crop & Straighten"));

        h.press(Modifiers::NONE, Key::Enter);
        let output = h.frame(vec![]);
        assert!(!texts(&output).iter().any(|(t, _)| t == "Crop & Straighten"));
    }

    #[test]
    fn aspect_menu_crops_to_square_and_original_restores() {
        let folder = Folder::new("crop-aspect-square", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::R);

        let orig = h.text("Original");
        h.click(orig.center());
        let one_by_one = h.text("1 x 1");
        h.click(one_by_one.center());

        let dims = h.app.dims().unwrap();
        let rect = h.settings().crop.geometry.rect_px(dims.0, dims.1);
        assert!((rect.width() - rect.height()).abs() < 1e-3, "square in pixels: {rect:?}");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));

        let output = h.frame(vec![]);
        assert!(texts(&output).iter().any(|(t, _)| t == "1 x 1"));

        h.press(Modifiers::COMMAND, Key::Z);
        assert!(h.settings().crop.geometry.is_identity(), "one step to undo");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));

        let orig = h.text("Original");
        h.click(orig.center());
        let one = h.text("1 x 1");
        h.click(one.center());
        assert!(!h.settings().crop.geometry.is_identity());

        let combo = h.text("1 x 1");
        h.click(combo.center());
        let orig_menu = h.text("Original");
        h.click(orig_menu.center());
        assert!(h.settings().crop.geometry.is_identity(), "whole frame back");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));
    }

    #[test]
    fn swap_turns_crop_on_its_side_and_combo_still_shows_ratio() {
        let folder = Folder::new("crop-swap", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::R);

        let orig = h.text("Original");
        h.click(orig.center());
        let ratio = h.text("4 x 5 / 8 x 10");
        h.click(ratio.center());

        let dims = h.app.dims().unwrap();
        let landscape = h.settings().crop.geometry.rect_px(dims.0, dims.1);
        assert!(landscape.width() > landscape.height());

        let swap = h.text("Swap");
        h.click(swap.center());

        let portrait = h.settings().crop.geometry.rect_px(dims.0, dims.1);
        assert!(portrait.width() < portrait.height());

        let output = h.frame(vec![]);
        assert!(texts(&output).iter().any(|(t, _)| t == "4 x 5 / 8 x 10"));
    }

    #[test]
    fn angle_slider_drag_and_double_click_reset() {
        let folder = Folder::new("crop-angle-drag", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::R);

        let track = h.track("crop.angle");
        let dims = h.app.dims().unwrap();

        h.pointer(track.center(), Some(true));
        h.pointer(track.center() + egui::vec2(track.width() / 4.0, 0.0), None);

        assert!(h.settings().crop.geometry.angle > 0.0);
        assert!(h.settings().crop.geometry.is_within_image(dims.0, dims.1));
        assert_eq!(h.saved(), None, "not saved while dragging");

        h.pointer(track.center() + egui::vec2(track.width() / 4.0, 0.0), Some(false));
        assert!(h.saved().is_some(), "saved when drag ends");
        let saved_angle = h.saved().unwrap().crop.geometry.angle;
        assert_eq!(saved_angle, h.settings().crop.geometry.angle);

        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.settings().crop.geometry.angle, 0.0, "whole drag is one step to undo");
        assert_eq!(h.saved().map(|s| s.crop.geometry.angle), Some(0.0));

        h.press(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        assert_eq!(h.settings().crop.geometry.angle, saved_angle);

        let name = h.text("Angle");
        for _ in 0..2 {
            h.pointer(name.center(), Some(true));
            h.pointer(name.center(), Some(false));
        }
        assert_eq!(h.settings().crop.geometry.angle, 0.0);
        assert_eq!(h.saved().map(|s| s.crop.geometry.angle), Some(0.0));
    }

    #[test]
    fn typing_a_value_into_angle_sets_it() {
        let folder = Folder::new("crop-angle-type", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::R);

        let value = h.value("crop.angle");
        h.click(value.center());
        h.type_text("5.0");
        h.press(Modifiers::NONE, Key::Enter);

        assert_eq!(h.settings().crop.geometry.angle, 5.0);
        assert_eq!(h.saved().map(|s| s.crop.geometry.angle), Some(5.0));
    }

    #[test]
    fn rotate_buttons_swap_dims_and_put_it_back_each_a_step() {
        let folder = Folder::new("crop-rotate-btns", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::R);

        let (w, h0) = h.app.dims().unwrap();

        let right = h.text("Rotate Right");
        h.click(right.center());
        assert_eq!(h.app.dims(), Some((h0, w)));
        assert_eq!(h.saved().as_ref(), Some(h.settings()));

        let left = h.text("Rotate Left");
        h.click(left.center());
        assert_eq!(h.app.dims(), Some((w, h0)));
        assert_eq!(h.saved().as_ref(), Some(h.settings()));

        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.app.dims(), Some((h0, w)), "undo rotate left");

        h.press(Modifiers::COMMAND, Key::Z);
        assert_eq!(h.app.dims(), Some((w, h0)), "undo rotate right");
    }

    #[test]
    fn reset_clears_crop_and_angle_but_not_quarter_turn_and_close_closes() {
        let folder = Folder::new("crop-reset-close", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::R);

        let (w, h0) = h.app.dims().unwrap();
        let right = h.text("Rotate Right");
        h.click(right.center());
        assert_eq!(h.app.dims(), Some((h0, w)));

        let dims = h.app.dims().unwrap();
        h.app.edit(|history, _| {
            history.set(|settings| {
                crop::set_aspect(settings, dims, crop::Aspect::Ratio(1, 1));
                crop::straighten(settings, dims, 4.0);
            })
        });
        assert_eq!(h.settings().crop.geometry.angle, 4.0);
        assert!(!h.settings().crop.geometry.is_identity());

        let reset = h.text("Reset");
        h.click(reset.center());
        assert_eq!(h.settings().crop.geometry.angle, 0.0);
        assert!(h.settings().crop.geometry.is_identity());
        assert_eq!(h.app.dims(), Some((h0, w)), "quarter turn stays");
        assert_eq!(h.saved().as_ref(), Some(h.settings()));

        assert!(h.app.crop.is_some());
        let close = h.text("Close");
        h.click(close.center());
        assert!(h.app.crop.is_none());
    }

    #[test]
    fn crop_lock_button_flips_crop_locked_and_shown_word() {
        let folder = Folder::new("crop-lock", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::R);

        assert!(h.app.crop_locked);
        let locked = h.text("Locked");
        h.click(locked.center());

        assert!(!h.app.crop_locked);
        let output = h.frame(vec![]);
        assert!(texts(&output).iter().any(|(t, _)| t == "Unlocked"));
        assert!(!texts(&output).iter().any(|(t, _)| t == "Locked"));

        let unlocked = h.text("Unlocked");
        h.click(unlocked.center());

        assert!(h.app.crop_locked);
        let output = h.frame(vec![]);
        assert!(texts(&output).iter().any(|(t, _)| t == "Locked"));
        assert!(!texts(&output).iter().any(|(t, _)| t == "Unlocked"));
    }

    #[test]
    fn auto_on_flat_grey_frame_changes_nothing_and_status_bar_says_so() {
        let folder = Folder::new("crop-auto-level", &[1]);
        let mut h = Harness::new(&[]);
        h.screen = egui::vec2(800.0, 2000.0);
        h.app.open(folder.0.clone());
        h.settle();
        h.press(Modifiers::NONE, Key::R);

        assert_eq!(h.settings().crop.geometry.angle, 0.0);
        // The crop panel's Auto, above White Balance's.
        let output = h.frame(vec![]);
        let auto = texts(&output).into_iter().filter(|(text, _)| text == "Auto").map(|(_, rect)| rect).min_by(|a, b| a.top().total_cmp(&b.top())).unwrap();
        h.click(auto.center());

        assert_eq!(h.settings().crop.geometry.angle, 0.0);
        assert_eq!(h.said(), "Couldn't find a horizon or uprights to level by");
    }

    #[test]
    fn right_click_filmstrip_cell_and_click_pick() {
        let folder = Folder::new("filmstrip-menu-pick", &[1, 1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.frame(vec![]);

        let cell = |h: &Harness, index: usize| h.app.filmstrip.cells.iter().find(|(i, _)| *i == index).map(|(_, r)| r.center()).expect("filmstrip cell");
        h.right_click(cell(&h, 0));
        let output = h.frame(vec![]);
        let pick = texts(&output).into_iter().find(|(text, _)| text == "Pick").map(|(_, rect)| rect).expect("Pick item in menu");
        h.click(pick.center());

        assert_eq!(h.app.shoot.as_ref().unwrap().frame().rating, omalux_engine::shoot::PICK);
    }

    #[test]
    fn right_click_loupe_and_click_before_after() {
        let folder = Folder::new("loupe-menu-before-after", &[1]);
        let mut h = Harness::new(&[]);
        h.app.open(folder.0.clone());
        h.settle();

        assert!(!h.app.before);

        let loupe_center = h.app.loupe.area().center();
        h.right_click(loupe_center);

        let output = h.frame(vec![]);
        let before_after = texts(&output).into_iter().find(|(text, _)| text == "Before/After").map(|(_, rect)| rect).expect("Before/After item in menu");
        h.click(before_after.center());

        assert!(h.app.before);
    }
}
