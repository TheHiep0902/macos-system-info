use std::collections::hash_map::Entry;
use std::collections::{BTreeSet, HashMap};
use std::mem::{size_of, transmute};
use std::ptr;
use std::time::{Duration, Instant};

use objc2::rc::autoreleasepool;
use objc2_foundation::{
    NSArray, NSNumber, NSString, NSURL, NSURLResourceKey,
    NSURLVolumeAvailableCapacityForImportantUsageKey, NSURLVolumeIsEjectableKey,
    NSURLVolumeIsInternalKey, NSURLVolumeIsRemovableKey, NSURLVolumeLocalizedNameKey,
};

use crate::model::DiskSample;
use crate::sys::SysError;

const MNT_REMOVABLE: u32 = 0x0000_0200;
const MNT_LOCAL: u32 = libc::MNT_LOCAL as u32;
const MNT_DONTBROWSE: u32 = libc::MNT_DONTBROWSE as u32;
const MNT_SNAPSHOT: u32 = libc::MNT_SNAPSHOT as u32;
const NETWORK_FS_TYPES: [&str; 4] = ["smbfs", "nfs", "afpfs", "webdav"];
const EXCLUDED_FS_TYPES: [&str; 3] = ["devfs", "autofs", "nullfs"];
const HIDDEN_MARKERS: [&str; 3] = ["CoreSimulator", "com.apple.TimeMachine", ".timemachine"];
const MOUNT_SLACK: usize = 4;
const BOOT_MOUNT: &str = "/";
const IMPORTANT_REFRESH: Duration = Duration::from_secs(30);

type Fsid = [i32; 2];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiskScanOptions {
    pub include_network: bool,
    pub query_important_capacity: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiskScan {
    pub disks: Vec<DiskSample>,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountEntry {
    pub mount_point: String,
    pub mounted_from: String,
    pub fs_type: String,
    pub flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountClass {
    Boot,
    Local,
    Network,
}

fn is_hidden_location(entry: &MountEntry) -> bool {
    let mount_point = entry.mount_point.as_str();
    mount_point.starts_with("/System/Volumes/")
        || mount_point.starts_with("/private/var/folders/")
        || HIDDEN_MARKERS
            .iter()
            .any(|marker| mount_point.contains(marker) || entry.mounted_from.contains(marker))
}

pub fn classify_mount(entry: &MountEntry, include_network: bool) -> Option<MountClass> {
    let fs_type = entry.fs_type.as_str();
    if EXCLUDED_FS_TYPES.contains(&fs_type) {
        return None;
    }
    if entry.mount_point == BOOT_MOUNT {
        return Some(MountClass::Boot);
    }
    if entry.flags & (MNT_DONTBROWSE | MNT_SNAPSHOT) != 0 || is_hidden_location(entry) {
        return None;
    }
    if !entry.mount_point.starts_with("/Volumes/") {
        return None;
    }
    if NETWORK_FS_TYPES.contains(&fs_type) {
        return include_network.then_some(MountClass::Network);
    }
    Some(MountClass::Local)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Usage {
    total: u64,
    available: u64,
    used: u64,
    used_pct: f32,
}

fn usage_from_blocks(blocks: u64, available_blocks: u64, block_size: u32) -> Usage {
    let block_size = u64::from(block_size);
    let total = blocks.saturating_mul(block_size);
    let available = available_blocks.saturating_mul(block_size);
    let used = total.saturating_sub(available);
    let used_pct = if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0) as f32
    };
    Usage {
        total,
        available,
        used,
        used_pct,
    }
}

fn important_due(last_query: Option<Instant>, now: Instant) -> bool {
    last_query.is_none_or(|at| now.saturating_duration_since(at) >= IMPORTANT_REFRESH)
}

fn important_or_available(important: Option<i64>, available: u64) -> Option<u64> {
    match important.map(u64::try_from) {
        Some(Ok(0)) => Some(available),
        Some(Ok(bytes)) => Some(bytes),
        Some(Err(_)) | None => None,
    }
}

fn fsids_changed(previous: Option<&BTreeSet<Fsid>>, current: &BTreeSet<Fsid>) -> bool {
    previous.is_some_and(|previous| previous != current)
}

fn removable_from(
    flags: u32,
    ejectable: Option<bool>,
    removable: Option<bool>,
    internal: Option<bool>,
) -> bool {
    flags & MNT_REMOVABLE != 0
        || ejectable == Some(true)
        || removable == Some(true)
        || internal == Some(false)
}

fn fallback_name(mount_point: &str) -> String {
    mount_point
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(mount_point)
        .to_string()
}

fn disk_id(fsid: Fsid) -> String {
    format!("fsid-{}-{}", fsid[0], fsid[1])
}

fn c_chars_to_string(chars: &[libc::c_char]) -> String {
    let bytes: Vec<u8> = chars
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| u8::from_ne_bytes(c.to_ne_bytes()))
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn fsid_of(fs: &libc::statfs) -> Fsid {
    unsafe { transmute::<libc::fsid_t, Fsid>(fs.f_fsid) }
}

fn mount_entry(fs: &libc::statfs) -> MountEntry {
    MountEntry {
        mount_point: c_chars_to_string(&fs.f_mntonname),
        mounted_from: c_chars_to_string(&fs.f_mntfromname),
        fs_type: c_chars_to_string(&fs.f_fstypename),
        flags: fs.f_flags,
    }
}

fn mount_count() -> Result<usize, SysError> {
    let count = unsafe { libc::getfsstat(ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    if count < 0 {
        return Err(SysError::last_os("getfsstat"));
    }
    Ok(count as usize)
}

fn reserve_mounts(buf: &mut Vec<libc::statfs>, count: usize) {
    buf.clear();
    buf.reserve_exact(count + MOUNT_SLACK);
}

fn fill_mounts(buf: &mut Vec<libc::statfs>) -> Result<usize, SysError> {
    buf.clear();
    let capacity = buf.capacity();
    let bytes = capacity
        .checked_mul(size_of::<libc::statfs>())
        .and_then(|bytes| libc::c_int::try_from(bytes).ok())
        .ok_or_else(|| SysError::invalid("getfsstat", "mount buffer too large"))?;
    let count = unsafe { libc::getfsstat(buf.as_mut_ptr(), bytes, libc::MNT_NOWAIT) };
    if count < 0 {
        return Err(SysError::last_os("getfsstat"));
    }
    let count = (count as usize).min(capacity);
    unsafe { buf.set_len(count) };
    Ok(count)
}

fn read_mounts(buf: &mut Vec<libc::statfs>) -> Result<(), SysError> {
    if buf.capacity() == 0 {
        reserve_mounts(buf, mount_count()?);
    }
    if fill_mounts(buf)? < buf.capacity() {
        return Ok(());
    }
    reserve_mounts(buf, mount_count()?);
    fill_mounts(buf)?;
    Ok(())
}

#[derive(Debug, Default)]
struct VolumeResources {
    name: Option<String>,
    ejectable: Option<bool>,
    removable: Option<bool>,
    internal: Option<bool>,
    important_bytes: Option<i64>,
}

fn read_volume_resources(
    mount_point: &str,
    identity: bool,
    important: bool,
) -> Result<VolumeResources, SysError> {
    let name_key: &NSURLResourceKey = unsafe { NSURLVolumeLocalizedNameKey };
    let ejectable_key: &NSURLResourceKey = unsafe { NSURLVolumeIsEjectableKey };
    let removable_key: &NSURLResourceKey = unsafe { NSURLVolumeIsRemovableKey };
    let internal_key: &NSURLResourceKey = unsafe { NSURLVolumeIsInternalKey };
    let important_key: &NSURLResourceKey =
        unsafe { NSURLVolumeAvailableCapacityForImportantUsageKey };
    autoreleasepool(|_| {
        let mut keys: Vec<&NSURLResourceKey> = Vec::with_capacity(5);
        if identity {
            keys.extend([name_key, ejectable_key, removable_key, internal_key]);
        }
        if important {
            keys.push(important_key);
        }
        let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(mount_point), true);
        let values = url
            .resourceValuesForKeys_error(&NSArray::from_slice(&keys))
            .map_err(|err| {
                SysError::invalid(
                    "NSURL resourceValuesForKeys",
                    format!("{} code {}", err.domain(), err.code()),
                )
            })?;
        let number = |key: &NSURLResourceKey| {
            values
                .objectForKey(key)
                .and_then(|value| value.downcast::<NSNumber>().ok())
        };
        let flag = |key: &NSURLResourceKey| {
            if identity {
                number(key).map(|value| value.boolValue())
            } else {
                None
            }
        };
        let name = if identity {
            values
                .objectForKey(name_key)
                .and_then(|value| value.downcast::<NSString>().ok())
                .map(|value| value.to_string())
        } else {
            None
        };
        Ok(VolumeResources {
            name,
            ejectable: flag(ejectable_key),
            removable: flag(removable_key),
            internal: flag(internal_key),
            important_bytes: if important {
                number(important_key).map(|value| value.longLongValue())
            } else {
                None
            },
        })
    })
}

#[derive(Debug, Clone)]
struct VolumeIdentity {
    mount_point: String,
    name: String,
    removable: bool,
}

fn read_identity(fsid: Fsid, entry: &MountEntry) -> VolumeIdentity {
    let fallback = VolumeIdentity {
        mount_point: entry.mount_point.clone(),
        name: fallback_name(&entry.mount_point),
        removable: removable_from(entry.flags, None, None, None),
    };
    if entry.flags & MNT_LOCAL == 0 {
        return fallback;
    }
    match read_volume_resources(&entry.mount_point, true, false) {
        Ok(resources) => VolumeIdentity {
            removable: removable_from(
                entry.flags,
                resources.ejectable,
                resources.removable,
                resources.internal,
            ),
            name: resources
                .name
                .filter(|name| !name.is_empty())
                .unwrap_or(fallback.name),
            mount_point: fallback.mount_point,
        },
        Err(err) => {
            log::debug!(
                "volume identity for {} unavailable, using mount path: {err}",
                disk_id(fsid)
            );
            fallback
        }
    }
}

fn identity_for<'a>(
    cache: &'a mut HashMap<Fsid, VolumeIdentity>,
    fsid: Fsid,
    entry: &MountEntry,
) -> &'a VolumeIdentity {
    match cache.entry(fsid) {
        Entry::Occupied(mut occupied) => {
            if occupied.get().mount_point != entry.mount_point {
                occupied.insert(read_identity(fsid, entry));
            }
            occupied.into_mut()
        }
        Entry::Vacant(vacant) => vacant.insert(read_identity(fsid, entry)),
    }
}

pub struct DiskScanner {
    mounts: Vec<libc::statfs>,
    identities: HashMap<Fsid, VolumeIdentity>,
    previous: Option<BTreeSet<Fsid>>,
    boot_important: Option<u64>,
    important_at: Option<Instant>,
    important_error_logged: bool,
}

impl DiskScanner {
    pub fn new() -> Self {
        DiskScanner {
            mounts: Vec::new(),
            identities: HashMap::new(),
            previous: None,
            boot_important: None,
            important_at: None,
            important_error_logged: false,
        }
    }

    pub fn scan(&mut self, options: DiskScanOptions) -> Result<DiskScan, SysError> {
        read_mounts(&mut self.mounts)?;
        let mut boot: Option<DiskSample> = None;
        let mut others: Vec<DiskSample> = Vec::new();
        let mut current = BTreeSet::new();
        for fs in &self.mounts {
            let entry = mount_entry(fs);
            let Some(class) = classify_mount(&entry, options.include_network) else {
                continue;
            };
            let fsid = fsid_of(fs);
            if !current.insert(fsid) {
                continue;
            }
            let (name, is_removable) = match class {
                MountClass::Network => (fallback_name(&entry.mount_point), false),
                MountClass::Boot | MountClass::Local => {
                    let identity = identity_for(&mut self.identities, fsid, &entry);
                    (identity.name.clone(), identity.removable)
                }
            };
            let usage = usage_from_blocks(fs.f_blocks, fs.f_bavail, fs.f_bsize);
            let sample = DiskSample {
                id: disk_id(fsid),
                name,
                mount_point: entry.mount_point,
                fs_type: entry.fs_type,
                is_boot: class == MountClass::Boot,
                is_removable,
                is_network: class == MountClass::Network,
                total_bytes: usage.total,
                available_bytes: usage.available,
                available_important_bytes: None,
                used_bytes: usage.used,
                used_pct: usage.used_pct,
            };
            if sample.is_boot && boot.is_none() {
                boot = Some(sample);
            } else {
                others.push(sample);
            }
        }
        self.identities.retain(|fsid, _| current.contains(fsid));
        if let Some(boot) = boot.as_mut() {
            boot.available_important_bytes = self
                .boot_important_capacity(options.query_important_capacity, boot.available_bytes);
        }
        others.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
        let changed = fsids_changed(self.previous.as_ref(), &current);
        self.previous = Some(current);
        let mut disks = Vec::with_capacity(others.len() + 1);
        disks.extend(boot);
        disks.extend(others);
        Ok(DiskScan { disks, changed })
    }

    fn boot_important_capacity(&mut self, query: bool, available: u64) -> Option<u64> {
        let now = Instant::now();
        if !query || !important_due(self.important_at, now) {
            return self.boot_important;
        }
        self.important_at = Some(now);
        match read_volume_resources(BOOT_MOUNT, false, true) {
            Ok(resources) => {
                self.important_error_logged = false;
                self.boot_important = important_or_available(resources.important_bytes, available);
                self.boot_important
            }
            Err(err) => {
                if !self.important_error_logged {
                    self.important_error_logged = true;
                    log::debug!("boot important-usage capacity unavailable: {err}");
                }
                self.boot_important = None;
                None
            }
        }
    }
}

impl Default for DiskScanner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(mount_point: &str, mounted_from: &str, fs_type: &str, flags: u32) -> MountEntry {
        MountEntry {
            mount_point: mount_point.into(),
            mounted_from: mounted_from.into(),
            fs_type: fs_type.into(),
            flags,
        }
    }

    #[test]
    fn classify_mount_table() {
        let local = MNT_LOCAL;
        let cases = [
            (
                entry("/", "/dev/disk3s1s1", "apfs", local),
                false,
                Some(MountClass::Boot),
            ),
            (
                entry("/", "/dev/disk3s1s1", "apfs", local | MNT_SNAPSHOT),
                false,
                Some(MountClass::Boot),
            ),
            (
                entry(
                    "/Volumes/USB",
                    "/dev/disk4s1",
                    "msdos",
                    local | MNT_REMOVABLE,
                ),
                false,
                Some(MountClass::Local),
            ),
            (
                entry("/Volumes/Windows 11", "/dev/disk0s2", "ntfs", local),
                false,
                Some(MountClass::Local),
            ),
            (
                entry("/Volumes/Share", "//u@nas/share", "smbfs", 0),
                false,
                None,
            ),
            (
                entry("/Volumes/Share", "//u@nas/share", "smbfs", 0),
                true,
                Some(MountClass::Network),
            ),
            (
                entry("/Volumes/export", "nas:/export", "nfs", 0),
                false,
                None,
            ),
            (
                entry("/Volumes/export", "nas:/export", "nfs", 0),
                true,
                Some(MountClass::Network),
            ),
            (
                entry("/Volumes/Old", "afp_x@nas/Old", "afpfs", 0),
                true,
                Some(MountClass::Network),
            ),
            (
                entry("/Volumes/dav", "https://dav.example", "webdav", 0),
                true,
                Some(MountClass::Network),
            ),
            (
                entry("/Volumes/dav", "https://dav.example", "webdav", 0),
                false,
                None,
            ),
            (
                entry("/dev", "devfs", "devfs", local | MNT_DONTBROWSE),
                false,
                None,
            ),
            (
                entry("/Volumes/auto", "map -static", "autofs", 0),
                true,
                None,
            ),
            (
                entry("/Volumes/bind", "/Users/x/dir", "nullfs", local),
                false,
                None,
            ),
            (
                entry(
                    "/Volumes/Hidden",
                    "/dev/disk5s1",
                    "apfs",
                    local | MNT_DONTBROWSE,
                ),
                false,
                None,
            ),
            (
                entry(
                    "/Volumes/Snap",
                    "/dev/disk5s1@snap",
                    "apfs",
                    local | MNT_SNAPSHOT,
                ),
                false,
                None,
            ),
            (
                entry("/System/Volumes/Data", "/dev/disk3s5", "apfs", local),
                false,
                None,
            ),
            (
                entry("/System/Volumes/Preboot", "/dev/disk3s2", "apfs", local),
                false,
                None,
            ),
            (
                entry(
                    "/Library/Developer/CoreSimulator/Volumes/iOS_22D8075",
                    "/dev/disk6s1",
                    "apfs",
                    local,
                ),
                false,
                None,
            ),
            (
                entry(
                    "/Volumes/iOS Runtime",
                    "/Library/Developer/CoreSimulator/x.dmg",
                    "apfs",
                    local,
                ),
                false,
                None,
            ),
            (
                entry(
                    "/private/var/folders/ab/T/mnt",
                    "/dev/disk7s1",
                    "apfs",
                    local,
                ),
                false,
                None,
            ),
            (
                entry(
                    "/Volumes/com.apple.TimeMachine.localsnapshots/Backups.backupdb/Mac",
                    "com.apple.TimeMachine.2025-01-01-000000.local@/dev/disk3s5",
                    "apfs",
                    local,
                ),
                false,
                None,
            ),
            (
                entry(
                    "/Volumes/.timemachine/ABC/2025.backup",
                    "//tm@nas/tm",
                    "apfs",
                    local,
                ),
                false,
                None,
            ),
            (
                entry("/Users/me/mnt", "/dev/disk8s1", "apfs", local),
                false,
                None,
            ),
            (
                entry("/Users/me/share", "//u@nas/share", "smbfs", 0),
                true,
                None,
            ),
        ];
        for (mount, include_network, expected) in cases {
            assert_eq!(
                classify_mount(&mount, include_network),
                expected,
                "{mount:?} include_network={include_network}"
            );
        }
    }

    #[test]
    fn usage_math_uses_available_blocks() {
        let usage = usage_from_blocks(1000, 250, 4096);
        assert_eq!(usage.total, 4_096_000);
        assert_eq!(usage.available, 1_024_000);
        assert_eq!(usage.used, 3_072_000);
        assert!((usage.used_pct - 75.0).abs() < 1e-4);
    }

    #[test]
    fn usage_math_saturates_and_handles_empty_volumes() {
        let empty = usage_from_blocks(0, 0, 4096);
        assert_eq!((empty.total, empty.used, empty.used_pct), (0, 0, 0.0));
        let odd = usage_from_blocks(100, 200, 512);
        assert_eq!(odd.used, 0);
        assert_eq!(odd.used_pct, 0.0);
        let huge = usage_from_blocks(u64::MAX, 0, 4096);
        assert_eq!(huge.total, u64::MAX);
        assert!((huge.used_pct - 100.0).abs() < 1e-4);
    }

    #[test]
    fn change_detection_compares_fsid_sets() {
        let a: BTreeSet<Fsid> = [[1, 26], [2, 26]].into_iter().collect();
        let same: BTreeSet<Fsid> = [[2, 26], [1, 26]].into_iter().collect();
        let added: BTreeSet<Fsid> = [[1, 26], [2, 26], [3, 26]].into_iter().collect();
        let removed: BTreeSet<Fsid> = [[1, 26]].into_iter().collect();
        assert!(!fsids_changed(None, &a));
        assert!(!fsids_changed(Some(&a), &same));
        assert!(fsids_changed(Some(&a), &added));
        assert!(fsids_changed(Some(&a), &removed));
        assert!(fsids_changed(Some(&a), &BTreeSet::new()));
    }

    #[test]
    fn removable_combines_flag_and_resource_values() {
        assert!(!removable_from(0, None, None, None));
        assert!(!removable_from(0, Some(false), Some(false), Some(true)));
        assert!(removable_from(MNT_REMOVABLE, None, None, None));
        assert!(removable_from(0, Some(true), None, None));
        assert!(removable_from(0, None, Some(true), None));
        assert!(removable_from(0, None, None, Some(false)));
    }

    #[test]
    fn fallback_name_is_last_path_component() {
        assert_eq!(fallback_name("/Volumes/Share"), "Share");
        assert_eq!(fallback_name("/Volumes/My Disk/"), "My Disk");
        assert_eq!(fallback_name("/"), "/");
    }

    #[test]
    fn disk_id_matches_contract_format() {
        assert_eq!(disk_id([16_777_234, 26]), "fsid-16777234-26");
        assert_eq!(disk_id([-5, 0]), "fsid--5-0");
    }

    #[test]
    fn c_chars_stop_at_nul() {
        let mut raw = [0 as libc::c_char; 8];
        for (slot, byte) in raw.iter_mut().zip(b"apfs") {
            *slot = libc::c_char::from_ne_bytes([*byte]);
        }
        assert_eq!(c_chars_to_string(&raw), "apfs");
        assert_eq!(c_chars_to_string(&[0; 4]), "");
    }

    #[test]
    fn scan_lists_boot_first_and_detects_no_change_on_rescan() {
        let mut scanner = DiskScanner::new();
        let first = scanner.scan(DiskScanOptions::default()).unwrap();
        assert!(!first.changed);
        let boot = &first.disks[0];
        assert!(boot.is_boot);
        assert_eq!(boot.mount_point, "/");
        assert!(boot.id.starts_with("fsid-"));
        assert!(boot.total_bytes > 0);
        assert!(boot.used_bytes <= boot.total_bytes);
        assert_eq!(boot.used_bytes, boot.total_bytes - boot.available_bytes);
        assert!(!boot.is_network);
        assert_eq!(first.disks.iter().filter(|d| d.is_boot).count(), 1);
        let rest: Vec<&str> = first.disks[1..]
            .iter()
            .map(|d| d.mount_point.as_str())
            .collect();
        let mut sorted = rest.clone();
        sorted.sort_unstable();
        assert_eq!(rest, sorted);
        assert!(rest.iter().all(|m| m.starts_with("/Volumes/")));
        let second = scanner.scan(DiskScanOptions::default()).unwrap();
        assert!(!second.changed);
        assert_eq!(
            first.disks.iter().map(|d| &d.id).collect::<Vec<_>>(),
            second.disks.iter().map(|d| &d.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn important_capacity_is_cached_between_queries() {
        let mut scanner = DiskScanner::default();
        let queried = scanner
            .scan(DiskScanOptions {
                include_network: false,
                query_important_capacity: true,
            })
            .unwrap();
        let boot = &queried.disks[0];
        assert!(
            boot.available_important_bytes
                .is_none_or(|bytes| bytes > 0 && bytes <= boot.total_bytes)
        );
        let reused = scanner.scan(DiskScanOptions::default()).unwrap();
        assert_eq!(
            reused.disks[0].available_important_bytes,
            boot.available_important_bytes
        );
        assert!(
            reused.disks[1..]
                .iter()
                .all(|d| d.available_important_bytes.is_none())
        );
    }

    #[test]
    fn important_capacity_is_requeried_at_most_every_thirty_seconds() {
        let start = Instant::now();
        assert!(important_due(None, start));
        assert!(!important_due(Some(start), start + Duration::from_secs(29)));
        assert!(important_due(Some(start), start + IMPORTANT_REFRESH));
    }

    #[test]
    fn zero_important_capacity_falls_back_to_available_blocks() {
        assert_eq!(important_or_available(Some(0), 1_000), Some(1_000));
        assert_eq!(important_or_available(Some(5_000), 1_000), Some(5_000));
        assert_eq!(important_or_available(Some(-1), 1_000), None);
        assert_eq!(important_or_available(None, 1_000), None);
    }
}
