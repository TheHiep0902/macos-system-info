use objc2::MainThreadMarker;
use objc2_app_kit::{NSFont, NSFontWeightRegular};
use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::login_item::LaunchAtLoginStatus;
use crate::model::{MetricState, PowerSample, TemperatureSample};
use crate::popover::{self, Anchor, RequestedView};

pub const TRAY_ID: &str = "main";
pub const INITIAL_TITLE: &str = "CPU –% · –W · RAM –%";
pub const MENU_OPEN: &str = "open";
pub const MENU_LAUNCH_AT_LOGIN: &str = "launch-at-login";
pub const MENU_SETTINGS: &str = "settings";
pub const MENU_QUIT: &str = "quit";

const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-template@2x.png");
const FIGURE_SPACE: char = '\u{2007}';
const UNKNOWN: &str = "–";
const PCT_WIDTH: usize = 2;
const WATTS_WIDTH: usize = 3;
const TEMP_WIDTH: usize = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TitleFormat {
    #[default]
    Default,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TitleMetrics {
    pub cpu: bool,
    pub power: bool,
    pub memory: bool,
    pub temperature: bool,
}

impl Default for TitleMetrics {
    fn default() -> Self {
        TitleMetrics {
            cpu: true,
            power: true,
            memory: true,
            temperature: false,
        }
    }
}

impl TitleMetrics {
    pub fn is_empty(self) -> bool {
        !(self.cpu || self.power || self.memory || self.temperature)
    }
}

pub struct TitleValues<'a> {
    pub cpu_pct: Option<f32>,
    pub power: &'a MetricState<PowerSample>,
    pub ram_pct: Option<f32>,
    pub temperature: &'a MetricState<TemperatureSample>,
}

fn pad(value: String, width: usize) -> String {
    let len = value.chars().count();
    if len >= width {
        return value;
    }
    let mut padded: String = std::iter::repeat_n(FIGURE_SPACE, width - len).collect();
    padded.push_str(&value);
    padded
}

fn format_pct(value: Option<f32>) -> String {
    match value {
        Some(pct) if pct.is_finite() => pad(format!("{:.0}", pct.clamp(0.0, 100.0)), PCT_WIDTH),
        _ => UNKNOWN.to_owned(),
    }
}

fn format_watts(power: &MetricState<PowerSample>) -> String {
    let MetricState::Ok(sample) = power else {
        return UNKNOWN.to_owned();
    };
    let watts = sample.smoothed_cpu_watts;
    if !watts.is_finite() || watts < 0.0 {
        return UNKNOWN.to_owned();
    }
    let tenths = (watts * 10.0).round() / 10.0;
    let text = if tenths < 10.0 {
        format!("{tenths:.1}")
    } else {
        format!("{:.0}", watts.round())
    };
    pad(text, WATTS_WIDTH)
}

fn format_temp(temperature: &MetricState<TemperatureSample>) -> String {
    let MetricState::Ok(sample) = temperature else {
        return UNKNOWN.to_owned();
    };
    let celsius = sample.celsius;
    if !celsius.is_finite() || celsius < 0.0 {
        return UNKNOWN.to_owned();
    }
    pad(format!("{:.0}", celsius.round()), TEMP_WIDTH)
}

pub fn format_title(
    values: &TitleValues<'_>,
    format: TitleFormat,
    metrics: TitleMetrics,
) -> String {
    let mut parts: Vec<String> = Vec::with_capacity(4);
    if metrics.cpu {
        let cpu = format_pct(values.cpu_pct);
        parts.push(match format {
            TitleFormat::Default => format!("CPU {cpu}%"),
            TitleFormat::Compact => format!("{cpu}%"),
        });
    }
    if metrics.power {
        parts.push(format!("{}W", format_watts(values.power)));
    }
    if metrics.memory {
        let ram = format_pct(values.ram_pct);
        parts.push(match format {
            TitleFormat::Default => format!("RAM {ram}%"),
            TitleFormat::Compact => format!("{ram}%"),
        });
    }
    if metrics.temperature {
        let temp = format_temp(values.temperature);
        parts.push(match format {
            TitleFormat::Default => format!("{temp}°C"),
            TitleFormat::Compact => format!("{temp}°"),
        });
    }
    let separator = match format {
        TitleFormat::Default => " · ",
        TitleFormat::Compact => " ",
    };
    parts.join(separator)
}

pub struct TrayMenu<R: Runtime> {
    launch_at_login: CheckMenuItem<R>,
}

fn handle_menu<R: Runtime>(app: &AppHandle<R>, event: &MenuEvent) {
    match event.id().as_ref() {
        MENU_OPEN => popover::show(app, Anchor::Tray, Some(RequestedView::Overview)),
        MENU_SETTINGS => popover::show(app, Anchor::Tray, Some(RequestedView::Settings)),
        MENU_LAUNCH_AT_LOGIN => {
            let target = !crate::settings::launch_at_login().status.is_on();
            crate::settings::set_launch_at_login(app, target);
            popover::refresh_if_visible(app);
        }
        MENU_QUIT => app.exit(0),
        _ => {}
    }
}

fn handle_tray_event<R: Runtime>(tray: &TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        rect,
        ..
    } = event
    {
        popover::toggle(tray.app_handle(), rect);
    }
}

pub fn create<R: Runtime>(
    app: &AppHandle<R>,
    show_icon: bool,
    launch_at_login: LaunchAtLoginStatus,
) -> tauri::Result<(TrayIcon<R>, TrayMenu<R>)> {
    let launch_item = CheckMenuItemBuilder::with_id(MENU_LAUNCH_AT_LOGIN, "Khởi động cùng macOS")
        .checked(launch_at_login.is_on())
        .enabled(launch_at_login != LaunchAtLoginStatus::Blocked)
        .build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id(MENU_OPEN, "Mở Pulsebar").build(app)?)
        .item(&launch_item)
        .item(&MenuItemBuilder::with_id(MENU_SETTINGS, "Cài đặt…").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(MENU_QUIT, "Thoát").build(app)?)
        .build()?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .icon_as_template(true)
        .title(INITIAL_TITLE)
        .tooltip("Pulsebar")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu(app, &event))
        .on_tray_icon_event(handle_tray_event);
    if show_icon {
        builder = builder.icon(Image::from_bytes(TRAY_ICON)?);
    }
    let tray = builder.build(app)?;
    if let Err(err) = apply_monospaced_digits(&tray) {
        log::warn!("could not apply monospaced digit font to tray title: {err}");
    }
    Ok((
        tray,
        TrayMenu {
            launch_at_login: launch_item,
        },
    ))
}

pub fn set_icon_visible<R: Runtime>(tray: &TrayIcon<R>, visible: bool) -> tauri::Result<()> {
    let icon = if visible {
        Some(Image::from_bytes(TRAY_ICON)?)
    } else {
        None
    };
    tray.set_icon_with_as_template(icon, true)
}

pub fn sync_launch_at_login<R: Runtime>(app: &AppHandle<R>, status: LaunchAtLoginStatus) {
    let Some(menu) = app.try_state::<TrayMenu<R>>() else {
        return;
    };
    let result = menu
        .launch_at_login
        .set_checked(status.is_on())
        .and_then(|()| {
            menu.launch_at_login
                .set_enabled(status != LaunchAtLoginStatus::Blocked)
        });
    if let Err(err) = result {
        log::warn!("could not update the launch-at-login menu item: {err}");
    }
}

fn apply_monospaced_digits<R: Runtime>(tray: &TrayIcon<R>) -> tauri::Result<()> {
    let applied = tray.with_inner_tray_icon(|inner| {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(button) = inner.ns_status_item().and_then(|item| item.button(mtm)) else {
            return false;
        };
        let size = NSFont::menuBarFontOfSize(0.0).pointSize();
        let font =
            NSFont::monospacedDigitSystemFontOfSize_weight(size, unsafe { NSFontWeightRegular });
        button.setFont(Some(&font));
        true
    })?;
    if !applied {
        log::warn!("tray status item button unavailable; title keeps the default font");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(
        cpu_pct: Option<f32>,
        power: &MetricState<PowerSample>,
        ram_pct: Option<f32>,
        format: TitleFormat,
    ) -> String {
        let values = TitleValues {
            cpu_pct,
            power,
            ram_pct,
            temperature: &MetricState::Loading,
        };
        format_title(&values, format, TitleMetrics::default())
    }

    fn temp(celsius: f32) -> MetricState<TemperatureSample> {
        MetricState::Ok(TemperatureSample {
            celsius,
            source: "smc:TC0D".into(),
        })
    }

    fn with_temp(
        temperature: &MetricState<TemperatureSample>,
        format: TitleFormat,
        metrics: TitleMetrics,
    ) -> String {
        let values = TitleValues {
            cpu_pct: Some(23.4),
            power: &power(8.44),
            ram_pct: Some(61.0),
            temperature,
        };
        format_title(&values, format, metrics)
    }

    fn all_metrics() -> TitleMetrics {
        TitleMetrics {
            temperature: true,
            ..TitleMetrics::default()
        }
    }

    fn power(watts: f32) -> MetricState<PowerSample> {
        MetricState::Ok(PowerSample {
            cpu_watts: watts,
            smoothed_cpu_watts: watts,
            source: "ioreport".into(),
            scope: "package".into(),
        })
    }

    #[test]
    fn default_format_pads_with_figure_space() {
        let title = render(Some(23.4), &power(8.44), Some(61.0), TitleFormat::Default);
        assert_eq!(title, "CPU 23% · 8.4W · RAM 61%");
        let small = render(Some(5.2), &power(12.6), Some(7.0), TitleFormat::Default);
        assert_eq!(small, "CPU \u{2007}5% · \u{2007}13W · RAM \u{2007}7%");
    }

    #[test]
    fn compact_format() {
        let title = render(Some(23.4), &power(8.44), Some(61.0), TitleFormat::Compact);
        assert_eq!(title, "23% 8.4W 61%");
    }

    #[test]
    fn watts_use_one_decimal_below_ten_and_integers_above() {
        assert_eq!(format_watts(&power(0.04)), "0.0");
        assert_eq!(format_watts(&power(9.94)), "9.9");
        assert_eq!(format_watts(&power(9.96)), "\u{2007}10");
        assert_eq!(format_watts(&power(10.4)), "\u{2007}10");
        assert_eq!(format_watts(&power(123.6)), "124");
    }

    #[test]
    fn unavailable_or_loading_power_shows_dash() {
        let unavailable = MetricState::Unavailable { reason: "x".into() };
        assert_eq!(
            render(Some(1.0), &unavailable, Some(2.0), TitleFormat::Default),
            "CPU \u{2007}1% · –W · RAM \u{2007}2%"
        );
        assert_eq!(
            render(None, &MetricState::Loading, None, TitleFormat::Compact),
            "–% –W –%"
        );
        assert_eq!(format_watts(&power(f32::NAN)), "–");
        assert_eq!(format_watts(&power(-1.0)), "–");
    }

    #[test]
    fn percentages_are_clamped_and_rounded() {
        assert_eq!(format_pct(Some(99.6)), "100");
        assert_eq!(format_pct(Some(-3.0)), "\u{2007}0");
        assert_eq!(format_pct(Some(250.0)), "100");
        assert_eq!(format_pct(Some(f32::INFINITY)), "–");
    }

    #[test]
    fn initial_title_matches_unknown_default_format() {
        assert_eq!(
            render(None, &MetricState::Loading, None, TitleFormat::Default),
            INITIAL_TITLE
        );
    }

    #[test]
    fn temperature_is_appended_last_in_both_formats() {
        assert_eq!(
            with_temp(&temp(54.5), TitleFormat::Default, all_metrics()),
            "CPU 23% · 8.4W · RAM 61% · 55°C"
        );
        assert_eq!(
            with_temp(&temp(54.4), TitleFormat::Compact, all_metrics()),
            "23% 8.4W 61% 54°"
        );
        assert_eq!(
            with_temp(&temp(7.0), TitleFormat::Compact, all_metrics()),
            "23% 8.4W 61% \u{2007}7°"
        );
        assert_eq!(
            with_temp(&temp(101.0), TitleFormat::Compact, all_metrics()),
            "23% 8.4W 61% 101°"
        );
    }

    #[test]
    fn unavailable_temperature_shows_dash() {
        let unavailable = MetricState::Unavailable { reason: "x".into() };
        assert_eq!(
            with_temp(&unavailable, TitleFormat::Default, all_metrics()),
            "CPU 23% · 8.4W · RAM 61% · –°C"
        );
        assert_eq!(
            with_temp(&MetricState::Loading, TitleFormat::Compact, all_metrics()),
            "23% 8.4W 61% –°"
        );
        assert_eq!(format_temp(&temp(f32::NAN)), "–");
        assert_eq!(format_temp(&temp(-3.0)), "–");
    }

    #[test]
    fn metrics_select_parts_in_fixed_order() {
        let only_temp = TitleMetrics {
            cpu: false,
            power: false,
            memory: false,
            temperature: true,
        };
        assert_eq!(
            with_temp(&temp(54.0), TitleFormat::Default, only_temp),
            "54°C"
        );
        let cpu_and_ram = TitleMetrics {
            power: false,
            ..TitleMetrics::default()
        };
        assert_eq!(
            with_temp(&temp(54.0), TitleFormat::Default, cpu_and_ram),
            "CPU 23% · RAM 61%"
        );
        assert_eq!(
            with_temp(&temp(54.0), TitleFormat::Compact, cpu_and_ram),
            "23% 61%"
        );
    }

    #[test]
    fn empty_metrics_give_empty_title() {
        let none = TitleMetrics {
            cpu: false,
            power: false,
            memory: false,
            temperature: false,
        };
        assert!(none.is_empty());
        assert!(!TitleMetrics::default().is_empty());
        assert_eq!(with_temp(&temp(54.0), TitleFormat::Default, none), "");
        assert_eq!(with_temp(&temp(54.0), TitleFormat::Compact, none), "");
    }

    #[test]
    fn title_metrics_default_to_d1_title_and_fill_missing_keys() {
        assert_eq!(
            with_temp(&temp(54.0), TitleFormat::Default, TitleMetrics::default()),
            "CPU 23% · 8.4W · RAM 61%"
        );
        let partial: TitleMetrics = serde_json::from_str(r#"{"temperature":true}"#).unwrap();
        assert_eq!(partial, all_metrics());
        assert_eq!(
            serde_json::to_value(TitleMetrics::default()).unwrap(),
            serde_json::json!({"cpu": true, "power": true, "memory": true, "temperature": false})
        );
    }
}
