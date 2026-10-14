use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_autostart::ManagerExt;

use crate::ipc::AppState;
use crate::popover;
use crate::sampler::SharedState;
use crate::tray::{self, TRAY_ID};

pub const SETTINGS_FILE: &str = "settings.json";
pub const MAX_SETTINGS_BYTES: u64 = 64 * 1024;

const TEMP_FILE: &str = ".settings.json.tmp";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TitleFormatSetting {
    #[default]
    Default,
    Compact,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub title_format: TitleFormatSetting,
    pub show_tray_icon: bool,
    pub theme: ThemePreference,
    pub reduce_effects: bool,
    pub show_network_disks: bool,
    pub memory_saver: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            title_format: TitleFormatSetting::Default,
            show_tray_icon: true,
            theme: ThemePreference::System,
            reduce_effects: false,
            show_network_disks: false,
            memory_saver: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPayload {
    #[serde(flatten)]
    pub settings: Settings,
    pub launch_at_login: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub settings: Settings,
    pub repaired: bool,
}

fn field<T: DeserializeOwned>(
    object: &Map<String, Value>,
    key: &str,
    repaired: &mut bool,
) -> Option<T> {
    let value = object.get(key)?;
    match T::deserialize(value) {
        Ok(parsed) => Some(parsed),
        Err(_) => {
            *repaired = true;
            None
        }
    }
}

pub fn parse_settings(text: &str) -> Parsed {
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(text) else {
        return Parsed {
            settings: Settings::default(),
            repaired: true,
        };
    };
    let defaults = Settings::default();
    let mut repaired = false;
    let settings = Settings {
        title_format: field(&object, "titleFormat", &mut repaired).unwrap_or(defaults.title_format),
        show_tray_icon: field(&object, "showTrayIcon", &mut repaired)
            .unwrap_or(defaults.show_tray_icon),
        theme: field(&object, "theme", &mut repaired).unwrap_or(defaults.theme),
        reduce_effects: field(&object, "reduceEffects", &mut repaired)
            .unwrap_or(defaults.reduce_effects),
        show_network_disks: field(&object, "showNetworkDisks", &mut repaired)
            .unwrap_or(defaults.show_network_disks),
        memory_saver: field(&object, "memorySaver", &mut repaired).unwrap_or(defaults.memory_saver),
    };
    Parsed { settings, repaired }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings directory is unknown")]
    NoDirectory,
    #[error("settings file is larger than {MAX_SETTINGS_BYTES} bytes")]
    TooLarge,
    #[error("settings could not be encoded: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("settings I/O failed: {0}")]
    Io(#[from] io::Error),
}

fn read_limited(path: &Path) -> Result<Option<String>, SettingsError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut text = String::new();
    file.take(MAX_SETTINGS_BYTES + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(SettingsError::TooLarge);
    }
    Ok(Some(text))
}

pub fn load_from(dir: &Path) -> Parsed {
    match read_limited(&dir.join(SETTINGS_FILE)) {
        Ok(Some(text)) => {
            let parsed = parse_settings(&text);
            if parsed.repaired {
                log::warn!("settings file had invalid values; using defaults for those fields");
            }
            parsed
        }
        Ok(None) => Parsed {
            settings: Settings::default(),
            repaired: false,
        },
        Err(err) => {
            log::warn!("could not read settings, using defaults: {err}");
            Parsed {
                settings: Settings::default(),
                repaired: true,
            }
        }
    }
}

pub fn save_to(dir: &Path, settings: &Settings) -> Result<(), SettingsError> {
    fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec_pretty(settings)?;
    let temp = dir.join(TEMP_FILE);
    let result = (|| -> io::Result<()> {
        let mut file = File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, dir.join(SETTINGS_FILE))
    })();
    if result.is_err() {
        fs::remove_file(&temp).ok();
    }
    result.map_err(SettingsError::from)
}

#[derive(Debug)]
pub struct SettingsStore {
    dir: Option<PathBuf>,
    current: Mutex<Settings>,
}

fn lock(mutex: &Mutex<Settings>) -> MutexGuard<'_, Settings> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SettingsStore {
    pub fn load(dir: Option<PathBuf>) -> Self {
        let settings = match &dir {
            Some(dir) => load_from(dir).settings,
            None => {
                log::warn!("app config directory unavailable; settings will not persist");
                Settings::default()
            }
        };
        SettingsStore {
            dir,
            current: Mutex::new(settings),
        }
    }

    pub fn get(&self) -> Settings {
        lock(&self.current).clone()
    }

    pub fn save(&self, settings: Settings) -> Result<(), SettingsError> {
        let dir = self.dir.as_deref().ok_or(SettingsError::NoDirectory)?;
        let mut current = lock(&self.current);
        save_to(dir, &settings)?;
        *current = settings;
        Ok(())
    }
}

pub fn apply_shared(shared: &SharedState, settings: &Settings) {
    shared.compact_title.store(
        settings.title_format == TitleFormatSetting::Compact,
        Ordering::Relaxed,
    );
    shared
        .include_network_disks
        .store(settings.show_network_disks, Ordering::Relaxed);
}

pub fn apply<R: Runtime>(app: &AppHandle<R>, previous: &Settings, next: &Settings) {
    apply_shared(&app.state::<AppState>().0, next);
    if previous.show_tray_icon != next.show_tray_icon
        && let Some(tray) = app.tray_by_id(TRAY_ID)
        && let Err(err) = tray::set_icon_visible(&tray, next.show_tray_icon)
    {
        log::warn!("could not update tray icon visibility: {err}");
    }
    if previous.theme != next.theme || previous.reduce_effects != next.reduce_effects {
        popover::apply_appearance(app, next);
    }
    if previous.memory_saver != next.memory_saver {
        popover::memory_saver_changed(app, next.memory_saver);
    }
}

pub fn launch_at_login<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.autolaunch().is_enabled().unwrap_or_else(|err| {
        log::warn!("could not read launch-at-login state: {err}");
        false
    })
}

pub fn set_launch_at_login<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> bool {
    if launch_at_login(app) != enabled {
        let manager = app.autolaunch();
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        if let Err(err) = result {
            log::warn!("could not change launch-at-login: {err}");
        }
    }
    let actual = launch_at_login(app);
    tray::sync_launch_at_login(app, actual);
    actual
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    const FIXTURE: &str = include_str!("../../src/mocks/contract-settings.json");

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "pulsebar-settings-{}-{name}-{unique}",
                std::process::id()
            ));
            fs::remove_dir_all(&path).ok();
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn fixture(key: &str) -> Value {
        let all: Value = serde_json::from_str(FIXTURE).expect("fixture is valid JSON");
        all[key].clone()
    }

    fn custom() -> Settings {
        Settings {
            title_format: TitleFormatSetting::Compact,
            show_tray_icon: false,
            theme: ThemePreference::Dark,
            reduce_effects: true,
            show_network_disks: true,
            memory_saver: true,
        }
    }

    #[test]
    fn payload_matches_contract_fixture() {
        let payload = SettingsPayload {
            settings: custom(),
            launch_at_login: true,
        };
        assert_eq!(serde_json::to_value(&payload).unwrap(), fixture("settings"));
        let defaults = SettingsPayload {
            settings: Settings::default(),
            launch_at_login: false,
        };
        assert_eq!(
            serde_json::to_value(&defaults).unwrap(),
            fixture("defaultSettings")
        );
    }

    #[test]
    fn payload_round_trips_from_frontend_json() {
        let parsed: SettingsPayload = serde_json::from_value(fixture("settings")).unwrap();
        assert_eq!(parsed.settings, custom());
        assert!(parsed.launch_at_login);
    }

    #[test]
    fn payload_rejects_unknown_enum_values() {
        let mut value = fixture("settings");
        value["theme"] = Value::from("sepia");
        assert!(serde_json::from_value::<SettingsPayload>(value).is_err());
    }

    #[test]
    fn defaults_keep_tray_icon_and_disable_everything_else() {
        let defaults = Settings::default();
        assert!(defaults.show_tray_icon);
        assert!(!defaults.reduce_effects && !defaults.show_network_disks && !defaults.memory_saver);
        assert_eq!(defaults.theme, ThemePreference::System);
        assert_eq!(defaults.title_format, TitleFormatSetting::Default);
    }

    #[test]
    fn valid_file_parses_without_repair() {
        let text = serde_json::to_string(&custom()).unwrap();
        assert_eq!(
            parse_settings(&text),
            Parsed {
                settings: custom(),
                repaired: false
            }
        );
    }

    #[test]
    fn missing_fields_fall_back_to_defaults_silently() {
        let parsed = parse_settings(r#"{"theme":"light"}"#);
        assert_eq!(parsed.settings.theme, ThemePreference::Light);
        assert!(parsed.settings.show_tray_icon);
        assert!(!parsed.repaired);
    }

    #[test]
    fn invalid_values_are_replaced_field_by_field() {
        let parsed = parse_settings(
            r#"{"titleFormat":"huge","showTrayIcon":"yes","theme":"dark","memorySaver":1,"extra":true}"#,
        );
        assert!(parsed.repaired);
        assert_eq!(parsed.settings.title_format, TitleFormatSetting::Default);
        assert!(parsed.settings.show_tray_icon);
        assert_eq!(parsed.settings.theme, ThemePreference::Dark);
        assert!(!parsed.settings.memory_saver);
    }

    #[test]
    fn non_object_or_corrupt_input_yields_defaults() {
        for text in ["", "null", "[1,2]", "\"x\"", "{not json", "42"] {
            let parsed = parse_settings(text);
            assert_eq!(parsed.settings, Settings::default(), "input {text:?}");
            assert!(parsed.repaired, "input {text:?}");
        }
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_no_temp_file() {
        let dir = TempDir::new("roundtrip");
        save_to(&dir.0, &custom()).unwrap();
        assert!(!dir.0.join(TEMP_FILE).exists());
        assert_eq!(load_from(&dir.0).settings, custom());
        save_to(&dir.0, &Settings::default()).unwrap();
        assert_eq!(load_from(&dir.0).settings, Settings::default());
    }

    #[test]
    fn missing_file_loads_defaults_without_repair() {
        let dir = TempDir::new("missing");
        assert_eq!(
            load_from(&dir.0),
            Parsed {
                settings: Settings::default(),
                repaired: false
            }
        );
    }

    #[test]
    fn oversized_file_is_ignored() {
        let dir = TempDir::new("oversized");
        fs::create_dir_all(&dir.0).unwrap();
        let padding = " ".repeat(usize::try_from(MAX_SETTINGS_BYTES).unwrap());
        fs::write(
            dir.0.join(SETTINGS_FILE),
            format!("{{\"theme\":\"dark\"}}{padding}"),
        )
        .unwrap();
        let parsed = load_from(&dir.0);
        assert_eq!(parsed.settings, Settings::default());
        assert!(parsed.repaired);
    }

    #[test]
    fn shared_flags_follow_title_format_and_network_disks() {
        let shared = SharedState::default();
        apply_shared(&shared, &custom());
        assert!(shared.compact_title.load(Ordering::Relaxed));
        assert!(shared.include_network_disks.load(Ordering::Relaxed));
        apply_shared(&shared, &Settings::default());
        assert!(!shared.compact_title.load(Ordering::Relaxed));
        assert!(!shared.include_network_disks.load(Ordering::Relaxed));
    }

    #[test]
    fn store_persists_and_reports_missing_directory() {
        let dir = TempDir::new("store");
        let store = SettingsStore::load(Some(dir.0.clone()));
        assert_eq!(store.get(), Settings::default());
        store.save(custom()).unwrap();
        assert_eq!(store.get(), custom());
        assert_eq!(SettingsStore::load(Some(dir.0.clone())).get(), custom());

        let detached = SettingsStore::load(None);
        assert!(matches!(
            detached.save(custom()),
            Err(SettingsError::NoDirectory)
        ));
        assert_eq!(detached.get(), Settings::default());
    }
}
