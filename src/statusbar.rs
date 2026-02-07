use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2_app_kit::{NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
use objc2_foundation::{MainThreadMarker, NSString};

pub struct StatusBar {
    _status_item: Retained<NSStatusItem>,
}

impl StatusBar {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);

        // Set the status bar button title
        if let Some(button) = status_item.button(mtm) {
            button.setTitle(&NSString::from_str("📷"));
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
            _status_item: status_item,
        }
    }
}
