use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2_app_kit::{NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{MainThreadMarker, NSString};

pub struct StatusBar {
    status_item: Retained<NSStatusItem>,
    capture_item: Retained<NSMenuItem>,
    stop_recording_item: Retained<NSMenuItem>,
}

impl StatusBar {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);

        // Set the status bar button title
        if let Some(button) = status_item.button(mtm) {
            button.setTitle(&NSString::from_str("\u{1F4F7}")); // 📷
        }

        // Create menu
        let menu = NSMenu::new(mtm);

        // Capture item - action routes through responder chain to app delegate
        let capture_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str("Capture"),
                Some(Sel::register(c"captureScreenshot:")),
                &NSString::from_str(""),
            )
        };
        menu.addItem(&capture_item);

        // Stop Recording item - hidden by default
        let stop_recording_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str("Stop Recording"),
                Some(Sel::register(c"stopRecording:")),
                &NSString::from_str(""),
            )
        };
        stop_recording_item.setHidden(true);
        menu.addItem(&stop_recording_item);

        // Separator
        menu.addItem(&NSMenuItem::separatorItem(mtm));

        // Quit item
        let quit_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str("Quit"),
                Some(Sel::register(c"terminate:")),
                &NSString::from_str("q"),
            )
        };
        menu.addItem(&quit_item);

        status_item.setMenu(Some(&menu));

        StatusBar {
            status_item,
            capture_item,
            stop_recording_item,
        }
    }

    pub fn enter_recording_mode(&self, mtm: MainThreadMarker) {
        if let Some(button) = self.status_item.button(mtm) {
            button.setTitle(&NSString::from_str("\u{1F534}")); // 🔴
        }
        self.capture_item.setHidden(true);
        self.stop_recording_item.setHidden(false);
    }

    pub fn exit_recording_mode(&self, mtm: MainThreadMarker) {
        if let Some(button) = self.status_item.button(mtm) {
            button.setTitle(&NSString::from_str("\u{1F4F7}")); // 📷
        }
        self.capture_item.setHidden(false);
        self.stop_recording_item.setHidden(true);
    }
}
