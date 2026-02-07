use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::NSApplicationDelegate;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSTimer,
};

use crate::hotkey::HotkeyManager;
use crate::overlay::OverlayWindow;
use crate::overlay::view::ActiveTool;
use crate::statusbar::StatusBar;
use crate::toolbar::ToolbarWindow;

pub struct AppDelegateIvars {
    status_bar: RefCell<Option<StatusBar>>,
    hotkey_manager: RefCell<Option<HotkeyManager>>,
    overlay: RefCell<Option<OverlayWindow>>,
    toolbar: RefCell<Option<ToolbarWindow>>,
    /// The full-screen CGImage from the last capture (for cropping)
    captured_image: RefCell<Option<CFRetained<CGImage>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AppDelegate"]
    #[ivars = AppDelegateIvars]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            let mtm = MainThreadMarker::from(self);

            let status_bar = StatusBar::new(mtm);
            *self.ivars().status_bar.borrow_mut() = Some(status_bar);

            let hotkey_manager = HotkeyManager::new();
            *self.ivars().hotkey_manager.borrow_mut() = Some(hotkey_manager);

            let overlay = OverlayWindow::new(mtm);
            *self.ivars().overlay.borrow_mut() = Some(overlay);

            let toolbar = ToolbarWindow::new(mtm);
            *self.ivars().toolbar.borrow_mut() = Some(toolbar);

            // Set up a timer to poll for global hotkey events (100ms interval)
            let target: &AnyObject = unsafe { &*(self as *const Self as *const AnyObject) };
            unsafe {
                NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                    0.1,
                    target,
                    sel!(pollHotkeys:),
                    None,
                    true,
                );
            }

            eprintln!("Screenshot app started. Use Ctrl+Shift+A to capture.");
        }
    }

    // --- Hotkey polling (called by NSTimer) ---
    impl AppDelegate {
        #[unsafe(method(pollHotkeys:))]
        fn poll_hotkeys(&self, _timer: &NSObject) {
            use global_hotkey::GlobalHotKeyEvent;
            if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.state() == global_hotkey::HotKeyState::Pressed {
                    eprintln!("Hotkey pressed, triggering capture...");
                    self.do_capture();
                }
            }
        }
    }

    // --- Capture trigger ---
    impl AppDelegate {
        #[unsafe(method(captureScreenshot:))]
        fn capture_screenshot(&self, _sender: &AnyObject) {
            eprintln!("Capture triggered from menu!");
            self.do_capture();
        }
    }

    // --- Tool selection ---
    impl AppDelegate {
        #[unsafe(method(toolSelect:))]
        fn tool_select(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Select);
        }

        #[unsafe(method(toolArrow:))]
        fn tool_arrow(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Arrow);
        }

        #[unsafe(method(toolRect:))]
        fn tool_rect(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Rectangle);
        }

        #[unsafe(method(toolEllipse:))]
        fn tool_ellipse(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Ellipse);
        }

        #[unsafe(method(toolPencil:))]
        fn tool_pencil(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Pencil);
        }

        #[unsafe(method(toolText:))]
        fn tool_text(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Text);
        }
    }

    // --- Color selection ---
    impl AppDelegate {
        #[unsafe(method(colorRed:))]
        fn color_red(&self, _sender: &AnyObject) {
            self.set_annotation_color((1.0, 0.0, 0.0));
        }

        #[unsafe(method(colorBlue:))]
        fn color_blue(&self, _sender: &AnyObject) {
            self.set_annotation_color((0.0, 0.4, 1.0));
        }

        #[unsafe(method(colorGreen:))]
        fn color_green(&self, _sender: &AnyObject) {
            self.set_annotation_color((0.0, 0.8, 0.0));
        }

        #[unsafe(method(colorYellow:))]
        fn color_yellow(&self, _sender: &AnyObject) {
            self.set_annotation_color((1.0, 0.8, 0.0));
        }
    }

    // --- Actions ---
    impl AppDelegate {
        #[unsafe(method(actionUndo:))]
        fn action_undo(&self, _sender: &AnyObject) {
            if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
                overlay.view.ivars().annotations.borrow_mut().pop();
                overlay.view.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(actionCancel:))]
        fn action_cancel(&self, _sender: &AnyObject) {
            self.dismiss_all();
        }

        #[unsafe(method(actionSave:))]
        fn action_save(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            let image = self.get_final_image();
            // Dismiss overlay first so the NSSavePanel isn't hidden behind it.
            self.dismiss_all();
            if let Some(image) = image {
                crate::actions::save_to_file(&image, mtm);
            }
        }

        #[unsafe(method(actionConfirm:))]
        fn action_confirm(&self, _sender: &AnyObject) {
            if let Some(image) = self.get_final_image() {
                if let Err(e) = crate::actions::copy_to_clipboard(&image) {
                    eprintln!("Clipboard error: {}", e);
                }
            }
            self.dismiss_all();
        }
    }

    // --- Selection notification (called from overlay view) ---
    impl AppDelegate {
        #[unsafe(method(selectionChanged:))]
        fn selection_changed(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            self.update_toolbar_position(mtm);
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(AppDelegateIvars {
            status_bar: RefCell::new(None),
            hotkey_manager: RefCell::new(None),
            overlay: RefCell::new(None),
            toolbar: RefCell::new(None),
            captured_image: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn do_capture(&self) {
        let mtm = MainThreadMarker::from(self);

        // Hide toolbar from previous session
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.hide();
        }

        if let Some(cg_image) = crate::capture::capture_full_screen() {
            if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
                overlay.show_with_screenshot(&cg_image, mtm);
            }
            *self.ivars().captured_image.borrow_mut() = Some(cg_image);
        }
    }

    fn set_active_tool(&self, tool: ActiveTool) {
        if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
            overlay.view.ivars().active_tool.set(tool);
        }
    }

    fn set_annotation_color(&self, color: (f64, f64, f64)) {
        if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
            overlay.view.ivars().annotation_color.set(color);
        }
    }

    fn dismiss_all(&self) {
        if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
            overlay.hide();
        }
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.hide();
        }
        *self.ivars().captured_image.borrow_mut() = None;
    }

    fn get_final_image(&self) -> Option<CFRetained<CGImage>> {
        let overlay_ref = self.ivars().overlay.borrow();
        let overlay = overlay_ref.as_ref()?;
        let selection = overlay.view.ivars().selection.get()?;
        let norm = crate::overlay::view::normalize_rect(selection);
        let scale_factor = overlay.view.ivars().scale_factor.get();

        let captured = self.ivars().captured_image.borrow();
        let cg_image = captured.as_ref()?;
        let annotations = overlay.view.ivars().annotations.borrow();

        crate::actions::crop_and_composite(cg_image, norm, scale_factor, &annotations)
    }

    fn update_toolbar_position(&self, mtm: MainThreadMarker) {
        let overlay_ref = self.ivars().overlay.borrow();
        let overlay = overlay_ref.as_ref();
        let toolbar_ref = self.ivars().toolbar.borrow();
        let toolbar = toolbar_ref.as_ref();

        if let (Some(overlay), Some(toolbar)) = (overlay, toolbar) {
            if let Some(selection) = overlay.view.ivars().selection.get() {
                let norm = crate::overlay::view::normalize_rect(selection);
                if norm.size.width > 5.0 && norm.size.height > 5.0 {
                    toolbar.show_near_selection(norm, mtm);
                    // Attach toolbar as child window of overlay to guarantee
                    // it stacks above regardless of window level arithmetic.
                    // NSWindowOrderingMode::Above = 1
                    let _: () = unsafe {
                        msg_send![&*overlay.window, addChildWindow: &*toolbar.panel, ordered: 1i64]
                    };
                }
            }
        }
    }
}
