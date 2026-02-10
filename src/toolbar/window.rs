use objc2::rc::Retained;
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSColorPanel, NSPanel, NSView, NSWindowStyleMask,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
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
    /// `selection` is in overlay-local flipped coordinates (top-left origin).
    /// `screen_frame` is the overlay's screen frame in global AppKit coordinates.
    pub fn show_near_selection(&self, selection: CGRect, screen_frame: CGRect) {
        let toolbar_size = self.view.frame().size;

        // Selection coords are local to the overlay (which covers the screen).
        // Convert to global AppKit coords by adding screen_frame.origin and
        // flipping Y from top-left to bottom-left.
        let gap: CGFloat = 8.0;

        // Try below the selection first (in flipped coords: below = higher y)
        let flipped_y = selection.origin.y + selection.size.height + gap;
        let screen_y = screen_frame.origin.y + screen_frame.size.height - flipped_y - toolbar_size.height;

        // Center horizontally with the selection (offset by screen origin)
        let x = screen_frame.origin.x + selection.origin.x + (selection.size.width - toolbar_size.width) / 2.0;
        let x = x.max(screen_frame.origin.x)
            .min(screen_frame.origin.x + screen_frame.size.width - toolbar_size.width);

        let final_y = if screen_y < screen_frame.origin.y {
            // Not enough space below; place above the selection
            let flipped_y_above = selection.origin.y - gap - toolbar_size.height;
            screen_frame.origin.y + screen_frame.size.height - flipped_y_above - toolbar_size.height
        } else {
            screen_y
        };

        let origin = CGPoint::new(x, final_y);
        self.panel.setFrameOrigin(origin);
        // Use orderFrontRegardless — in accessory/agent apps, orderFront(None)
        // may fail to bring the panel above the key overlay window.
        self.panel.orderFrontRegardless();
    }

    /// Show the system color panel above the color button in the toolbar.
    pub fn show_color_panel(&self, mtm: MainThreadMarker) {
        let color_panel = NSColorPanel::sharedColorPanel(mtm);

        // Set level above the overlay so it's not hidden
        color_panel.setLevel((kCGOverlayWindowLevel + 2) as _);
        color_panel.setHidesOnDeactivate(false);

        // Add the color panel as a child of the toolbar panel so it stays
        // in the same window group as the overlay → toolbar hierarchy.
        use objc2::msg_send;
        let _: () = unsafe {
            msg_send![&*self.panel, addChildWindow: &*color_panel, ordered: 1i64]
        };

        // Position the color panel above the color button
        if let Some(btn_frame) = self.view.color_button_frame() {
            let btn_in_window = NSView::convertRect_toView(&*self.view, btn_frame, None);
            let btn_on_screen = self.panel.convertRectToScreen(btn_in_window);

            let panel_frame = color_panel.frame();
            let panel_x = btn_on_screen.origin.x + btn_on_screen.size.width / 2.0
                - panel_frame.size.width / 2.0;
            let panel_y = btn_on_screen.origin.y + btn_on_screen.size.height + 8.0;

            let new_frame = CGRect::new(
                CGPoint::new(panel_x, panel_y),
                CGSize::new(panel_frame.size.width, panel_frame.size.height),
            );
            color_panel.setFrame_display(new_frame, false);
        }

        color_panel.orderFrontRegardless();
        self.view.set_color_picker_active(true);
    }

    /// Hide the system color panel and update the button state.
    pub fn hide_color_panel(&self, mtm: MainThreadMarker) {
        let color_panel = NSColorPanel::sharedColorPanel(mtm);

        // Remove the color panel from the child window group
        use objc2::msg_send;
        let _: () = unsafe {
            msg_send![&*self.panel, removeChildWindow: &*color_panel]
        };

        color_panel.orderOut(None);
        self.view.set_color_picker_active(false);
    }

    pub fn hide(&self) {
        // Reset the color picker active state
        self.view.set_color_picker_active(false);
        self.panel.orderOut(None);
    }

    /// Enable or disable all toolbar buttons except Confirm.
    pub fn set_non_confirm_buttons_enabled(&self, enabled: bool) {
        self.view.set_non_confirm_buttons_enabled(enabled);
    }
}
