use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::{AppHandle, Manager, Runtime};

use crate::ipc::AppState;
use crate::login_item::{self, LaunchAtLoginStatus, LaunchState, LoginAction};
use crate::popover;
use crate::sampler::SharedState;
use crate::tray::{self, TitleMetrics};
use crate::tray_image;

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
    pub title_metrics: TitleMetrics,
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
            title_metrics: TitleMetrics::default(),
            theme: ThemePreference::System,
            reduce_effects: false,
            show_network_disks: false,
            memory_saver: false,
        }
    }
}

impl Settings {
    pub fn normalized(mut self) -> Self {
        if self.title_metrics.is_empty() && !self.show_tray_icon {
            self.show_tray_icon = true;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPayload {
    #[serde(flatten)]
    pub settings: Settings,
    pub launch_at_login: bool,
    #[serde(default, skip_deserializing)]
    pub launch_at_login_status: LaunchAtLoginStatus,
    #[serde(default, skip_deserializing)]
    pub launch_at_login_reason: Option<String>,
}

impl SettingsPayload {
    pub fn new(settings: Settings, launch: LaunchState) -> Self {
        SettingsPayload {
            settings,
            launch_at_login: launch.status.is_on(),
            launch_at_login_status: launch.status,
            launch_at_login_reason: launch.reason.map(str::to_owned),
        }
    }
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
        title_metrics: field(&object, "titleMetrics", &mut repaired)
            .unwrap_or(defaults.title_metrics),
        theme: field(&object, "theme", &mut repaired).unwrap_or(defaults.theme),
        reduce_effects: field(&object, "reduceEffects", &mut repaired)
            .unwrap_or(defaults.reduce_effects),
        show_network_disks: field(&object, "showNetworkDisks", &mut repaired)
            .unwrap_or(defaults.show_network_disks),
        memory_saver: field(&object, "memorySaver", &mut repaired).unwrap_or(defaults.memory_saver),
    }
    .normalized();
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
    shared.set_title_metrics(settings.title_metrics);
}

pub fn apply<R: Runtime>(app: &AppHandle<R>, previous: &Settings, next: &Settings) {
    apply_shared(&app.state::<AppState>().0, next);
    if previous.show_tray_icon != next.show_tray_icon {
        tray_image::update(app, |state| state.show_icon = next.show_tray_icon);
    }
    if previous.theme != next.theme || previous.reduce_effects != next.reduce_effects {
        popover::apply_appearance(app, next);
    }
    if previous.memory_saver != next.memory_saver {
        popover::memory_saver_changed(app, next.memory_saver);
    }
}

pub fn launch_at_login() -> LaunchState {
    login_item::current_state()
}

pub fn set_launch_at_login<R: Runtime>(app: &AppHandle<R>, enabled: bool) -> LaunchState {
    let issue = login_item::current_location_issue();
    let status = login_item::status();
    let action = login_item::login_action(enabled, status, issue);
    log::debug!("launch-at-login requested {enabled}: {action:?}");
    let result = match action {
        LoginAction::Noop => Ok(()),
        LoginAction::Register => login_item::register(),
        LoginAction::Unregister => login_item::unregister(),
        LoginAction::Blocked => {
            log::info!(
                "launch at login unavailable here: {}",
                issue.unwrap_or_default()
            );
            Ok(())
        }
    };
    if let Err(err) = result {
        log::warn!("could not change launch-at-login: {err}");
    }
    let status = match action {
        LoginAction::Register | LoginAction::Unregister => login_item::status(),
        LoginAction::Noop | LoginAction::Blocked => status,
    };
    let actual = login_item::launch_state(status, issue);
    tray::sync_launch_at_login(app, actual.status);
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
            title_metrics: TitleMetrics {
                cpu: true,
                power: false,
                memory: true,
                temperature: true,
                network: true,
            },
            theme: ThemePreference::Dark,
            reduce_effects: true,
            show_network_disks: true,
            memory_saver: true,
        }
    }

    fn launch(status: LaunchAtLoginStatus, reason: Option<&'static str>) -> LaunchState {
        LaunchState { status, reason }
    }

    #[test]
    fn payload_matches_contract_fixture() {
        let payload = SettingsPayload::new(custom(), launch(LaunchAtLoginStatus::Enabled, None));
        assert_eq!(serde_json::to_value(&payload).unwrap(), fixture("settings"));
        let defaults = SettingsPayload::new(
            Settings::default(),
            launch(LaunchAtLoginStatus::Disabled, None),
        );
        assert_eq!(
            serde_json::to_value(&defaults).unwrap(),
            fixture("defaultSettings")
        );
    }

    #[test]
    fn launch_at_login_variants_match_contract_fixture() {
        let blocked = SettingsPayload::new(
            Settings::default(),
            launch(
                LaunchAtLoginStatus::Blocked,
                Some(login_item::ISSUE_MOVE_TO_APPLICATIONS),
            ),
        );
        let approval = SettingsPayload::new(
            Settings::default(),
            launch(LaunchAtLoginStatus::RequiresApproval, None),
        );
        for (payload, key) in [
            (blocked, "launchAtLoginBlocked"),
            (approval, "launchAtLoginRequiresApproval"),
        ] {
            let value = serde_json::to_value(&payload).unwrap();
            let expected = fixture(key);
            for field in [
                "launchAtLogin",
                "launchAtLoginStatus",
                "launchAtLoginReason",
            ] {
                assert_eq!(value[field], expected[field], "{key}.{field}");
            }
        }
    }

    #[test]
    fn payload_round_trips_from_frontend_json() {
        let parsed: SettingsPayload = serde_json::from_value(fixture("settings")).unwrap();
        assert_eq!(parsed.settings, custom());
        assert!(parsed.launch_at_login);
    }

    #[test]
    fn output_only_launch_fields_are_ignored_on_input() {
        let mut value = fixture("defaultSettings");
        value["launchAtLoginStatus"] = Value::from("enabled");
        value["launchAtLoginReason"] = Value::from("forged");
        let parsed: SettingsPayload = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.launch_at_login_status, LaunchAtLoginStatus::Disabled);
        assert_eq!(parsed.launch_at_login_reason, None);
        let mut invalid = fixture("defaultSettings");
        invalid["launchAtLoginStatus"] = Value::from(42);
        assert!(serde_json::from_value::<SettingsPayload>(invalid).is_ok());
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
        assert_eq!(defaults.title_metrics, TitleMetrics::default());
    }

    fn no_metrics() -> TitleMetrics {
        TitleMetrics {
            cpu: false,
            power: false,
            memory: false,
            temperature: false,
            network: false,
        }
    }

    #[test]
    fn legacy_file_without_title_metrics_uses_defaults_without_repair() {
        let parsed = parse_settings(
            r#"{"titleFormat":"compact","showTrayIcon":true,"theme":"system","reduceEffects":true,"showNetworkDisks":false,"memorySaver":false}"#,
        );
        assert!(!parsed.repaired);
        assert_eq!(parsed.settings.title_metrics, TitleMetrics::default());
        assert_eq!(parsed.settings.title_format, TitleFormatSetting::Compact);
    }

    #[test]
    fn partial_title_metrics_fill_missing_keys_and_bad_types_are_repaired() {
        let parsed = parse_settings(r#"{"titleMetrics":{"temperature":true,"power":false}}"#);
        assert!(!parsed.repaired);
        assert_eq!(
            parsed.settings.title_metrics,
            TitleMetrics {
                cpu: true,
                power: false,
                memory: true,
                temperature: true,
                network: false,
            }
        );
        let parsed = parse_settings(r#"{"titleMetrics":{"cpu":"yes"}}"#);
        assert!(parsed.repaired);
        assert_eq!(parsed.settings.title_metrics, TitleMetrics::default());
        let parsed = parse_settings(r#"{"titleMetrics":"cpu"}"#);
        assert!(parsed.repaired);
        assert_eq!(parsed.settings.title_metrics, TitleMetrics::default());
    }

    #[test]
    fn normalized_forces_icon_when_no_metric_is_shown() {
        let hidden = Settings {
            show_tray_icon: false,
            title_metrics: no_metrics(),
            ..Settings::default()
        };
        assert!(hidden.clone().normalized().show_tray_icon);
        assert_eq!(custom().normalized(), custom());
        let icon_only = Settings {
            title_metrics: no_metrics(),
            ..Settings::default()
        };
        assert_eq!(icon_only.clone().normalized(), icon_only);
        let parsed = parse_settings(
            r#"{"showTrayIcon":false,"titleMetrics":{"cpu":false,"power":false,"memory":false}}"#,
        );
        assert!(parsed.settings.show_tray_icon);
        assert_eq!(parsed.settings.title_metrics, no_metrics());
    }

    #[test]
    fn network_alone_keeps_the_tray_icon_hidden() {
        let network_only = Settings {
            show_tray_icon: false,
            title_metrics: TitleMetrics {
                network: true,
                ..no_metrics()
            },
            ..Settings::default()
        };
        assert_eq!(network_only.clone().normalized(), network_only);
        let parsed = parse_settings(
            r#"{"showTrayIcon":false,"titleMetrics":{"cpu":false,"power":false,"memory":false,"network":true}}"#,
        );
        assert!(!parsed.repaired);
        assert!(!parsed.settings.show_tray_icon);
        assert!(parsed.settings.title_metrics.network);
        let legacy = parse_settings(
            r#"{"showTrayIcon":true,"titleMetrics":{"cpu":true,"power":true,"memory":true,"temperature":false}}"#,
        );
        assert!(!legacy.repaired);
        assert!(!legacy.settings.title_metrics.network);
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
        assert_eq!(shared.title_metrics(), custom().title_metrics);
        apply_shared(&shared, &Settings::default());
        assert!(!shared.compact_title.load(Ordering::Relaxed));
        assert!(!shared.include_network_disks.load(Ordering::Relaxed));
        assert_eq!(shared.title_metrics(), TitleMetrics::default());
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
