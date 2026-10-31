use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const STATE_FILE: &str = "updater-state.json";
pub const MAX_STATE_BYTES: u64 = 4 * 1024;

const TEMP_FILE: &str = ".updater-state.json.tmp";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub from_version: String,
    pub launch_at_login_was_on: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<Pending>,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("updater state file is larger than {MAX_STATE_BYTES} bytes")]
    TooLarge,
    #[error("updater state could not be encoded: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("updater state I/O failed: {0}")]
    Io(#[from] io::Error),
}

fn field<T: DeserializeOwned>(
    object: &Map<String, Value>,
    key: &str,
    repaired: &mut bool,
) -> Option<T> {
    let value = object.get(key)?;
    if value.is_null() {
        return None;
    }
    match T::deserialize(value) {
        Ok(parsed) => Some(parsed),
        Err(_) => {
            *repaired = true;
            None
        }
    }
}

pub fn parse_state(text: &str) -> (PersistedState, bool) {
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(text) else {
        return (PersistedState::default(), true);
    };
    let mut repaired = false;
    let state = PersistedState {
        last_check_at: field(&object, "lastCheckAt", &mut repaired),
        pending: field(&object, "pending", &mut repaired),
    };
    (state, repaired)
}

fn read_limited(path: &Path) -> Result<Option<String>, StateError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut text = String::new();
    file.take(MAX_STATE_BYTES + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MAX_STATE_BYTES {
        return Err(StateError::TooLarge);
    }
    Ok(Some(text))
}

pub fn load_from(dir: &Path) -> PersistedState {
    match read_limited(&dir.join(STATE_FILE)) {
        Ok(Some(text)) => {
            let (state, repaired) = parse_state(&text);
            if repaired {
                log::warn!("updater state file had invalid values; ignoring those fields");
            }
            state
        }
        Ok(None) => PersistedState::default(),
        Err(err) => {
            log::warn!("could not read updater state, using defaults: {err}");
            PersistedState::default()
        }
    }
}

pub fn save_to(dir: &Path, state: &PersistedState) -> Result<(), StateError> {
    fs::create_dir_all(dir)?;
    let bytes = serde_json::to_vec_pretty(state)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        return Err(StateError::TooLarge);
    }
    let temp = dir.join(TEMP_FILE);
    let result = (|| -> io::Result<()> {
        let mut file = File::create(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, dir.join(STATE_FILE))
    })();
    if result.is_err() {
        fs::remove_file(&temp).ok();
    }
    result.map_err(StateError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "pulsebar-updater-state-{}-{name}-{unique}",
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

    fn full() -> PersistedState {
        PersistedState {
            last_check_at: Some(1_793_437_323_000),
            pending: Some(Pending {
                from_version: "0.3.0".into(),
                launch_at_login_was_on: true,
            }),
        }
    }

    #[test]
    fn save_then_load_round_trips_and_leaves_no_temp_file() {
        let dir = TempDir::new("roundtrip");
        save_to(&dir.0, &full()).unwrap();
        assert!(!dir.0.join(TEMP_FILE).exists());
        assert_eq!(load_from(&dir.0), full());
        let only_check = PersistedState {
            pending: None,
            ..full()
        };
        save_to(&dir.0, &only_check).unwrap();
        assert_eq!(load_from(&dir.0), only_check);
        let text = fs::read_to_string(dir.0.join(STATE_FILE)).unwrap();
        assert!(!text.contains("pending"), "{text}");
    }

    #[test]
    fn missing_file_loads_defaults() {
        let dir = TempDir::new("missing");
        assert_eq!(load_from(&dir.0), PersistedState::default());
    }

    #[test]
    fn serialized_keys_are_camel_case() {
        let value = serde_json::to_value(full()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "lastCheckAt": 1_793_437_323_000_u64,
                "pending": { "fromVersion": "0.3.0", "launchAtLoginWasOn": true }
            })
        );
        assert_eq!(
            serde_json::to_value(PersistedState::default()).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn invalid_fields_are_dropped_one_by_one() {
        let (state, repaired) = parse_state(
            r#"{"lastCheckAt":-5,"pending":{"fromVersion":"0.3.0","launchAtLoginWasOn":false}}"#,
        );
        assert!(repaired);
        assert_eq!(state.last_check_at, None);
        assert_eq!(
            state.pending,
            Some(Pending {
                from_version: "0.3.0".into(),
                launch_at_login_was_on: false
            })
        );
        let (state, repaired) = parse_state(r#"{"lastCheckAt":12,"pending":{"fromVersion":3}}"#);
        assert!(repaired);
        assert_eq!(state.last_check_at, Some(12));
        assert_eq!(state.pending, None);
        let (state, repaired) = parse_state(r#"{"lastCheckAt":null,"extra":1}"#);
        assert!(!repaired);
        assert_eq!(state, PersistedState::default());
    }

    #[test]
    fn corrupt_or_non_object_input_yields_defaults() {
        for text in ["", "null", "[1]", "{oops", "7"] {
            let (state, repaired) = parse_state(text);
            assert_eq!(state, PersistedState::default(), "input {text:?}");
            assert!(repaired, "input {text:?}");
        }
    }

    #[test]
    fn oversized_file_is_ignored() {
        let dir = TempDir::new("oversized");
        fs::create_dir_all(&dir.0).unwrap();
        let padding = " ".repeat(usize::try_from(MAX_STATE_BYTES).unwrap());
        fs::write(
            dir.0.join(STATE_FILE),
            format!("{{\"lastCheckAt\":5}}{padding}"),
        )
        .unwrap();
        assert_eq!(load_from(&dir.0), PersistedState::default());
    }

    #[test]
    fn oversized_state_is_not_written() {
        let dir = TempDir::new("too-big");
        let state = PersistedState {
            last_check_at: None,
            pending: Some(Pending {
                from_version: "9".repeat(5000),
                launch_at_login_was_on: false,
            }),
        };
        assert!(matches!(save_to(&dir.0, &state), Err(StateError::TooLarge)));
        assert!(!dir.0.join(STATE_FILE).exists());
    }
}
