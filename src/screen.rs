use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSScreen};
use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGError, CGGetDisplaysWithPoint, CGMainDisplayID,
};
use objc2_foundation::MainThreadMarker;

/// Get the CGDirectDisplayID of the display containing the mouse cursor.
/// Falls back to CGMainDisplayID if detection fails.
pub fn display_with_mouse() -> CGDirectDisplayID {
    let mouse_loc = NSEvent::mouseLocation();

    // NSEvent::mouseLocation() is in AppKit screen coords (bottom-left origin).
    // CGGetDisplaysWithPoint uses CG coords (top-left origin of primary display).
    // Convert: cg_y = primary_display_height - appkit_y
    let main_bounds = CGDisplayBounds(CGMainDisplayID());
    let cg_point = CGPoint::new(mouse_loc.x, main_bounds.size.height - mouse_loc.y);

    let mut display_id: CGDirectDisplayID = 0;
    let mut count: u32 = 0;

    let result = unsafe { CGGetDisplaysWithPoint(cg_point, 1, &mut display_id, &mut count) };

    if result == CGError(0) && count > 0 {
        display_id
    } else {
        CGMainDisplayID()
    }
}

/// Get the NSScreen that contains the mouse cursor.
/// Falls back to the main screen if none found.
pub fn screen_with_mouse(mtm: MainThreadMarker) -> Retained<NSScreen> {
    let mouse_loc = NSEvent::mouseLocation();

    // NSScreen frames use AppKit coords (bottom-left origin), same as mouseLocation
    let screens = NSScreen::screens(mtm);
    for screen in &screens {
        let frame = screen.frame();
        if mouse_loc.x >= frame.origin.x
            && mouse_loc.x < frame.origin.x + frame.size.width
            && mouse_loc.y >= frame.origin.y
            && mouse_loc.y < frame.origin.y + frame.size.height
        {
            return screen.clone();
        }
    }

    NSScreen::mainScreen(mtm).expect("no screen available")
}
