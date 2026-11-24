use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::Value;

use crate::features::FeatureSet;
use crate::fsutil::{atomic_write, copy_with_times, find_files, read_text, stamp, Stamp};
use crate::jars::{discover_mods, installed_jars};
use crate::jsonc::{detect_indent, parse_jsonc, to_json_text, Indent};
use crate::text::{fmt, is_debug, make_label};
use crate::value::{leaves, node_mut, same, Seg};
use crate::versions::{switch_names, State};
use crate::{MAX_FILE_BYTES, OFF};

pub struct ConfigFile {
    pub path: PathBuf,
    pub raw: Value,
    pub comments: bool,
    pub backup: Option<PathBuf>,
    pub warned: bool,
    pub indent: Indent,
    pub stamp: Option<Stamp>,
}

pub struct Setting {
    pub uid: usize,
    pub mod_name: String,
    pub cf: usize,
    pub path: Vec<Seg>,
    pub key: String,
    pub label: String,
    pub value: Value,
    pub orig: Value,
    pub hay: String,
    pub shared: bool,
    pub domain: String,
    pub feature: Option<usize>,
    pub rec: Option<Value>,
    pub order: (usize, usize, usize, usize),
    pub locked: bool,
}

impl Setting {
    pub fn is_conflict(&self) -> bool {
        self.rec.as_ref().is_some_and(|r| !same(&self.value, r))
    }

    pub fn status_text(&self) -> String {
        if self.locked {
            "locked (debug)".into()
        } else if self.is_conflict() {
            format!("conflict; expect {}", fmt(self.rec.as_ref().unwrap_or(&Value::Null), 80))
        } else {
            "OK".into()
        }
    }
}

pub struct ModInfo {
    pub name: String,
    pub files: Vec<usize>,
    pub errors: Vec<String>,
    pub settings: Vec<usize>,
    pub shared: usize,
    pub installed: Option<String>,
    pub jar: Option<PathBuf>,
    pub latest: Option<(String, State)>,
    pub auto: bool,
}

impl ModInfo {
    fn new(name: &str, auto: bool) -> ModInfo {
        ModInfo { name: name.into(), files: Vec::new(), errors: Vec::new(), settings: Vec::new(), shared: 0,
                  installed: None, jar: None, latest: None, auto }
    }

    pub fn loaded(&self) -> bool {
        !self.files.is_empty()
    }
}

pub struct Feature {
    pub index: usize,
    pub area: String,
    pub name: String,
    pub key: String,
    pub switches: IndexMap<String, Vec<usize>>,
    pub winner: Option<String>,
}

impl Feature {
    pub fn contenders(&self) -> Vec<String> {
        self.switches.keys().cloned().collect()
    }

    pub fn live(&self) -> bool {
        self.switches.len() > 1 && self.switches.values().any(|v| !v.is_empty())
    }
}

pub type Counts = (HashMap<String, usize>, HashMap<String, usize>, HashMap<String, usize>);

pub struct Workspace {
    pub set: Arc<FeatureSet>,
    pub folder: Option<PathBuf>,
    pub game_version: String,
    pub mods: IndexMap<String, ModInfo>,
    pub files: Vec<ConfigFile>,
    pub settings: Vec<Setting>,
    pub index: HashMap<(String, String), usize>,
    pub changed: BTreeSet<usize>,
    pub priority: HashMap<String, String>,
    pub overrides: HashMap<String, String>,
    pub features: Vec<Feature>,
    pub choices: IndexMap<String, Vec<String>>,
    pub owner: HashMap<String, String>,
    pub shared: Vec<usize>,
    pub unique: Vec<usize>,
    ruled: Vec<usize>,
}

fn features_of(set: &FeatureSet) -> Vec<Feature> {
    set.features
        .iter()
        .enumerate()
        .map(|(i, f)| Feature { index: i, area: f.area.clone(), name: f.name.clone(), key: format!("{}/{}", f.area, f.name),
                                switches: IndexMap::new(), winner: None })
        .collect()
}

fn expand_home(folder: &Path) -> PathBuf {
    let s = folder.to_string_lossy();
    if s == "~" || s.starts_with("~/") || s.starts_with("~\\") {
        if let Some(h) = crate::prefs::home() {
            return h.join(s[1..].trim_start_matches(['/', '\\']));
        }
    }
    folder.to_path_buf()
}

impl Workspace {
    pub fn new(set: Arc<FeatureSet>, priority: HashMap<String, String>, overrides: HashMap<String, String>,
               game_version: &str) -> Workspace {
        let features = features_of(&set);
        Workspace { set, folder: None, game_version: game_version.into(), mods: IndexMap::new(), files: Vec::new(),
                    settings: Vec::new(), index: HashMap::new(), changed: BTreeSet::new(), priority, overrides, features,
                    choices: IndexMap::new(), owner: HashMap::new(), shared: Vec::new(), unique: Vec::new(),
                    ruled: Vec::new() }
    }

    pub fn load(&mut self, folder: &Path) -> Result<(), String> {
        let folder = expand_home(folder);
        if !folder.is_dir() {
            return Err(format!("Folder not found: {}", folder.display()));
        }
        let mut b = Building::default();
        let set = Arc::clone(&self.set);
        for (name, spec) in &set.mods {
            let paths = find_files(&folder, spec);
            b.add(&set, name, spec.dir.is_some(), &paths, false);
        }
        let claimed = b.claimed.clone();
        for (name, found) in discover_mods(&folder, &claimed, &set) {
            b.add(&set, &name, false, &found.paths, true);
            if b.mods[&name].loaded() {
                let m = b.mods.get_mut(&name).expect("just added");
                m.installed = Some(found.version.clone());
                m.jar = Some(found.jar.clone());
            } else {
                b.mods.shift_remove(&name);
            }
        }
        self.folder = Some(folder);
        self.mods = b.mods;
        self.files = b.files;
        self.settings = b.settings;
        self.index = b.index;
        self.changed.clear();
        self.ruled.clear();
        self.resolve_features();
        self.recompute();
        Ok(())
    }

    pub fn attach_installed(&mut self) {
        let Some(folder) = &self.folder else { return };
        let mods_dir = folder.parent().map(|p| p.join("mods")).unwrap_or_else(|| PathBuf::from("mods"));
        for (name, (version, jar)) in installed_jars(&mods_dir, &self.set) {
            if let Some(m) = self.mods.get_mut(&name) {
                m.installed = Some(version);
                m.jar = Some(jar);
            }
        }
    }

    pub fn set_feature_set(&mut self, set: Arc<FeatureSet>) {
        self.features = features_of(&set);
        self.set = set;
        if self.folder.is_some() {
            self.resolve_features();
            self.recompute();
        }
    }

    pub fn use_feature_set(&mut self, set: Arc<FeatureSet>) -> Result<(), String> {
        self.set_feature_set(set);
        match self.folder.clone() {
            Some(folder) => self.load(&folder),
            None => Ok(()),
        }
    }

    pub fn set_game_version(&mut self, version: &str) {
        self.game_version = version.into();
        if self.folder.is_some() {
            self.resolve_features();
            self.recompute();
        }
    }

    fn match_key(&self, mod_name: &str, key: &str, by_leaf: &HashMap<String, Vec<usize>>) -> Vec<usize> {
        if key.starts_with('*') {
            let tail = key.trim_start_matches(['*', '.']);
            let dotted = format!(".{tail}");
            let leaf = tail.rsplit('.').next().unwrap_or(tail);
            return by_leaf
                .get(leaf)
                .map(|v| v.iter().copied().filter(|&u| self.settings[u].key == tail || self.settings[u].key.ends_with(&dotted)).collect())
                .unwrap_or_default();
        }
        self.index.get(&(mod_name.to_string(), key.to_string())).map(|&u| vec![u]).unwrap_or_default()
    }

    fn resolve_features(&mut self) {
        let mut leaf_index: HashMap<String, HashMap<String, Vec<usize>>> = HashMap::new();
        let set = Arc::clone(&self.set);
        let mut all = Vec::with_capacity(self.features.len());
        for spec in &set.features {
            let mut switches = IndexMap::new();
            for (mod_name, sw) in &spec.mods {
                if !self.mods.get(mod_name).is_some_and(ModInfo::loaded) {
                    continue;
                }
                if matches!(sw, crate::features::SwitchSpec::NoSwitch) {
                    switches.insert(mod_name.clone(), Vec::new());
                    continue;
                }
                if !leaf_index.contains_key(mod_name) {
                    let mut d: HashMap<String, Vec<usize>> = HashMap::new();
                    for &u in &self.mods[mod_name].settings {
                        let leaf = self.settings[u].key.rsplit('.').next().unwrap_or("").to_string();
                        d.entry(leaf).or_default().push(u);
                    }
                    leaf_index.insert(mod_name.clone(), d);
                }
                let by_leaf = &leaf_index[mod_name];
                for keys in switch_names(sw, &self.game_version) {
                    let (mut found, mut seen) = (Vec::new(), HashSet::new());
                    for key in keys.iter() {
                        for u in self.match_key(mod_name, key, by_leaf) {
                            let s = &self.settings[u];
                            if s.value.is_boolean() && !s.locked && seen.insert(u) {
                                found.push(u);
                            }
                        }
                    }
                    if !found.is_empty() {
                        switches.insert(mod_name.clone(), found);
                        break;
                    }
                }
            }
            all.push(switches);
        }
        for (f, switches) in self.features.iter_mut().zip(all) {
            f.switches = switches;
        }
    }

    pub fn live_features(&self, area: Option<&str>) -> Vec<usize> {
        self.features.iter().filter(|f| f.live() && area.is_none_or(|a| f.area == a)).map(|f| f.index).collect()
    }

    pub fn recompute(&mut self) {
        for &u in &self.ruled {
            let s = &mut self.settings[u];
            (s.rec, s.shared, s.feature, s.order) = (None, false, None, (0, 0, 0, 0));
            s.domain.clear();
        }
        for m in self.mods.values_mut() {
            m.shared = 0;
        }
        let live = self.live_features(None);
        let mut choices: HashMap<String, Vec<String>> = HashMap::new();
        for &fi in &live {
            let f = &self.features[fi];
            let pool = choices.entry(f.area.clone()).or_default();
            for m in f.switches.keys() {
                if !pool.contains(m) {
                    pool.push(m.clone());
                }
            }
        }
        self.choices = self.set.areas.iter().filter_map(|a| choices.remove(a).map(|p| (a.clone(), p))).collect();
        self.owner = self
            .choices
            .iter()
            .map(|(area, pool)| {
                let want = self.priority.get(area).filter(|w| pool.contains(w));
                (area.clone(), want.cloned().unwrap_or_else(|| pool[0].clone()))
            })
            .collect();

        let (mut ruled, mut seen) = (Vec::new(), HashSet::new());
        for f in &mut self.features {
            f.winner = None;
        }
        for fi in live {
            let (present, area, key) = {
                let f = &self.features[fi];
                (f.contenders(), f.area.clone(), f.key.clone())
            };
            let Some(owner) = self.owner.get(&area).cloned() else { continue };
            let winner = match self.overrides.get(&key) {
                Some(ov) if ov == OFF => None,
                Some(ov) if present.contains(ov) => Some(ov.clone()),
                _ => Some(if present.contains(&owner) { owner } else { present[0].clone() }),
            };
            self.features[fi].winner = winner.clone();
            let area_i = self.set.area_index(&area);
            let mut ranked = present.clone();
            ranked.sort_by_key(|m| Some(m) != winner.as_ref());
            for (rank, m) in ranked.iter().enumerate() {
                let uids = self.features[fi].switches[m].clone();
                for (j, u) in uids.into_iter().enumerate() {
                    if !seen.insert(u) {
                        continue;
                    }
                    let s = &mut self.settings[u];
                    s.rec = Some(Value::Bool(Some(m) == winner.as_ref()));
                    s.shared = true;
                    s.domain = area.clone();
                    s.feature = Some(fi);
                    s.order = (area_i, fi, rank, j);
                    if let Some(info) = self.mods.get_mut(m) {
                        info.shared += 1;
                    }
                    ruled.push(u);
                }
            }
        }
        for p in &self.set.preferred {
            if let Some(&u) = self.index.get(&(p.mod_name.clone(), p.key.clone())) {
                let s = &mut self.settings[u];
                if !s.shared && !s.locked && s.value.is_boolean() == p.value.is_boolean() {
                    s.rec = Some(p.value.clone());
                    ruled.push(u);
                }
            }
        }
        let mut shared: Vec<usize> = ruled.iter().copied().filter(|&u| self.settings[u].shared).collect();
        shared.sort_by_key(|&u| self.settings[u].order);
        self.shared = shared;
        self.unique = self.settings.iter().filter(|s| !s.shared).map(|s| s.uid).collect();
        self.ruled = ruled;
    }

    pub fn set_priority(&mut self, area: &str, mod_name: &str) {
        self.priority.insert(area.into(), mod_name.into());
        self.recompute();
    }

    pub fn set_override(&mut self, feature_key: &str, mod_name: Option<&str>) {
        match mod_name {
            None => self.overrides.remove(feature_key),
            Some(m) => self.overrides.insert(feature_key.into(), m.into()),
        };
        self.recompute();
    }

    pub fn conflicts(&self) -> Counts {
        let (mut by_mod, mut by_area, mut by_feature) = (HashMap::new(), HashMap::new(), HashMap::new());
        for &u in &self.ruled {
            let s = &self.settings[u];
            if s.is_conflict() {
                *by_mod.entry(s.mod_name.clone()).or_insert(0) += 1;
                if s.shared {
                    *by_area.entry(s.domain.clone()).or_insert(0) += 1;
                    if let Some(fi) = s.feature {
                        *by_feature.entry(self.features[fi].key.clone()).or_insert(0) += 1;
                    }
                }
            }
        }
        (by_mod, by_area, by_feature)
    }

    pub fn set_value(&mut self, uid: usize, value: Value) -> Result<(), String> {
        let s = &self.settings[uid];
        if s.locked {
            return Err(format!("{} › {} is a debug setting, which can't be edited", s.mod_name, s.key));
        }
        let node = node_mut(&mut self.files[s.cf].raw, &s.path).ok_or("the setting is no longer in its file")?;
        *node = value.clone();
        let s = &mut self.settings[uid];
        s.value = value;
        if same(&s.value, &s.orig) {
            self.changed.remove(&uid);
        } else {
            self.changed.insert(uid);
        }
        Ok(())
    }

    pub fn resolve(&mut self, area: Option<&str>, feature_key: Option<&str>) -> usize {
        let mut n = 0;
        for u in self.ruled.clone() {
            let s = &self.settings[u];
            if !s.is_conflict() {
                continue;
            }
            if area.is_some_and(|a| !(s.shared && s.domain == a)) {
                continue;
            }
            if feature_key.is_some_and(|k| !(s.shared && s.feature.is_some_and(|fi| self.features[fi].key == k))) {
                continue;
            }
            let rec = s.rec.clone().expect("a conflict has a recommendation");
            if self.set_value(u, rec).is_ok() {
                n += 1;
            }
        }
        n
    }

    pub fn dirty_files(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for &u in &self.changed {
            let cf = self.settings[u].cf;
            if !out.contains(&cf) {
                out.push(cf);
            }
        }
        out
    }

    pub fn save(&mut self) -> (Vec<String>, Vec<String>) {
        let when = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let (mut saved, mut errors) = (Vec::new(), Vec::new());
        let mut by_file: HashMap<usize, Vec<usize>> = HashMap::new();
        for &u in &self.changed {
            by_file.entry(self.settings[u].cf).or_default().push(u);
        }
        for cf in self.dirty_files() {
            let file = &mut self.files[cf];
            let name = file.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if let Err(e) = write_one(file, &name, &when) {
                errors.push(format!("{name}: {e}"));
                continue;
            }
            for &u in &by_file[&cf] {
                let s = &mut self.settings[u];
                s.orig = s.value.clone();
                self.changed.remove(&u);
            }
            saved.push(name);
        }
        (saved, errors)
    }
}

fn write_one(file: &mut ConfigFile, name: &str, when: &str) -> Result<(), String> {
    if let Some(st) = file.stamp {
        if stamp(&file.path).map_err(|e| e.to_string())? != st {
            return Err("it was changed on disk after loading (is the game running?). Reload, then make the change again".into());
        }
    }
    if file.backup.is_none() {
        let mut bak = file.path.with_file_name(format!("{name}.bak_{when}"));
        let mut n = 1;
        while bak.exists() {
            bak = file.path.with_file_name(format!("{name}.bak_{when}_{n}"));
            n += 1;
        }
        copy_with_times(&file.path, &bak).map_err(|e| e.to_string())?;
        file.backup = Some(bak);
    }
    let text = to_json_text(&file.raw, &file.indent) + "\n";
    atomic_write(&file.path, &text).map_err(|e| e.to_string())?;
    file.stamp = stamp(&file.path).ok();
    Ok(())
}

#[derive(Default)]
struct Building {
    mods: IndexMap<String, ModInfo>,
    files: Vec<ConfigFile>,
    settings: Vec<Setting>,
    index: HashMap<(String, String), usize>,
    claimed: HashSet<PathBuf>,
}

impl Building {
    fn add(&mut self, set: &FeatureSet, name: &str, has_dir: bool, paths: &[PathBuf], auto: bool) {
        let mut info = ModInfo::new(name, auto);
        for path in paths {
            let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let loaded = (|| -> Result<(Value, bool, Indent, Stamp), String> {
                let st = stamp(path).map_err(|e| e.to_string())?;
                if st.0 > MAX_FILE_BYTES {
                    return Err("file is larger than 25 MB".into());
                }
                let text = read_text(path).map_err(|e| e.to_string())?;
                let (raw, comments) = parse_jsonc(&text)?;
                Ok((raw, comments, detect_indent(&text), st))
            })();
            let (raw, comments, indent, st) = match loaded {
                Ok(x) => x,
                Err(e) => {
                    info.errors.push(format!("{file_name} ({e})"));
                    continue;
                }
            };
            if !(raw.is_object() || raw.is_array()) {
                info.errors.push(format!("{file_name} (not a JSON object)"));
                continue;
            }
            let cf = self.files.len();
            let prefix: Vec<String> = if has_dir || paths.len() > 1 {
                vec![path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()]
            } else {
                Vec::new()
            };
            for (real, shown, value) in leaves(&raw) {
                if real.is_empty() {
                    continue;
                }
                let shown: Vec<String> = prefix.iter().cloned().chain(shown).collect();
                let key = shown.join(".");
                let label = make_label(&set.labels, name, &key, &shown);
                let uid = self.settings.len();
                let hay = format!("{name} {label} {key}").to_lowercase();
                self.settings.push(Setting {
                    uid, mod_name: name.into(), cf, path: real, key: key.clone(), label, value: value.clone(),
                    orig: value.clone(), hay, shared: false, domain: String::new(), feature: None, rec: None,
                    order: (0, 0, 0, 0), locked: is_debug(&shown),
                });
                info.settings.push(uid);
                self.index.entry((name.to_string(), key)).or_insert(uid);
            }
            self.files.push(ConfigFile { path: path.clone(), raw, comments, backup: None, warned: false, indent, stamp: Some(st) });
            info.files.push(cf);
            self.claimed.insert(path.clone());
        }
        self.mods.insert(name.into(), info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_feature_set_never_leaves_settings_pointing_at_old_features() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("skyblocker.json"), r#"{"dungeons": {"dungeonMap": {"enableMap": true}}}"#).unwrap();
        std::fs::create_dir_all(dir.path().join("odin")).unwrap();
        std::fs::write(dir.path().join("odin/odin-config.json"), r#"[{"name": "Dungeon Map", "enabled": true}]"#).unwrap();
        let set = Arc::new(FeatureSet::builtin());
        let mut ws = Workspace::new(Arc::clone(&set), HashMap::new(), HashMap::new(), &set.default_game_version);
        ws.load(dir.path()).unwrap();
        assert!(!ws.shared.is_empty(), "the dungeon map should be shared by Odin and Skyblocker");
        assert!(!ws.conflicts().0.is_empty(), "both maps are on, so one of them should be a conflict");

        let mut smaller = (*set).clone();
        smaller.features.clear();
        ws.set_feature_set(Arc::new(smaller));
        assert!(ws.shared.is_empty());
        assert!(ws.settings.iter().all(|s| s.feature.is_none() && !s.shared));
        assert!(ws.conflicts().2.is_empty());
    }
}
