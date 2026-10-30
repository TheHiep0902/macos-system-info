use std::cell::OnceCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, MainThreadMarker};
use objc2_app_kit::{
    NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace, NSFont, NSFontAttributeName,
    NSFontWeightRegular, NSForegroundColorAttributeName, NSGraphicsContext, NSImage, NSScreen,
    NSStringDrawing,
};
use objc2_foundation::{
    NSAttributedStringKey, NSData, NSDictionary, NSPoint, NSRect, NSSize, NSString,
};
use tauri::image::Image;
use tauri::{AppHandle, Manager, Runtime};

use crate::tray::{self, TRAY_ID};

pub const ICON_HEIGHT: f64 = 18.0;
const FONT_SIZE: f64 = 9.5;
const LOGO_SIZE: f64 = 16.0;
const LOGO_GAP: f64 = 3.0;
const WIDEST_LINE: &str = "↓ 9999+";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayImageState {
    pub show_icon: bool,
    pub lines: Option<[String; 2]>,
}

pub struct TrayImage(pub Mutex<TrayImageState>);

impl TrayImage {
    pub fn new(show_icon: bool) -> Self {
        TrayImage(Mutex::new(TrayImageState {
            show_icon,
            lines: None,
        }))
    }
}

static RENDER_FAILED: AtomicBool = AtomicBool::new(false);

pub fn line_baselines(cap_height: f64, height: f64) -> [f64; 2] {
    let half = height / 2.0;
    let inset = ((half - cap_height) / 2.0).max(0.0);
    [half + inset, inset]
}

pub fn image_width(logo: bool, text_width: f64) -> f64 {
    let logo_width = if logo { LOGO_SIZE + LOGO_GAP } else { 0.0 };
    (logo_width + text_width.ceil()).ceil()
}

pub fn update<R: Runtime>(app: &AppHandle<R>, change: impl FnOnce(&mut TrayImageState)) {
    let Some(image) = app.try_state::<TrayImage>() else {
        return;
    };
    {
        let mut state = image.0.lock().unwrap_or_else(PoisonError::into_inner);
        let before = state.clone();
        change(&mut state);
        if *state == before && !RENDER_FAILED.load(Ordering::Relaxed) {
            return;
        }
    }
    let handle = app.clone();
    if let Err(err) = app.run_on_main_thread(move || apply(&handle)) {
        log::warn!("could not schedule the tray image update: {err}");
    }
}

fn apply<R: Runtime>(app: &AppHandle<R>) {
    let Some(image) = app.try_state::<TrayImage>() else {
        return;
    };
    let state = image
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let result = match &state.lines {
        None => tray::set_icon_visible(&tray, state.show_icon).map_err(|err| err.to_string()),
        Some(lines) => render(state.show_icon, lines).and_then(|rendered| {
            tray.set_icon_with_as_template(Some(rendered), true)
                .map_err(|err| err.to_string())
        }),
    };
    match result {
        Ok(()) => {
            if RENDER_FAILED.swap(false, Ordering::Relaxed) {
                log::info!("tray image update recovered");
            }
        }
        Err(err) => {
            if !RENDER_FAILED.swap(true, Ordering::Relaxed) {
                log::warn!("tray image update failed: {err}");
            }
        }
    }
}

fn render(logo: bool, lines: &[String; 2]) -> Result<Image<'static>, String> {
    let mtm = MainThreadMarker::new().ok_or("tray image must be drawn on the main thread")?;
    autoreleasepool(|_| {
        CACHE.with(|cache| draw(mtm, cache.get_or_init(drawing_cache), logo, lines))
    })
}

struct DrawingCache {
    font: Retained<NSFont>,
    attributes: Retained<NSDictionary<NSAttributedStringKey, AnyObject>>,
    text_width: f64,
    logo: Option<Retained<NSImage>>,
}

thread_local! {
    static CACHE: OnceCell<DrawingCache> = const { OnceCell::new() };
}

fn drawing_cache() -> DrawingCache {
    let font =
        NSFont::monospacedDigitSystemFontOfSize_weight(FONT_SIZE, unsafe { NSFontWeightRegular });
    let color = NSColor::blackColor();
    let keys: [&NSAttributedStringKey; 2] =
        unsafe { [NSFontAttributeName, NSForegroundColorAttributeName] };
    let values: [&AnyObject; 2] = [&font, &color];
    let attributes = NSDictionary::from_slices(&keys, &values);
    let text_width =
        unsafe { NSString::from_str(WIDEST_LINE).sizeWithAttributes(Some(&attributes)) }
            .width
            .ceil();
    let logo = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(tray::TRAY_ICON));
    DrawingCache {
        font,
        attributes,
        text_width,
        logo,
    }
}

fn draw(
    mtm: MainThreadMarker,
    cache: &DrawingCache,
    logo: bool,
    lines: &[String; 2],
) -> Result<Image<'static>, String> {
    let scale = NSScreen::mainScreen(mtm)
        .map(|screen| screen.backingScaleFactor())
        .filter(|scale| scale.is_finite() && *scale >= 1.0)
        .unwrap_or(2.0)
        .round();
    let DrawingCache {
        font,
        attributes,
        text_width,
        logo: logo_image,
    } = cache;
    let text_width = *text_width;
    let width = image_width(logo, text_width);
    let pixels_wide = (width * scale) as isize;
    let pixels_high = (ICON_HEIGHT * scale) as isize;
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            pixels_wide,
            pixels_high,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            pixels_wide * 4,
            32,
        )
    }
    .ok_or("could not allocate the tray bitmap")?;
    rep.setSize(NSSize::new(width, ICON_HEIGHT));
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)
        .ok_or("could not create the tray drawing context")?;
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    let mut text_x = 0.0;
    if logo {
        if let Some(image) = logo_image {
            image.drawInRect(NSRect::new(
                NSPoint::new(0.0, (ICON_HEIGHT - LOGO_SIZE) / 2.0),
                NSSize::new(LOGO_SIZE, LOGO_SIZE),
            ));
        }
        text_x = LOGO_SIZE + LOGO_GAP;
    }
    let baselines = line_baselines(font.capHeight(), ICON_HEIGHT);
    for (line, baseline) in lines.iter().zip(baselines) {
        let (arrow, value) = line.split_once(' ').unwrap_or(("", line.as_str()));
        let y = baseline + font.descender();
        let value = NSString::from_str(value);
        let value_width = unsafe { value.sizeWithAttributes(Some(attributes)) }.width;
        unsafe {
            NSString::from_str(arrow)
                .drawAtPoint_withAttributes(NSPoint::new(text_x, y), Some(attributes));
            value.drawAtPoint_withAttributes(
                NSPoint::new(text_x + (text_width - value_width).max(0.0), y),
                Some(attributes),
            );
        }
    }
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    copy_pixels(&rep, pixels_wide as usize, pixels_high as usize)
}

fn copy_pixels(
    rep: &NSBitmapImageRep,
    pixels_wide: usize,
    pixels_high: usize,
) -> Result<Image<'static>, String> {
    let data = rep.bitmapData();
    let stride = usize::try_from(rep.bytesPerRow()).map_err(|_| "invalid bitmap stride")?;
    let row = pixels_wide * 4;
    if data.is_null() || stride < row {
        return Err("tray bitmap has no pixel data".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, stride * pixels_high) };
    let mut rgba = Vec::with_capacity(row * pixels_high);
    for chunk in bytes.chunks_exact(stride) {
        rgba.extend_from_slice(&chunk[..row]);
    }
    Ok(Image::new_owned(
        rgba,
        pixels_wide as u32,
        pixels_high as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_lines_fit_inside_the_icon_height() {
        for cap_height in [6.0, 6.7, 7.5, 9.0] {
            let [top, bottom] = line_baselines(cap_height, ICON_HEIGHT);
            assert!(bottom >= 0.0);
            assert!(bottom + cap_height <= top);
            assert!(top + cap_height <= ICON_HEIGHT);
        }
    }

    #[test]
    fn baselines_never_go_negative_for_oversized_caps() {
        let [top, bottom] = line_baselines(12.0, ICON_HEIGHT);
        assert_eq!(bottom, 0.0);
        assert_eq!(top, ICON_HEIGHT / 2.0);
    }

    #[test]
    fn width_depends_only_on_logo_and_reference_text() {
        assert_eq!(image_width(false, 41.2), 42.0);
        assert_eq!(image_width(true, 41.2), 61.0);
    }
}
