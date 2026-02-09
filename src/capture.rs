use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    CGDisplayBounds, CGImage, CGMainDisplayID, CGWindowID, CGWindowImageOption,
    CGWindowListOption,
};
#[allow(deprecated)]
use objc2_core_graphics::CGWindowListCreateImage;

/// Capture the main display as a CGImage.
/// Returns None if screen recording permission is not granted or capture fails.
#[allow(deprecated)] // CGWindowListCreateImage deprecated in favor of ScreenCaptureKit
pub fn capture_full_screen() -> Option<CFRetained<CGImage>> {
    capture_full_screen_excluding(None)
}

/// Capture the main display, optionally excluding a specific window by its ID.
/// When `exclude_window_id` is Some, captures everything on screen below that window,
/// effectively excluding it from the capture.
#[allow(deprecated)]
pub fn capture_full_screen_excluding(
    exclude_window_id: Option<u32>,
) -> Option<CFRetained<CGImage>> {
    let display_id = CGMainDisplayID();
    let bounds = CGDisplayBounds(display_id);

    let (list_option, window_id) = match exclude_window_id {
        Some(wid) => (
            CGWindowListOption::OptionOnScreenBelowWindow,
            wid as CGWindowID,
        ),
        None => (
            CGWindowListOption::OptionOnScreenOnly,
            0 as CGWindowID,
        ),
    };

    let image = CGWindowListCreateImage(
        bounds,
        list_option,
        window_id,
        CGWindowImageOption::BestResolution,
    );

    if let Some(ref img) = image {
        let width = CGImage::width(Some(img));
        let height = CGImage::height(Some(img));
        eprintln!(
            "Screen captured: {}x{} pixels (display bounds: {},{} {}x{})",
            width, height,
            bounds.origin.x, bounds.origin.y,
            bounds.size.width, bounds.size.height,
        );
    } else {
        eprintln!("Screen capture failed - check Screen Recording permission");
    }

    image
}

/// Check if we have screen recording permission by attempting a minimal capture.
#[allow(deprecated)]
pub fn has_screen_recording_permission() -> bool {
    let display_id = CGMainDisplayID();
    let bounds = CGDisplayBounds(display_id);
    let image = CGWindowListCreateImage(
        bounds,
        CGWindowListOption::OptionOnScreenOnly,
        0 as CGWindowID,
        CGWindowImageOption::NominalResolution,
    );
    image.is_some()
}
