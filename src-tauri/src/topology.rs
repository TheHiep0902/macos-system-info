use std::cmp::Reverse;
use std::ffi::c_char;
use std::iter;

use objc2_core_foundation::{CFData, CFRetained, CFString};
use objc2_io_kit::{
    IOIteratorNext, IOObjectRelease, IORegistryEntryCreateCFProperty, IORegistryEntryFromPath,
    IORegistryEntryGetChildIterator, io_object_t, kIOMainPortDefault,
};

use crate::model::ClusterInfo;
use crate::sys::{self, SysError};

const FALLBACK_LABEL: &str = "CPU";
const DEVICE_TREE_CPUS: &str = "IODeviceTree:/cpus";
const DEVICE_TREE_PLANE: &str = "IODeviceTree";
const LOGICAL_CPU_ID_KEY: &str = "logical-cpu-id";
const CLUSTER_TYPE_KEY: &str = "cluster-type";
const DIE_ID_KEY: &str = "die-id";
const MAX_PERF_LEVELS: u32 = 16;
const MAX_NAME_CHARS: usize = 32;
const KNOWN_TIERS: [(&str, &str); 3] = [("Efficiency", "E"), ("Performance", "P"), ("Super", "S")];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceTreeCpu {
    pub logical_id: u32,
    pub cluster_type: char,
    pub die_id: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerfLevel {
    pub index: u32,
    pub name: String,
    pub logical_cpus: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Topology {
    pub clusters: Vec<ClusterInfo>,
    pub core_cluster: Vec<usize>,
    pub assumed: bool,
}

impl Topology {
    pub fn single_cluster(logical_cpus: u32) -> Topology {
        Topology {
            clusters: vec![ClusterInfo {
                label: FALLBACK_LABEL.to_string(),
                name: FALLBACK_LABEL.to_string(),
                core_count: logical_cpus,
                max_mhz: None,
            }],
            core_cluster: vec![0; logical_cpus as usize],
            assumed: false,
        }
    }

    pub fn cluster_label(&self, cpu: usize) -> &str {
        self.core_cluster
            .get(cpu)
            .and_then(|&cluster| self.clusters.get(cluster))
            .map_or(FALLBACK_LABEL, |cluster| cluster.label.as_str())
    }
}

pub fn build_topology(
    device_tree: &[DeviceTreeCpu],
    perf_levels: &[PerfLevel],
    logical_cpus: u32,
) -> Topology {
    let levels = tiered_levels(perf_levels);
    if levels.len() < 2 {
        return Topology::single_cluster(logical_cpus);
    }
    let cpu_count = logical_cpus as usize;
    let (core_cluster, tier_types, assumed) =
        match device_tree_mapping(device_tree, &levels, cpu_count) {
            Some((core_cluster, tier_types)) => (core_cluster, Some(tier_types), false),
            None => (consecutive_mapping(&levels, cpu_count), None, true),
        };
    let clusters = levels
        .iter()
        .enumerate()
        .map(|(cluster, level)| {
            let label = tier_label(
                &level.name,
                tier_types
                    .as_ref()
                    .and_then(|types| types.get(cluster).copied()),
            );
            let core_count = core_cluster
                .iter()
                .filter(|&&owner| owner == cluster)
                .count();
            ClusterInfo {
                name: display_name(&level.name, &label),
                label,
                core_count: u32::try_from(core_count).unwrap_or(u32::MAX),
                max_mhz: None,
            }
        })
        .collect();
    Topology {
        clusters,
        core_cluster,
        assumed,
    }
}

pub fn read_device_tree_cpus() -> Result<Vec<DeviceTreeCpu>, SysError> {
    let mut path = c_buffer::<512>(DEVICE_TREE_CPUS)?;
    let mut plane = c_buffer::<128>(DEVICE_TREE_PLANE)?;
    let raw_entry = unsafe { IORegistryEntryFromPath(kIOMainPortDefault, &mut path) };
    let entry = IoObject::new(raw_entry).ok_or_else(|| {
        SysError::invalid(
            "IORegistryEntryFromPath",
            format!("{DEVICE_TREE_CPUS} not found"),
        )
    })?;
    let mut raw_iterator: io_object_t = 0;
    let kr = unsafe { IORegistryEntryGetChildIterator(entry.raw(), &mut plane, &mut raw_iterator) };
    if kr != 0 {
        return Err(SysError::kern("IORegistryEntryGetChildIterator", kr));
    }
    let iterator = IoObject::new(raw_iterator).ok_or_else(|| {
        SysError::invalid(
            "IORegistryEntryGetChildIterator",
            "returned a null iterator",
        )
    })?;
    let keys = PropertyKeys::new();
    let mut cpus = Vec::new();
    while let Some(child) = IoObject::new(IOIteratorNext(iterator.raw())) {
        if let Some(cpu) = read_cpu_node(&child, &keys)? {
            cpus.push(cpu);
        }
    }
    Ok(cpus)
}

pub fn read_perf_levels() -> Vec<PerfLevel> {
    match try_read_perf_levels() {
        Ok(levels) => levels,
        Err(err) => {
            log::debug!("hw.perflevel sysctls unavailable: {err}");
            Vec::new()
        }
    }
}

pub fn read_topology(logical_cpus: u32) -> Topology {
    let perf_levels = read_perf_levels();
    if tiered_levels(&perf_levels).len() < 2 {
        return Topology::single_cluster(logical_cpus);
    }
    let device_tree = read_device_tree_cpus().unwrap_or_else(|err| {
        log::debug!("device tree CPU list unavailable, assuming consecutive cluster order: {err}");
        Vec::new()
    });
    let topology = build_topology(&device_tree, &perf_levels, logical_cpus);
    if topology.assumed {
        log::debug!(
            "device tree ({} cpus) does not match hw.perflevel* ({} levels); cluster order assumed",
            device_tree.len(),
            perf_levels.len()
        );
    }
    topology
}

fn tiered_levels(perf_levels: &[PerfLevel]) -> Vec<&PerfLevel> {
    let mut levels: Vec<&PerfLevel> = perf_levels
        .iter()
        .filter(|level| level.logical_cpus > 0)
        .collect();
    levels.sort_by_key(|level| Reverse(level.index));
    levels
}

fn device_tree_mapping(
    device_tree: &[DeviceTreeCpu],
    levels: &[&PerfLevel],
    cpu_count: usize,
) -> Option<(Vec<usize>, Vec<char>)> {
    if device_tree.len() != cpu_count {
        return None;
    }
    let mut sorted: Vec<&DeviceTreeCpu> = device_tree.iter().collect();
    sorted.sort_by_key(|cpu| cpu.logical_id);
    let consecutive = sorted
        .iter()
        .enumerate()
        .all(|(position, cpu)| usize::try_from(cpu.logical_id).is_ok_and(|id| id == position));
    if !consecutive {
        return None;
    }
    let mut tier_types: Vec<char> = Vec::new();
    for cpu in &sorted {
        if !tier_types.contains(&cpu.cluster_type) {
            tier_types.push(cpu.cluster_type);
        }
    }
    if tier_types.len() != levels.len() {
        return None;
    }
    let core_cluster: Vec<usize> = sorted
        .iter()
        .map(|cpu| tier_types.iter().position(|&kind| kind == cpu.cluster_type))
        .collect::<Option<_>>()?;
    let counts_match = levels.iter().enumerate().all(|(cluster, level)| {
        let count = core_cluster
            .iter()
            .filter(|&&owner| owner == cluster)
            .count();
        u32::try_from(count).is_ok_and(|count| count == level.logical_cpus)
    });
    counts_match.then_some((core_cluster, tier_types))
}

fn consecutive_mapping(levels: &[&PerfLevel], cpu_count: usize) -> Vec<usize> {
    let mut mapping = Vec::with_capacity(cpu_count);
    for (cluster, level) in levels.iter().enumerate() {
        let remaining = cpu_count - mapping.len();
        let take = usize::try_from(level.logical_cpus).map_or(remaining, |n| n.min(remaining));
        mapping.extend(iter::repeat_n(cluster, take));
    }
    mapping.resize(cpu_count, levels.len().saturating_sub(1));
    mapping
}

fn tier_label(name: &str, device_tree_type: Option<char>) -> String {
    let trimmed = name.trim();
    if let Some((_, label)) = KNOWN_TIERS
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(trimmed))
    {
        return (*label).to_string();
    }
    if let Some(kind) = device_tree_type {
        return kind.to_string();
    }
    trimmed
        .chars()
        .find(char::is_ascii_uppercase)
        .or_else(|| {
            trimmed
                .chars()
                .find(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_uppercase())
        })
        .map_or_else(|| FALLBACK_LABEL.to_string(), String::from)
}

fn display_name(name: &str, label: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_CHARS)
        .collect();
    if cleaned.is_empty() {
        label.to_string()
    } else {
        cleaned
    }
}

fn try_read_perf_levels() -> Result<Vec<PerfLevel>, SysError> {
    let count = sys::sysctl_u32("hw.nperflevels")?;
    if count > MAX_PERF_LEVELS {
        return Err(SysError::invalid(
            "sysctlbyname",
            format!("hw.nperflevels = {count} exceeds {MAX_PERF_LEVELS}"),
        ));
    }
    (0..count)
        .map(|index| {
            Ok(PerfLevel {
                index,
                name: sys::sysctl_string(&format!("hw.perflevel{index}.name"))?,
                logical_cpus: sys::sysctl_u32(&format!("hw.perflevel{index}.logicalcpu"))?,
            })
        })
        .collect()
}

fn c_buffer<const N: usize>(value: &str) -> Result<[c_char; N], SysError> {
    let bytes = value.as_bytes();
    if bytes.len() >= N || bytes.contains(&0) {
        return Err(SysError::invalid(
            "IORegistry",
            format!("{value:?} does not fit a {N}-byte C string"),
        ));
    }
    let mut buffer = [0 as c_char; N];
    for (slot, &byte) in buffer.iter_mut().zip(bytes) {
        *slot = byte as c_char;
    }
    Ok(buffer)
}

struct IoObject(io_object_t);

impl IoObject {
    fn new(raw: io_object_t) -> Option<IoObject> {
        (raw != 0).then_some(IoObject(raw))
    }

    fn raw(&self) -> io_object_t {
        self.0
    }
}

impl Drop for IoObject {
    fn drop(&mut self) {
        let kr = IOObjectRelease(self.0);
        if kr != 0 {
            log::debug!("IOObjectRelease failed with kern_return_t {kr}");
        }
    }
}

struct PropertyKeys {
    logical_id: CFRetained<CFString>,
    cluster_type: CFRetained<CFString>,
    die_id: CFRetained<CFString>,
}

impl PropertyKeys {
    fn new() -> PropertyKeys {
        PropertyKeys {
            logical_id: CFString::from_str(LOGICAL_CPU_ID_KEY),
            cluster_type: CFString::from_str(CLUSTER_TYPE_KEY),
            die_id: CFString::from_str(DIE_ID_KEY),
        }
    }
}

fn read_cpu_node(node: &IoObject, keys: &PropertyKeys) -> Result<Option<DeviceTreeCpu>, SysError> {
    let logical_id = data_property(node, &keys.logical_id, LOGICAL_CPU_ID_KEY)?;
    let cluster_type = data_property(node, &keys.cluster_type, CLUSTER_TYPE_KEY)?;
    let die_id = data_property(node, &keys.die_id, DIE_ID_KEY)?;
    parse_cpu_node(
        logical_id.as_deref(),
        cluster_type.as_deref(),
        die_id.as_deref(),
    )
    .map_err(|detail| SysError::invalid(DEVICE_TREE_CPUS, detail))
}

fn data_property(node: &IoObject, key: &CFString, name: &str) -> Result<Option<Vec<u8>>, SysError> {
    let Some(value) = (unsafe { IORegistryEntryCreateCFProperty(node.raw(), Some(key), None, 0) })
    else {
        return Ok(None);
    };
    value
        .downcast::<CFData>()
        .map(|data| Some(data.to_vec()))
        .map_err(|_| SysError::invalid(DEVICE_TREE_CPUS, format!("{name} is not CFData")))
}

fn parse_cpu_node(
    logical_id: Option<&[u8]>,
    cluster_type: Option<&[u8]>,
    die_id: Option<&[u8]>,
) -> Result<Option<DeviceTreeCpu>, String> {
    let Some(logical_id) = logical_id else {
        return Ok(None);
    };
    let logical_id = parse_u32_le(logical_id).ok_or_else(|| {
        format!(
            "{LOGICAL_CPU_ID_KEY} has invalid length {}",
            logical_id.len()
        )
    })?;
    let cluster_bytes =
        cluster_type.ok_or_else(|| format!("cpu {logical_id} has no {CLUSTER_TYPE_KEY}"))?;
    let cluster_type = parse_cluster_type(cluster_bytes).ok_or_else(|| {
        format!("cpu {logical_id} has invalid {CLUSTER_TYPE_KEY} {cluster_bytes:?}")
    })?;
    let die_id = match die_id {
        None => 0,
        Some(bytes) => parse_u32_le(bytes).ok_or_else(|| {
            format!(
                "cpu {logical_id} has invalid {DIE_ID_KEY} length {}",
                bytes.len()
            )
        })?,
    };
    Ok(Some(DeviceTreeCpu {
        logical_id,
        cluster_type,
        die_id,
    }))
}

fn parse_u32_le(bytes: &[u8]) -> Option<u32> {
    match *bytes {
        [a, b, c, d] => Some(u32::from_le_bytes([a, b, c, d])),
        [a, b, c, d, e, f, g, h] => {
            u32::try_from(u64::from_le_bytes([a, b, c, d, e, f, g, h])).ok()
        }
        _ => None,
    }
}

fn parse_cluster_type(bytes: &[u8]) -> Option<char> {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    match bytes[..end] {
        [byte] if byte.is_ascii_alphabetic() => Some(char::from(byte.to_ascii_uppercase())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(index: u32, name: &str, logical_cpus: u32) -> PerfLevel {
        PerfLevel {
            index,
            name: name.to_string(),
            logical_cpus,
        }
    }

    fn device_tree(types: &str) -> Vec<DeviceTreeCpu> {
        types
            .chars()
            .enumerate()
            .map(|(id, cluster_type)| DeviceTreeCpu {
                logical_id: id as u32,
                cluster_type,
                die_id: 0,
            })
            .collect()
    }

    fn labels(topology: &Topology) -> Vec<&str> {
        topology.clusters.iter().map(|c| c.label.as_str()).collect()
    }

    fn names(topology: &Topology) -> Vec<&str> {
        topology.clusters.iter().map(|c| c.name.as_str()).collect()
    }

    fn counts(topology: &Topology) -> Vec<u32> {
        topology.clusters.iter().map(|c| c.core_count).collect()
    }

    fn runs(spec: &[(usize, usize)]) -> Vec<usize> {
        spec.iter()
            .flat_map(|&(cluster, n)| iter::repeat_n(cluster, n))
            .collect()
    }

    #[test]
    fn m1_four_efficiency_four_performance() {
        let levels = [level(0, "Performance", 4), level(1, "Efficiency", 4)];
        let topology = build_topology(&device_tree("EEEEPPPP"), &levels, 8);
        assert_eq!(labels(&topology), ["E", "P"]);
        assert_eq!(names(&topology), ["Efficiency", "Performance"]);
        assert_eq!(counts(&topology), [4, 4]);
        assert_eq!(topology.core_cluster, runs(&[(0, 4), (1, 4)]));
        assert!(!topology.assumed);
        assert!(topology.clusters.iter().all(|c| c.max_mhz.is_none()));
    }

    #[test]
    fn m4_max_four_efficiency_twelve_performance() {
        let levels = [level(0, "Performance", 12), level(1, "Efficiency", 4)];
        let topology = build_topology(&device_tree("EEEEPPPPPPPPPPPP"), &levels, 16);
        assert_eq!(labels(&topology), ["E", "P"]);
        assert_eq!(counts(&topology), [4, 12]);
        assert_eq!(topology.core_cluster, runs(&[(0, 4), (1, 12)]));
        assert!(!topology.assumed);
        assert_eq!(topology.cluster_label(3), "E");
        assert_eq!(topology.cluster_label(4), "P");
    }

    #[test]
    fn m5_pro_super_and_performance_use_perflevel_names_not_device_tree_letters() {
        let levels = [level(0, "Super", 6), level(1, "Performance", 12)];
        let topology = build_topology(&device_tree("MMMMMMMMMMMMPPPPPP"), &levels, 18);
        assert_eq!(labels(&topology), ["P", "S"]);
        assert_eq!(names(&topology), ["Performance", "Super"]);
        assert_eq!(counts(&topology), [12, 6]);
        assert_eq!(topology.core_cluster, runs(&[(0, 12), (1, 6)]));
        assert!(!topology.assumed);
        assert_eq!(topology.cluster_label(11), "P");
        assert_eq!(topology.cluster_label(12), "S");
    }

    #[test]
    fn base_m5_six_efficiency_four_super() {
        let levels = [level(0, "Super", 4), level(1, "Efficiency", 6)];
        let topology = build_topology(&device_tree("EEEEEEPPPP"), &levels, 10);
        assert_eq!(labels(&topology), ["E", "S"]);
        assert_eq!(counts(&topology), [6, 4]);
        assert_eq!(topology.core_cluster, runs(&[(0, 6), (1, 4)]));
        assert!(!topology.assumed);
    }

    #[test]
    fn intel_single_standard_level_is_one_cpu_cluster() {
        let topology = build_topology(&[], &[level(0, "Standard", 8)], 8);
        assert_eq!(topology, Topology::single_cluster(8));
        assert_eq!(labels(&topology), ["CPU"]);
        assert_eq!(names(&topology), ["CPU"]);
        assert_eq!(counts(&topology), [8]);
        assert_eq!(topology.core_cluster, vec![0; 8]);
        assert!(!topology.assumed);
    }

    #[test]
    fn no_perf_levels_is_one_cpu_cluster() {
        let topology = build_topology(&device_tree("EEPP"), &[], 4);
        assert_eq!(topology, Topology::single_cluster(4));
    }

    #[test]
    fn mismatched_device_tree_counts_fall_back_to_consecutive_order() {
        let levels = [level(0, "Performance", 4), level(1, "Efficiency", 4)];
        let topology = build_topology(&device_tree("EEEEEPPP"), &levels, 8);
        assert_eq!(labels(&topology), ["E", "P"]);
        assert_eq!(topology.core_cluster, runs(&[(0, 4), (1, 4)]));
        assert_eq!(counts(&topology), [4, 4]);
        assert!(topology.assumed);
    }

    #[test]
    fn device_tree_with_extra_tier_falls_back() {
        let levels = [level(0, "Performance", 4), level(1, "Efficiency", 4)];
        let topology = build_topology(&device_tree("EEEEMMPP"), &levels, 8);
        assert!(topology.assumed);
        assert_eq!(topology.core_cluster, runs(&[(0, 4), (1, 4)]));
    }

    #[test]
    fn empty_device_tree_falls_back_to_consecutive_order() {
        let levels = [level(0, "Performance", 12), level(1, "Efficiency", 4)];
        let topology = build_topology(&[], &levels, 16);
        assert_eq!(labels(&topology), ["E", "P"]);
        assert_eq!(topology.core_cluster, runs(&[(0, 4), (1, 12)]));
        assert!(topology.assumed);
    }

    #[test]
    fn duplicate_or_gapped_logical_ids_fall_back() {
        let levels = [level(0, "Performance", 2), level(1, "Efficiency", 2)];
        let mut duplicated = device_tree("EEPP");
        duplicated[1].logical_id = 0;
        assert!(build_topology(&duplicated, &levels, 4).assumed);
        let mut gapped = device_tree("EEPP");
        gapped[3].logical_id = 7;
        assert!(build_topology(&gapped, &levels, 4).assumed);
    }

    #[test]
    fn device_tree_order_is_independent_of_node_order_and_may_interleave() {
        let levels = [level(0, "Performance", 2), level(1, "Efficiency", 2)];
        let mut cpus = device_tree("EPEP");
        cpus.reverse();
        let topology = build_topology(&cpus, &levels, 4);
        assert!(!topology.assumed);
        assert_eq!(topology.core_cluster, [0, 1, 0, 1]);
        assert_eq!(counts(&topology), [2, 2]);
    }

    #[test]
    fn logical_cpus_above_level_sum_extends_highest_cluster() {
        let levels = [level(0, "Performance", 4), level(1, "Efficiency", 4)];
        let topology = build_topology(&device_tree("EEEEPPPPPP"), &levels, 10);
        assert!(topology.assumed);
        assert_eq!(topology.core_cluster, runs(&[(0, 4), (1, 6)]));
        assert_eq!(counts(&topology), [4, 6]);
    }

    #[test]
    fn logical_cpus_below_level_sum_truncates_highest_cluster() {
        let levels = [level(0, "Performance", 4), level(1, "Efficiency", 4)];
        let topology = build_topology(&[], &levels, 6);
        assert!(topology.assumed);
        assert_eq!(topology.core_cluster, runs(&[(0, 4), (1, 2)]));
        assert_eq!(counts(&topology), [4, 2]);
        let tiny = build_topology(&[], &levels, 3);
        assert_eq!(tiny.core_cluster, [0, 0, 0]);
        assert_eq!(counts(&tiny), [3, 0]);
    }

    #[test]
    fn huge_untrusted_level_count_is_bounded_by_logical_cpus() {
        let levels = [
            level(0, "Performance", u32::MAX),
            level(1, "Efficiency", u32::MAX),
        ];
        let topology = build_topology(&[], &levels, 4);
        assert_eq!(topology.core_cluster, [0, 0, 0, 0]);
        assert!(topology.assumed);
    }

    #[test]
    fn perf_levels_are_ordered_by_index_regardless_of_input_order() {
        let levels = [level(1, "Efficiency", 2), level(0, "Performance", 2)];
        let topology = build_topology(&device_tree("EEPP"), &levels, 4);
        assert_eq!(labels(&topology), ["E", "P"]);
        assert!(!topology.assumed);
    }

    #[test]
    fn zero_cpu_levels_are_ignored() {
        let levels = [level(0, "Performance", 4), level(1, "Efficiency", 0)];
        assert_eq!(build_topology(&[], &levels, 4), Topology::single_cluster(4));
        let three = [
            level(0, "Super", 2),
            level(1, "Performance", 0),
            level(2, "Efficiency", 2),
        ];
        let topology = build_topology(&device_tree("EESS"), &three, 4);
        assert_eq!(labels(&topology), ["E", "S"]);
        assert!(!topology.assumed);
    }

    #[test]
    fn unknown_level_names_use_device_tree_letter_then_name_initial() {
        let levels = [level(0, "Turbo", 2), level(1, "Mid", 2)];
        let with_tree = build_topology(&device_tree("MMTT"), &levels, 4);
        assert_eq!(labels(&with_tree), ["M", "T"]);
        assert_eq!(names(&with_tree), ["Mid", "Turbo"]);
        let without_tree = build_topology(&[], &levels, 4);
        assert_eq!(labels(&without_tree), ["M", "T"]);
        assert!(without_tree.assumed);
    }

    #[test]
    fn level_names_are_matched_case_insensitively_and_sanitised() {
        assert_eq!(tier_label(" efficiency ", None), "E");
        assert_eq!(tier_label("SUPER", Some('P')), "S");
        assert_eq!(tier_label("bigCore", None), "C");
        assert_eq!(tier_label("tier2", None), "T");
        assert_eq!(tier_label("  ", None), "CPU");
        assert_eq!(display_name("  ", "E"), "E");
        assert_eq!(display_name("Per\u{7}formance\n", "P"), "Performance");
        assert_eq!(
            display_name(&"x".repeat(100), "X").chars().count(),
            MAX_NAME_CHARS
        );
    }

    #[test]
    fn cluster_label_falls_back_out_of_range() {
        let topology = Topology::single_cluster(2);
        assert_eq!(topology.cluster_label(0), "CPU");
        assert_eq!(topology.cluster_label(99), "CPU");
        let broken = Topology {
            clusters: Vec::new(),
            core_cluster: vec![3],
            assumed: true,
        };
        assert_eq!(broken.cluster_label(0), "CPU");
    }

    #[test]
    fn single_cluster_with_zero_cpus_is_empty_mapping() {
        let topology = Topology::single_cluster(0);
        assert!(topology.core_cluster.is_empty());
        assert_eq!(counts(&topology), [0]);
    }

    #[test]
    fn parses_device_tree_scalars() {
        assert_eq!(parse_u32_le(&[3, 0, 0, 0]), Some(3));
        assert_eq!(parse_u32_le(&[1, 1, 0, 0, 0, 0, 0, 0]), Some(257));
        assert_eq!(parse_u32_le(&[0, 0, 0, 0, 1, 0, 0, 0]), None);
        assert_eq!(parse_u32_le(&[1, 0]), None);
        assert_eq!(parse_u32_le(&[]), None);
    }

    #[test]
    fn parses_cluster_type_with_optional_nul() {
        assert_eq!(parse_cluster_type(b"E"), Some('E'));
        assert_eq!(parse_cluster_type(b"P\0"), Some('P'));
        assert_eq!(parse_cluster_type(b"M\0\0\0"), Some('M'));
        assert_eq!(parse_cluster_type(b"e"), Some('E'));
        assert_eq!(parse_cluster_type(b""), None);
        assert_eq!(parse_cluster_type(b"\0"), None);
        assert_eq!(parse_cluster_type(b"EP"), None);
        assert_eq!(parse_cluster_type(b"1"), None);
        assert_eq!(parse_cluster_type(&[0xff]), None);
    }

    #[test]
    fn parses_cpu_node_properties() {
        let id = 5u32.to_le_bytes();
        let die = 1u32.to_le_bytes();
        assert_eq!(parse_cpu_node(None, Some(b"E"), None), Ok(None));
        assert_eq!(
            parse_cpu_node(Some(&id), Some(b"P\0"), Some(&die)),
            Ok(Some(DeviceTreeCpu {
                logical_id: 5,
                cluster_type: 'P',
                die_id: 1,
            }))
        );
        assert_eq!(
            parse_cpu_node(Some(&id), Some(b"E"), None).map(|cpu| cpu.map(|c| c.die_id)),
            Ok(Some(0))
        );
        assert!(parse_cpu_node(Some(&[1, 2, 3]), Some(b"E"), None).is_err());
        assert!(parse_cpu_node(Some(&id), None, None).is_err());
        assert!(parse_cpu_node(Some(&id), Some(b"EE"), None).is_err());
        assert!(parse_cpu_node(Some(&id), Some(b"E"), Some(&[1])).is_err());
    }

    #[test]
    fn c_buffer_rejects_oversized_or_nul_strings() {
        let buffer = c_buffer::<8>("abc").unwrap();
        assert_eq!(
            &buffer[..4],
            &[b'a' as c_char, b'b' as c_char, b'c' as c_char, 0]
        );
        assert!(c_buffer::<4>("abcd").is_err());
        assert!(c_buffer::<8>("a\0b").is_err());
    }

    #[test]
    fn host_device_tree_read_is_well_formed() {
        if let Ok(cpus) = read_device_tree_cpus() {
            let logical = sys::sysctl_u32("hw.logicalcpu").unwrap();
            assert!(cpus.iter().all(|cpu| cpu.logical_id < logical));
            assert!(cpus.iter().all(|cpu| cpu.cluster_type.is_ascii_uppercase()));
        }
    }

    #[test]
    fn host_perf_levels_are_indexed_consecutively() {
        let levels = read_perf_levels();
        assert!(
            levels
                .iter()
                .enumerate()
                .all(|(i, level)| level.index as usize == i)
        );
    }

    #[test]
    fn host_topology_covers_every_logical_cpu() {
        let logical = sys::sysctl_u32("hw.logicalcpu").unwrap();
        let topology = read_topology(logical);
        assert_eq!(topology.core_cluster.len(), logical as usize);
        assert!(
            topology
                .core_cluster
                .iter()
                .all(|&cluster| cluster < topology.clusters.len())
        );
        let total: u32 = topology.clusters.iter().map(|c| c.core_count).sum();
        assert_eq!(total, logical);
    }
}
