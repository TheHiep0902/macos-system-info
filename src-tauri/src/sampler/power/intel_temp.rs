use std::time::Instant;

use crate::sampler::power::apple_silicon::{RETRY_INTERVAL, retry_due};
use crate::sampler::power::{
    REASON_NO_TEMP_SENSOR, REASON_READ_FAILED, REASON_SENSOR_INIT, SourceError, TempReading,
    TempSource, Tick,
};
use crate::smc::{SmcConnection, SmcError};

pub const TEMP_KEYS: [&str; 6] = ["TCXC", "TCXc", "TC0D", "TC0E", "TC0F", "TC0P"];

const TEMP_SOURCES: [&str; 6] = [
    "smc:TCXC", "smc:TCXc", "smc:TC0D", "smc:TC0E", "smc:TC0F", "smc:TC0P",
];

const MIN_CELSIUS: f32 = 10.0;

const MAX_CELSIUS: f32 = 115.0;

const INVALID_READ_LIMIT: u32 = 3;

pub fn is_valid_celsius(celsius: f32) -> bool {
    celsius.is_finite() && (MIN_CELSIUS..=MAX_CELSIUS).contains(&celsius)
}

#[derive(Debug, Clone, Copy)]
struct TempKey {
    key: &'static str,
    source: &'static str,
}

impl TempKey {
    fn reading(self, celsius: f32) -> TempReading {
        TempReading {
            celsius,
            source: self.source,
        }
    }
}

fn describe_rejection(key: &str, value: Result<f32, SmcError>) -> String {
    match value {
        Ok(celsius) => format!("{key}={celsius} °C"),
        Err(err) => format!("{key}: {err}"),
    }
}

fn find_temp_key(
    mut read: impl FnMut(&'static str) -> Result<f32, SmcError>,
) -> Result<(TempKey, f32), String> {
    let mut notes = Vec::with_capacity(TEMP_KEYS.len());
    for (key, source) in TEMP_KEYS.into_iter().zip(TEMP_SOURCES) {
        match read(key) {
            Ok(celsius) if is_valid_celsius(celsius) => {
                return Ok((TempKey { key, source }, celsius));
            }
            value => notes.push(describe_rejection(key, value)),
        }
    }
    Err(notes.join("; "))
}

pub fn select_temp_key(
    read: impl FnMut(&'static str) -> Result<f32, SmcError>,
) -> Result<(&'static str, f32), String> {
    find_temp_key(read).map(|(found, celsius)| (found.key, celsius))
}

enum KeyState {
    Active {
        key: TempKey,
        bad_reads: u32,
    },
    Failed {
        last_attempt: Instant,
        detail: String,
    },
}

struct KeyTracker {
    state: KeyState,
}

impl KeyTracker {
    fn new(key: TempKey) -> Self {
        KeyTracker {
            state: KeyState::Active { key, bad_reads: 0 },
        }
    }

    fn read(
        &mut self,
        tick: &Tick,
        mut read: impl FnMut(&'static str) -> Result<f32, SmcError>,
    ) -> Result<Option<TempReading>, SourceError> {
        match &mut self.state {
            KeyState::Active { key, bad_reads } => {
                let key = *key;
                let value = read(key.key);
                if let Ok(celsius) = value
                    && is_valid_celsius(celsius)
                {
                    *bad_reads = 0;
                    return Ok(Some(key.reading(celsius)));
                }
                *bad_reads = bad_reads.saturating_add(1);
                let note = describe_rejection(key.key, value);
                if *bad_reads < INVALID_READ_LIMIT {
                    log::debug!(
                        "SMC temperature read rejected ({bad_reads}/{INVALID_READ_LIMIT}): {note}"
                    );
                    return Ok(None);
                }
                log::info!(
                    "SMC temperature key {} rejected {INVALID_READ_LIMIT} times in a row ({note}), probing keys again",
                    key.key
                );
                self.reprobe(tick, read)
            }
            KeyState::Failed {
                last_attempt,
                detail,
            } => {
                if !retry_due(Some(*last_attempt), tick.now) {
                    return Err(SourceError::new(REASON_READ_FAILED, detail.clone()));
                }
                self.reprobe(tick, read)
            }
        }
    }

    fn reprobe(
        &mut self,
        tick: &Tick,
        read: impl FnMut(&'static str) -> Result<f32, SmcError>,
    ) -> Result<Option<TempReading>, SourceError> {
        match find_temp_key(read) {
            Ok((key, celsius)) => {
                log::info!("SMC temperature key {} selected ({celsius:.1} °C)", key.key);
                self.state = KeyState::Active { key, bad_reads: 0 };
                Ok(Some(key.reading(celsius)))
            }
            Err(detail) => {
                if matches!(self.state, KeyState::Active { .. }) {
                    log::warn!(
                        "SMC temperature sensor lost, retrying every {}s: {detail}",
                        RETRY_INTERVAL.as_secs()
                    );
                } else {
                    log::debug!("SMC temperature key probe retry failed: {detail}");
                }
                self.state = KeyState::Failed {
                    last_attempt: tick.now,
                    detail: detail.clone(),
                };
                Err(SourceError::new(REASON_READ_FAILED, detail))
            }
        }
    }

    fn reset(&mut self) {
        if let KeyState::Active { bad_reads, .. } = &mut self.state {
            *bad_reads = 0;
        }
    }
}

pub struct SmcTemp {
    connection: SmcConnection,
    tracker: KeyTracker,
}

impl SmcTemp {
    pub fn probe() -> Result<Self, SourceError> {
        let mut connection = SmcConnection::open()
            .map_err(|err| SourceError::new(REASON_SENSOR_INIT, err.to_string()))?;
        let (key, celsius) = find_temp_key(|key| connection.read_f32(key))
            .map_err(|detail| SourceError::new(REASON_NO_TEMP_SENSOR, detail))?;
        log::info!("SMC temperature key {} selected ({celsius:.1} °C)", key.key);
        Ok(SmcTemp {
            connection,
            tracker: KeyTracker::new(key),
        })
    }
}

impl TempSource for SmcTemp {
    fn read(&mut self, tick: &Tick) -> Result<Option<TempReading>, SourceError> {
        let connection = &mut self.connection;
        self.tracker.read(tick, |key| connection.read_f32(key))
    }

    fn reset(&mut self) {
        self.tracker.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::time::Duration;

    #[test]
    fn sources_follow_keys_in_order() {
        for (key, source) in TEMP_KEYS.into_iter().zip(TEMP_SOURCES) {
            assert_eq!(source, format!("smc:{key}"));
        }
    }

    #[test]
    fn valid_celsius_boundaries() {
        for good in [10.0, 10.5, 54.0, 100.0, 115.0] {
            assert!(is_valid_celsius(good), "{good}");
        }
        for bad in [
            0.0,
            -44.0,
            -70.0,
            -127.0,
            -128.0,
            9.99,
            115.01,
            116.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            assert!(!is_valid_celsius(bad), "{bad}");
        }
    }

    type Reply = Option<f32>;

    struct FakeSmc {
        replies: HashMap<&'static str, VecDeque<Reply>>,
        fallback: HashMap<&'static str, Reply>,
        reads: Vec<&'static str>,
    }

    impl FakeSmc {
        fn new(entries: &[(&'static str, &[Reply])]) -> Self {
            FakeSmc {
                replies: entries
                    .iter()
                    .map(|(key, replies)| (*key, replies.iter().copied().collect()))
                    .collect(),
                fallback: HashMap::new(),
                reads: Vec::new(),
            }
        }

        fn steady(mut self, key: &'static str, reply: Reply) -> Self {
            self.fallback.insert(key, reply);
            self
        }

        fn read(&mut self, key: &'static str) -> Result<f32, SmcError> {
            self.reads.push(key);
            let reply = self
                .replies
                .get_mut(key)
                .and_then(VecDeque::pop_front)
                .or_else(|| self.fallback.get(key).copied());
            match reply.flatten() {
                Some(celsius) => Ok(celsius),
                None => Err(SmcError::KeyNotFound { key: key.into() }),
            }
        }

        fn select(&mut self) -> Result<(&'static str, f32), String> {
            select_temp_key(|key| self.read(key))
        }

        fn take_reads(&mut self) -> Vec<&'static str> {
            std::mem::take(&mut self.reads)
        }
    }

    fn key(name: &'static str) -> TempKey {
        let (key, source) = TEMP_KEYS
            .into_iter()
            .zip(TEMP_SOURCES)
            .find(|(candidate, _)| *candidate == name)
            .unwrap();
        TempKey { key, source }
    }

    fn tick(seq: u64, start: Instant, secs: u64) -> Tick {
        Tick {
            seq,
            now: start + Duration::from_secs(secs),
        }
    }

    fn reading(source: &'static str, celsius: f32) -> Option<TempReading> {
        Some(TempReading { celsius, source })
    }

    fn read_with(
        tracker: &mut KeyTracker,
        smc: &mut FakeSmc,
        tick: &Tick,
    ) -> Result<Option<TempReading>, SourceError> {
        tracker.read(tick, |key| smc.read(key))
    }

    #[test]
    fn chooses_tcxc_when_present() {
        let mut smc = FakeSmc::new(&[("TCXC", &[Some(61.0)]), ("TC0D", &[Some(55.0)])]);
        assert_eq!(smc.select(), Ok(("TCXC", 61.0)));
        assert_eq!(smc.reads, ["TCXC"]);
    }

    #[test]
    fn rejects_out_of_range_values_and_falls_through() {
        let mut smc = FakeSmc::new(&[
            ("TCXC", &[Some(-44.0)]),
            ("TCXc", &[Some(-127.0)]),
            ("TC0D", &[Some(0.0)]),
            ("TC0E", &[Some(116.0)]),
            ("TC0F", &[Some(f32::NAN)]),
            ("TC0P", &[Some(48.0)]),
        ]);
        assert_eq!(smc.select(), Ok(("TC0P", 48.0)));
        assert_eq!(smc.reads, TEMP_KEYS);
    }

    #[test]
    fn fallback_order_matches_temp_keys() {
        let expected_order = ["TCXC", "TCXc", "TC0D", "TC0E", "TC0F", "TC0P"];
        assert_eq!(TEMP_KEYS, expected_order);
        for (index, wanted) in TEMP_KEYS.into_iter().enumerate() {
            let mut smc = FakeSmc::new(&[(wanted, &[Some(50.0)])]);
            assert_eq!(smc.select(), Ok((wanted, 50.0)));
            assert_eq!(smc.reads, TEMP_KEYS[..=index]);
        }
    }

    #[test]
    fn this_host_layout_selects_tc0d() {
        let mut smc = FakeSmc::new(&[
            ("TC0D", &[Some(47.0)]),
            ("TC0E", &[Some(47.0)]),
            ("TC0F", &[Some(47.0)]),
            ("TC0H", &[Some(47.0)]),
            ("TC0P", &[Some(47.0)]),
        ]);
        assert_eq!(smc.select(), Ok(("TC0D", 47.0)));
    }

    #[test]
    fn no_valid_key_lists_every_attempt() {
        let mut smc = FakeSmc::new(&[("TCXC", &[Some(-127.0)]), ("TC0P", &[Some(0.0)])]);
        let detail = smc.select().unwrap_err();
        assert_eq!(smc.reads, TEMP_KEYS);
        assert!(detail.contains("TCXC=-127 °C"), "{detail}");
        assert!(detail.contains("TCXc: SMC key TCXc not found"), "{detail}");
        assert!(detail.contains("TC0P=0 °C"), "{detail}");
        let error = SourceError::new(REASON_NO_TEMP_SENSOR, detail);
        assert_eq!(error.reason, REASON_NO_TEMP_SENSOR);
    }

    #[test]
    fn valid_reads_report_the_selected_source() {
        let start = Instant::now();
        let mut smc = FakeSmc::new(&[("TC0D", &[Some(52.0), Some(53.0)])]);
        let mut tracker = KeyTracker::new(key("TC0D"));
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(1, start, 2)),
            Ok(reading("smc:TC0D", 52.0))
        );
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(2, start, 4)),
            Ok(reading("smc:TC0D", 53.0))
        );
        assert_eq!(smc.reads, ["TC0D", "TC0D"]);
    }

    #[test]
    fn third_consecutive_bad_read_reprobes_and_may_switch_key() {
        let start = Instant::now();
        let mut smc = FakeSmc::new(&[("TC0D", &[Some(0.0), None, Some(-127.0), Some(0.0)])])
            .steady("TC0P", Some(49.0));
        let mut tracker = KeyTracker::new(key("TC0D"));
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(1, start, 2)),
            Ok(None)
        );
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(2, start, 4)),
            Ok(None)
        );
        assert_eq!(smc.take_reads(), ["TC0D", "TC0D"]);
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(3, start, 6)),
            Ok(reading("smc:TC0P", 49.0))
        );
        let mut expected = vec!["TC0D"];
        expected.extend(TEMP_KEYS);
        assert_eq!(smc.take_reads(), expected);
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(4, start, 8)),
            Ok(reading("smc:TC0P", 49.0))
        );
        assert_eq!(smc.take_reads(), ["TC0P"]);
    }

    #[test]
    fn reprobe_can_keep_the_same_key() {
        let start = Instant::now();
        let mut smc =
            FakeSmc::new(&[("TC0D", &[Some(200.0), Some(200.0), Some(200.0), Some(51.0)])]);
        let mut tracker = KeyTracker::new(key("TC0D"));
        for seq in 1..=2 {
            assert_eq!(
                read_with(&mut tracker, &mut smc, &tick(seq, start, seq * 2)),
                Ok(None)
            );
        }
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(3, start, 6)),
            Ok(reading("smc:TC0D", 51.0))
        );
    }

    #[test]
    fn valid_read_resets_the_bad_read_counter() {
        let start = Instant::now();
        let mut smc = FakeSmc::new(&[(
            "TC0D",
            &[Some(0.0), Some(0.0), Some(50.0), Some(0.0), Some(0.0)],
        )]);
        let mut tracker = KeyTracker::new(key("TC0D"));
        let results: Vec<_> = (1..=5)
            .map(|seq| read_with(&mut tracker, &mut smc, &tick(seq, start, seq * 2)))
            .collect();
        assert_eq!(
            results,
            [
                Ok(None),
                Ok(None),
                Ok(reading("smc:TC0D", 50.0)),
                Ok(None),
                Ok(None)
            ]
        );
        assert_eq!(smc.reads, ["TC0D"; 5]);
    }

    #[test]
    fn reset_clears_the_bad_read_counter() {
        let start = Instant::now();
        let mut smc = FakeSmc::new(&[]).steady("TC0D", Some(0.0));
        let mut tracker = KeyTracker::new(key("TC0D"));
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(1, start, 2)),
            Ok(None)
        );
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(2, start, 4)),
            Ok(None)
        );
        tracker.reset();
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(3, start, 600)),
            Ok(None)
        );
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(4, start, 602)),
            Ok(None)
        );
        assert_eq!(smc.reads, ["TC0D"; 4]);
    }

    #[test]
    fn failed_reprobe_errors_and_retries_only_after_sixty_seconds() {
        let start = Instant::now();
        let mut smc = FakeSmc::new(&[]);
        let mut tracker = KeyTracker::new(key("TC0D"));
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(1, start, 2)),
            Ok(None)
        );
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(2, start, 4)),
            Ok(None)
        );
        let err = read_with(&mut tracker, &mut smc, &tick(3, start, 6)).unwrap_err();
        assert_eq!(err.reason, REASON_READ_FAILED);
        assert!(err.detail.contains("TC0D: SMC key TC0D not found"), "{err}");
        smc.take_reads();

        for (seq, secs) in [(4, 8), (5, 30), (6, 65)] {
            assert_eq!(
                read_with(&mut tracker, &mut smc, &tick(seq, start, secs)),
                Err(err.clone())
            );
        }
        assert!(smc.reads.is_empty());

        let retry = read_with(&mut tracker, &mut smc, &tick(7, start, 66)).unwrap_err();
        assert_eq!(retry.reason, REASON_READ_FAILED);
        assert_eq!(smc.take_reads(), TEMP_KEYS);

        assert!(read_with(&mut tracker, &mut smc, &tick(8, start, 125)).is_err());
        assert!(smc.take_reads().is_empty());

        smc = smc.steady("TC0E", Some(58.0));
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(9, start, 126)),
            Ok(reading("smc:TC0E", 58.0))
        );
        assert_eq!(smc.take_reads(), ["TCXC", "TCXc", "TC0D", "TC0E"]);
        assert_eq!(
            read_with(&mut tracker, &mut smc, &tick(10, start, 128)),
            Ok(reading("smc:TC0E", 58.0))
        );
        assert_eq!(smc.take_reads(), ["TC0E"]);
    }

    #[test]
    fn reset_while_failed_keeps_the_retry_interval() {
        let start = Instant::now();
        let mut smc = FakeSmc::new(&[]);
        let mut tracker = KeyTracker::new(key("TC0D"));
        for seq in 1..=3 {
            let _ = read_with(&mut tracker, &mut smc, &tick(seq, start, seq * 2));
        }
        smc.take_reads();
        tracker.reset();
        assert!(read_with(&mut tracker, &mut smc, &tick(4, start, 10)).is_err());
        assert!(smc.reads.is_empty());
    }
}
