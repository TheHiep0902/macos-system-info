use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

use objc2::rc::Retained;
use objc2_foundation::NSError;
use objc2_service_management::{SMAppService, SMAppServiceStatus, kSMErrorJobNotFound};
use serde::Serialize;

pub const ISSUE_MOVE_TO_APPLICATIONS: &str =
    "Hãy chép Pulsebar vào thư mục Applications rồi bật lại";
pub const ISSUE_DEV_BUILD: &str = "Bản chạy thử không đăng ký được";

const TRANSLOCATION_MARKER: &str = "/AppTranslocation/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginItemStatus {
    Enabled,
    Disabled,
    RequiresApproval,
}

impl LoginItemStatus {
    fn code(self) -> u8 {
        match self {
            LoginItemStatus::Disabled => 1,
            LoginItemStatus::Enabled => 2,
            LoginItemStatus::RequiresApproval => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LaunchAtLoginStatus {
    Enabled,
    #[default]
    Disabled,
    RequiresApproval,
    Blocked,
}

impl LaunchAtLoginStatus {
    pub fn is_on(self) -> bool {
        matches!(
            self,
            LaunchAtLoginStatus::Enabled | LaunchAtLoginStatus::RequiresApproval
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchState {
    pub status: LaunchAtLoginStatus,
    pub reason: Option<&'static str>,
}

pub fn launch_state(status: LoginItemStatus, issue: Option<&'static str>) -> LaunchState {
    let (status, reason) = match (status, issue) {
        (LoginItemStatus::Enabled, _) => (LaunchAtLoginStatus::Enabled, None),
        (LoginItemStatus::RequiresApproval, _) => (LaunchAtLoginStatus::RequiresApproval, None),
        (LoginItemStatus::Disabled, Some(reason)) => (LaunchAtLoginStatus::Blocked, Some(reason)),
        (LoginItemStatus::Disabled, None) => (LaunchAtLoginStatus::Disabled, None),
    };
    LaunchState { status, reason }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginAction {
    Noop,
    Register,
    Unregister,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SMAppService error {code}: {description}")]
pub struct LoginItemError {
    pub code: isize,
    pub description: String,
}

impl LoginItemError {
    fn from_ns(error: &NSError) -> Self {
        LoginItemError {
            code: error.code(),
            description: error.localizedDescription().to_string(),
        }
    }
}

pub fn map_status(status: SMAppServiceStatus) -> LoginItemStatus {
    if status == SMAppServiceStatus::Enabled {
        LoginItemStatus::Enabled
    } else if status == SMAppServiceStatus::RequiresApproval {
        LoginItemStatus::RequiresApproval
    } else {
        LoginItemStatus::Disabled
    }
}

fn inside_app_bundle(exe: &Path) -> bool {
    let mut ancestors = exe.ancestors().skip(1);
    let macos = ancestors.next().and_then(Path::file_name);
    let contents = ancestors.next().and_then(Path::file_name);
    let bundle = ancestors.next().and_then(Path::extension);
    macos.is_some_and(|name| name == "MacOS")
        && contents.is_some_and(|name| name == "Contents")
        && bundle.is_some_and(|ext| ext == "app")
}

pub fn launch_location_issue(exe: &Path, on_read_only_mount: bool) -> Option<&'static str> {
    let translocated = exe
        .to_str()
        .is_some_and(|path| path.contains(TRANSLOCATION_MARKER));
    if on_read_only_mount || translocated {
        return Some(ISSUE_MOVE_TO_APPLICATIONS);
    }
    if !inside_app_bundle(exe) {
        return Some(ISSUE_DEV_BUILD);
    }
    None
}

pub fn login_action(requested: bool, status: LoginItemStatus, issue: Option<&str>) -> LoginAction {
    match (requested, status) {
        (true, LoginItemStatus::Enabled | LoginItemStatus::RequiresApproval) => LoginAction::Noop,
        (true, LoginItemStatus::Disabled) if issue.is_some() => LoginAction::Blocked,
        (true, LoginItemStatus::Disabled) => LoginAction::Register,
        (false, LoginItemStatus::Disabled) => LoginAction::Noop,
        (false, _) => LoginAction::Unregister,
    }
}

fn is_read_only_mount(path: &Path) -> bool {
    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    let rc = unsafe { libc::statfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if rc != 0 {
        log::debug!(
            "statfs failed for {}: {}",
            path.display(),
            std::io::Error::last_os_error()
        );
        return false;
    }
    let stats = unsafe { stats.assume_init() };
    stats.f_flags & libc::MNT_RDONLY as u32 != 0
}

pub fn current_location_issue() -> Option<&'static str> {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            log::warn!("could not resolve the executable path: {err}");
            return Some(ISSUE_DEV_BUILD);
        }
    };
    let bundle = exe.ancestors().nth(3).unwrap_or(exe.as_path());
    launch_location_issue(&exe, is_read_only_mount(bundle))
}

fn main_app() -> Retained<SMAppService> {
    unsafe { SMAppService::mainAppService() }
}

static LAST_STATUS: AtomicU8 = AtomicU8::new(0);

pub fn status() -> LoginItemStatus {
    let raw = unsafe { main_app().status() };
    let mapped = map_status(raw);
    if LAST_STATUS.swap(mapped.code(), Ordering::Relaxed) != mapped.code() {
        log::info!("login item status {mapped:?} (SMAppService {})", raw.0);
    }
    mapped
}

pub fn register() -> Result<(), LoginItemError> {
    let started = Instant::now();
    let result = unsafe { main_app().registerAndReturnError() };
    log::debug!("SMAppService register took {:?}", started.elapsed());
    result.map_err(|err| LoginItemError::from_ns(&err))
}

pub fn unregister() -> Result<(), LoginItemError> {
    let started = Instant::now();
    let result = unsafe { main_app().unregisterAndReturnError() };
    log::debug!("SMAppService unregister took {:?}", started.elapsed());
    match result {
        Ok(()) => Ok(()),
        Err(err) if err.code() == kSMErrorJobNotFound as isize => Ok(()),
        Err(err) => Err(LoginItemError::from_ns(&err)),
    }
}

pub fn current_state() -> LaunchState {
    launch_state(status(), current_location_issue())
}

pub fn open_system_settings() {
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATUSES: [LoginItemStatus; 3] = [
        LoginItemStatus::Enabled,
        LoginItemStatus::Disabled,
        LoginItemStatus::RequiresApproval,
    ];

    #[test]
    fn read_only_mount_and_translocation_ask_to_move_the_app() {
        let dmg = Path::new("/Volumes/Pulsebar/Pulsebar.app/Contents/MacOS/pulsebar");
        assert_eq!(
            launch_location_issue(dmg, true),
            Some(ISSUE_MOVE_TO_APPLICATIONS)
        );
        let translocated = Path::new(
            "/private/var/folders/xy/T/AppTranslocation/1A2B/d/Pulsebar.app/Contents/MacOS/pulsebar",
        );
        assert_eq!(
            launch_location_issue(translocated, false),
            Some(ISSUE_MOVE_TO_APPLICATIONS)
        );
    }

    #[test]
    fn binaries_outside_an_app_bundle_are_dev_builds() {
        for exe in [
            "/Users/me/pulsebar/src-tauri/target/debug/pulsebar",
            "/Applications/Pulsebar.app/pulsebar",
            "/Applications/Pulsebar/Contents/MacOS/pulsebar",
            "pulsebar",
        ] {
            assert_eq!(
                launch_location_issue(Path::new(exe), false),
                Some(ISSUE_DEV_BUILD),
                "{exe}"
            );
        }
    }

    #[test]
    fn installed_bundles_have_no_issue() {
        for exe in [
            "/Applications/Pulsebar.app/Contents/MacOS/pulsebar",
            "/Volumes/Data/Apps/Pulsebar.app/Contents/MacOS/pulsebar",
            "/Users/me/src-tauri/target/x86_64-apple-darwin/release/bundle/macos/Pulsebar.app/Contents/MacOS/pulsebar",
        ] {
            assert_eq!(launch_location_issue(Path::new(exe), false), None, "{exe}");
        }
    }

    #[test]
    fn enabling_registers_only_when_disabled_and_not_blocked() {
        for status in ALL_STATUSES {
            for issue in [None, Some(ISSUE_DEV_BUILD)] {
                let expected = match (status, issue) {
                    (LoginItemStatus::Disabled, Some(_)) => LoginAction::Blocked,
                    (LoginItemStatus::Disabled, None) => LoginAction::Register,
                    _ => LoginAction::Noop,
                };
                assert_eq!(
                    login_action(true, status, issue),
                    expected,
                    "{status:?} {issue:?}"
                );
            }
        }
    }

    #[test]
    fn disabling_unregisters_unless_already_disabled() {
        for status in ALL_STATUSES {
            for issue in [None, Some(ISSUE_MOVE_TO_APPLICATIONS)] {
                let expected = if status == LoginItemStatus::Disabled {
                    LoginAction::Noop
                } else {
                    LoginAction::Unregister
                };
                assert_eq!(
                    login_action(false, status, issue),
                    expected,
                    "{status:?} {issue:?}"
                );
            }
        }
    }

    #[test]
    fn launch_state_reports_blocked_only_while_disabled() {
        assert_eq!(
            launch_state(LoginItemStatus::Disabled, Some(ISSUE_MOVE_TO_APPLICATIONS)),
            LaunchState {
                status: LaunchAtLoginStatus::Blocked,
                reason: Some(ISSUE_MOVE_TO_APPLICATIONS)
            }
        );
        assert_eq!(
            launch_state(LoginItemStatus::Disabled, None),
            LaunchState {
                status: LaunchAtLoginStatus::Disabled,
                reason: None
            }
        );
        for issue in [None, Some(ISSUE_DEV_BUILD)] {
            assert_eq!(
                launch_state(LoginItemStatus::Enabled, issue).status,
                LaunchAtLoginStatus::Enabled
            );
            assert_eq!(
                launch_state(LoginItemStatus::RequiresApproval, issue),
                LaunchState {
                    status: LaunchAtLoginStatus::RequiresApproval,
                    reason: None
                }
            );
        }
        assert!(LaunchAtLoginStatus::Enabled.is_on());
        assert!(LaunchAtLoginStatus::RequiresApproval.is_on());
        assert!(!LaunchAtLoginStatus::Disabled.is_on());
        assert!(!LaunchAtLoginStatus::Blocked.is_on());
    }

    #[test]
    fn service_status_maps_not_found_and_not_registered_to_disabled() {
        assert_eq!(
            map_status(SMAppServiceStatus::Enabled),
            LoginItemStatus::Enabled
        );
        assert_eq!(
            map_status(SMAppServiceStatus::RequiresApproval),
            LoginItemStatus::RequiresApproval
        );
        assert_eq!(
            map_status(SMAppServiceStatus::NotRegistered),
            LoginItemStatus::Disabled
        );
        assert_eq!(
            map_status(SMAppServiceStatus::NotFound),
            LoginItemStatus::Disabled
        );
        assert_eq!(
            map_status(SMAppServiceStatus(42)),
            LoginItemStatus::Disabled
        );
    }
}
