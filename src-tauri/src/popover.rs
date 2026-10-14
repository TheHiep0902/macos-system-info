use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::utils::config::WindowEffectsConfig;
use tauri::webview::PageLoadEvent;
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Monitor, Rect, Runtime, Theme,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::ipc::{AppState, EVENT_POPOVER_VISIBILITY, POPOVER_LABEL};
use crate::settings::{Settings, SettingsStore, ThemePreference};
use crate::tray::TRAY_ID;

pub const WIDTH: f64 = 360.0;
pub const DEFAULT_HEIGHT: u32 = 520;
pub const MIN_HEIGHT: u32 = 200;
pub const MAX_HEIGHT: u32 = 640;
pub const BLUR_DEBOUNCE: Duration = Duration::from_millis(200);
pub const MEMORY_SAVER_DELAY: Duration = Duration::from_secs(300);
pub const CORNER_RADIUS: f64 = 12.0;

const ANCHOR_GAP: f64 = 4.0;
const SCREEN_MARGIN: f64 = 8.0;
const CURSOR_TOLERANCE: f64 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestedView {
    Overview,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Visibility {
    pub visible: bool,
    pub view: Option<RequestedView>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Frame {
    fn point(x: f64, y: f64) -> Self {
        Frame {
            x,
            y,
            width: 0.0,
            height: 0.0,
        }
    }

    fn right(&self) -> f64 {
        self.x + self.width
    }

    fn bottom(&self) -> f64 {
        self.y + self.height
    }

    fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    fn scaled_down(&self, scale: f64) -> Self {
        Frame {
            x: self.x / scale,
            y: self.y / scale,
            width: self.width / scale,
            height: self.height / scale,
        }
    }

    fn expanded(&self, by: f64) -> Self {
        Frame {
            x: self.x - by,
            y: self.y - by,
            width: self.width + 2.0 * by,
            height: self.height + 2.0 * by,
        }
    }

    fn is_usable_rect(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub bounds: Frame,
    pub work_area: Frame,
    pub scale: f64,
}

impl Screen {
    fn from_monitor(monitor: &Monitor) -> Option<Self> {
        let scale = monitor.scale_factor();
        if !(scale.is_finite() && scale > 0.0) {
            return None;
        }
        let position = monitor.position();
        let size = monitor.size();
        let work = monitor.work_area();
        let physical = |x: i32, y: i32, width: u32, height: u32| Frame {
            x: f64::from(x),
            y: f64::from(y),
            width: f64::from(width),
            height: f64::from(height),
        };
        Some(Screen {
            bounds: physical(position.x, position.y, size.width, size.height).scaled_down(scale),
            work_area: physical(
                work.position.x,
                work.position.y,
                work.size.width,
                work.size.height,
            )
            .scaled_down(scale),
            scale,
        })
    }
}

pub fn clamp_height(height: u32) -> u32 {
    height.clamp(MIN_HEIGHT, MAX_HEIGHT)
}

fn cursor_point(cursor: Option<(f64, f64)>, primary: Option<Screen>) -> Option<(f64, f64)> {
    let scale = primary?.scale;
    let (x, y) = cursor?;
    let point = (x / scale, y / scale);
    (point.0.is_finite() && point.1.is_finite()).then_some(point)
}

pub fn locate_rect(
    rect: Frame,
    cursor: Option<(f64, f64)>,
    screens: &[Screen],
) -> Option<(Frame, Screen)> {
    if !rect.is_usable_rect() {
        return None;
    }
    let candidates: Vec<(Frame, Screen)> = screens
        .iter()
        .filter_map(|screen| {
            let logical = rect.scaled_down(screen.scale);
            screen
                .bounds
                .contains(logical.center())
                .then_some((logical, *screen))
        })
        .collect();
    let near_cursor = cursor.and_then(|point| {
        candidates
            .iter()
            .find(|(logical, _)| logical.expanded(CURSOR_TOLERANCE).contains(point))
    });
    let in_menu_bar = || {
        candidates
            .iter()
            .find(|(logical, screen)| logical.center().1 < screen.work_area.y)
    };
    near_cursor
        .or_else(in_menu_bar)
        .or_else(|| candidates.first())
        .copied()
}

pub fn resolve_anchor(
    tray_rect: Option<Frame>,
    cursor: Option<(f64, f64)>,
    screens: &[Screen],
    primary: Option<Screen>,
) -> Option<(Frame, Screen)> {
    let cursor = cursor_point(cursor, primary);
    tray_rect
        .and_then(|rect| locate_rect(rect, cursor, screens))
        .or_else(|| {
            let point = cursor?;
            screens
                .iter()
                .find(|screen| screen.bounds.contains(point))
                .map(|screen| (Frame::point(point.0, point.1), *screen))
        })
        .or_else(|| {
            primary.map(|screen| {
                (
                    Frame::point(screen.work_area.right(), screen.work_area.y),
                    screen,
                )
            })
        })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub top: f64,
    pub work_area: Frame,
}

impl Placement {
    pub fn origin(&self, height: f64) -> (f64, f64) {
        let max_y = (self.work_area.bottom() - height).max(self.work_area.y);
        (self.x, self.top.min(max_y).round())
    }
}

pub fn place_below(anchor: Frame, width: f64, work_area: Frame) -> Placement {
    let min_x = work_area.x + SCREEN_MARGIN;
    let max_x = (work_area.right() - SCREEN_MARGIN - width).max(min_x);
    let x = (anchor.x + anchor.width / 2.0 - width / 2.0).clamp(min_x, max_x);
    Placement {
        x: x.round(),
        top: anchor.bottom().max(work_area.y) + ANCHOR_GAP,
        work_area,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickAction {
    Show,
    Hide,
    Ignore,
}

pub fn click_action(visible: bool, hidden_at: Option<Instant>, now: Instant) -> ClickAction {
    if visible {
        return ClickAction::Hide;
    }
    match hidden_at {
        Some(at) if now.saturating_duration_since(at) < BLUR_DEBOUNCE => ClickAction::Ignore,
        _ => ClickAction::Show,
    }
}

#[derive(Debug, Default)]
struct Inner {
    visible: bool,
    view: Option<RequestedView>,
    hidden_at: Option<Instant>,
    height: Option<u32>,
    placement: Option<Placement>,
    destroy_at: Option<Instant>,
    reaper_running: bool,
    shutdown: bool,
}

#[derive(Debug, Default)]
pub struct PopoverState {
    inner: Mutex<Inner>,
    wake: Condvar,
}

impl PopoverState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn is_visible(&self) -> bool {
        self.lock().visible
    }

    fn height(&self) -> u32 {
        self.lock().height.unwrap_or(DEFAULT_HEIGHT)
    }

    fn resized(&self, height: u32) -> Option<(f64, f64)> {
        let mut inner = self.lock();
        inner.height = Some(height);
        if !inner.visible {
            return None;
        }
        inner
            .placement
            .map(|placement| placement.origin(f64::from(height)))
    }

    fn current(&self) -> (bool, Option<RequestedView>) {
        let inner = self.lock();
        (inner.visible, inner.view)
    }

    fn click(&self, now: Instant) -> ClickAction {
        let inner = self.lock();
        click_action(inner.visible, inner.hidden_at, now)
    }

    fn mark_shown(&self, view: Option<RequestedView>, placement: Option<Placement>) -> bool {
        let was_visible = {
            let mut inner = self.lock();
            inner.view = view;
            inner.destroy_at = None;
            let was_visible = std::mem::replace(&mut inner.visible, true);
            if !was_visible {
                inner.placement = placement;
            }
            was_visible
        };
        self.wake.notify_all();
        was_visible
    }

    fn mark_hidden(&self, now: Instant) -> bool {
        let mut inner = self.lock();
        if !inner.visible {
            return false;
        }
        inner.visible = false;
        inner.view = None;
        inner.hidden_at = Some(now);
        true
    }

    fn schedule_destroy(&self, at: Instant) -> bool {
        let start_reaper = {
            let mut inner = self.lock();
            inner.destroy_at = Some(at);
            !std::mem::replace(&mut inner.reaper_running, true)
        };
        self.wake.notify_all();
        start_reaper
    }

    fn cancel_destroy(&self) {
        self.lock().destroy_at = None;
        self.wake.notify_all();
    }

    fn reaper_failed(&self) {
        self.lock().reaper_running = false;
    }

    pub fn shutdown(&self) {
        {
            let mut inner = self.lock();
            inner.shutdown = true;
            inner.destroy_at = None;
        }
        self.wake.notify_all();
    }

    fn wait_for_deadline(&self) -> bool {
        let mut inner = self.lock();
        loop {
            if inner.shutdown {
                return false;
            }
            match inner.destroy_at {
                None => {
                    inner = self
                        .wake
                        .wait(inner)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                Some(at) => {
                    let now = Instant::now();
                    if now >= at {
                        inner.destroy_at = None;
                        return true;
                    }
                    inner = self
                        .wake
                        .wait_timeout(inner, at - now)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Anchor {
    Rect(Rect),
    Tray,
}

fn theme_for(preference: ThemePreference) -> Option<Theme> {
    match preference {
        ThemePreference::System => None,
        ThemePreference::Light => Some(Theme::Light),
        ThemePreference::Dark => Some(Theme::Dark),
    }
}

fn effects_for(reduce_effects: bool) -> Option<WindowEffectsConfig> {
    (!reduce_effects).then(|| {
        EffectsBuilder::new()
            .effects([Effect::LiquidGlassRegular, Effect::Popover])
            .state(EffectState::Active)
            .radius(CORNER_RADIUS)
            .build()
    })
}

fn emit_visibility<R: Runtime>(app: &AppHandle<R>, visibility: Visibility) {
    if let Err(err) = app.emit_to(POPOVER_LABEL, EVENT_POPOVER_VISIBILITY, visibility) {
        log::debug!("popover visibility event not delivered: {err}");
    }
}

pub fn create<R: Runtime>(
    app: &AppHandle<R>,
    settings: &Settings,
) -> tauri::Result<WebviewWindow<R>> {
    let height = app.state::<PopoverState>().height();
    let mut builder =
        WebviewWindowBuilder::new(app, POPOVER_LABEL, WebviewUrl::App("index.html".into()))
            .title("Pulsebar")
            .inner_size(WIDTH, f64::from(height))
            .visible(false)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .skip_taskbar(true)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .accept_first_mouse(true)
            .theme(theme_for(settings.theme))
            .on_page_load(|window, payload| {
                if payload.event() == PageLoadEvent::Finished {
                    let app = window.app_handle();
                    let (visible, view) = app.state::<PopoverState>().current();
                    if visible {
                        emit_visibility(app, Visibility { visible, view });
                    }
                }
            });
    if let Some(effects) = effects_for(settings.reduce_effects) {
        builder = builder.effects(effects);
    }
    let window = builder.build()?;
    let handle = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => hide(&handle, false),
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide(&handle, true);
        }
        _ => {}
    });
    log::debug!("popover window created");
    Ok(window)
}

fn current_settings<R: Runtime>(app: &AppHandle<R>) -> Option<Settings> {
    app.try_state::<SettingsStore>().map(|store| store.get())
}

fn ensure_window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        return Some(window);
    }
    let Some(settings) = current_settings(app) else {
        log::debug!("popover requested before the app finished starting");
        return None;
    };
    create(app, &settings)
        .map_err(|err| log::warn!("could not create popover window: {err}"))
        .ok()
}

fn screens<R: Runtime>(app: &AppHandle<R>) -> Vec<Screen> {
    app.available_monitors()
        .map(|monitors| monitors.iter().filter_map(Screen::from_monitor).collect())
        .unwrap_or_else(|err| {
            log::debug!("monitor list unavailable: {err}");
            Vec::new()
        })
}

fn rect_frame(rect: &Rect) -> Frame {
    let position = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    Frame {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    }
}

fn tray_rect<R: Runtime>(app: &AppHandle<R>) -> Option<Rect> {
    app.tray_by_id(TRAY_ID)?.rect().ok().flatten()
}

fn placement_for<R: Runtime>(app: &AppHandle<R>, anchor: Anchor) -> Option<Placement> {
    let rect = match anchor {
        Anchor::Rect(rect) => Some(rect),
        Anchor::Tray => tray_rect(app),
    };
    let screens = screens(app);
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .as_ref()
        .and_then(Screen::from_monitor);
    let cursor = app
        .cursor_position()
        .ok()
        .map(|position| (position.x, position.y));
    let (anchor, screen) =
        resolve_anchor(rect.as_ref().map(rect_frame), cursor, &screens, primary)?;
    Some(place_below(anchor, WIDTH, screen.work_area))
}

fn mark_hidden<R: Runtime>(app: &AppHandle<R>) -> bool {
    if !app.state::<PopoverState>().mark_hidden(Instant::now()) {
        return false;
    }
    let shared = &app.state::<AppState>().0;
    shared.popover_visible.store(false, Ordering::Release);
    shared.detail_enabled.store(false, Ordering::Relaxed);
    true
}

pub fn show<R: Runtime>(app: &AppHandle<R>, anchor: Anchor, view: Option<RequestedView>) {
    let Some(window) = ensure_window(app) else {
        return;
    };
    let state = app.state::<PopoverState>();
    let height = state.height();
    let placement = if state.is_visible() {
        None
    } else {
        placement_for(app, anchor)
    };
    let already_visible = state.mark_shown(view, placement);
    app.state::<AppState>()
        .0
        .popover_visible
        .store(true, Ordering::Release);
    if let Err(err) = app.show() {
        log::debug!("could not unhide the app before showing the popover: {err}");
    }
    let shown = window
        .set_size(LogicalSize::new(WIDTH, f64::from(height)))
        .and_then(|()| match placement {
            Some(placement) if !already_visible => {
                let (x, y) = placement.origin(f64::from(height));
                window.set_position(LogicalPosition::new(x, y))
            }
            _ => Ok(()),
        })
        .and_then(|()| window.show())
        .and_then(|()| window.set_focus());
    if let Err(err) = shown {
        log::warn!("could not show popover: {err}");
        mark_hidden(app);
        return;
    }
    log::debug!("popover shown");
    emit_visibility(
        app,
        Visibility {
            visible: true,
            view,
        },
    );
}

pub fn hide<R: Runtime>(app: &AppHandle<R>, yield_focus: bool) {
    if !mark_hidden(app) {
        return;
    }
    if let Some(window) = app.get_webview_window(POPOVER_LABEL)
        && let Err(err) = window.hide()
    {
        log::warn!("could not hide popover: {err}");
    }
    if yield_focus && let Err(err) = app.hide() {
        log::debug!("could not return focus to the previous app: {err}");
    }
    log::debug!("popover hidden");
    emit_visibility(
        app,
        Visibility {
            visible: false,
            view: None,
        },
    );
    if current_settings(app).is_some_and(|settings| settings.memory_saver) {
        schedule_destroy(app);
    }
}

pub fn refresh_if_visible<R: Runtime>(app: &AppHandle<R>) {
    let (visible, _) = app.state::<PopoverState>().current();
    if visible {
        emit_visibility(
            app,
            Visibility {
                visible: true,
                view: None,
            },
        );
    }
}

pub fn toggle<R: Runtime>(app: &AppHandle<R>, rect: Rect) {
    let action = app.state::<PopoverState>().click(Instant::now());
    match action {
        ClickAction::Show => show(app, Anchor::Rect(rect), None),
        ClickAction::Hide => hide(app, true),
        ClickAction::Ignore => {
            log::debug!("tray click ignored right after the popover lost focus");
            if let Err(err) = app.hide() {
                log::debug!("could not return focus to the previous app: {err}");
            }
        }
    }
}

pub fn resize<R: Runtime>(app: &AppHandle<R>, height: u32) {
    let height = clamp_height(height);
    let origin = app.state::<PopoverState>().resized(height);
    let Some(window) = app.get_webview_window(POPOVER_LABEL) else {
        return;
    };
    let resized = window
        .set_size(LogicalSize::new(WIDTH, f64::from(height)))
        .and_then(|()| match origin {
            Some((x, y)) => window.set_position(LogicalPosition::new(x, y)),
            None => Ok(()),
        });
    if let Err(err) = resized {
        log::debug!("could not resize popover: {err}");
    }
}

pub fn apply_appearance<R: Runtime>(app: &AppHandle<R>, settings: &Settings) {
    let Some(window) = app.get_webview_window(POPOVER_LABEL) else {
        return;
    };
    if let Err(err) = window.set_theme(theme_for(settings.theme)) {
        log::warn!("could not apply popover theme: {err}");
    }
    if let Err(err) = window.set_effects(effects_for(settings.reduce_effects)) {
        log::warn!("could not apply popover effects: {err}");
    }
}

pub fn memory_saver_changed<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    let state = app.state::<PopoverState>();
    if !enabled {
        state.cancel_destroy();
    } else if !state.is_visible() {
        schedule_destroy(app);
    }
}

fn schedule_destroy<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<PopoverState>();
    if state.schedule_destroy(Instant::now() + MEMORY_SAVER_DELAY) {
        spawn_reaper(app.clone());
    }
}

fn destroy_if_hidden<R: Runtime>(app: &AppHandle<R>) {
    let memory_saver = current_settings(app).is_some_and(|settings| settings.memory_saver);
    if app.state::<PopoverState>().is_visible() || !memory_saver {
        return;
    }
    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        match window.destroy() {
            Ok(()) => log::debug!("popover window released by memory saver"),
            Err(err) => log::warn!("could not release popover window: {err}"),
        }
    }
}

fn spawn_reaper<R: Runtime>(app: AppHandle<R>) {
    let spawned = thread::Builder::new().name("popover-reaper".into()).spawn({
        let app = app.clone();
        move || {
            while app.state::<PopoverState>().wait_for_deadline() {
                let handle = app.clone();
                if let Err(err) = app.run_on_main_thread(move || destroy_if_hidden(&handle)) {
                    log::warn!("could not schedule popover release: {err}");
                }
            }
        }
    });
    if let Err(err) = spawned {
        log::warn!("could not start popover memory saver: {err}");
        app.state::<PopoverState>().reaper_failed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const FIXTURE: &str = include_str!("../../src/mocks/contract-settings.json");

    fn frame(x: f64, y: f64, width: f64, height: f64) -> Frame {
        Frame {
            x,
            y,
            width,
            height,
        }
    }

    fn screen(bounds: Frame, menu_bar: f64, scale: f64) -> Screen {
        Screen {
            bounds,
            work_area: frame(
                bounds.x,
                bounds.y + menu_bar,
                bounds.width,
                bounds.height - menu_bar,
            ),
            scale,
        }
    }

    fn laptop() -> Screen {
        screen(frame(0.0, 0.0, 1440.0, 900.0), 25.0, 2.0)
    }

    fn external() -> Screen {
        screen(frame(1440.0, -200.0, 1920.0, 1080.0), 25.0, 1.0)
    }

    #[test]
    fn visibility_payload_matches_contract_fixture() {
        let all: Value = serde_json::from_str(FIXTURE).unwrap();
        let shown = Visibility {
            visible: true,
            view: Some(RequestedView::Settings),
        };
        let hidden = Visibility {
            visible: false,
            view: None,
        };
        assert_eq!(serde_json::to_value(shown).unwrap(), all["visibilityShown"]);
        assert_eq!(
            serde_json::to_value(hidden).unwrap(),
            all["visibilityHidden"]
        );
        assert_eq!(
            serde_json::to_value(RequestedView::Overview).unwrap(),
            Value::from("overview")
        );
    }

    #[test]
    fn height_is_clamped_to_popover_limits() {
        assert_eq!(clamp_height(0), MIN_HEIGHT);
        assert_eq!(clamp_height(199), MIN_HEIGHT);
        assert_eq!(clamp_height(200), 200);
        assert_eq!(clamp_height(480), 480);
        assert_eq!(clamp_height(640), 640);
        assert_eq!(clamp_height(u32::MAX), MAX_HEIGHT);
    }

    #[test]
    fn popover_is_centred_below_the_tray_icon() {
        let anchor = frame(1000.0, 0.0, 120.0, 24.0);
        let (x, y) = place_below(anchor, WIDTH, laptop().work_area).origin(520.0);
        assert_eq!((x, y), (880.0, 29.0));
    }

    #[test]
    fn popover_is_clamped_inside_the_work_area() {
        let near_right = frame(1400.0, 0.0, 30.0, 24.0);
        assert_eq!(
            place_below(near_right, WIDTH, laptop().work_area).origin(520.0),
            (1072.0, 29.0)
        );
        let near_left = frame(2.0, 0.0, 30.0, 24.0);
        assert_eq!(
            place_below(near_left, WIDTH, laptop().work_area).origin(520.0),
            (8.0, 29.0)
        );
        let tall = place_below(near_left, WIDTH, laptop().work_area).origin(2000.0);
        assert_eq!(tall.1, 25.0);
    }

    #[test]
    fn tray_rect_is_converted_with_the_scale_of_its_own_screen() {
        let screens = [external(), laptop()];
        let retina_rect = frame(2000.0, 0.0, 240.0, 48.0);
        let (anchor, found) = locate_rect(retina_rect, None, &screens).unwrap();
        assert_eq!(found, laptop());
        assert_eq!(anchor, frame(1000.0, 0.0, 120.0, 24.0));

        let external_rect = frame(3000.0, -200.0, 100.0, 24.0);
        let (anchor, found) = locate_rect(external_rect, None, &screens).unwrap();
        assert_eq!(found, external());
        assert_eq!(anchor, external_rect);
        let (x, y) = place_below(anchor, WIDTH, found.work_area).origin(520.0);
        assert_eq!((x, y), (2870.0, -171.0));
    }

    #[test]
    fn cursor_disambiguates_screens_that_share_a_top_edge() {
        let level = screen(frame(1440.0, 0.0, 1920.0, 1080.0), 25.0, 1.0);
        let screens = [laptop(), level];
        let rect = frame(2000.0, 0.0, 100.0, 24.0);
        let on_external =
            resolve_anchor(Some(rect), Some((4100.0, 20.0)), &screens, Some(laptop()));
        assert_eq!(on_external, Some((rect, level)));
        let on_laptop = resolve_anchor(Some(rect), Some((2100.0, 20.0)), &screens, Some(laptop()));
        assert_eq!(on_laptop, Some((frame(1000.0, 0.0, 50.0, 12.0), laptop())));
    }

    #[test]
    fn unusable_tray_rect_falls_back_to_cursor_then_primary_screen() {
        let screens = [laptop(), external()];
        let empty = frame(0.0, 0.0, 0.0, 0.0);
        let nan = frame(f64::NAN, 0.0, 10.0, 10.0);
        let offscreen = frame(-9000.0, -9000.0, 40.0, 40.0);
        for rect in [empty, nan, offscreen] {
            let (anchor, found) =
                resolve_anchor(Some(rect), Some((2400.0, 20.0)), &screens, Some(laptop())).unwrap();
            assert_eq!(found, laptop());
            assert_eq!(anchor, Frame::point(1200.0, 10.0));
        }
        let (anchor, found) =
            resolve_anchor(None, Some((f64::INFINITY, 0.0)), &screens, Some(laptop())).unwrap();
        assert_eq!(found, laptop());
        assert_eq!(anchor, Frame::point(1440.0, 25.0));
        assert!(resolve_anchor(None, None, &screens, None).is_none());
    }

    #[test]
    fn tray_click_toggles_and_is_debounced_after_blur() {
        let now = Instant::now();
        assert_eq!(click_action(true, None, now), ClickAction::Hide);
        assert_eq!(click_action(false, None, now), ClickAction::Show);
        let just_hidden = now - Duration::from_millis(150);
        assert_eq!(
            click_action(false, Some(just_hidden), now),
            ClickAction::Ignore
        );
        let hidden_earlier = now - BLUR_DEBOUNCE;
        assert_eq!(
            click_action(false, Some(hidden_earlier), now),
            ClickAction::Show
        );
    }

    #[test]
    fn state_tracks_visibility_view_and_blur_debounce() {
        let state = PopoverState::default();
        let start = Instant::now();
        assert_eq!(state.click(start), ClickAction::Show);
        assert!(!state.mark_shown(Some(RequestedView::Settings), None));
        assert_eq!(state.current(), (true, Some(RequestedView::Settings)));
        assert!(state.mark_shown(None, None));
        assert_eq!(state.click(start), ClickAction::Hide);
        assert!(state.mark_hidden(start));
        assert!(!state.mark_hidden(start));
        assert_eq!(state.current(), (false, None));
        assert_eq!(
            state.click(start + Duration::from_millis(50)),
            ClickAction::Ignore
        );
        assert_eq!(state.click(start + BLUR_DEBOUNCE), ClickAction::Show);
    }

    #[test]
    fn showing_cancels_a_pending_release() {
        let state = PopoverState::default();
        state.schedule_destroy(Instant::now() + MEMORY_SAVER_DELAY);
        state.mark_shown(None, None);
        assert_eq!(state.lock().destroy_at, None);
    }

    #[test]
    fn resizing_a_visible_popover_keeps_its_top_edge_under_the_icon() {
        let state = PopoverState::default();
        let placement = place_below(frame(1000.0, 0.0, 120.0, 24.0), WIDTH, laptop().work_area);
        assert_eq!(state.resized(400), None);
        assert!(!state.mark_shown(None, Some(placement)));
        assert_eq!(state.resized(400), Some((880.0, 29.0)));
        assert_eq!(state.resized(640), Some((880.0, 29.0)));
        assert_eq!(state.height(), 640);
        let short_screen = Placement {
            work_area: frame(0.0, 25.0, 1440.0, 600.0),
            ..placement
        };
        assert_eq!(short_screen.origin(640.0), (880.0, 25.0));
        assert_eq!(short_screen.origin(300.0), (880.0, 29.0));
        state.mark_hidden(Instant::now());
        assert_eq!(state.resized(300), None);
    }

    #[test]
    fn reopening_replaces_the_previous_placement() {
        let state = PopoverState::default();
        let first = place_below(frame(1000.0, 0.0, 120.0, 24.0), WIDTH, laptop().work_area);
        state.mark_shown(None, Some(first));
        let second = place_below(frame(200.0, 0.0, 120.0, 24.0), WIDTH, laptop().work_area);
        state.mark_shown(None, Some(second));
        assert_eq!(state.resized(300), Some(first.origin(300.0)));
        state.mark_hidden(Instant::now());
        state.mark_shown(None, None);
        assert_eq!(state.resized(300), None);
        state.mark_hidden(Instant::now());
        state.mark_shown(None, Some(second));
        assert_eq!(state.resized(300), Some(second.origin(300.0)));
    }

    #[test]
    fn reaper_fires_for_elapsed_deadlines_and_stops_on_shutdown() {
        let state = PopoverState::default();
        assert!(state.schedule_destroy(Instant::now()));
        assert!(!state.schedule_destroy(Instant::now()));
        assert!(state.wait_for_deadline());
        assert_eq!(state.lock().destroy_at, None);
        state.schedule_destroy(Instant::now() + MEMORY_SAVER_DELAY);
        state.cancel_destroy();
        state.shutdown();
        assert!(!state.wait_for_deadline());
    }

    #[test]
    fn reduced_effects_remove_the_material() {
        assert!(effects_for(true).is_none());
        let effects = effects_for(false).unwrap();
        assert_eq!(
            effects.effects,
            vec![Effect::LiquidGlassRegular, Effect::Popover]
        );
        assert_eq!(effects.radius, Some(CORNER_RADIUS));
        assert_eq!(theme_for(ThemePreference::System), None);
        assert_eq!(theme_for(ThemePreference::Dark), Some(Theme::Dark));
    }
}
