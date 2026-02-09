use objc2::define_class;
use objc2::rc::Retained;
use objc2::{msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSGraphicsContext, NSScreen, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGContext, kCGFloatingWindowLevel};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSRect};

use std::cell::Cell;

// --- Border view that draws a colored rectangle outline ---

pub struct BorderViewIvars {
    border_color: Cell<(CGFloat, CGFloat, CGFloat)>,
    border_width: Cell<CGFloat>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BorderView"]
    #[ivars = BorderViewIvars]
    pub struct BorderView;

    unsafe impl NSObjectProtocol for BorderView {}

    impl BorderView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let Some(context) = NSGraphicsContext::currentContext() else {
                return;
            };
            let cg = context.CGContext();
            let bounds = self.bounds();
            let (r, g, b) = self.ivars().border_color.get();
            let w = self.ivars().border_width.get();

            // Draw border as a stroked rectangle inset by half the line width
            let inset = w / 2.0;
            let border_rect = CGRect::new(
                CGPoint::new(inset, inset),
                CGSize::new(bounds.size.width - w, bounds.size.height - w),
            );

            CGContext::set_rgb_stroke_color(Some(&cg), r, g, b, 1.0);
            CGContext::set_line_width(Some(&cg), w);
            CGContext::stroke_rect(Some(&cg), border_rect);
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl BorderView {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(BorderViewIvars {
            border_color: Cell::new((1.0, 0.0, 0.0)), // red
            border_width: Cell::new(2.0),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view
    }
}

// --- Click-through border window ---

pub struct RecordingBorder {
    window: Retained<NSWindow>,
}

impl RecordingBorder {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let frame = CGRect::new(CGPoint::ZERO, CGSize::new(1.0, 1.0));

        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };

        window.setLevel((kCGFloatingWindowLevel + 1) as _);
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setIgnoresMouseEvents(true);
        unsafe { window.setReleasedWhenClosed(false) };

        let view = BorderView::new(mtm, frame);
        window.setContentView(Some(&view));

        RecordingBorder { window }
    }

    /// Show the border around the given selection rect (in screen logical points).
    /// The rect uses top-left origin (flipped coordinates from the overlay view).
    pub fn show(&self, selection: CGRect, mtm: MainThreadMarker) {
        // macOS window coordinates use bottom-left origin.
        // Convert from top-left (overlay) to bottom-left (screen).
        let screen = NSScreen::mainScreen(mtm).expect("no main screen");
        let screen_h = screen.frame().size.height;

        // Add a small margin so the border is drawn just outside the selection
        let margin: CGFloat = 1.0;
        let window_frame = CGRect::new(
            CGPoint::new(
                selection.origin.x - margin,
                screen_h - selection.origin.y - selection.size.height - margin,
            ),
            CGSize::new(
                selection.size.width + margin * 2.0,
                selection.size.height + margin * 2.0,
            ),
        );

        self.window.setFrame_display(window_frame, true);
        self.window.orderFront(None);
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
    }

    /// Get the CGWindowID for this window (used to exclude from screen capture).
    pub fn window_number(&self) -> u32 {
        self.window.windowNumber() as u32
    }
}
