use objc2::MainThreadMarker;
use objc2_app_kit::{NSFont, NSFontWeightRegular};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuBuilder, MenuEvent, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Runtime};

use crate::model::{MetricState, PowerSample};
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TitleFormat {
    #[default]
    Default,
    Compact,
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

pub fn format_title(
    cpu_pct: Option<f32>,
    power: &MetricState<PowerSample>,
    ram_pct: Option<f32>,
    format: TitleFormat,
) -> String {
    let cpu = format_pct(cpu_pct);
    let watts = format_watts(power);
    let ram = format_pct(ram_pct);
    match format {
        TitleFormat::Default => format!("CPU {cpu}% · {watts}W · RAM {ram}%"),
        TitleFormat::Compact => format!("{cpu}% {watts}W {ram}%"),
    }
}

pub struct TrayMenu<R: Runtime> {
    launch_at_login: CheckMenuItem<R>,
}

fn handle_menu<R: Runtime>(app: &AppHandle<R>, event: &MenuEvent) {
    match event.id().as_ref() {
        MENU_OPEN => popover::show(app, Anchor::Tray, Some(RequestedView::Overview)),
        MENU_SETTINGS => popover::show(app, Anchor::Tray, Some(RequestedView::Settings)),
        MENU_LAUNCH_AT_LOGIN => {
            let target = !crate::settings::launch_at_login(app);
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
    launch_at_login: bool,
) -> tauri::Result<(TrayIcon<R>, TrayMenu<R>)> {
    let launch_item = CheckMenuItemBuilder::with_id(MENU_LAUNCH_AT_LOGIN, "Khởi động cùng macOS")
        .checked(launch_at_login)
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

pub fn sync_launch_at_login<R: Runtime>(app: &AppHandle<R>, enabled: bool) {
    if let Some(menu) = app.try_state::<TrayMenu<R>>()
        && let Err(err) = menu.launch_at_login.set_checked(enabled)
    {
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
        let title = format_title(Some(23.4), &power(8.44), Some(61.0), TitleFormat::Default);
        assert_eq!(title, "CPU 23% · 8.4W · RAM 61%");
        let small = format_title(Some(5.2), &power(12.6), Some(7.0), TitleFormat::Default);
        assert_eq!(small, "CPU \u{2007}5% · \u{2007}13W · RAM \u{2007}7%");
    }

    #[test]
    fn compact_format() {
        let title = format_title(Some(23.4), &power(8.44), Some(61.0), TitleFormat::Compact);
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
            format_title(Some(1.0), &unavailable, Some(2.0), TitleFormat::Default),
            "CPU \u{2007}1% · –W · RAM \u{2007}2%"
        );
        assert_eq!(
            format_title(None, &MetricState::Loading, None, TitleFormat::Compact),
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
            format_title(None, &MetricState::Loading, None, TitleFormat::Default),
            INITIAL_TITLE
        );
    }
}
