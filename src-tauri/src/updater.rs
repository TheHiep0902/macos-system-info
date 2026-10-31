use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use semver::Version;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Listener, Manager, Runtime};
use tauri_plugin_updater::{Error as PluginError, Update, UpdaterExt};

use crate::ipc::{EVENT_POPOVER_VISIBILITY, EVENT_UPDATE_STATE, POPOVER_LABEL};
use crate::login_item::{self, LoginItemStatus};
use crate::settings::SettingsStore;
use crate::tray;
use crate::updater_state::{self, Pending, PersistedState};

pub const THREAD_NAME: &str = "pulsebar-updater";
pub const STARTUP_DELAY: Duration = Duration::from_secs(60);
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
pub const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60 * 60);
pub const MAX_SLEEP: Duration = Duration::from_secs(60 * 60);
pub const FUTURE_TOLERANCE: Duration = Duration::from_secs(60 * 60);
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
pub const MAX_DOWNLOAD_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_NOTES_CHARS: usize = 2000;
pub const MAX_REDIRECTS: usize = 5;
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

const ELLIPSIS: char = '…';

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(Deserialize))]
#[serde(rename_all = "camelCase")]
pub enum UpdateError {
    Network,
    Timeout,
    Signature,
    TooLarge,
    Install,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(Deserialize))]
#[serde(rename_all = "camelCase")]
pub enum LoginItemOutcome {
    Unchanged,
    ReRegistered,
    RequiresApproval,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(Deserialize))]
#[serde(rename_all = "camelCase")]
pub struct PostUpdate {
    pub from_version: String,
    pub login_item: LoginItemOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(Deserialize))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
        notes: String,
        date: Option<u64>,
    },
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    Installing,
    Failed {
        error: UpdateError,
    },
    Blocked {
        reason: String,
    },
}

impl UpdatePhase {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            UpdatePhase::Checking | UpdatePhase::Downloading { .. } | UpdatePhase::Installing
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(Deserialize))]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub current_version: String,
    pub auto_check: bool,
    pub last_check_at: Option<u64>,
    pub phase: UpdatePhase,
    pub post_update: Option<PostUpdate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<u64>,
}

pub type CheckResult<H> = Result<Option<(ReleaseInfo, H)>, UpdateError>;

pub trait UpdateSource {
    type Release;

    fn check(&self) -> CheckResult<Self::Release>;

    fn download(
        &self,
        release: &Self::Release,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<Vec<u8>, UpdateError>;

    fn install(&self, release: &Self::Release, bytes: &[u8]) -> Result<(), UpdateError>;
}

fn keep_note_char(c: char) -> bool {
    if c == '\n' || c == '\t' {
        return true;
    }
    !(c.is_control()
        || matches!(
            c,
            '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
        ))
}

pub fn sanitize_notes(notes: &str) -> String {
    let cleaned: String = notes.chars().filter(|c| keep_note_char(*c)).collect();
    let trimmed = cleaned.trim();
    if trimmed.chars().count() <= MAX_NOTES_CHARS {
        return trimmed.to_owned();
    }
    let mut truncated: String = trimmed.chars().take(MAX_NOTES_CHARS - 1).collect();
    truncated.truncate(truncated.trim_end().len());
    truncated.push(ELLIPSIS);
    truncated
}

pub fn unix_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

pub fn from_unix_ms(ms: u64) -> SystemTime {
    UNIX_EPOCH
        .checked_add(Duration::from_millis(ms))
        .unwrap_or(UNIX_EPOCH)
}

fn later(time: SystemTime, by: Duration) -> SystemTime {
    time.checked_add(by).unwrap_or(time)
}

pub fn next_due(
    now: SystemTime,
    started_at: SystemTime,
    last_check_at: Option<SystemTime>,
    last_failure_at: Option<SystemTime>,
    auto: bool,
) -> Option<SystemTime> {
    if !auto {
        return None;
    }
    let first = later(started_at, STARTUP_DELAY);
    let regular = match last_check_at {
        Some(at) if at > later(now, FUTURE_TOLERANCE) => now,
        Some(at) => later(at, CHECK_INTERVAL),
        None => first,
    };
    let mut due = regular.max(first);
    if let Some(failed) = last_failure_at
        && failed <= now
    {
        due = due.max(later(failed, RETRY_AFTER_FAILURE));
    }
    Some(due)
}

pub fn progress_due(
    last: Option<(Instant, u64)>,
    now: Instant,
    received: u64,
    total: Option<u64>,
) -> bool {
    let Some((at, previous)) = last else {
        return true;
    };
    if now.saturating_duration_since(at) >= PROGRESS_INTERVAL {
        return true;
    }
    match total {
        Some(total) if total > 0 => received.saturating_sub(previous).saturating_mul(100) >= total,
        _ => false,
    }
}

pub fn exceeds_cap(received: u64, total: Option<u64>) -> bool {
    received > MAX_DOWNLOAD_BYTES || total.is_some_and(|total| total > MAX_DOWNLOAD_BYTES)
}

pub fn append_chunk(buffer: &mut Vec<u8>, chunk: &[u8]) -> Result<(), UpdateError> {
    let received = u64::try_from(buffer.len().saturating_add(chunk.len())).unwrap_or(u64::MAX);
    if exceeds_cap(received, None) {
        return Err(UpdateError::TooLarge);
    }
    buffer.extend_from_slice(chunk);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostUpdateAction {
    Ignore,
    Notice { register: bool },
}

pub fn post_update_action(
    current_version: &Version,
    pending: Option<&Pending>,
    status: LoginItemStatus,
    issue: Option<&str>,
) -> PostUpdateAction {
    let Some(pending) = pending else {
        return PostUpdateAction::Ignore;
    };
    let Ok(from) = Version::parse(&pending.from_version) else {
        return PostUpdateAction::Ignore;
    };
    if *current_version <= from {
        return PostUpdateAction::Ignore;
    }
    PostUpdateAction::Notice {
        register: pending.launch_at_login_was_on
            && status != LoginItemStatus::Enabled
            && issue.is_none(),
    }
}

pub fn login_outcome(registered: bool, status: LoginItemStatus) -> LoginItemOutcome {
    if !registered {
        return LoginItemOutcome::Failed;
    }
    match status {
        LoginItemStatus::Enabled => LoginItemOutcome::ReRegistered,
        LoginItemStatus::RequiresApproval => LoginItemOutcome::RequiresApproval,
        LoginItemStatus::Disabled => LoginItemOutcome::Failed,
    }
}

#[derive(Debug)]
pub struct Machine<H> {
    state: UpdateState,
    release: Option<H>,
    last_failure_at: Option<SystemTime>,
    post_update_seen: bool,
    location_issue: Option<String>,
}

impl<H> Machine<H> {
    pub fn new(
        current_version: String,
        auto_check: bool,
        last_check_at: Option<u64>,
        blocked: Option<&str>,
        post_update: Option<PostUpdate>,
    ) -> Self {
        let phase = match blocked {
            Some(reason) => UpdatePhase::Blocked {
                reason: reason.to_owned(),
            },
            None => UpdatePhase::Idle,
        };
        Machine {
            state: UpdateState {
                current_version,
                auto_check,
                last_check_at,
                phase,
                post_update,
            },
            release: None,
            last_failure_at: None,
            post_update_seen: false,
            location_issue: blocked.map(str::to_owned),
        }
    }

    pub fn state(&self) -> &UpdateState {
        &self.state
    }

    pub fn due(&self, now: SystemTime, started_at: SystemTime) -> Option<SystemTime> {
        next_due(
            now,
            started_at,
            self.state.last_check_at.map(from_unix_ms),
            self.last_failure_at,
            self.state.auto_check,
        )
    }

    pub fn begin_check(&mut self) -> bool {
        if self.state.phase.is_busy() {
            return false;
        }
        self.state.phase = UpdatePhase::Checking;
        true
    }

    pub fn finish_check(&mut self, result: CheckResult<H>, now: SystemTime) -> Option<u64> {
        match result {
            Ok(found) => {
                let checked_at = unix_ms(now);
                self.state.last_check_at = Some(checked_at);
                self.last_failure_at = None;
                self.state.phase = match found {
                    Some(_) if self.location_issue.is_some() => {
                        self.release = None;
                        UpdatePhase::Blocked {
                            reason: self.location_issue.clone().unwrap_or_default(),
                        }
                    }
                    Some((info, release)) => {
                        self.release = Some(release);
                        UpdatePhase::Available {
                            version: info.version,
                            notes: sanitize_notes(info.notes.as_deref().unwrap_or_default()),
                            date: info.date,
                        }
                    }
                    None => {
                        self.release = None;
                        UpdatePhase::UpToDate
                    }
                };
                Some(checked_at)
            }
            Err(error) => {
                self.release = None;
                self.last_failure_at = Some(now);
                self.state.phase = UpdatePhase::Failed { error };
                None
            }
        }
    }

    pub fn begin_install(&mut self, issue: Option<&str>) -> Option<H> {
        if !matches!(self.state.phase, UpdatePhase::Available { .. }) {
            return None;
        }
        if let Some(reason) = issue {
            self.release = None;
            self.state.phase = UpdatePhase::Blocked {
                reason: reason.to_owned(),
            };
            return None;
        }
        let release = self.release.take()?;
        self.state.phase = UpdatePhase::Downloading {
            received: 0,
            total: None,
        };
        Some(release)
    }

    pub fn progress(&mut self, received: u64, total: Option<u64>) {
        if matches!(self.state.phase, UpdatePhase::Downloading { .. }) {
            self.state.phase = UpdatePhase::Downloading { received, total };
        }
    }

    pub fn installing(&mut self) {
        self.state.phase = UpdatePhase::Installing;
    }

    pub fn fail(&mut self, error: UpdateError) {
        self.state.phase = UpdatePhase::Failed { error };
    }

    pub fn set_auto_check(&mut self, enabled: bool) -> bool {
        std::mem::replace(&mut self.state.auto_check, enabled) != enabled
    }

    pub fn visibility_changed(&mut self, visible: bool) -> bool {
        if self.state.post_update.is_none() {
            return false;
        }
        if visible {
            self.post_update_seen = true;
            return false;
        }
        if !self.post_update_seen {
            return false;
        }
        self.state.post_update = None;
        self.post_update_seen = false;
        true
    }
}

fn lock<H>(machine: &Mutex<Machine<H>>) -> MutexGuard<'_, Machine<H>> {
    machine.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn perform_check<S: UpdateSource>(
    machine: &Mutex<Machine<S::Release>>,
    source: &S,
    notify: &dyn Fn(),
) -> Option<u64> {
    let result = source.check();
    let checked_at = lock(machine).finish_check(result, SystemTime::now());
    notify();
    checked_at
}

pub fn perform_install<S: UpdateSource>(
    machine: &Mutex<Machine<S::Release>>,
    source: &S,
    release: &S::Release,
    notify: &dyn Fn(),
) -> bool {
    let mut last: Option<(Instant, u64)> = None;
    let mut progress = |received: u64, total: Option<u64>| {
        lock(machine).progress(received, total);
        let now = Instant::now();
        if progress_due(last, now, received, total) {
            last = Some((now, received));
            notify();
        }
    };
    let bytes = match source.download(release, &mut progress) {
        Ok(bytes) => bytes,
        Err(error) => {
            lock(machine).fail(error);
            notify();
            return false;
        }
    };
    lock(machine).installing();
    notify();
    match source.install(release, &bytes) {
        Ok(()) => true,
        Err(error) => {
            lock(machine).fail(error);
            notify();
            false
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Check,
    Download,
    Install,
}

pub fn classify(err: &PluginError, stage: Stage) -> UpdateError {
    match err {
        PluginError::Reqwest(inner) if inner.is_timeout() => UpdateError::Timeout,
        PluginError::Reqwest(_)
        | PluginError::Network(_)
        | PluginError::ReleaseNotFound
        | PluginError::Http(_) => UpdateError::Network,
        PluginError::Minisign(_)
        | PluginError::Base64(_)
        | PluginError::SignatureUtf8(_)
        | PluginError::SignedVersionMismatch { .. }
        | PluginError::MissingSignedVersion => UpdateError::Signature,
        PluginError::Io(_)
        | PluginError::TempDirNotOnSameMountPoint
        | PluginError::PreviousAppNotRestored(_)
        | PluginError::BinaryNotFoundInArchive
        | PluginError::TempDirNotFound
        | PluginError::InvalidUpdaterFormat
        | PluginError::FailedToDetermineExtractPath
            if stage == Stage::Install =>
        {
            UpdateError::Install
        }
        _ => UpdateError::Unknown,
    }
}

fn describe(err: &PluginError) -> String {
    match err {
        PluginError::Reqwest(inner) => format!(
            "http request failed (timeout: {}, connect: {}, status: {:?})",
            inner.is_timeout(),
            inner.is_connect(),
            inner.status().map(|status| status.as_u16())
        ),
        PluginError::PreviousAppNotRestored(_) => {
            "the update failed and the previous app could not be moved back".to_owned()
        }
        other => other.to_string(),
    }
}

fn report(err: &PluginError, stage: Stage) -> UpdateError {
    let kind = classify(err, stage);
    log::warn!("update {stage:?} failed ({kind:?}): {}", describe(err));
    kind
}

fn release_date_ms(update: &Update) -> Option<u64> {
    let nanos = update.date?.unix_timestamp_nanos();
    u64::try_from(nanos / 1_000_000).ok()
}

fn http_error(err: &reqwest::Error) -> UpdateError {
    log::warn!(
        "update download failed (timeout: {}, connect: {}, status: {:?})",
        err.is_timeout(),
        err.is_connect(),
        err.status().map(|status| status.as_u16())
    );
    if err.is_timeout() {
        UpdateError::Timeout
    } else {
        UpdateError::Network
    }
}

pub fn allowed_download_url(url: &reqwest::Url, insecure_loopback: bool) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => insecure_loopback && url.host_str() == Some("127.0.0.1"),
        _ => false,
    }
}

fn insecure_transport_allowed<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|updater| updater.get("dangerousInsecureTransportProtocol"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

async fn fetch_package(
    release: &Update,
    insecure_loopback: bool,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, UpdateError> {
    if !allowed_download_url(&release.download_url, insecure_loopback) {
        log::warn!("update package refused: the download URL is not https");
        return Err(UpdateError::Network);
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if allowed_download_url(attempt.url(), insecure_loopback) {
                attempt.follow()
            } else {
                attempt.error("redirect to a URL that is not https")
            }
        }))
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|err| http_error(&err))?;
    let mut response = client
        .get(release.download_url.clone())
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|err| http_error(&err))?;
    let total = response.content_length();
    if exceeds_cap(0, total) {
        log::warn!("update package refused: larger than {MAX_DOWNLOAD_BYTES} bytes");
        return Err(UpdateError::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|err| http_error(&err))? {
        if let Err(error) = append_chunk(&mut bytes, &chunk) {
            log::warn!("update download stopped: larger than {MAX_DOWNLOAD_BYTES} bytes");
            return Err(error);
        }
        progress(u64::try_from(bytes.len()).unwrap_or(u64::MAX), total);
    }
    Ok(bytes)
}

struct PluginSource<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> UpdateSource for PluginSource<R> {
    type Release = Update;

    fn check(&self) -> CheckResult<Update> {
        let updater = self
            .app
            .updater_builder()
            .timeout(CHECK_TIMEOUT)
            .configure_client(|client| {
                client
                    .connect_timeout(CONNECT_TIMEOUT)
                    .read_timeout(READ_TIMEOUT)
            })
            .build()
            .map_err(|err| report(&err, Stage::Check))?;
        let found = tauri::async_runtime::block_on(updater.check())
            .map_err(|err| report(&err, Stage::Check))?;
        Ok(found.map(|update| {
            let info = ReleaseInfo {
                version: update.version.clone(),
                notes: update.body.clone(),
                date: release_date_ms(&update),
            };
            (info, update)
        }))
    }

    fn download(
        &self,
        release: &Update,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<Vec<u8>, UpdateError> {
        let insecure_loopback = insecure_transport_allowed(&self.app);
        tauri::async_runtime::block_on(fetch_package(release, insecure_loopback, progress))
    }

    fn install(&self, release: &Update, bytes: &[u8]) -> Result<(), UpdateError> {
        release
            .install(bytes)
            .map_err(|err| report(&err, Stage::Install))
    }
}

enum Command<H> {
    Check,
    Install {
        release: Box<H>,
        launch_at_login_was_on: bool,
    },
    Wake,
}

pub struct UpdaterState {
    machine: Arc<Mutex<Machine<Update>>>,
    commands: Option<Sender<Command<Update>>>,
}

impl UpdaterState {
    pub fn snapshot(&self) -> UpdateState {
        lock(&self.machine).state().clone()
    }

    fn send(&self, command: Command<Update>) -> bool {
        match &self.commands {
            Some(commands) => commands.send(command).is_ok(),
            None => false,
        }
    }
}

fn publish<R: Runtime>(app: &AppHandle<R>) {
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let Some(state) = handle.try_state::<UpdaterState>() else {
            return;
        };
        let snapshot = state.snapshot();
        if let Err(err) = handle.emit_to(POPOVER_LABEL, EVENT_UPDATE_STATE, &snapshot) {
            log::debug!("update state event not delivered: {err}");
        }
        tray::sync_update_item(&handle, &snapshot.phase);
    });
    if let Err(err) = scheduled {
        log::warn!("could not publish the update state: {err}");
    }
}

pub fn request_check<R: Runtime>(app: &AppHandle<R>, state: &UpdaterState) -> UpdateState {
    let started = lock(&state.machine).begin_check();
    if started {
        if !state.send(Command::Check) {
            log::warn!("updater thread unavailable; check not started");
            lock(&state.machine).finish_check(Err(UpdateError::Unknown), SystemTime::now());
        }
        publish(app);
    }
    state.snapshot()
}

pub fn request_install<R: Runtime>(app: &AppHandle<R>, state: &UpdaterState) -> UpdateState {
    let issue = login_item::current_location_issue();
    let (release, changed) = {
        let mut machine = lock(&state.machine);
        let before = machine.state().phase.clone();
        let release = machine.begin_install(issue);
        let changed = machine.state().phase != before;
        (release, changed)
    };
    if let Some(release) = release {
        let launch_at_login_was_on = matches!(login_item::status(), LoginItemStatus::Enabled);
        let command = Command::Install {
            release: Box::new(release),
            launch_at_login_was_on,
        };
        if !state.send(command) {
            log::warn!("updater thread unavailable; install not started");
            lock(&state.machine).fail(UpdateError::Unknown);
        }
    }
    if changed {
        publish(app);
    }
    state.snapshot()
}

pub fn is_update_available<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.try_state::<UpdaterState>().is_some_and(|state| {
        matches!(
            lock(&state.machine).state().phase,
            UpdatePhase::Available { .. }
        )
    })
}

pub fn check_now<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<UpdaterState>() {
        request_check(app, &state);
    }
}

pub fn settings_changed<R: Runtime>(app: &AppHandle<R>, auto_check: bool) {
    let Some(state) = app.try_state::<UpdaterState>() else {
        return;
    };
    if lock(&state.machine).set_auto_check(auto_check) {
        state.send(Command::Wake);
        publish(app);
    }
}

#[derive(Deserialize)]
struct VisibilityEvent {
    visible: bool,
}

fn popover_visibility_changed<R: Runtime>(app: &AppHandle<R>, payload: &str) {
    let Ok(event) = serde_json::from_str::<VisibilityEvent>(payload) else {
        log::debug!("ignoring a popover visibility event with an unexpected payload");
        return;
    };
    let Some(state) = app.try_state::<UpdaterState>() else {
        return;
    };
    if lock(&state.machine).visibility_changed(event.visible) {
        publish(app);
    }
}

fn save_persisted(dir: Option<&Path>, persisted: &PersistedState) {
    let Some(dir) = dir else {
        return;
    };
    if let Err(err) = updater_state::save_to(dir, persisted) {
        log::warn!("could not save the updater state: {err}");
    }
}

fn resolve_post_update(
    current_version: &Version,
    persisted: &mut PersistedState,
    dir: Option<&Path>,
    issue: Option<&str>,
) -> Option<PostUpdate> {
    let pending = persisted.pending.take()?;
    let status = login_item::status();
    let outcome = match post_update_action(current_version, Some(&pending), status, issue) {
        PostUpdateAction::Ignore => None,
        PostUpdateAction::Notice { register: false } => Some(LoginItemOutcome::Unchanged),
        PostUpdateAction::Notice { register: true } => {
            let registered = match login_item::register() {
                Ok(()) => true,
                Err(err) => {
                    log::warn!("could not register the login item after the update: {err}");
                    false
                }
            };
            Some(login_outcome(registered, login_item::status()))
        }
    };
    save_persisted(dir, persisted);
    outcome.map(|login_item| PostUpdate {
        from_version: pending.from_version,
        login_item,
    })
}

struct Worker<R: Runtime> {
    app: AppHandle<R>,
    source: PluginSource<R>,
    machine: Arc<Mutex<Machine<Update>>>,
    dir: Option<PathBuf>,
    persisted: PersistedState,
    started: Instant,
}

impl<R: Runtime> Worker<R> {
    fn notify(&self) {
        publish(&self.app);
    }

    fn check(&mut self) {
        if let Some(checked_at) = perform_check(&self.machine, &self.source, &|| self.notify()) {
            self.persisted.last_check_at = Some(checked_at);
            save_persisted(self.dir.as_deref(), &self.persisted);
        }
    }

    fn install(&mut self, release: &Update, launch_at_login_was_on: bool) {
        let from_version = lock(&self.machine).state().current_version.clone();
        self.persisted.pending = Some(Pending {
            from_version,
            launch_at_login_was_on,
        });
        save_persisted(self.dir.as_deref(), &self.persisted);
        if perform_install(&self.machine, &self.source, release, &|| self.notify()) {
            log::info!("update installed; restarting");
            let handle = self.app.clone();
            if let Err(err) = self
                .app
                .run_on_main_thread(move || handle.request_restart())
            {
                log::error!("could not restart after the update: {err}");
                lock(&self.machine).fail(UpdateError::Unknown);
                self.notify();
            }
            return;
        }
        self.persisted.pending = None;
        save_persisted(self.dir.as_deref(), &self.persisted);
    }

    fn run(mut self, commands: &Receiver<Command<Update>>) {
        loop {
            let now = SystemTime::now();
            let started_at = now.checked_sub(self.started.elapsed()).unwrap_or(now);
            let due = lock(&self.machine).due(now, started_at);
            if let Some(due) = due
                && due <= now
                && lock(&self.machine).begin_check()
            {
                self.notify();
                self.check();
                continue;
            }
            let wait = due
                .map(|due| due.duration_since(now).unwrap_or(MAX_SLEEP))
                .unwrap_or(MAX_SLEEP)
                .min(MAX_SLEEP);
            match commands.recv_timeout(wait) {
                Ok(Command::Check) => self.check(),
                Ok(Command::Install {
                    release,
                    launch_at_login_was_on,
                }) => self.install(&release, launch_at_login_was_on),
                Ok(Command::Wake) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let current_version = app.package_info().version.clone();
    let dir = match app.path().app_config_dir() {
        Ok(dir) => Some(dir),
        Err(err) => {
            log::warn!("app config directory unavailable; updater state will not persist: {err}");
            None
        }
    };
    let mut persisted = dir
        .as_deref()
        .map(updater_state::load_from)
        .unwrap_or_default();
    let issue = login_item::current_location_issue();
    let post_update = resolve_post_update(&current_version, &mut persisted, dir.as_deref(), issue);
    if post_update.is_some() {
        tray::sync_launch_at_login(app, login_item::current_state().status);
    }
    let auto_check = app
        .try_state::<SettingsStore>()
        .is_none_or(|store| store.get().auto_check_updates);
    let machine = Arc::new(Mutex::new(Machine::new(
        current_version.to_string(),
        auto_check,
        persisted.last_check_at,
        issue,
        post_update,
    )));
    let (sender, receiver) = mpsc::channel();
    let worker = Worker {
        app: app.clone(),
        source: PluginSource { app: app.clone() },
        machine: Arc::clone(&machine),
        dir,
        persisted,
        started: Instant::now(),
    };
    let spawned = thread::Builder::new()
        .name(THREAD_NAME.into())
        .spawn(move || worker.run(&receiver));
    let commands = match spawned {
        Ok(_) => Some(sender),
        Err(err) => {
            log::error!("could not start the updater thread: {err}");
            None
        }
    };
    let phase = lock(&machine).state().phase.clone();
    app.manage(UpdaterState { machine, commands });
    let handle = app.clone();
    app.listen_any(EVENT_POPOVER_VISIBILITY, move |event| {
        popover_visibility_changed(&handle, event.payload());
    });
    tray::sync_update_item(app, &phase);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeSet;

    const FIXTURE: &str = include_str!("../../src/mocks/contract-update.json");
    const CONFIG: &str = include_str!("../tauri.conf.json");
    const RELEASE_CONFIG: &str = include_str!("../tauri.release.conf.json");
    const ENDPOINT: &str =
        "https://github.com/TheHiep0902/macos-system-info/releases/latest/download/latest.json";
    const PUBKEY_PLACEHOLDER: &str = "REPLACE_WITH_OWNER_PUBKEY";
    const CSP: &str = "default-src 'self' ipc: http://ipc.localhost; script-src 'self'; style-src 'self'; img-src 'self' data:";
    const DEV_CSP: &str = "default-src 'self' ipc: http://ipc.localhost; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost ws://localhost:1420";
    const ISSUE: &str = "Hãy chép Pulsebar vào thư mục Applications rồi bật lại";

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    const NOW: u64 = 1_800_000_000;
    const HOUR: u64 = 3600;
    const DAY: u64 = 24 * HOUR;

    fn fixture() -> Value {
        serde_json::from_str(FIXTURE).expect("fixture is valid JSON")
    }

    #[test]
    fn every_fixture_state_round_trips_exactly() {
        let all = fixture();
        let object = all.as_object().unwrap();
        let mut states = 0;
        for (key, value) in object {
            if key == "failedErrors" || key == "loginItemOutcomes" {
                continue;
            }
            let parsed: UpdateState = serde_json::from_value(value.clone())
                .unwrap_or_else(|err| panic!("{key} does not parse: {err}"));
            assert_eq!(&serde_json::to_value(&parsed).unwrap(), value, "{key}");
            states += 1;
        }
        assert_eq!(states, 11);
    }

    #[test]
    fn fixture_lists_every_error_and_login_outcome() {
        let all = fixture();
        let errors: BTreeSet<String> = [
            UpdateError::Network,
            UpdateError::Timeout,
            UpdateError::Signature,
            UpdateError::TooLarge,
            UpdateError::Install,
            UpdateError::Unknown,
        ]
        .iter()
        .map(|error| {
            serde_json::to_value(error)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
        let listed: BTreeSet<String> = all["failedErrors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| {
                let parsed: UpdateError = serde_json::from_value(value.clone()).unwrap();
                assert_eq!(&serde_json::to_value(parsed).unwrap(), value);
                value.as_str().unwrap().to_owned()
            })
            .collect();
        assert_eq!(listed, errors);
        let outcomes: BTreeSet<String> = [
            LoginItemOutcome::Unchanged,
            LoginItemOutcome::ReRegistered,
            LoginItemOutcome::RequiresApproval,
            LoginItemOutcome::Failed,
        ]
        .iter()
        .map(|outcome| {
            serde_json::to_value(outcome)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
        let listed: BTreeSet<String> = all["loginItemOutcomes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| {
                let parsed: LoginItemOutcome = serde_json::from_value(value.clone()).unwrap();
                assert_eq!(&serde_json::to_value(parsed).unwrap(), value);
                value.as_str().unwrap().to_owned()
            })
            .collect();
        assert_eq!(listed, outcomes);
    }

    fn config() -> Value {
        serde_json::from_str(CONFIG).expect("tauri.conf.json is valid JSON")
    }

    #[test]
    fn updater_config_is_locked_down() {
        let config = config();
        let updater = &config["plugins"]["updater"];
        assert_eq!(updater["endpoints"], serde_json::json!([ENDPOINT]));
        assert_eq!(updater["requireSignedVersion"], Value::Bool(true));
        let keys: Vec<&String> = updater.as_object().unwrap().keys().collect();
        assert!(
            keys.iter()
                .all(|key| !key.to_ascii_lowercase().starts_with("dangerous")),
            "{keys:?}"
        );
        assert!(updater.get("allowDowngrades").is_none());
        assert!(updater.get("allow-downgrades").is_none());
        assert!(config["bundle"].get("createUpdaterArtifacts").is_none());
        let release: Value = serde_json::from_str(RELEASE_CONFIG).unwrap();
        assert_eq!(
            release,
            serde_json::json!({ "bundle": { "createUpdaterArtifacts": true } })
        );
        assert_eq!(config["app"]["security"]["csp"], Value::from(CSP));
        assert_eq!(config["app"]["security"]["devCsp"], Value::from(DEV_CSP));
        let parsed: tauri_plugin_updater::Config =
            serde_json::from_value(updater.clone()).expect("plugin accepts the config");
        assert!(parsed.require_signed_version && !parsed.allow_downgrades);
        assert!(!parsed.dangerous_insecure_transport_protocol);
        assert!(
            !parsed.dangerous_accept_invalid_certs && !parsed.dangerous_accept_invalid_hostnames
        );
    }

    fn base64_decode(text: &str) -> Vec<u8> {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bits: u32 = 0;
        let mut count = 0;
        let mut out = Vec::new();
        for byte in text.trim().bytes().take_while(|byte| *byte != b'=') {
            let value = ALPHABET
                .iter()
                .position(|candidate| *candidate == byte)
                .unwrap_or_else(|| panic!("invalid base64 byte {byte}"));
            bits = (bits << 6) | u32::try_from(value).unwrap();
            count += 6;
            if count >= 8 {
                count -= 8;
                out.push(u8::try_from((bits >> count) & 0xFF).unwrap());
            }
        }
        out
    }

    #[test]
    fn base64_helper_decodes_known_vectors() {
        assert_eq!(base64_decode("TWFu"), b"Man");
        assert_eq!(base64_decode("TWE="), b"Ma");
        assert_eq!(base64_decode("TQ=="), b"M");
    }

    #[test]
    fn updater_pubkey_is_the_owner_minisign_key() {
        let config = config();
        let pubkey = config["plugins"]["updater"]["pubkey"].as_str().unwrap();
        assert_ne!(
            pubkey, PUBKEY_PLACEHOLDER,
            "insert the owner's updater public key into tauri.conf.json"
        );
        let text = String::from_utf8(base64_decode(pubkey)).expect("pubkey decodes to text");
        let mut lines = text.lines();
        assert!(lines.next().unwrap().starts_with("untrusted comment:"));
        let key = base64_decode(lines.next().expect("key line"));
        assert_eq!(key.len(), 42);
        assert_eq!(&key[..2], b"Ed");
    }

    #[test]
    fn notes_drop_control_bidi_and_zero_width_characters() {
        let raw = "  Sửa\u{0007} lỗi\u{200B}\u{202E}\u{2066}\u{FEFF}\r\n\tmục 2\u{202A}\u{200F}\u{2069}\u{0000}  ";
        assert_eq!(sanitize_notes(raw), "Sửa lỗi\n\tmục 2");
        assert_eq!(sanitize_notes(""), "");
        assert_eq!(sanitize_notes("\u{200B}\n "), "");
        assert_eq!(sanitize_notes("a\u{2010}b"), "a\u{2010}b");
    }

    #[test]
    fn long_notes_are_truncated_on_a_char_boundary() {
        let exact = "ệ".repeat(MAX_NOTES_CHARS);
        assert_eq!(sanitize_notes(&exact), exact);
        let long = "ệ".repeat(MAX_NOTES_CHARS + 5);
        let cut = sanitize_notes(&long);
        assert_eq!(cut.chars().count(), MAX_NOTES_CHARS);
        assert!(cut.ends_with(ELLIPSIS));
        assert!(cut.starts_with("ệệ"));
    }

    #[test]
    fn first_check_waits_for_the_startup_delay() {
        let started = at(NOW);
        assert_eq!(
            next_due(at(NOW), started, None, None, true),
            Some(at(NOW + 60))
        );
        assert_eq!(
            next_due(at(NOW), started, Some(at(NOW - 30 * DAY)), None, true),
            Some(at(NOW + 60))
        );
    }

    #[test]
    fn regular_checks_follow_the_last_check_by_a_day() {
        let started = at(NOW - 2 * HOUR);
        let last = at(NOW - HOUR);
        assert_eq!(
            next_due(at(NOW), started, Some(last), None, true),
            Some(at(NOW - HOUR + DAY))
        );
    }

    #[test]
    fn a_long_sleep_makes_the_check_due_on_wake() {
        let started = at(NOW - 3 * DAY - HOUR);
        let last = at(NOW - 3 * DAY);
        let due = next_due(at(NOW), started, Some(last), None, true).unwrap();
        assert!(due <= at(NOW));
    }

    #[test]
    fn clock_jumps_are_handled() {
        let started = at(NOW - DAY);
        let future = at(NOW + 2 * HOUR);
        assert_eq!(
            next_due(at(NOW), started, Some(future), None, true),
            Some(at(NOW))
        );
        let near_future = at(NOW + HOUR / 2);
        assert_eq!(
            next_due(at(NOW), started, Some(near_future), None, true),
            Some(at(NOW + HOUR / 2 + DAY))
        );
        let jumped_forward = at(NOW + 10 * DAY);
        let due = next_due(jumped_forward, started, Some(at(NOW)), None, true).unwrap();
        assert!(due <= jumped_forward);
    }

    #[test]
    fn failures_back_off_for_an_hour() {
        let started = at(NOW - DAY);
        let last = at(NOW - 2 * DAY);
        let failed = at(NOW - 10 * 60);
        assert_eq!(
            next_due(at(NOW), started, Some(last), Some(failed), true),
            Some(at(NOW - 10 * 60 + HOUR))
        );
        let future_failure = at(NOW + 5 * DAY);
        assert_eq!(
            next_due(at(NOW), started, Some(last), Some(future_failure), true),
            Some(at(NOW - DAY + 60))
        );
    }

    #[test]
    fn disabled_auto_check_is_never_due() {
        assert_eq!(next_due(at(NOW), at(NOW - DAY), None, None, false), None);
    }

    #[test]
    fn progress_is_throttled() {
        let start = Instant::now();
        assert!(progress_due(None, start, 0, None));
        let last = Some((start, 1000));
        assert!(!progress_due(
            last,
            start + Duration::from_millis(100),
            2000,
            None
        ));
        assert!(progress_due(last, start + PROGRESS_INTERVAL, 2000, None));
        assert!(!progress_due(
            last,
            start + Duration::from_millis(10),
            1500,
            Some(100_000)
        ));
        assert!(progress_due(
            last,
            start + Duration::from_millis(10),
            2000,
            Some(100_000)
        ));
        assert!(!progress_due(last, start, 5000, Some(0)));
    }

    #[test]
    fn download_cap_uses_length_and_received_bytes() {
        assert!(!exceeds_cap(0, None));
        assert!(!exceeds_cap(MAX_DOWNLOAD_BYTES, Some(MAX_DOWNLOAD_BYTES)));
        assert!(exceeds_cap(1, Some(MAX_DOWNLOAD_BYTES + 1)));
        assert!(exceeds_cap(MAX_DOWNLOAD_BYTES + 1, None));
    }

    #[test]
    fn download_url_must_be_https_unless_insecure_loopback_is_configured() {
        let url = |text: &str| reqwest::Url::parse(text).unwrap();
        for insecure in [false, true] {
            assert!(allowed_download_url(
                &url("https://github.com/a/b/releases/download/v1/x.tar.gz"),
                insecure
            ));
            assert!(!allowed_download_url(
                &url("http://example.com/x.tar.gz"),
                insecure
            ));
            assert!(!allowed_download_url(
                &url("http://192.168.1.2/x.tar.gz"),
                insecure
            ));
            assert!(!allowed_download_url(
                &url("file:///tmp/x.tar.gz"),
                insecure
            ));
            assert!(!allowed_download_url(
                &url("ftp://example.com/x.tar.gz"),
                insecure
            ));
        }
        assert!(!allowed_download_url(
            &url("http://127.0.0.1:8765/x.tar.gz"),
            false
        ));
        assert!(allowed_download_url(
            &url("http://127.0.0.1:8765/x.tar.gz"),
            true
        ));
    }

    #[test]
    fn webview_gets_no_plugin_permissions() {
        let capability: serde_json::Value =
            serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        for permission in capability["permissions"].as_array().unwrap() {
            let permission = permission.as_str().unwrap();
            assert!(!permission.starts_with("updater:"), "{permission}");
            assert!(!permission.starts_with("process:"), "{permission}");
        }
    }

    #[test]
    fn append_chunk_stops_at_the_cap() {
        let mut buffer = vec![0u8; usize::try_from(MAX_DOWNLOAD_BYTES).unwrap() - 2];
        assert_eq!(append_chunk(&mut buffer, &[1, 2]), Ok(()));
        assert_eq!(buffer.len() as u64, MAX_DOWNLOAD_BYTES);
        assert_eq!(append_chunk(&mut buffer, &[3]), Err(UpdateError::TooLarge));
        assert_eq!(buffer.len() as u64, MAX_DOWNLOAD_BYTES);
        let mut empty = Vec::new();
        assert_eq!(append_chunk(&mut empty, &[]), Ok(()));
        assert!(empty.is_empty());
    }

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn pending(from: &str, was_on: bool) -> Pending {
        Pending {
            from_version: from.into(),
            launch_at_login_was_on: was_on,
        }
    }

    #[test]
    fn post_update_only_after_a_real_upgrade() {
        let current = version("0.3.1");
        let on = LoginItemStatus::Disabled;
        assert_eq!(
            post_update_action(&current, None, on, None),
            PostUpdateAction::Ignore
        );
        for from in ["0.3.1", "0.3.2", "1.0.0", "garbage"] {
            assert_eq!(
                post_update_action(&current, Some(&pending(from, true)), on, None),
                PostUpdateAction::Ignore,
                "{from}"
            );
        }
        assert_eq!(
            post_update_action(&current, Some(&pending("0.3.0", true)), on, None),
            PostUpdateAction::Notice { register: true }
        );
        assert_eq!(
            post_update_action(
                &version("0.3.0"),
                Some(&pending("0.3.0-test.1", true)),
                on,
                None
            ),
            PostUpdateAction::Notice { register: true }
        );
    }

    #[test]
    fn post_update_registers_only_when_needed_and_possible() {
        let current = version("0.3.1");
        let was_on = pending("0.3.0", true);
        assert_eq!(
            post_update_action(
                &current,
                Some(&pending("0.3.0", false)),
                LoginItemStatus::Disabled,
                None
            ),
            PostUpdateAction::Notice { register: false }
        );
        assert_eq!(
            post_update_action(&current, Some(&was_on), LoginItemStatus::Enabled, None),
            PostUpdateAction::Notice { register: false }
        );
        assert_eq!(
            post_update_action(
                &current,
                Some(&was_on),
                LoginItemStatus::RequiresApproval,
                None
            ),
            PostUpdateAction::Notice { register: true }
        );
        assert_eq!(
            post_update_action(
                &current,
                Some(&was_on),
                LoginItemStatus::Disabled,
                Some(ISSUE)
            ),
            PostUpdateAction::Notice { register: false }
        );
    }

    #[test]
    fn login_outcome_follows_the_status_after_registering() {
        assert_eq!(
            login_outcome(true, LoginItemStatus::Enabled),
            LoginItemOutcome::ReRegistered
        );
        assert_eq!(
            login_outcome(true, LoginItemStatus::RequiresApproval),
            LoginItemOutcome::RequiresApproval
        );
        assert_eq!(
            login_outcome(true, LoginItemStatus::Disabled),
            LoginItemOutcome::Failed
        );
        assert_eq!(
            login_outcome(false, LoginItemStatus::Enabled),
            LoginItemOutcome::Failed
        );
    }

    #[test]
    fn plugin_errors_map_to_contract_kinds() {
        let io = || PluginError::Io(std::io::Error::other("disk"));
        assert_eq!(
            classify(&PluginError::ReleaseNotFound, Stage::Check),
            UpdateError::Network
        );
        assert_eq!(
            classify(&PluginError::Network("status 500".into()), Stage::Download),
            UpdateError::Network
        );
        assert_eq!(
            classify(&PluginError::MissingSignedVersion, Stage::Download),
            UpdateError::Signature
        );
        assert_eq!(
            classify(
                &PluginError::SignedVersionMismatch {
                    signed: "0.3.0".into(),
                    announced: "0.4.0".into()
                },
                Stage::Install
            ),
            UpdateError::Signature
        );
        assert_eq!(classify(&io(), Stage::Install), UpdateError::Install);
        assert_eq!(classify(&io(), Stage::Check), UpdateError::Unknown);
        assert_eq!(
            classify(&PluginError::TempDirNotOnSameMountPoint, Stage::Install),
            UpdateError::Install
        );
        assert_eq!(
            classify(&PluginError::EmptyEndpoints, Stage::Check),
            UpdateError::Unknown
        );
    }

    fn info(version: &str) -> ReleaseInfo {
        ReleaseInfo {
            version: version.into(),
            notes: Some("Ghi chú\u{202E}".into()),
            date: Some(1_793_430_000_000),
        }
    }

    #[test]
    fn a_blocked_location_stays_blocked_when_an_update_is_found() {
        let mut machine: Machine<u32> = Machine::new("0.3.0".into(), true, None, Some(ISSUE), None);
        assert!(machine.begin_check());
        let info = ReleaseInfo {
            version: "0.3.1".into(),
            notes: None,
            date: None,
        };
        let now = from_unix_ms(1_000);
        assert_eq!(machine.finish_check(Ok(Some((info, 7))), now), Some(1_000));
        assert_eq!(
            machine.state().phase,
            UpdatePhase::Blocked {
                reason: ISSUE.into()
            }
        );
        assert_eq!(machine.begin_install(None), None);
        assert!(machine.begin_check());
        machine.finish_check(Ok(None), now);
        assert_eq!(machine.state().phase, UpdatePhase::UpToDate);
    }

    fn machine() -> Machine<u32> {
        Machine::new("0.3.0".into(), true, None, None, None)
    }

    #[test]
    fn check_transitions_and_last_check_only_on_success() {
        let mut machine = machine();
        assert!(machine.begin_check());
        assert!(!machine.begin_check());
        assert_eq!(
            machine.finish_check(Err(UpdateError::Network), at(NOW)),
            None
        );
        assert_eq!(
            machine.state().phase,
            UpdatePhase::Failed {
                error: UpdateError::Network
            }
        );
        assert_eq!(machine.state().last_check_at, None);
        assert_eq!(machine.due(at(NOW), at(NOW - DAY)), Some(at(NOW + HOUR)));
        assert!(machine.begin_check());
        assert_eq!(machine.finish_check(Ok(None), at(NOW)), Some(NOW * 1000));
        assert_eq!(machine.state().phase, UpdatePhase::UpToDate);
        assert_eq!(machine.state().last_check_at, Some(NOW * 1000));
        assert!(machine.begin_check());
        machine.finish_check(Ok(Some((info("0.3.1"), 9))), at(NOW + 5));
        assert_eq!(
            machine.state().phase,
            UpdatePhase::Available {
                version: "0.3.1".into(),
                notes: "Ghi chú".into(),
                date: Some(1_793_430_000_000)
            }
        );
        assert_eq!(machine.state().last_check_at, Some((NOW + 5) * 1000));
    }

    #[test]
    fn install_only_starts_from_available_with_the_held_release() {
        let mut machine = machine();
        assert_eq!(machine.begin_install(None), None);
        assert_eq!(machine.state().phase, UpdatePhase::Idle);
        machine.begin_check();
        machine.finish_check(Ok(Some((info("0.3.1"), 9))), at(NOW));
        assert_eq!(machine.begin_install(None), Some(9));
        assert_eq!(
            machine.state().phase,
            UpdatePhase::Downloading {
                received: 0,
                total: None
            }
        );
        assert_eq!(machine.begin_install(None), None);
        assert!(!machine.begin_check());
        machine.progress(10, Some(20));
        assert_eq!(
            machine.state().phase,
            UpdatePhase::Downloading {
                received: 10,
                total: Some(20)
            }
        );
        machine.installing();
        assert!(!machine.begin_check());
        assert_eq!(machine.begin_install(None), None);
        machine.progress(30, None);
        assert_eq!(machine.state().phase, UpdatePhase::Installing);
    }

    #[test]
    fn location_issue_blocks_install_but_not_checks() {
        let mut machine: Machine<u32> = Machine::new("0.3.0".into(), true, None, Some(ISSUE), None);
        assert_eq!(
            machine.state().phase,
            UpdatePhase::Blocked {
                reason: ISSUE.into()
            }
        );
        assert!(machine.begin_check());
        machine.finish_check(Ok(Some((info("0.3.1"), 4))), at(NOW));
        assert_eq!(machine.begin_install(Some(ISSUE)), None);
        assert_eq!(
            machine.state().phase,
            UpdatePhase::Blocked {
                reason: ISSUE.into()
            }
        );
        assert_eq!(machine.begin_install(None), None);
    }

    #[test]
    fn auto_check_toggle_reports_changes() {
        let mut machine = machine();
        assert!(!machine.set_auto_check(true));
        assert!(machine.set_auto_check(false));
        assert!(!machine.state().auto_check);
        assert_eq!(machine.due(at(NOW), at(NOW - DAY)), None);
    }

    #[test]
    fn post_update_notice_clears_after_it_was_shown_then_hidden() {
        let notice = PostUpdate {
            from_version: "0.3.0".into(),
            login_item: LoginItemOutcome::Unchanged,
        };
        let mut machine: Machine<u32> =
            Machine::new("0.3.1".into(), true, None, None, Some(notice.clone()));
        assert!(!machine.visibility_changed(false));
        assert_eq!(machine.state().post_update, Some(notice));
        assert!(!machine.visibility_changed(true));
        assert!(!machine.visibility_changed(true));
        assert!(machine.visibility_changed(false));
        assert_eq!(machine.state().post_update, None);
        assert!(!machine.visibility_changed(true));
        assert!(!machine.visibility_changed(false));
    }

    struct FakeSource {
        check: RefCell<Option<CheckResult<u32>>>,
        chunks: Vec<(u64, Option<u64>)>,
        download: Result<Vec<u8>, UpdateError>,
        install: Result<(), UpdateError>,
        installed: RefCell<Vec<(u32, Vec<u8>)>>,
    }

    impl FakeSource {
        fn new() -> Self {
            FakeSource {
                check: RefCell::new(Some(Ok(None))),
                chunks: vec![(5, Some(10)), (10, Some(10))],
                download: Ok(vec![1, 2, 3]),
                install: Ok(()),
                installed: RefCell::new(Vec::new()),
            }
        }
    }

    impl UpdateSource for FakeSource {
        type Release = u32;

        fn check(&self) -> CheckResult<u32> {
            self.check.borrow_mut().take().expect("one check")
        }

        fn download(
            &self,
            _release: &u32,
            progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<Vec<u8>, UpdateError> {
            for (received, total) in &self.chunks {
                progress(*received, *total);
            }
            self.download.clone()
        }

        fn install(&self, release: &u32, bytes: &[u8]) -> Result<(), UpdateError> {
            self.installed.borrow_mut().push((*release, bytes.to_vec()));
            self.install
        }
    }

    #[test]
    fn perform_check_publishes_and_returns_the_check_time() {
        let machine = Mutex::new(machine());
        let source = FakeSource::new();
        *source.check.borrow_mut() = Some(Ok(Some((info("0.4.0"), 3))));
        let notified = Cell::new(0);
        lock(&machine).begin_check();
        let checked = perform_check(&machine, &source, &|| notified.set(notified.get() + 1));
        assert!(checked.is_some());
        assert_eq!(notified.get(), 1);
        assert!(matches!(
            lock(&machine).state().phase,
            UpdatePhase::Available { .. }
        ));
        let failing = FakeSource::new();
        *failing.check.borrow_mut() = Some(Err(UpdateError::Timeout));
        lock(&machine).begin_check();
        assert_eq!(perform_check(&machine, &failing, &|| {}), None);
        assert_eq!(
            lock(&machine).state().phase,
            UpdatePhase::Failed {
                error: UpdateError::Timeout
            }
        );
        assert!(lock(&machine).state().last_check_at.is_some());
    }

    fn available_machine() -> Mutex<Machine<u32>> {
        let mut machine = machine();
        machine.begin_check();
        machine.finish_check(Ok(Some((info("0.4.0"), 3))), at(NOW));
        Mutex::new(machine)
    }

    #[test]
    fn perform_install_downloads_then_installs_the_verified_bytes() {
        let machine = available_machine();
        let release = lock(&machine).begin_install(None).unwrap();
        let source = FakeSource::new();
        let phases = RefCell::new(Vec::new());
        let ok = perform_install(&machine, &source, &release, &|| {
            phases
                .borrow_mut()
                .push(lock(&machine).state().phase.clone());
        });
        assert!(ok);
        assert_eq!(*source.installed.borrow(), vec![(3, vec![1, 2, 3])]);
        let phases = phases.into_inner();
        assert_eq!(
            phases.first(),
            Some(&UpdatePhase::Downloading {
                received: 5,
                total: Some(10)
            })
        );
        assert!(phases.contains(&UpdatePhase::Downloading {
            received: 10,
            total: Some(10)
        }));
        assert_eq!(phases.last(), Some(&UpdatePhase::Installing));
    }

    #[test]
    fn perform_install_failures_end_in_failed_without_installing() {
        let machine = available_machine();
        let release = lock(&machine).begin_install(None).unwrap();
        let mut source = FakeSource::new();
        source.download = Err(UpdateError::TooLarge);
        assert!(!perform_install(&machine, &source, &release, &|| {}));
        assert!(source.installed.borrow().is_empty());
        assert_eq!(
            lock(&machine).state().phase,
            UpdatePhase::Failed {
                error: UpdateError::TooLarge
            }
        );
        assert_eq!(lock(&machine).begin_install(None), None);

        let machine = available_machine();
        let release = lock(&machine).begin_install(None).unwrap();
        let mut source = FakeSource::new();
        source.install = Err(UpdateError::Signature);
        assert!(!perform_install(&machine, &source, &release, &|| {}));
        assert_eq!(
            lock(&machine).state().phase,
            UpdatePhase::Failed {
                error: UpdateError::Signature
            }
        );
    }
}
