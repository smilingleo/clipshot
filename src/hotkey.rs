use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::GlobalHotKeyManager;

pub struct HotkeyManager {
    _manager: GlobalHotKeyManager,
    pub capture_hotkey_id: u32,
    pub record_hotkey_id: u32,
}

impl HotkeyManager {
    pub fn new() -> Self {
        let manager = GlobalHotKeyManager::new().expect("failed to create hotkey manager");

        // Register Ctrl+Shift+A for screenshot
        let capture_hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::KeyA,
        );
        manager
            .register(capture_hotkey)
            .expect("failed to register capture hotkey");

        // Register Ctrl+Shift+R for screen recording
        let record_hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::KeyR,
        );
        manager
            .register(record_hotkey)
            .expect("failed to register record hotkey");

        eprintln!(
            "Global hotkeys registered: Ctrl+Shift+A (id={}), Ctrl+Shift+R (id={})",
            capture_hotkey.id(),
            record_hotkey.id()
        );

        HotkeyManager {
            _manager: manager,
            capture_hotkey_id: capture_hotkey.id(),
            record_hotkey_id: record_hotkey.id(),
        }
    }
}
