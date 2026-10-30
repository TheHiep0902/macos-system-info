use std::collections::HashMap;
use std::io;
use std::mem::{offset_of, size_of};
use std::ptr;
use std::time::{Duration, Instant};

use crate::model::{MetricState, ThroughputSample};

const MIN_ELAPSED: Duration = Duration::from_millis(500);
const READ_ATTEMPTS: usize = 3;
const COMMON_HEADER_LEN: usize = offset_of!(libc::if_msghdr2, ifm_addrs);
const TYPE_OFFSET: usize = offset_of!(libc::if_msghdr2, ifm_type);
const HEADER_LEN: usize = size_of::<libc::if_msghdr2>();
const LINK_LEN: usize = size_of::<libc::sockaddr_dl>();
const NAME_OFFSET: usize = HEADER_LEN + offset_of!(libc::sockaddr_dl, sdl_data);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfCounters {
    pub name: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

#[derive(Debug, Default)]
pub struct CounterReader {
    buf: Vec<u8>,
}

impl CounterReader {
    pub fn new() -> Self {
        CounterReader::default()
    }

    pub fn read(&mut self) -> io::Result<Vec<IfCounters>> {
        let mut attempt = 1;
        let len = loop {
            match self.fill() {
                Ok(len) => break len,
                Err(err) if err.raw_os_error() == Some(libc::ENOMEM) && attempt < READ_ATTEMPTS => {
                    attempt += 1;
                }
                Err(err) => return Err(err),
            }
        };
        Ok(parse_iflist2(&self.buf[..len]))
    }

    fn fill(&mut self) -> io::Result<usize> {
        let needed = sysctl_iflist2(ptr::null_mut(), 0)?;
        if self.buf.len() < needed {
            self.buf.resize(needed, 0);
        }
        let written = sysctl_iflist2(self.buf.as_mut_ptr().cast(), self.buf.len())?;
        Ok(written.min(self.buf.len()))
    }
}

fn sysctl_iflist2(buf: *mut libc::c_void, cap: usize) -> io::Result<usize> {
    let mut mib = [libc::CTL_NET, libc::PF_ROUTE, 0, 0, libc::NET_RT_IFLIST2, 0];
    let mut len = cap;
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            buf,
            &mut len,
            ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(len)
}

fn read_header(record: &[u8]) -> Option<libc::if_msghdr2> {
    if record.len() < HEADER_LEN {
        return None;
    }
    Some(unsafe { ptr::read_unaligned(record.as_ptr().cast::<libc::if_msghdr2>()) })
}

fn read_link(record: &[u8]) -> Option<libc::sockaddr_dl> {
    let bytes = record.get(HEADER_LEN..HEADER_LEN.checked_add(LINK_LEN)?)?;
    Some(unsafe { ptr::read_unaligned(bytes.as_ptr().cast::<libc::sockaddr_dl>()) })
}

pub fn is_counted(name: &str) -> bool {
    name.strip_prefix("en")
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

fn parse_record(record: &[u8]) -> Option<IfCounters> {
    let header = read_header(record)?;
    let flags = header.ifm_flags;
    let wanted = libc::IFF_UP | libc::IFF_RUNNING;
    if flags & wanted != wanted {
        return None;
    }
    let link = read_link(record)?;
    if libc::c_int::from(link.sdl_family) != libc::AF_LINK {
        return None;
    }
    let end = NAME_OFFSET.checked_add(usize::from(link.sdl_nlen))?;
    let name = std::str::from_utf8(record.get(NAME_OFFSET..end)?).ok()?;
    if !is_counted(name) {
        return None;
    }
    let data = header.ifm_data;
    Some(IfCounters {
        name: name.to_owned(),
        rx_bytes: data.ifi_ibytes,
        tx_bytes: data.ifi_obytes,
    })
}

pub fn parse_iflist2(buf: &[u8]) -> Vec<IfCounters> {
    let mut counters = Vec::new();
    let mut offset = 0usize;
    while let Some(common) = buf.get(offset..offset.saturating_add(COMMON_HEADER_LEN)) {
        let msglen = usize::from(u16::from_ne_bytes([common[0], common[1]]));
        if msglen < COMMON_HEADER_LEN {
            break;
        }
        let Some(record) = offset
            .checked_add(msglen)
            .and_then(|end| buf.get(offset..end))
        else {
            break;
        };
        if libc::c_int::from(common[TYPE_OFFSET]) == libc::RTM_IFINFO2
            && let Some(entry) = parse_record(record)
        {
            counters.push(entry);
        }
        offset += msglen;
    }
    counters
}

#[derive(Debug, Default)]
pub struct ThroughputMeter {
    previous: HashMap<String, (u64, u64)>,
    previous_at: Option<Instant>,
}

impl ThroughputMeter {
    pub fn update(
        &mut self,
        counters: &[IfCounters],
        now: Instant,
    ) -> MetricState<ThroughputSample> {
        let elapsed = self.previous_at.map(|at| now.saturating_duration_since(at));
        if elapsed.is_some_and(|elapsed| elapsed < MIN_ELAPSED) {
            return MetricState::Loading;
        }
        let current: HashMap<String, (u64, u64)> = counters
            .iter()
            .map(|entry| (entry.name.clone(), (entry.rx_bytes, entry.tx_bytes)))
            .collect();
        let previous = std::mem::replace(&mut self.previous, current);
        self.previous_at = Some(now);
        let Some(elapsed) = elapsed else {
            return MetricState::Loading;
        };
        let mut rx = 0.0f64;
        let mut tx = 0.0f64;
        let mut interfaces = Vec::new();
        for (name, (rx_now, tx_now)) in &self.previous {
            let Some((rx_before, tx_before)) = previous.get(name) else {
                continue;
            };
            let (Some(rx_delta), Some(tx_delta)) = (
                rx_now.checked_sub(*rx_before),
                tx_now.checked_sub(*tx_before),
            ) else {
                continue;
            };
            rx += rx_delta as f64;
            tx += tx_delta as f64;
            interfaces.push(name.clone());
        }
        interfaces.sort();
        let secs = elapsed.as_secs_f64();
        MetricState::Ok(ThroughputSample {
            rx_bytes_per_sec: rx / secs,
            tx_bytes_per_sec: tx / secs,
            interfaces,
        })
    }

    pub fn reset(&mut self) {
        self.previous.clear();
        self.previous_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RX_OFFSET: usize =
        offset_of!(libc::if_msghdr2, ifm_data) + offset_of!(libc::if_data64, ifi_ibytes);
    const TX_OFFSET: usize =
        offset_of!(libc::if_msghdr2, ifm_data) + offset_of!(libc::if_data64, ifi_obytes);
    const FLAGS_OFFSET: usize = offset_of!(libc::if_msghdr2, ifm_flags);
    const FAMILY_OFFSET: usize = HEADER_LEN + offset_of!(libc::sockaddr_dl, sdl_family);
    const NLEN_OFFSET: usize = HEADER_LEN + offset_of!(libc::sockaddr_dl, sdl_nlen);
    const UP: libc::c_int = libc::IFF_UP | libc::IFF_RUNNING;

    fn put(record: &mut [u8], offset: usize, bytes: &[u8]) {
        record[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    fn record(name: &str, flags: libc::c_int, rx: u64, tx: u64) -> Vec<u8> {
        let len = (NAME_OFFSET + name.len()).max(HEADER_LEN + LINK_LEN);
        let mut bytes = vec![0u8; len];
        put(&mut bytes, 0, &(len as u16).to_ne_bytes());
        bytes[TYPE_OFFSET] = libc::RTM_IFINFO2 as u8;
        put(&mut bytes, FLAGS_OFFSET, &flags.to_ne_bytes());
        put(&mut bytes, RX_OFFSET, &rx.to_ne_bytes());
        put(&mut bytes, TX_OFFSET, &tx.to_ne_bytes());
        bytes[FAMILY_OFFSET] = libc::AF_LINK as u8;
        bytes[NLEN_OFFSET] = name.len() as u8;
        put(&mut bytes, NAME_OFFSET, name.as_bytes());
        bytes
    }

    fn address_record() -> Vec<u8> {
        let mut bytes = vec![0u8; 20];
        put(&mut bytes, 0, &20u16.to_ne_bytes());
        bytes[TYPE_OFFSET] = libc::RTM_NEWADDR as u8;
        bytes
    }

    fn counters(name: &str, rx: u64, tx: u64) -> IfCounters {
        IfCounters {
            name: name.into(),
            rx_bytes: rx,
            tx_bytes: tx,
        }
    }

    fn sample(state: MetricState<ThroughputSample>) -> ThroughputSample {
        match state {
            MetricState::Ok(sample) => sample,
            other => panic!("expected ok, got {other:?}"),
        }
    }

    #[test]
    fn only_wired_and_wifi_interfaces_are_counted() {
        for name in ["en0", "en1", "en12"] {
            assert!(is_counted(name), "{name}");
        }
        for name in [
            "en", "enx", "en0x", "utun0", "vmenet0", "bridge0", "awdl0", "lo0", "",
        ] {
            assert!(!is_counted(name), "{name}");
        }
    }

    #[test]
    fn parser_reads_up_interfaces_from_an_odd_offset() {
        let mut stream = vec![0xAAu8];
        stream.extend(record("en0", UP, 1_000, 2_000));
        stream.extend(address_record());
        stream.extend(record("utun0", UP, 5, 5));
        stream.extend(record("en1", libc::IFF_UP, 7, 7));
        stream.extend(record("en5", UP, u64::MAX, 3));
        let parsed = parse_iflist2(&stream[1..]);
        assert_eq!(
            parsed,
            vec![counters("en0", 1_000, 2_000), counters("en5", u64::MAX, 3)]
        );
    }

    #[test]
    fn parser_stops_on_truncated_or_zero_length_records() {
        let full = record("en0", UP, 1, 2);
        assert!(parse_iflist2(&full[..full.len() - 1]).is_empty());
        assert!(parse_iflist2(&full[..3]).is_empty());
        assert!(parse_iflist2(&[]).is_empty());
        let mut zero = full.clone();
        put(&mut zero, 0, &0u16.to_ne_bytes());
        zero.extend(record("en1", UP, 1, 2));
        assert!(parse_iflist2(&zero).is_empty());
        let mut short = full.clone();
        put(&mut short, 0, &(HEADER_LEN as u16 - 1).to_ne_bytes());
        let parsed = parse_iflist2(&short);
        assert!(parsed.is_empty());
    }

    #[test]
    fn parser_skips_names_outside_the_record() {
        let mut bad = record("en0", UP, 1, 2);
        let room = bad.len() - NAME_OFFSET;
        bad[NLEN_OFFSET] = u8::try_from(room + 1).unwrap();
        let mut stream = bad;
        stream.extend(record("en1", UP, 3, 4));
        assert_eq!(parse_iflist2(&stream), vec![counters("en1", 3, 4)]);
        let mut invalid = record("en0", UP, 1, 2);
        invalid[NAME_OFFSET] = 0xFF;
        assert!(parse_iflist2(&invalid).is_empty());
    }

    #[test]
    fn reader_returns_counters_from_the_kernel() {
        let mut reader = CounterReader::new();
        let first = reader.read().expect("sysctl NET_RT_IFLIST2");
        assert!(first.iter().all(|entry| is_counted(&entry.name)));
        let second = reader.read().expect("sysctl NET_RT_IFLIST2");
        assert!(second.iter().all(|entry| is_counted(&entry.name)));
    }

    #[test]
    fn first_update_is_loading_then_rates_follow_deltas() {
        let mut meter = ThroughputMeter::default();
        let start = Instant::now();
        let first = meter.update(&[counters("en0", 1_000, 500)], start);
        assert_eq!(first, MetricState::Loading);
        let next = meter.update(
            &[counters("en0", 5_000, 1_500), counters("en1", 10, 10)],
            start + Duration::from_secs(2),
        );
        assert_eq!(
            sample(next),
            ThroughputSample {
                rx_bytes_per_sec: 2_000.0,
                tx_bytes_per_sec: 500.0,
                interfaces: vec!["en0".into()],
            }
        );
        let both = meter.update(
            &[counters("en1", 30, 50), counters("en0", 5_000, 1_500)],
            start + Duration::from_secs(4),
        );
        assert_eq!(
            sample(both),
            ThroughputSample {
                rx_bytes_per_sec: 10.0,
                tx_bytes_per_sec: 20.0,
                interfaces: vec!["en0".into(), "en1".into()],
            }
        );
    }

    #[test]
    fn vanished_and_decreasing_interfaces_are_skipped() {
        let mut meter = ThroughputMeter::default();
        let start = Instant::now();
        meter.update(
            &[counters("en0", 100, 100), counters("en1", 100, 100)],
            start,
        );
        let next =
            sample(meter.update(&[counters("en0", 50, 300)], start + Duration::from_secs(1)));
        assert_eq!(next.rx_bytes_per_sec, 0.0);
        assert!(next.interfaces.is_empty());
        let after =
            sample(meter.update(&[counters("en0", 150, 400)], start + Duration::from_secs(2)));
        assert_eq!(after.rx_bytes_per_sec, 100.0);
        assert_eq!(after.interfaces, vec!["en0".to_owned()]);
    }

    #[test]
    fn extreme_counters_do_not_panic() {
        let mut meter = ThroughputMeter::default();
        let start = Instant::now();
        meter.update(&[counters("en0", 0, u64::MAX)], start);
        let next = sample(meter.update(
            &[counters("en0", u64::MAX, 0)],
            start + Duration::from_secs(2),
        ));
        assert!(next.interfaces.is_empty());
        let wrapped = sample(meter.update(
            &[counters("en0", u64::MAX, u64::MAX)],
            start + Duration::from_secs(4),
        ));
        assert!(wrapped.tx_bytes_per_sec.is_finite());
        assert_eq!(wrapped.rx_bytes_per_sec, 0.0);
    }

    #[test]
    fn short_or_zero_elapsed_keeps_loading_and_reset_drops_baseline() {
        let mut meter = ThroughputMeter::default();
        let start = Instant::now();
        meter.update(&[counters("en0", 0, 0)], start);
        assert_eq!(
            meter.update(&[counters("en0", 10, 10)], start),
            MetricState::Loading
        );
        assert_eq!(
            meter.update(
                &[counters("en0", 10, 10)],
                start + Duration::from_millis(400)
            ),
            MetricState::Loading
        );
        let later =
            sample(meter.update(&[counters("en0", 10, 10)], start + Duration::from_secs(1)));
        assert_eq!(later.rx_bytes_per_sec, 10.0);
        meter.reset();
        assert_eq!(
            meter.update(&[counters("en0", 20, 20)], start + Duration::from_secs(3)),
            MetricState::Loading
        );
    }
}
