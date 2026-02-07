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
    // Capture exactly the main display (not all displays).
    // CGRect::ZERO is NOT CGRectNull — it would capture an incorrect region.
    let display_id = CGMainDisplayID();
    let bounds = CGDisplayBounds(display_id);

    let image = CGWindowListCreateImage(
        bounds,
        CGWindowListOption::OptionOnScreenOnly,
        0 as CGWindowID,
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
