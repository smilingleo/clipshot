use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::GlobalHotKeyManager;

pub struct HotkeyManager {
    _manager: GlobalHotKeyManager,
    pub capture_hotkey_id: u32,
    pub record_hotkey_id: u32,
    pub scroll_capture_hotkey_id: u32,
}

impl HotkeyManager {
    pub fn new() -> Self {
        let manager = GlobalHotKeyManager::new().expect("failed to create hotkey manager");

        // Register Ctrl+Cmd+A for screenshot
        let capture_hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::META),
            Code::KeyA,
        );
        manager
            .register(capture_hotkey)
            .expect("failed to register capture hotkey");

        // Register Ctrl+Cmd+V for screen recording
        let record_hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::META),
            Code::KeyV,
        );
        manager
            .register(record_hotkey)
            .expect("failed to register record hotkey");

        // Register Ctrl+Cmd+S for scrolling capture
        let scroll_capture_hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::META),
            Code::KeyS,
        );
        manager
            .register(scroll_capture_hotkey)
            .expect("failed to register scroll capture hotkey");

        eprintln!(
            "Global hotkeys registered: Ctrl+Cmd+A (id={}), Ctrl+Cmd+V (id={}), Ctrl+Cmd+S (id={})",
            capture_hotkey.id(),
            record_hotkey.id(),
            scroll_capture_hotkey.id(),
        );

        HotkeyManager {
            _manager: manager,
            capture_hotkey_id: capture_hotkey.id(),
            record_hotkey_id: record_hotkey.id(),
            scroll_capture_hotkey_id: scroll_capture_hotkey.id(),
        }
    }
}
