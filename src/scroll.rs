use std::ffi::c_void;

use objc2_core_foundation::{CGFloat, CGPoint};

// CoreGraphics CGEvent FFI — these functions are not exposed by the objc2 crates.
unsafe extern "C" {
    fn CGEventCreateScrollWheelEvent(
        source: *const c_void,
        units: u32,
        wheel_count: u32,
        wheel1: i32,
    ) -> *mut c_void;
    fn CGEventSetLocation(event: *mut c_void, point: CGPoint);
    fn CGEventPost(tap: u32, event: *mut c_void);
    fn CFRelease(cf: *const c_void);
}

/// kCGScrollEventUnitPixel = 0
const UNIT_PIXEL: u32 = 0;
/// kCGHIDEventTap — post events at the HID event tap
const HID_EVENT_TAP: u32 = 0;

/// Simulate a pixel-based scroll wheel event at the given screen point.
///
/// `screen_point` uses macOS native coordinates (origin at bottom-left).
/// `delta_pixels`: negative = scroll down (content moves up), positive = scroll up.
pub fn simulate_scroll(screen_point: CGPoint, delta_pixels: i32) {
    unsafe {
        let event = CGEventCreateScrollWheelEvent(
            std::ptr::null(),
            UNIT_PIXEL,
            1,
            delta_pixels,
        );
        if event.is_null() {
            eprintln!("Failed to create scroll event");
            return;
        }
        CGEventSetLocation(event, screen_point);
        CGEventPost(HID_EVENT_TAP, event);
        CFRelease(event);
    }
}

/// Convert a point from flipped (top-left origin) coordinates to macOS
/// native screen coordinates (bottom-left origin).
pub fn flipped_to_screen(x: CGFloat, y: CGFloat, screen_height: CGFloat) -> CGPoint {
    CGPoint::new(x, screen_height - y)
}

