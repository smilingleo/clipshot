use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2_app_kit::{
    NSEventModifierFlags, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{MainThreadMarker, NSString};

pub struct StatusBar {
    status_item: Retained<NSStatusItem>,
    camera_icon: Option<Retained<NSImage>>,
    recording_icon: Option<Retained<NSImage>>,
    /// Items shown in normal (non-recording) mode.
    normal_items: Vec<Retained<NSMenuItem>>,
    /// "Stop Recording" item, shown only during recording/scroll capture.
    stop_recording_item: Retained<NSMenuItem>,
}

impl StatusBar {
    pub fn new(mtm: MainThreadMarker) -> Self {
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(NSVariableStatusItemLength);

        // Prefer SF Symbols (crisp, template-rendered in dark/light mode).
        // Fallback to emoji if unavailable (older macOS / missing symbol name).
        let camera_icon = make_template_symbol("camera", "ClipShot");
        let recording_icon = make_template_symbol("record.circle.fill", "Recording");

        if let Some(button) = status_item.button(mtm) {
            if let Some(icon) = camera_icon.as_deref() {
                button.setTitle(&NSString::from_str(""));
                button.setImage(Some(icon));
            } else {
                button.setImage(None);
                button.setTitle(&NSString::from_str("\u{1F4F7}")); // 📷
            }
        }

        let menu = NSMenu::new(mtm);
        let ctrl_cmd =
            NSEventModifierFlags::Command.union(NSEventModifierFlags::Control);

        // Capture Screenshot  (Ctrl+Cmd+A)
        let capture_item = create_menu_item(
            mtm,
            "Capture Screenshot",
            c"captureScreenshot:",
            "a",
            ctrl_cmd,
        );
        menu.addItem(&capture_item);

        // Record Screen  (Ctrl+Cmd+Z)
        let record_item = create_menu_item(
            mtm,
            "Record Screen",
            c"startRecording:",
            "z",
            ctrl_cmd,
        );
        menu.addItem(&record_item);

        // Scroll Capture  (Ctrl+Cmd+S)
        let scroll_item = create_menu_item(
            mtm,
            "Scroll Capture",
            c"startScrollCapture:",
            "s",
            ctrl_cmd,
        );
        menu.addItem(&scroll_item);

        // Stop Recording - hidden by default
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

        // Quit
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
            camera_icon,
            recording_icon,
            normal_items: vec![capture_item, record_item, scroll_item],
            stop_recording_item,
        }
    }

    pub fn enter_recording_mode(&self, mtm: MainThreadMarker) {
        if let Some(button) = self.status_item.button(mtm) {
            if let Some(icon) = self.recording_icon.as_deref() {
                button.setTitle(&NSString::from_str(""));
                button.setImage(Some(icon));
            } else {
                button.setTitle(&NSString::from_str("\u{1F534}")); // 🔴
            }
        }
        for item in &self.normal_items {
            item.setHidden(true);
        }
        self.stop_recording_item.setHidden(false);
    }

    pub fn exit_recording_mode(&self, mtm: MainThreadMarker) {
        if let Some(button) = self.status_item.button(mtm) {
            if let Some(icon) = self.camera_icon.as_deref() {
                button.setTitle(&NSString::from_str(""));
                button.setImage(Some(icon));
            } else {
                button.setTitle(&NSString::from_str("\u{1F4F7}")); // 📷
            }
        }
        for item in &self.normal_items {
            item.setHidden(false);
        }
        self.stop_recording_item.setHidden(true);
    }
}

fn make_template_symbol(symbol_name: &str, description: &str) -> Option<Retained<NSImage>> {
    let name = NSString::from_str(symbol_name);
    let desc = NSString::from_str(description);
    let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(&name, Some(&desc))?;
    img.setTemplate(true);
    Some(img)
}

fn create_menu_item(
    mtm: MainThreadMarker,
    title: &str,
    action: &std::ffi::CStr,
    key: &str,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str(title),
            Some(Sel::register(action)),
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(modifiers);
    item
}
