use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::GlobalHotKeyManager;

pub struct HotkeyManager {
    _manager: GlobalHotKeyManager,
}

impl HotkeyManager {
    pub fn new() -> Self {
        let manager = GlobalHotKeyManager::new().expect("failed to create hotkey manager");

        // Register Ctrl+Shift+A
        let hotkey = HotKey::new(
            Some(Modifiers::CONTROL | Modifiers::SHIFT),
            Code::KeyA,
        );
        manager.register(hotkey).expect("failed to register hotkey");

        eprintln!("Global hotkey registered: Ctrl+Shift+A (id={})", hotkey.id());

        HotkeyManager { _manager: manager }
    }
}
