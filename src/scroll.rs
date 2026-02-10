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
/// `screen_point` uses CG global coordinates (origin at top-left of primary display).
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

/// Convert a point from overlay coordinates (top-left origin, relative to display)
/// to CG global coordinates (top-left origin of primary display).
///
/// Both coordinate systems use top-left origin, so this is a simple offset
/// by the display's CG origin.
pub fn overlay_to_cg_global(x: CGFloat, y: CGFloat, screen_origin: CGPoint) -> CGPoint {
    CGPoint::new(screen_origin.x + x, screen_origin.y + y)
}

