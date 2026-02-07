use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSPanel, NSScreen, NSWindowStyleMask,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::kCGOverlayWindowLevel;
use objc2_foundation::MainThreadMarker;

use super::view::ToolbarView;

pub struct ToolbarWindow {
    pub panel: Retained<NSPanel>,
    pub view: Retained<ToolbarView>,
}

impl ToolbarWindow {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let view = ToolbarView::new(mtm);
        let view_frame = view.frame();

        let panel =
            NSPanel::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                view_frame,
                NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                NSBackingStoreType::Buffered,
                false,
            );

        panel.setLevel((kCGOverlayWindowLevel + 1) as _);
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));
        panel.setHasShadow(true);
        panel.setHidesOnDeactivate(false);
        panel.setFloatingPanel(true);
        panel.setWorksWhenModal(true);
        unsafe { panel.setReleasedWhenClosed(false) };

        panel.setContentView(Some(&view));

        ToolbarWindow { panel, view }
    }

    /// Position the toolbar below the selection rect, or above if below is off-screen.
    pub fn show_near_selection(&self, selection: CGRect, mtm: MainThreadMarker) {
        let screen = NSScreen::mainScreen(mtm).expect("no main screen");
        let screen_frame = screen.frame();
        let toolbar_size = self.view.frame().size;

        // In our flipped coordinate system, selection.origin is top-left.
        // But NSWindow uses the macOS bottom-left coordinate system.
        // Convert: screen_y = screen_height - flipped_y
        let gap: CGFloat = 8.0;

        // Try below the selection first (in flipped coords: below = higher y)
        let flipped_y = selection.origin.y + selection.size.height + gap;
        let screen_y = screen_frame.size.height - flipped_y - toolbar_size.height;

        // Center horizontally with the selection
        let x = selection.origin.x + (selection.size.width - toolbar_size.width) / 2.0;
        let x = x.max(screen_frame.origin.x)
            .min(screen_frame.origin.x + screen_frame.size.width - toolbar_size.width);

        let final_y = if screen_y < screen_frame.origin.y {
            // Not enough space below; place above the selection
            let flipped_y_above = selection.origin.y - gap - toolbar_size.height;
            screen_frame.size.height - flipped_y_above - toolbar_size.height
        } else {
            screen_y
        };

        let origin = CGPoint::new(x, final_y);
        self.panel.setFrameOrigin(origin);
        // Use orderFrontRegardless — in accessory/agent apps, orderFront(None)
        // may fail to bring the panel above the key overlay window.
        self.panel.orderFrontRegardless();
    }

    pub fn hide(&self) {
        self.panel.orderOut(None);
    }
}
