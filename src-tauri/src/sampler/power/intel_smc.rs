use std::thread;
use std::time::Duration;

use crate::sampler::power::{
    PowerScope, PowerSource, REASON_NO_POWER_SENSOR, REASON_READ_FAILED, REASON_SENSOR_INIT,
    SourceError, Tick,
};
use crate::smc::{SmcConnection, SmcError};

pub const KEY_CANDIDATES: [&str; 3] = ["PCPT", "PCPC", "PC0C"];

pub const MAX_WATTS: f32 = 300.0;

const ZERO_RETRY_DELAY: Duration = Duration::from_millis(600);

pub fn is_valid_watts(watts: f32) -> bool {
    watts.is_finite() && watts > 0.0 && watts < MAX_WATTS
}

pub fn is_plausible_watts(watts: f32) -> bool {
    watts.is_finite() && (0.0..MAX_WATTS).contains(&watts)
}

pub fn scope_for_key(key: &str) -> PowerScope {
    if key == "PCPT" {
        PowerScope::Package
    } else {
        PowerScope::Cpu
    }
}

fn select_key(
    mut read: impl FnMut(&'static str) -> Result<f32, SmcError>,
    mut wait: impl FnMut(),
) -> Result<(&'static str, f32), String> {
    let mut existing = Vec::with_capacity(KEY_CANDIDATES.len());
    let mut zero_seen = false;
    let mut notes = Vec::new();
    for key in KEY_CANDIDATES {
        match read(key) {
            Ok(watts) if is_valid_watts(watts) && !zero_seen => return Ok((key, watts)),
            Ok(watts) => {
                zero_seen |= watts == 0.0;
                existing.push(key);
                notes.push(format!("{key}={watts} W"));
            }
            Err(err) => notes.push(format!("{key}: {err}")),
        }
    }
    if zero_seen {
        wait();
        for key in existing {
            match read(key) {
                Ok(watts) if is_valid_watts(watts) => return Ok((key, watts)),
                Ok(watts) => notes.push(format!("{key}={watts} W after retry")),
                Err(err) => notes.push(format!("{key}: {err} after retry")),
            }
        }
    }
    Err(notes.join("; "))
}

fn accept_reading(key: &str, watts: f32) -> Result<f32, SourceError> {
    if is_plausible_watts(watts) {
        Ok(watts)
    } else {
        Err(SourceError::new(
            REASON_READ_FAILED,
            format!("smc:{key} returned {watts} W"),
        ))
    }
}

pub struct SmcPower {
    connection: SmcConnection,
    key: &'static str,
    source: String,
    primed: bool,
}

impl SmcPower {
    pub fn probe() -> Result<Self, SourceError> {
        let mut connection = SmcConnection::open()
            .map_err(|err| SourceError::new(REASON_SENSOR_INIT, err.to_string()))?;
        let (key, watts) = select_key(
            |key| connection.read_f32(key),
            || thread::sleep(ZERO_RETRY_DELAY),
        )
        .map_err(|detail| SourceError::new(REASON_NO_POWER_SENSOR, detail))?;
        log::info!("SMC power key {key} selected ({watts:.2} W)");
        Ok(SmcPower {
            connection,
            key,
            source: format!("smc:{key}"),
            primed: true,
        })
    }
}

impl PowerSource for SmcPower {
    fn source(&self) -> &str {
        &self.source
    }

    fn scope(&self) -> PowerScope {
        scope_for_key(self.key)
    }

    fn read(&mut self, _tick: &Tick) -> Result<Option<f32>, SourceError> {
        let watts = self
            .connection
            .read_f32(self.key)
            .map_err(|err| SourceError::new(REASON_READ_FAILED, err.to_string()))?;
        if !self.primed {
            self.primed = true;
            return Ok(None);
        }
        accept_reading(self.key, watts).map(Some)
    }

    fn reset(&mut self) {
        self.primed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn valid_watts_boundaries() {
        assert!(!is_valid_watts(0.0));
        assert!(!is_valid_watts(-0.5));
        assert!(is_valid_watts(f32::MIN_POSITIVE));
        assert!(is_valid_watts(0.01));
        assert!(is_valid_watts(299.99));
        assert!(!is_valid_watts(300.0));
        assert!(!is_valid_watts(f32::NAN));
        assert!(!is_valid_watts(f32::INFINITY));
    }

    #[test]
    fn plausible_watts_accepts_zero() {
        assert!(is_plausible_watts(0.0));
        assert!(is_plausible_watts(299.99));
        assert!(!is_plausible_watts(300.0));
        assert!(!is_plausible_watts(-0.01));
        assert!(!is_plausible_watts(f32::NAN));
        assert!(!is_plausible_watts(f32::NEG_INFINITY));
    }

    #[test]
    fn scope_follows_key() {
        assert_eq!(scope_for_key("PCPT"), PowerScope::Package);
        assert_eq!(scope_for_key("PCPC"), PowerScope::Cpu);
        assert_eq!(scope_for_key("PC0C"), PowerScope::Cpu);
    }

    #[test]
    fn reading_validation() {
        assert_eq!(accept_reading("PCPT", 0.0), Ok(0.0));
        assert_eq!(accept_reading("PCPT", 12.5), Ok(12.5));
        let err = accept_reading("PCPT", 300.0).unwrap_err();
        assert_eq!(err.reason, REASON_READ_FAILED);
        assert_eq!(err.detail, "smc:PCPT returned 300 W");
        assert!(accept_reading("PCPC", -1.0).is_err());
        assert!(accept_reading("PCPC", f32::NAN).is_err());
    }

    struct Script {
        values: HashMap<&'static str, Vec<Option<f32>>>,
        reads: Vec<&'static str>,
        waits: usize,
    }

    impl Script {
        fn new(entries: &[(&'static str, &[Option<f32>])]) -> Self {
            Script {
                values: entries
                    .iter()
                    .map(|(key, values)| (*key, values.iter().rev().copied().collect()))
                    .collect(),
                reads: Vec::new(),
                waits: 0,
            }
        }

        fn run(&mut self) -> Result<(&'static str, f32), String> {
            let values = &mut self.values;
            let reads = &mut self.reads;
            let waits = &mut self.waits;
            select_key(
                |key| {
                    reads.push(key);
                    match values.get_mut(key).and_then(Vec::pop) {
                        Some(Some(watts)) => Ok(watts),
                        _ => Err(SmcError::KeyNotFound { key: key.into() }),
                    }
                },
                || *waits += 1,
            )
        }
    }

    #[test]
    fn picks_first_valid_key_without_waiting() {
        let mut script = Script::new(&[("PCPT", &[Some(9.5)]), ("PCPC", &[Some(6.0)])]);
        assert_eq!(script.run(), Ok(("PCPT", 9.5)));
        assert_eq!(script.reads, ["PCPT"]);
        assert_eq!(script.waits, 0);
    }

    #[test]
    fn falls_through_missing_and_invalid_keys() {
        let mut script = Script::new(&[("PCPT", &[Some(450.0)]), ("PC0C", &[Some(4.25)])]);
        assert_eq!(script.run(), Ok(("PC0C", 4.25)));
        assert_eq!(script.reads, ["PCPT", "PCPC", "PC0C"]);
        assert_eq!(script.waits, 0);
    }

    #[test]
    fn zero_reading_is_retried_once_after_waiting() {
        let mut script = Script::new(&[("PCPT", &[Some(0.0), Some(8.0)]), ("PCPC", &[Some(5.0)])]);
        assert_eq!(script.run(), Ok(("PCPT", 8.0)));
        assert_eq!(script.reads, ["PCPT", "PCPC", "PC0C", "PCPT"]);
        assert_eq!(script.waits, 1);
    }

    #[test]
    fn persistent_zero_falls_back_to_next_existing_key() {
        let mut script = Script::new(&[
            ("PCPT", &[Some(0.0), Some(0.0)]),
            ("PCPC", &[Some(0.0), Some(5.0)]),
        ]);
        assert_eq!(script.run(), Ok(("PCPC", 5.0)));
        assert_eq!(script.reads, ["PCPT", "PCPC", "PC0C", "PCPT", "PCPC"]);
        assert_eq!(script.waits, 1);
    }

    #[test]
    fn no_usable_key_reports_every_attempt() {
        let mut script = Script::new(&[("PCPT", &[Some(0.0), Some(0.0)])]);
        let detail = script.run().unwrap_err();
        assert_eq!(script.waits, 1);
        assert!(detail.contains("PCPT=0 W"), "{detail}");
        assert!(detail.contains("PCPC: SMC key PCPC not found"), "{detail}");
        assert!(detail.contains("PCPT=0 W after retry"), "{detail}");
    }

    #[test]
    fn all_keys_missing_does_not_wait() {
        let mut script = Script::new(&[]);
        assert!(script.run().is_err());
        assert_eq!(script.waits, 0);
        assert_eq!(script.reads, KEY_CANDIDATES);
    }
}
