use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align, Color32, CornerRadius, Id, Key, Layout, RichText, Sense, Stroke};
use egui_extras::{Column, TableBuilder};
use indexmap::IndexMap;
use serde_json::{Map, Value};

use scm::features::{self, FeatureSet, Source};
use scm::prefs;
use scm::selfupdate::{self, Block, Progress, Release, Span};
use scm::text::{fmt, plural};
use scm::updates;
use scm::value::{parse_value, py_str, type_name};
use scm::versions::State;
use scm::workspace::{Counts, Workspace};
use scm::{APP_TITLE, OFF};

const BG: Color32 = Color32::from_rgb(0x14, 0x16, 0x1b);
const PANEL: Color32 = Color32::from_rgb(0x1b, 0x1e, 0x25);
const RAISED: Color32 = Color32::from_rgb(0x24, 0x28, 0x32);
const LINE: Color32 = Color32::from_rgb(0x2d, 0x32, 0x3d);
const FG: Color32 = Color32::from_rgb(0xe4, 0xe7, 0xec);
const MUTED: Color32 = Color32::from_rgb(0x8c, 0x94, 0xa3);
const DIM: Color32 = Color32::from_rgb(0x5b, 0x62, 0x70);
const ACCENT: Color32 = Color32::from_rgb(0x4c, 0xc4, 0xa2);
const ON_ACCENT: Color32 = Color32::from_rgb(0x0c, 0x17, 0x14);
const SELECT: Color32 = Color32::from_rgb(0x1f, 0x3a, 0x33);
const HOVER: Color32 = Color32::from_rgb(0x2b, 0x30, 0x3b);
const BAD: Color32 = Color32::from_rgb(0xff, 0x8f, 0x80);
const BAD_BG: Color32 = Color32::from_rgb(0x2e, 0x1d, 0x1f);
const GOOD: Color32 = Color32::from_rgb(0x7f, 0xd3, 0xab);
const NEW: Color32 = Color32::from_rgb(0xf2, 0xc4, 0x6d);

const FOLLOW: &str = "Area choice";
const OFF_LABEL: &str = "Off in every mod";
const ALL: &str = "All";
const FEATURES_EVERY: u64 = 6 * 3600;

pub fn run() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_TITLE)
        .with_inner_size([1160.0, 760.0])
        .with_min_inner_size([920.0, 560.0]);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(APP_TITLE, options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

fn window_icon() -> Option<egui::IconData> {
    let dir = ico::IconDir::read(std::io::Cursor::new(include_bytes!("../../packaging/icon.ico"))).ok()?;
    let entry = dir.entries().iter().max_by_key(|e| e.width())?;
    let image = entry.decode().ok()?;
    Some(egui::IconData { rgba: image.rgba_data().to_vec(), width: image.width(), height: image.height() })
}

fn style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.push("Hack".into());
    }
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        let v = &mut s.visuals;
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.extreme_bg_color = PANEL;
        v.faint_bg_color = Color32::from_rgb(0x19, 0x1c, 0x22);
        v.override_text_color = Some(FG);
        v.selection.bg_fill = SELECT;
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.window_stroke = Stroke::new(1.0, LINE);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.inactive.bg_fill = RAISED;
        v.widgets.inactive.weak_bg_fill = RAISED;
        v.widgets.hovered.bg_fill = HOVER;
        v.widgets.hovered.weak_bg_fill = HOVER;
        v.widgets.active.bg_fill = LINE;
        v.widgets.active.weak_bg_fill = LINE;
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(12.0, 5.0);
    });
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn edit_text(v: &Value) -> String {
    struct Spaced;
    impl serde_json::ser::Formatter for Spaced {
        fn begin_array_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W, first: bool) -> std::io::Result<()> {
            if first { Ok(()) } else { w.write_all(b", ") }
        }
        fn begin_object_key<W: ?Sized + std::io::Write>(&mut self, w: &mut W, first: bool) -> std::io::Result<()> {
            if first { Ok(()) } else { w.write_all(b", ") }
        }
        fn begin_object_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
            w.write_all(b": ")
        }
    }
    match v {
        Value::Array(_) | Value::Object(_) => {
            let mut buf = Vec::new();
            let mut ser = serde_json::Serializer::with_formatter(&mut buf, Spaced);
            serde::Serialize::serialize(v, &mut ser).ok();
            String::from_utf8(buf).unwrap_or_default()
        }
        Value::Bool(b) => (if *b { "True" } else { "False" }).into(),
        other => py_str(other),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Priorities,
    Shared,
    Unique,
    AppUpdates,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Shared,
    Unique,
}

#[derive(Default)]
struct ListView {
    query: String,
    group: String,
    only_bad: bool,
    selected: Option<usize>,
    rows: Vec<usize>,
    built: Option<(u64, String, String, bool)>,
}

struct EditState {
    uid: usize,
    text: String,
    error: String,
    focus: bool,
}

enum Then {
    Load(String),
    Quit,
    Save,
    Restart(PathBuf),
}

enum Dialog {
    Confirm { title: String, text: String, then: Then },
    Error { title: String, text: String },
}

enum Msg {
    Updates { version: String, result: Result<IndexMap<String, (String, State)>, String> },
    Features { manual: bool, result: Result<(String, FeatureSet), String> },
    Release(Result<Release, String>),
    Installed(Result<PathBuf, String>),
}

enum Action {
    Load(String),
    Browse,
    Resolve(Option<String>),
    Save,
    SetVersion(String),
    CheckUpdates,
    AutoFeatures(bool),
    SetPriority(String, String),
    SetOverride(String, Option<String>),
    Area(String),
    Select(Kind, usize),
    Edit(usize),
    UseRecommended(usize),
    CheckApp,
    InstallApp,
    RestartApp,
    OpenReleasePage,
}

pub struct App {
    ws: Workspace,
    prefs: Map<String, Value>,
    path_text: String,
    tab: Tab,
    status: String,
    activity: Vec<String>,
    shared: ListView,
    unique: ListView,
    area: String,
    edit: Option<EditState>,
    dialog: Option<Dialog>,
    actions: Vec<Action>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    checking: bool,
    fetching_features: bool,
    pending_set: Option<Arc<FeatureSet>>,
    generation: u64,
    counts: Counts,
    allow_close: bool,
    ctx: egui::Context,
    release: Option<Release>,
    release_error: Option<String>,
    checking_app: bool,
    checked_app: bool,
    download: Option<Arc<Progress>>,
    installed: Option<PathBuf>,
    install_error: Option<String>,
}

impl App {
    fn new(ctx: &egui::Context) -> App {
        style(ctx);
        if let Ok(exe) = std::env::current_exe() {
            std::thread::spawn(move || selfupdate::cleanup(&exe));
        }
        let prefs = prefs::load_prefs();
        let (set, notes) = features::choose(prefs::user_features_file().as_deref(), prefs::feature_cache_file().as_deref());
        let gv = prefs
            .get("game_version")
            .and_then(Value::as_str)
            .filter(|v| set.game_versions.iter().any(|g| g == v))
            .map(str::to_string)
            .unwrap_or_else(|| set.default_game_version.clone());
        let ws = Workspace::new(Arc::new(set), prefs::string_map(&prefs, "priority"), prefs::string_map(&prefs, "overrides"), &gv);
        let (tx, rx) = channel();
        let area = ws.set.areas[0].clone();
        let mut app = App {
            ws, prefs, path_text: String::new(), tab: Tab::Overview, status: String::new(), activity: Vec::new(),
            shared: ListView { group: ALL.into(), ..Default::default() },
            unique: ListView { group: ALL.into(), ..Default::default() }, area, edit: None, dialog: None,
            actions: Vec::new(), tx, rx, checking: false, fetching_features: false, pending_set: None, generation: 0,
            counts: Default::default(), allow_close: false, ctx: ctx.clone(), release: None, release_error: None,
            checking_app: false, checked_app: false, download: None, installed: None, install_error: None,
        };
        for n in notes {
            app.log(&n);
        }
        app.log(&format!("Using {}.", app.ws.set.describe()));
        let hint = app.prefs.get("config_dir").and_then(Value::as_str).unwrap_or("").to_string();
        match prefs::detect_config_dir(&[hint], &app.ws.set) {
            Some(folder) => app.load_now(&folder.to_string_lossy()),
            None => app.say("Choose your Minecraft config folder to begin."),
        }
        app.check_features(false);
        app
    }

    fn say(&mut self, msg: &str) {
        self.status = msg.to_string();
    }

    fn log(&mut self, msg: &str) {
        self.activity.push(format!("{}  {msg}", chrono::Local::now().format("%H:%M:%S")));
    }

    fn note(&mut self, msg: &str) {
        self.say(msg);
        self.log(msg);
    }

    fn changed(&mut self) {
        self.generation += 1;
        self.counts = self.ws.conflicts();
    }

    fn auto_features(&self) -> bool {
        self.prefs.get("auto_update_features").and_then(Value::as_bool).unwrap_or(true)
    }

    fn remember(&mut self) {
        self.prefs.insert("priority".into(), serde_json::to_value(&self.ws.priority).unwrap_or_default());
        self.prefs.insert("overrides".into(), serde_json::to_value(&self.ws.overrides).unwrap_or_default());
        prefs::save_prefs(&self.prefs);
    }

    fn load(&mut self, folder: &str) {
        let folder = folder.trim().trim_matches('"').to_string();
        if folder.is_empty() {
            return self.say("Choose your Minecraft config folder to begin.");
        }
        if !self.ws.changed.is_empty() {
            self.dialog = Some(Dialog::Confirm {
                title: "Unsaved changes".into(),
                text: "Reloading discards your unsaved changes. Continue?".into(),
                then: Then::Load(folder),
            });
            return;
        }
        self.load_now(&folder);
    }

    fn load_now(&mut self, folder: &str) {
        let t0 = Instant::now();
        if let Some(set) = self.pending_set.take() {
            self.ws.set_feature_set(set);
            self.changed();
            self.log(&format!("Now using {}.", self.ws.set.describe()));
        }
        if let Err(e) = self.ws.load(&PathBuf::from(folder)) {
            self.dialog = Some(Dialog::Error { title: "Can't open folder".into(), text: e });
            return;
        }
        self.edit = None;
        let folder = self.ws.folder.clone().unwrap_or_default();
        self.path_text = folder.to_string_lossy().into_owned();
        self.prefs.insert("config_dir".into(), Value::String(self.path_text.clone()));
        prefs::save_prefs(&self.prefs);
        self.ws.attach_installed();
        let mut lines = Vec::new();
        for m in self.ws.mods.values() {
            for err in &m.errors {
                lines.push(format!("{}: could not read {err}", m.name));
            }
        }
        let auto: Vec<String> = self.ws.mods.values().filter(|m| m.loaded() && m.auto).map(|m| m.name.clone()).collect();
        if !auto.is_empty() {
            lines.push(format!("Found {} on its own: {}. Their settings are on the Unique tab; they aren't compared with other mods.",
                               plural(auto.len(), "more Hypixel SkyBlock mod"), auto.join(", ")));
        }
        let locked = self.ws.settings.iter().filter(|s| s.locked).count();
        if locked > 0 {
            lines.push(format!("Locked {}: they're shown in gray and can't be edited.", plural(locked, "debug setting")));
        }
        for l in lines {
            self.log(&l);
        }
        for view in [&mut self.shared, &mut self.unique] {
            view.selected = None;
            view.built = None;
        }
        if !self.ws.choices.contains_key(&self.area) {
            if let Some(first) = self.ws.choices.keys().next() {
                self.area = first.clone();
            }
        }
        self.changed();
        let found = self.ws.mods.values().filter(|m| m.loaded()).count();
        self.note(&format!("Loaded {} settings from {} in {:.2}s.", thousands(self.ws.settings.len()),
                           plural(found, "mod"), t0.elapsed().as_secs_f64()));
    }

    fn apply(&mut self, uid: usize, value: Value) {
        match self.ws.set_value(uid, value) {
            Ok(()) => {
                let s = &self.ws.settings[uid];
                let msg = format!("{} › {} set to {}. Not saved yet.", s.mod_name, s.label, fmt(&s.value, 80));
                self.changed();
                self.say(&msg);
            }
            Err(e) => self.say(&e),
        }
    }

    fn edit_setting(&mut self, uid: usize) {
        let s = &self.ws.settings[uid];
        if s.locked {
            let msg = format!("{} › {} is a debug setting, so it can't be edited.", s.mod_name, s.label);
            return self.say(&msg);
        }
        if let Value::Bool(b) = s.value {
            return self.apply(uid, Value::Bool(!b));
        }
        self.edit = Some(EditState { uid, text: edit_text(&s.value), error: String::new(), focus: true });
    }

    fn resolve(&mut self, area: Option<String>) {
        let n = self.ws.resolve(area.as_deref(), None);
        if n == 0 {
            return self.say("No conflicts to resolve.");
        }
        self.changed();
        let at = area.map(|a| format!(" in {a}")).unwrap_or_default();
        self.note(&format!("Applied {}{at}. Save to keep them.", plural(n, "recommended value")));
    }

    fn save(&mut self) {
        let files = self.ws.dirty_files();
        if files.is_empty() {
            return self.say("Nothing to save.");
        }
        let folder = self.ws.folder.clone().unwrap_or_default();
        let risky: Vec<String> = files
            .iter()
            .map(|&f| &self.ws.files[f])
            .filter(|cf| cf.comments && !cf.warned)
            .map(|cf| cf.path.strip_prefix(&folder).unwrap_or(&cf.path).to_string_lossy().replace('\\', "/"))
            .collect();
        if !risky.is_empty() {
            self.dialog = Some(Dialog::Confirm {
                title: "Comments will be removed".into(),
                text: format!("These files contain comments, which can't be kept when saving:\n\n  {}\n\nA backup of each original is kept. Save anyway?",
                              risky.join("\n  ")),
                then: Then::Save,
            });
            return;
        }
        self.save_now();
    }

    fn save_now(&mut self) {
        for f in self.ws.dirty_files() {
            self.ws.files[f].warned = true;
        }
        let (saved, errors) = self.ws.save();
        self.changed();
        if !saved.is_empty() {
            self.note(&format!("Saved {} ({}). Originals are kept as .bak_* files.", plural(saved.len(), "file"), saved.join(", ")));
        }
        if !errors.is_empty() {
            for e in &errors {
                self.log(&format!("Could not save {e}"));
            }
            self.dialog = Some(Dialog::Error { title: "Some files weren't saved".into(), text: errors.join("\n") });
        }
    }

    fn set_game_version(&mut self, version: String) {
        if version == self.ws.game_version {
            return;
        }
        self.prefs.insert("game_version".into(), Value::String(version.clone()));
        prefs::save_prefs(&self.prefs);
        for m in self.ws.mods.values_mut() {
            m.latest = None;
        }
        self.ws.set_game_version(&version);
        if !self.ws.choices.contains_key(&self.area) {
            if let Some(first) = self.ws.choices.keys().next() {
                self.area = first.clone();
            }
        }
        self.changed();
        self.note(&format!("Minecraft {version} selected: setting names and Check for mod updates now follow it."));
    }

    fn check_updates(&mut self) {
        let jars: IndexMap<String, (String, PathBuf)> = self
            .ws
            .mods
            .values()
            .filter_map(|m| Some((m.name.clone(), (m.installed.clone()?, m.jar.clone()?))))
            .collect();
        self.check_features(true);
        if jars.is_empty() {
            return self.say("No mod jars found next to the config folder (../mods).");
        }
        let version = self.ws.game_version.clone();
        let set = Arc::clone(&self.ws.set);
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        self.checking = true;
        self.say(&format!("Checking Modrinth and CurseForge for Fabric {version} builds…"));
        std::thread::spawn(move || {
            let result = updates::update_status(&jars, &version, &set);
            let _ = tx.send(Msg::Updates { version, result });
            ctx.request_repaint();
        });
    }

    fn check_features(&mut self, manual: bool) {
        if !self.auto_features() || self.fetching_features || matches!(self.ws.set.source, Source::UserFile(_)) {
            return;
        }
        let last = self.prefs.get("features_checked").and_then(Value::as_u64).unwrap_or(0);
        if !manual && now_secs().saturating_sub(last) < FEATURES_EVERY {
            return;
        }
        self.fetching_features = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Features { manual, result: updates::fetch_features() });
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Updates { version, result } => self.got_updates(version, result),
                Msg::Features { manual, result } => self.got_features(manual, result),
                Msg::Release(result) => self.got_release(result),
                Msg::Installed(result) => self.got_installed(result),
            }
        }
    }

    fn got_updates(&mut self, version: String, result: Result<IndexMap<String, (String, State)>, String>) {
        self.checking = false;
        if version != self.ws.game_version {
            let msg = format!("That check was for Minecraft {version}. Click Check for mod updates again for {}.", self.ws.game_version);
            return self.say(&msg);
        }
        let results = match result {
            Ok(r) => r,
            Err(e) => {
                self.log(&format!("Update check failed: {e}"));
                return self.say("Update check failed; see Activity.");
            }
        };
        for (name, r) in &results {
            if let Some(m) = self.ws.mods.get_mut(name) {
                m.latest = Some(r.clone());
            }
        }
        let new: Vec<&str> = results.iter().filter(|(_, (_, s))| *s == State::New).map(|(n, _)| n.as_str()).collect();
        let failed: Vec<&str> = results.iter().filter(|(_, (_, s))| *s == State::Failed).map(|(n, _)| n.as_str()).collect();
        let mut parts = Vec::new();
        if !new.is_empty() {
            parts.push(format!("Updates available for {}.", new.join(", ")));
        }
        if !failed.is_empty() {
            parts.push(format!("Couldn't check {}; try again later.", failed.join(", ")));
        }
        let msg = if parts.is_empty() { format!("Everything installed is up to date for Fabric {version}.") } else { parts.join(" ") };
        self.note(&msg);
    }

    fn got_features(&mut self, manual: bool, result: Result<(String, FeatureSet), String>) {
        self.fetching_features = false;
        self.prefs.insert("features_checked".into(), Value::from(now_secs()));
        prefs::save_prefs(&self.prefs);
        let (text, set) = match result {
            Ok(x) => x,
            Err(e) => return self.log(&format!("Couldn't check for a newer mod list: {e}")),
        };
        let current = self.pending_set.as_ref().map_or(self.ws.set.revision, |p| p.revision);
        if set.revision <= current {
            if manual {
                self.log(&format!("The mod list is up to date (revision {current})."));
            }
            return;
        }
        let saved_at = prefs::save_feature_cache(&text).unwrap_or_default();
        let set = Arc::new(FeatureSet { source: Source::Downloaded(saved_at), ..set });
        let revision = set.revision;
        if self.ws.changed.is_empty() {
            let folder = self.ws.folder.clone();
            self.pending_set = Some(set);
            match folder {
                Some(f) => self.load_now(&f.to_string_lossy()),
                None => {
                    let set = self.pending_set.take().expect("just set");
                    self.ws.set_feature_set(set);
                }
            }
            if !self.ws.set.game_versions.contains(&self.ws.game_version) {
                let v = self.ws.set.default_game_version.clone();
                self.set_game_version(v);
            }
            self.note(&format!("Updated the mod list to revision {revision}."));
        } else {
            self.pending_set = Some(set);
            self.log(&format!("Downloaded a newer mod list (revision {revision}). It's used the next time you load or reload the folder."));
        }
    }

    fn check_app(&mut self) {
        if self.checking_app {
            return;
        }
        self.checking_app = true;
        self.checked_app = true;
        self.release_error = None;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Release(selfupdate::latest_release()));
            ctx.request_repaint();
        });
    }

    fn got_release(&mut self, result: Result<Release, String>) {
        self.checking_app = false;
        match result {
            Ok(r) => {
                let msg = if r.is_newer_than(scm::VERSION) {
                    format!("Version {} of the app is available on the App updates tab.", r.version())
                } else {
                    format!("You have the latest version of the app ({}).", scm::VERSION)
                };
                self.say(&msg);
                self.release = Some(r);
            }
            Err(e) => {
                self.log(&format!("Couldn't check for app updates: {e}"));
                self.release_error = Some(e);
                self.say("Couldn't check for app updates; see the App updates tab.");
            }
        }
    }

    fn install_app(&mut self) {
        let Some(release) = self.release.clone() else { return };
        let Some(asset) = release.asset(selfupdate::asset_name()).cloned() else { return };
        let Ok(exe) = std::env::current_exe() else {
            self.install_error = Some("couldn't find where the app is".into());
            return;
        };
        let progress = Arc::new(Progress::default());
        self.download = Some(Arc::clone(&progress));
        self.install_error = None;
        let tmp = selfupdate::download_path(&exe);
        let version = release.version().to_string();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let result = selfupdate::download(&asset, &tmp, &progress).and_then(|()| selfupdate::install(&tmp, &exe, &version));
            if result.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            let _ = tx.send(Msg::Installed(result));
            ctx.request_repaint();
        });
        self.say(&format!("Downloading version {}…", release.version()));
    }

    fn got_installed(&mut self, result: Result<PathBuf, String>) {
        self.download = None;
        match result {
            Ok(target) => {
                let v = self.release.as_ref().map(|r| r.version().to_string()).unwrap_or_default();
                self.installed = Some(target);
                self.note(&format!("Updated to version {v}. Restart the app to use it."));
            }
            Err(e) => {
                self.log(&format!("Couldn't update the app: {e}"));
                self.install_error = Some(e);
                self.say("Couldn't update the app; see the App updates tab.");
            }
        }
    }

    fn restart_app(&mut self) {
        let Some(target) = self.installed.clone() else { return };
        if !self.ws.changed.is_empty() {
            self.dialog = Some(Dialog::Confirm {
                title: "Unsaved changes".into(),
                text: format!("Restart and discard {}?", plural(self.ws.changed.len(), "unsaved change")),
                then: Then::Restart(target),
            });
            return;
        }
        self.restart_now(&target);
    }

    fn restart_now(&mut self, target: &Path) {
        match selfupdate::restart(target) {
            Ok(()) => {
                self.allow_close = true;
                self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => {
                self.dialog = Some(Dialog::Error {
                    title: "Couldn't restart".into(),
                    text: format!("{e}. Close the app and open it again to use the new version."),
                });
            }
        }
    }

    fn run_actions(&mut self) {
        for action in std::mem::take(&mut self.actions) {
            match action {
                Action::Load(folder) => self.load(&folder),
                Action::Browse => {
                    let start = if self.path_text.is_empty() { prefs::home().unwrap_or_default() } else { PathBuf::from(&self.path_text) };
                    if let Some(p) = rfd::FileDialog::new().set_title("Select your Minecraft config folder").set_directory(start).pick_folder() {
                        self.load(&p.to_string_lossy());
                    }
                }
                Action::Resolve(area) => self.resolve(area),
                Action::Save => self.save(),
                Action::SetVersion(v) => self.set_game_version(v),
                Action::CheckUpdates => self.check_updates(),
                Action::AutoFeatures(on) => {
                    self.prefs.insert("auto_update_features".into(), Value::Bool(on));
                    prefs::save_prefs(&self.prefs);
                    if on {
                        self.check_features(true);
                    }
                    self.note(if on { "The mod list will be kept up to date." } else { "The mod list won't be updated." });
                }
                Action::SetPriority(area, m) => {
                    self.ws.set_priority(&area, &m);
                    self.remember();
                    self.changed();
                    self.note(&format!("{m} now has priority for {area}."));
                }
                Action::SetOverride(key, m) => {
                    self.ws.set_override(&key, m.as_deref());
                    self.remember();
                    self.changed();
                    let name = self.ws.features.iter().find(|f| f.key == key).map_or(key.clone(), |f| f.name.clone());
                    let what = match m.as_deref() {
                        None => "follows the area choice.".to_string(),
                        Some(OFF) => "off in every mod.".to_string(),
                        Some(m) => format!("uses {m}."),
                    };
                    self.note(&format!("{name}: {what}"));
                }
                Action::Area(a) => self.area = a,
                Action::Select(kind, uid) => self.view(kind).selected = Some(uid),
                Action::Edit(uid) => self.edit_setting(uid),
                Action::CheckApp => self.check_app(),
                Action::InstallApp => self.install_app(),
                Action::RestartApp => self.restart_app(),
                Action::OpenReleasePage => {
                    let page = self.release.as_ref().map_or_else(selfupdate::release_page, |r| r.page.clone());
                    selfupdate::open_in_browser(&page);
                }
                Action::UseRecommended(uid) => {
                    if let Some(rec) = self.ws.settings[uid].rec.clone().filter(|_| self.ws.settings[uid].is_conflict()) {
                        self.apply(uid, rec);
                    }
                }
            }
        }
    }

    fn view(&mut self, kind: Kind) -> &mut ListView {
        match kind {
            Kind::Shared => &mut self.shared,
            Kind::Unique => &mut self.unique,
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Config folder").color(MUTED));
            let buttons_w = 420.0;
            let edit = ui.add(egui::TextEdit::singleline(&mut self.path_text).desired_width((ui.available_width() - buttons_w).max(200.0)));
            if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                self.actions.push(Action::Load(self.path_text.clone()));
            }
            if ui.button("Browse…").clicked() {
                self.actions.push(Action::Browse);
            }
            if ui.button("Reload").clicked() {
                self.actions.push(Action::Load(self.path_text.clone()));
            }
            ui.add_space(16.0);
            if ui.button("Resolve conflicts").clicked() {
                self.actions.push(Action::Resolve(None));
            }
            let save = egui::Button::new(RichText::new("Save changes").color(ON_ACCENT)).fill(ACCENT);
            if ui.add(save).on_hover_text("Ctrl+S").clicked() {
                self.actions.push(Action::Save);
            }
        });
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tab, name) in [(Tab::Overview, "Overview"), (Tab::Priorities, "Priorities"), (Tab::Shared, "Shared"), (Tab::Unique, "Unique"), (Tab::AppUpdates, "App updates")] {
                let selected = self.tab == tab;
                let text = RichText::new(name).size(15.0).color(if selected { ACCENT } else { MUTED });
                if ui.add(egui::Button::new(text).frame(false)).clicked() {
                    self.tab = tab;
                    if tab == Tab::AppUpdates && !self.checked_app {
                        self.actions.push(Action::CheckApp);
                    }
                }
                ui.add_space(10.0);
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.status).color(MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let n = self.ws.changed.len();
                if n > 0 {
                    ui.label(RichText::new(plural(n, "unsaved change")).color(ACCENT));
                }
            });
        });
    }

    fn overview(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Minecraft version").color(MUTED));
            let mut picked = self.ws.game_version.clone();
            egui::ComboBox::from_id_salt("mc-version").selected_text(picked.clone()).width(90.0).show_ui(ui, |ui| {
                for v in &self.ws.set.game_versions {
                    ui.selectable_value(&mut picked, v.clone(), v);
                }
            });
            if picked != self.ws.game_version {
                self.actions.push(Action::SetVersion(picked));
            }
            ui.add_space(12.0);
            let mut auto = self.auto_features();
            let hint = "Downloads the newest list of mods and settings from the project's page, so new mods and renamed settings work without a new release.";
            if ui.checkbox(&mut auto, "Keep the mod list up to date").on_hover_text(hint).changed() {
                self.actions.push(Action::AutoFeatures(auto));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(!self.checking, egui::Button::new("Check for mod updates")).clicked() {
                    self.actions.push(Action::CheckUpdates);
                }
            });
        });
        ui.add_space(6.0);

        let names: Vec<String> = self.ws.set.mods.keys().cloned().chain(self.ws.mods.values().filter(|m| m.auto).map(|m| m.name.clone())).collect();
        let row_h = 26.0;
        let by_mod = &self.counts.0;
        egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).show(ui, |ui| {
            TableBuilder::new(ui)
                .id_salt("overview")
                .striped(true)
                .vscroll(false)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::exact(175.0))
                .column(Column::initial(280.0).at_least(120.0).clip(true))
                .column(Column::exact(80.0))
                .column(Column::exact(70.0))
                .column(Column::exact(80.0))
                .column(Column::exact(150.0).clip(true))
                .column(Column::remainder().at_least(150.0).clip(true))
                .header(row_h, |mut h| {
                    for title in ["Mod", "Config file", "Settings", "Shared", "Conflicts", "Installed"] {
                        h.col(|ui| {
                            ui.label(RichText::new(title).color(MUTED).strong());
                        });
                    }
                    h.col(|ui| {
                        ui.label(RichText::new(format!("Latest for {}", self.ws.game_version)).color(MUTED).strong());
                    });
                })
                .body(|body| {
                    body.rows(row_h, names.len(), |mut row| {
                        let name = &names[row.index()];
                        let m = self.ws.mods.get(name);
                        let loaded = m.is_some_and(|m| m.loaded());
                        let latest = m.and_then(|m| m.latest.clone());
                        let color = if latest.as_ref().is_some_and(|(_, s)| *s == State::New) { NEW } else if loaded { FG } else { DIM };
                        let config = match m {
                            Some(m) if m.loaded() => {
                                let first = &self.ws.files[m.files[0]].path;
                                let folder = self.ws.folder.clone().unwrap_or_default();
                                let rel = first.strip_prefix(&folder).unwrap_or(first).to_string_lossy().replace('\\', "/");
                                if m.files.len() > 1 { format!("{rel}  (+{} more)", m.files.len() - 1) } else { rel }
                            }
                            Some(m) if !m.errors.is_empty() => format!("unreadable: {}", m.errors[0]),
                            _ => "not found".into(),
                        };
                        let auto = m.is_some_and(|m| m.auto);
                        let cells = [
                            if auto && !name.ends_with("(found)") { format!("{name}  (found)") } else { name.clone() },
                            config,
                            m.filter(|m| m.loaded()).map_or("—".into(), |m| thousands(m.settings.len())),
                            m.filter(|m| m.loaded() && !m.auto).map_or("—".into(), |m| m.shared.to_string()),
                            if loaded { by_mod.get(name).copied().unwrap_or(0).to_string() } else { "—".into() },
                            m.and_then(|m| m.installed.clone()).unwrap_or_else(|| "not installed".into()),
                            latest.map_or("—".into(), |(t, _)| t),
                        ];
                        for (i, text) in cells.into_iter().enumerate() {
                            row.col(|ui| {
                                let rt = RichText::new(text).color(color);
                                if (2..5).contains(&i) {
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(rt));
                                } else {
                                    ui.label(rt);
                                }
                            });
                        }
                    });
                });
        });

        ui.add_space(14.0);
        ui.label(RichText::new("Activity").color(MUTED));
        egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(8.0).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for line in &self.activity {
                    ui.label(RichText::new(line).monospace().color(MUTED));
                }
            });
        });
    }

    fn priorities(&mut self, ui: &mut egui::Ui) {
        let (_, by_area, by_feature) = &self.counts;
        egui::Panel::left("areas").resizable(false).exact_size(190.0).frame(egui::Frame::new().fill(PANEL).inner_margin(4.0)).show(ui, |ui| {
            for a in &self.ws.set.areas {
                let live = self.ws.choices.contains_key(a);
                let n = by_area.get(a).copied().unwrap_or(0);
                let selected = *a == self.area;
                let (count, color) = if n > 0 { (n.to_string(), BAD) } else if live { ("✔".to_string(), GOOD) } else { (String::new(), DIM) };
                let resp = ui.horizontal(|ui| {
                    ui.set_min_height(30.0);
                    let name_color = if selected { ACCENT } else if live { FG } else { DIM };
                    ui.add_space(8.0);
                    ui.label(RichText::new(a).color(name_color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new(count).color(color));
                    });
                });
                let rect = resp.response.rect;
                let click = ui.interact(rect, Id::unique(("area", a)), Sense::click());
                if selected {
                    ui.painter().rect_filled(rect, CornerRadius::same(3), SELECT.gamma_multiply(0.6));
                } else if click.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::same(3), HOVER.gamma_multiply(0.3));
                }
                if click.clicked() {
                    self.actions.push(Action::Area(a.clone()));
                }
            }
        });

        egui::CentralPanel::default_margins().frame(egui::Frame::new().inner_margin(egui::Margin { left: 22, ..Default::default() })).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let area = self.area.clone();
                let feats = self.ws.live_features(Some(&area));
                let n = by_area.get(&area).copied().unwrap_or(0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&area).size(20.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add_enabled(n > 0, egui::Button::new("Apply area")).clicked() {
                            self.actions.push(Action::Resolve(Some(area.clone())));
                        }
                        if !feats.is_empty() {
                            ui.label(RichText::new(if n > 0 { plural(n, "conflict") } else { "no conflicts".into() }).color(MUTED));
                        }
                    });
                });
                ui.add_space(10.0);
                if feats.is_empty() {
                    let lead = if self.ws.folder.is_some() { "No overlapping switches found in your configs for this area. " } else { "Load your config folder to see overlaps. " };
                    ui.label(RichText::new(format!("{lead}These features are compared once two of their mods are installed:")).color(MUTED));
                    ui.add_space(6.0);
                    egui::Grid::new("tracked").spacing([24.0, 6.0]).show(ui, |ui| {
                        for f in self.ws.set.features.iter().filter(|f| f.area == area) {
                            ui.label(RichText::new(&f.name).color(DIM));
                            ui.label(RichText::new(f.mods.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>().join(", ")).color(DIM));
                            ui.end_row();
                        }
                    });
                    return;
                }
                let owner = self.ws.owner.get(&area).cloned().unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.label("Prioritized mod");
                    let mut picked = owner.clone();
                    egui::ComboBox::from_id_salt(("owner", &area)).selected_text(&picked).width(160.0).show_ui(ui, |ui| {
                        for m in &self.ws.choices[&area] {
                            ui.selectable_value(&mut picked, m.clone(), m);
                        }
                    });
                    if picked != owner {
                        self.actions.push(Action::SetPriority(area.clone(), picked));
                    }
                });
                ui.label(RichText::new("Its version of every feature below stays on and the same feature in your other mods is turned off. Pick a different mod for any single feature to override it, or turn it off everywhere.").color(MUTED));
                ui.add_space(12.0);
                let follow = format!("{FOLLOW} ({owner})");
                egui::Grid::new(("features", &area)).spacing([18.0, 8.0]).min_col_width(60.0).show(ui, |ui| {
                    for title in ["Feature", "Use", "Turns off", ""] {
                        ui.label(RichText::new(title).color(MUTED).strong());
                    }
                    ui.end_row();
                    for fi in feats {
                        let f = &self.ws.features[fi];
                        ui.label(&f.name);
                        let current = match self.ws.overrides.get(&f.key) {
                            Some(o) if o == OFF => OFF_LABEL.to_string(),
                            Some(o) if f.switches.contains_key(o) => o.clone(),
                            _ => follow.clone(),
                        };
                        let mut picked = current.clone();
                        egui::ComboBox::from_id_salt(("use", &f.key)).selected_text(&picked).width(200.0).show_ui(ui, |ui| {
                            ui.selectable_value(&mut picked, follow.clone(), &follow);
                            for m in f.switches.keys() {
                                ui.selectable_value(&mut picked, m.clone(), m);
                            }
                            ui.selectable_value(&mut picked, OFF_LABEL.to_string(), OFF_LABEL);
                        });
                        if picked != current {
                            let choice = if picked == follow { None } else if picked == OFF_LABEL { Some(OFF.to_string()) } else { Some(picked) };
                            self.actions.push(Action::SetOverride(f.key.clone(), choice));
                        }
                        let losers: Vec<&str> = f.switches.keys().filter(|m| Some(*m) != f.winner.as_ref()).map(String::as_str).collect();
                        ui.label(RichText::new(if losers.is_empty() { "—".into() } else { losers.join(", ") }).color(MUTED));
                        let k = by_feature.get(&f.key).copied().unwrap_or(0);
                        ui.label(RichText::new(if k > 0 { plural(k, "conflict") } else { "OK".into() }).color(if k > 0 { BAD } else { GOOD }));
                        ui.end_row();
                    }
                });
            });
        });
    }

    fn settings_tab(&mut self, ui: &mut egui::Ui, kind: Kind) {
        let generation = self.generation;
        let (source, group_label): (Vec<usize>, &str) = match kind {
            Kind::Shared => (self.ws.shared.clone(), "Area"),
            Kind::Unique => (self.ws.unique.clone(), "Mod"),
        };
        let groups: Vec<String> = match kind {
            Kind::Shared => self.ws.choices.keys().cloned().collect(),
            Kind::Unique => self.ws.mods.values().filter(|m| m.loaded()).map(|m| m.name.clone()).collect(),
        };
        let selected_uid = self.view(kind).selected;
        let selected_conflict = selected_uid.is_some_and(|u| self.ws.settings[u].is_conflict());
        let selected_locked = selected_uid.is_some_and(|u| self.ws.settings[u].locked);

        let view = match kind {
            Kind::Shared => &mut self.shared,
            Kind::Unique => &mut self.unique,
        };
        if view.group != ALL && !groups.contains(&view.group) {
            view.group = ALL.into();
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("Search").color(MUTED));
            ui.add(egui::TextEdit::singleline(&mut view.query).desired_width(220.0));
            ui.add_space(12.0);
            ui.label(RichText::new(group_label).color(MUTED));
            egui::ComboBox::from_id_salt(("group", group_label)).selected_text(&view.group).width(150.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut view.group, ALL.to_string(), ALL);
                for g in &groups {
                    ui.selectable_value(&mut view.group, g.clone(), g);
                }
            });
            ui.add_space(12.0);
            ui.checkbox(&mut view.only_bad, "Conflicts only");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(selected_uid.is_some() && !selected_locked, egui::Button::new("Edit…")).clicked() {
                    if let Some(u) = selected_uid {
                        self.actions.push(Action::Edit(u));
                    }
                }
                if ui.add_enabled(selected_conflict, egui::Button::new("Use recommended")).clicked() {
                    if let Some(u) = selected_uid {
                        self.actions.push(Action::UseRecommended(u));
                    }
                }
            });
        });

        let key = (generation, view.query.clone(), view.group.clone(), view.only_bad);
        if view.built.as_ref() != Some(&key) {
            let terms: Vec<String> = view.query.to_lowercase().split_whitespace().map(str::to_string).collect();
            let ws = &self.ws;
            view.rows = source
                .into_iter()
                .filter(|&u| {
                    let s = &ws.settings[u];
                    let g = if kind == Kind::Shared { &s.domain } else { &s.mod_name };
                    (view.group == ALL || *g == view.group)
                        && (!view.only_bad || s.is_conflict())
                        && terms.iter().all(|t| s.hay.contains(t.as_str()))
                })
                .collect();
            view.built = Some(key);
        }

        let count = plural(view.rows.len(), "setting");
        let rows = view.rows.clone();
        let where_text = selected_uid
            .map(|u| {
                let s = &self.ws.settings[u];
                let mut w = format!("{} › {}", s.mod_name, s.key);
                if s.shared {
                    if let Some(fi) = s.feature {
                        w = format!("{} › {}   ·   {w}", s.domain, self.ws.features[fi].name);
                    }
                }
                if s.locked {
                    w.push_str("   ·   debug setting, can't be edited");
                }
                w
            })
            .unwrap_or_default();
        egui::Panel::bottom(("footer", group_label)).frame(egui::Frame::new()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(count).color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(RichText::new(where_text).color(MUTED)));
            });
        });

        let row_h = 24.0;
        let ws = &self.ws;
        let mut clicked: Option<(usize, bool)> = None;
        egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).show(ui, |ui| {
            TableBuilder::new(ui)
                .id_salt(("settings", group_label))
                .striped(true)
                .sense(Sense::click())
                .auto_shrink([false, false])
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(110.0).at_least(70.0).clip(true))
                .column(Column::initial(340.0).at_least(120.0).clip(true).resizable(true))
                .column(Column::initial(150.0).at_least(70.0).clip(true).resizable(true))
                .column(Column::initial(130.0).at_least(70.0).clip(true).resizable(true))
                .column(Column::remainder().at_least(120.0).clip(true))
                .header(row_h, |mut h| {
                    for title in ["Mod", "Setting", "Current", "Recommended", "Status"] {
                        h.col(|ui| {
                            ui.label(RichText::new(title).color(MUTED).strong());
                        });
                    }
                })
                .body(|body| {
                    body.rows(row_h, rows.len(), |mut row| {
                        let uid = rows[row.index()];
                        let s = &ws.settings[uid];
                        row.set_selected(selected_uid == Some(uid));
                        let bad = s.is_conflict();
                        let color = if s.locked { DIM } else if bad { BAD } else { FG };
                        let cells = [
                            s.mod_name.clone(),
                            s.label.clone(),
                            fmt(&s.value, 80),
                            s.rec.as_ref().map_or("—".into(), |r| fmt(r, 80)),
                            s.status_text(),
                        ];
                        for text in cells {
                            row.col(|ui| {
                                if bad && selected_uid != Some(uid) {
                                    ui.painter().rect_filled(ui.max_rect().expand2(egui::vec2(4.0, 2.0)), 0.0, BAD_BG);
                                }
                                ui.add(egui::Label::new(RichText::new(text).color(color)).truncate().selectable(false));
                            });
                        }
                        let resp = row.response();
                        if resp.double_clicked() {
                            clicked = Some((uid, true));
                        } else if resp.clicked() {
                            clicked = Some((uid, false));
                        }
                    });
                });
        });
        if let Some((uid, double)) = clicked {
            self.actions.push(Action::Select(kind, uid));
            if double {
                self.actions.push(Action::Edit(uid));
            }
        }
        if let Some(u) = selected_uid {
            if !ui.ctx().egui_wants_keyboard_input() && ui.input(|i| i.key_pressed(Key::Enter)) && self.edit.is_none() && self.dialog.is_none() {
                self.actions.push(Action::Edit(u));
            }
        }
    }

    fn edit_dialog(&mut self, ctx: &egui::Context) {
        let Some(edit) = &mut self.edit else { return };
        let s = &self.ws.settings[edit.uid];
        let (mut save, mut cancel) = (false, false);
        let modal = egui::Modal::new(Id::unique("edit-setting")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.label(RichText::new(format!("{} › {}", s.mod_name, s.label)).size(17.0).strong());
            ui.label(RichText::new(format!("{}  ·  {}", s.key, type_name(&s.value))).color(MUTED));
            ui.add_space(8.0);
            let field = ui.add(egui::TextEdit::singleline(&mut edit.text).desired_width(f32::INFINITY));
            if edit.focus {
                field.request_focus();
                edit.focus = false;
            }
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                save = true;
            }
            if !edit.error.is_empty() {
                ui.label(RichText::new(&edit.error).color(BAD));
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Button::new(RichText::new("Save").color(ON_ACCENT)).fill(ACCENT)).clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
        if modal.should_close() || ctx.input(|i| i.key_pressed(Key::Escape)) {
            cancel = true;
        }
        if save {
            match parse_value(&edit.text, &s.value) {
                Ok(v) => {
                    let (uid, unchanged) = (edit.uid, scm::value::same(&v, &s.value));
                    self.edit = None;
                    if !unchanged {
                        self.apply(uid, v);
                    }
                }
                Err(e) => {
                    edit.error = e;
                    edit.focus = true;
                }
            }
        } else if cancel {
            self.edit = None;
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &self.dialog else { return };
        let (title, text, confirm) = match dialog {
            Dialog::Confirm { title, text, .. } => (title.clone(), text.clone(), true),
            Dialog::Error { title, text } => (title.clone(), text.clone(), false),
        };
        let (mut yes, mut close) = (false, false);
        let modal = egui::Modal::new(Id::unique("dialog")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.label(RichText::new(&title).size(17.0).strong().color(if confirm { FG } else { BAD }));
            ui.add_space(6.0);
            ui.label(&text);
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if confirm {
                    if ui.add(egui::Button::new(RichText::new("Yes").color(ON_ACCENT)).fill(ACCENT)).clicked() {
                        yes = true;
                    }
                    if ui.button("No").clicked() {
                        close = true;
                    }
                } else if ui.button("OK").clicked() {
                    close = true;
                }
            });
        });
        if modal.should_close() {
            close = true;
        }
        if yes {
            if let Some(Dialog::Confirm { then, .. }) = self.dialog.take() {
                match then {
                    Then::Load(folder) => self.load_now(&folder),
                    Then::Save => self.save_now(),
                    Then::Restart(target) => self.restart_now(&target),
                    Then::Quit => {
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        } else if close {
            self.dialog = None;
        }
    }

    fn app_updates(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("App updates").size(20.0).strong());
        ui.add_space(6.0);
        ui.label(format!("You have version {}.", scm::VERSION));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!self.checking_app && self.download.is_none(), egui::Button::new("Check for app updates")).clicked() {
                self.actions.push(Action::CheckApp);
            }
            if self.checking_app {
                ui.spinner();
                ui.label(RichText::new("Checking GitHub…").color(MUTED));
            }
        });
        if let Some(e) = &self.release_error {
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Couldn't check for app updates: {e}")).color(BAD));
        }
        let Some(release) = self.release.clone() else { return };
        let newer = release.is_newer_than(scm::VERSION);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if self.installed.is_some() {
                ui.label(RichText::new(format!("Updated to version {}.", release.version())).color(GOOD).strong());
            } else if newer {
                ui.label(RichText::new(format!("Version {} is available.", release.version())).color(NEW).strong());
            } else {
                ui.label(RichText::new("You have the latest version.").color(GOOD).strong());
            }
            if !release.published.is_empty() {
                ui.label(RichText::new(format!("Released {}", release.published)).color(MUTED));
            }
        });
        ui.add_space(8.0);
        if newer {
            let asset = release.asset(selfupdate::asset_name()).cloned();
            if self.installed.is_some() {
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new(RichText::new("Restart now").color(ON_ACCENT)).fill(ACCENT)).clicked() {
                        self.actions.push(Action::RestartApp);
                    }
                    ui.label(RichText::new("Restart the app to use the new version.").color(MUTED));
                });
            } else if let Some(progress) = &self.download {
                let mb = |b: u64| b as f64 / 1_048_576.0;
                let text = format!("Downloading… {:.1} of {:.1} MB", mb(progress.done.load(Ordering::Relaxed)), mb(progress.total.load(Ordering::Relaxed)));
                ui.add(egui::ProgressBar::new(progress.fraction()).desired_width(380.0).text(text));
            } else if let Some(asset) = asset {
                ui.horizontal(|ui| {
                    let button = egui::Button::new(RichText::new(format!("Update to {}", release.version())).color(ON_ACCENT)).fill(ACCENT);
                    if ui.add(button).clicked() {
                        self.actions.push(Action::InstallApp);
                    }
                    ui.label(RichText::new(format!("Downloads {} ({:.1} MB) from GitHub and replaces this copy of the app.",
                                                   asset.name, asset.size as f64 / 1_048_576.0)).color(MUTED));
                });
            } else {
                ui.label(RichText::new("This release doesn't have a download for your system yet.").color(MUTED));
            }
            if let Some(e) = &self.install_error {
                ui.label(RichText::new(format!("Couldn't update the app: {e}")).color(BAD));
            }
        }
        ui.add_space(4.0);
        if ui.link("Open the release page").clicked() {
            self.actions.push(Action::OpenReleasePage);
        }
        ui.add_space(12.0);
        ui.label(RichText::new("Release notes").color(MUTED));
        egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(10.0).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| release_notes(ui, &release.notes));
        });
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && !self.ws.changed.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.dialog.is_none() {
                self.dialog = Some(Dialog::Confirm {
                    title: "Unsaved changes".into(),
                    text: format!("Quit and discard {}?", plural(self.ws.changed.len(), "unsaved change")),
                    then: Then::Quit,
                });
            }
        }
    }
}

fn release_notes(ui: &mut egui::Ui, notes: &str) {
    let lines = selfupdate::note_lines(notes);
    if lines.is_empty() {
        ui.label(RichText::new("No release notes.").color(MUTED));
        return;
    }
    for line in lines {
        let mut job = egui::text::LayoutJob::default();
        for (span, text) in &line.spans {
            let mut piece = RichText::new(text);
            if line.block == Block::Heading {
                piece = piece.size(16.0).strong();
            }
            piece = match span {
                Span::Plain => piece,
                Span::Strong => piece.strong(),
                Span::Code => piece.code(),
            };
            piece.append_to(&mut job, ui.style(), egui::FontSelection::Default, Align::Center);
        }
        match line.block {
            Block::Gap => ui.add_space(8.0),
            Block::Heading => {
                ui.add_space(2.0);
                ui.add(egui::Label::new(job).wrap());
                ui.add_space(2.0);
            }
            Block::Bullet => {
                ui.horizontal_top(|ui| {
                    ui.label("•");
                    ui.add(egui::Label::new(job).wrap());
                });
            }
            Block::Text => {
                ui.add(egui::Label::new(job).wrap());
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll();
        self.handle_close(&ctx);
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::S)) && self.edit.is_none() && self.dialog.is_none() {
            self.actions.push(Action::Save);
        }
        let frame = egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 4 });
        egui::Panel::top("top").frame(frame).show(ui, |ui| self.top_bar(ui));
        egui::Panel::top("tabs").frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 12, right: 16, top: 2, bottom: 6 })).show(ui, |ui| self.tab_bar(ui));
        egui::Panel::bottom("status").frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 16, right: 16, top: 6, bottom: 8 })).show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default_margins().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin { left: 16, right: 16, top: 6, bottom: 4 })).show(ui, |ui| {
            match self.tab {
                Tab::Overview => self.overview(ui),
                Tab::Priorities => self.priorities(ui),
                Tab::Shared => self.settings_tab(ui, Kind::Shared),
                Tab::Unique => self.settings_tab(ui, Kind::Unique),
                Tab::AppUpdates => self.app_updates(ui),
            }
        });
        if self.download.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.edit_dialog(&ctx);
        self.dialogs(&ctx);
        self.run_actions();
    }
}
