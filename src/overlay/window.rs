use objc2::rc::Retained;
use objc2::{define_class, msg_send, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSImage, NSScreen, NSWindow, NSWindowStyleMask,
};
use objc2_core_graphics::{CGImage, kCGOverlayWindowLevel};
use objc2_foundation::{MainThreadMarker, NSSize};

use super::view::OverlayView;

// Custom NSWindow subclass that can become key window (needed for borderless windows).
pub struct KeyableWindowIvars {}

define_class!(
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeyableWindow"]
    #[ivars = KeyableWindowIvars]
    pub struct KeyableWindow;

    impl KeyableWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            true
        }
    }
);

pub struct OverlayWindow {
    pub window: Retained<KeyableWindow>,
    pub view: Retained<OverlayView>,
}

impl OverlayWindow {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let screen = NSScreen::mainScreen(mtm).expect("no main screen");
        let frame = screen.frame();

        let this = mtm.alloc().set_ivars(KeyableWindowIvars {});
        let window: Retained<KeyableWindow> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: frame,
                styleMask: NSWindowStyleMask::Borderless,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };

        window.setLevel(kCGOverlayWindowLevel as _);
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(false);
        window.setAcceptsMouseMovedEvents(true);
        unsafe { window.setReleasedWhenClosed(false) };

        let view = OverlayView::new(mtm, frame);
        window.setContentView(Some(&view));

        OverlayWindow { window, view }
    }

    pub fn show_with_screenshot(&self, cg_image: &CGImage, screen: &NSScreen, mtm: MainThreadMarker) {
        let frame = screen.frame();
        let scale_factor = screen.backingScaleFactor();

        let ns_image = NSImage::initWithCGImage_size(
            mtm.alloc(),
            cg_image,
            NSSize::new(frame.size.width, frame.size.height),
        );

        self.view.reset();
        self.view.set_screenshot(ns_image, scale_factor);

        self.window.setFrame_display(frame, true);

        // Activate the app so the first click goes to the view, not to activation.
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);

        self.window.makeKeyAndOrderFront(None);
        self.window.makeFirstResponder(Some(&*self.view));
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
    }
}
