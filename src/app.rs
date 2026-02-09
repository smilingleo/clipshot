use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplicationDelegate, NSModalResponseOK, NSSavePanel};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSString, NSTimer,
};

use crate::border::RecordingBorder;
use crate::hotkey::HotkeyManager;
use crate::overlay::view::ActiveTool;
use crate::overlay::OverlayWindow;
use crate::recording::RecordingState;
use crate::statusbar::StatusBar;
use crate::toolbar::ToolbarWindow;

pub struct AppDelegateIvars {
    status_bar: RefCell<Option<StatusBar>>,
    hotkey_manager: RefCell<Option<HotkeyManager>>,
    overlay: RefCell<Option<OverlayWindow>>,
    toolbar: RefCell<Option<ToolbarWindow>>,
    /// The full-screen CGImage from the last capture (for cropping)
    captured_image: RefCell<Option<CFRetained<CGImage>>>,
    /// True when the overlay is being used for recording region selection
    recording_mode: Cell<bool>,
    /// Active recording state (encoder + timer)
    recording_state: RefCell<Option<RecordingState>>,
    /// Click-through border window shown during recording
    recording_border: RefCell<Option<RecordingBorder>>,
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

            let border = RecordingBorder::new(mtm);
            *self.ivars().recording_border.borrow_mut() = Some(border);

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

            eprintln!("Screenshot app started. Ctrl+Cmd+A=capture, Ctrl+Cmd+V=record.");
        }
    }

    // --- Hotkey polling (called by NSTimer) ---
    impl AppDelegate {
        #[unsafe(method(pollHotkeys:))]
        fn poll_hotkeys(&self, _timer: &NSObject) {
            use global_hotkey::GlobalHotKeyEvent;
            if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.state() == global_hotkey::HotKeyState::Pressed {
                    let hk = self.ivars().hotkey_manager.borrow();
                    let hk = hk.as_ref().unwrap();

                    if event.id() == hk.capture_hotkey_id {
                        // Don't allow screenshot while recording
                        if self.ivars().recording_state.borrow().is_some() {
                            eprintln!("Cannot capture screenshot while recording");
                            return;
                        }
                        eprintln!("Capture hotkey pressed");
                        self.do_capture();
                    } else if event.id() == hk.record_hotkey_id {
                        self.handle_record_hotkey();
                    }
                }
            }
        }
    }

    // --- Capture trigger ---
    impl AppDelegate {
        #[unsafe(method(captureScreenshot:))]
        fn capture_screenshot(&self, _sender: &AnyObject) {
            if self.ivars().recording_state.borrow().is_some() {
                eprintln!("Cannot capture screenshot while recording");
                return;
            }
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
            self.ivars().recording_mode.set(false);
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
            if self.ivars().recording_mode.get() {
                // In recording mode, confirm starts recording with the selection
                self.start_recording_with_selection();
                return;
            }

            // Normal screenshot mode: copy to clipboard
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

    // --- Recording frame capture (called by NSTimer) ---
    impl AppDelegate {
        #[unsafe(method(captureRecordingFrame:))]
        fn capture_recording_frame(&self, _timer: &NSObject) {
            let mut state = self.ivars().recording_state.borrow_mut();
            if let Some(ref mut recording) = *state {
                recording.capture_frame();
            }
        }
    }

    // --- Stop recording (called from status bar menu) ---
    impl AppDelegate {
        #[unsafe(method(stopRecording:))]
        fn stop_recording_action(&self, _sender: &AnyObject) {
            self.stop_recording();
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
            recording_mode: Cell::new(false),
            recording_state: RefCell::new(None),
            recording_border: RefCell::new(None),
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

    fn handle_record_hotkey(&self) {
        // If already recording, stop
        if self.ivars().recording_state.borrow().is_some() {
            self.stop_recording();
            return;
        }

        // Set recording mode and show overlay for region selection
        self.ivars().recording_mode.set(true);
        eprintln!("Record hotkey pressed — select region then confirm");
        self.do_capture();
    }

    fn start_recording_with_selection(&self) {
        let mtm = MainThreadMarker::from(self);

        // Read selection from overlay
        let (selection, scale_factor) = {
            let overlay_ref = self.ivars().overlay.borrow();
            let overlay = match overlay_ref.as_ref() {
                Some(o) => o,
                None => return,
            };
            let sel = match overlay.view.ivars().selection.get() {
                Some(s) => crate::overlay::view::normalize_rect(s),
                None => {
                    eprintln!("No selection for recording");
                    return;
                }
            };
            let sf = overlay.view.ivars().scale_factor.get();
            (sel, sf)
        };

        // Dismiss overlay and toolbar
        self.ivars().recording_mode.set(false);
        self.dismiss_all();

        // Calculate pixel dimensions (H.264 requires even dimensions)
        let pixel_w = (selection.size.width * scale_factor) as usize & !1;
        let pixel_h = (selection.size.height * scale_factor) as usize & !1;

        if pixel_w == 0 || pixel_h == 0 {
            eprintln!("Selection too small for recording");
            return;
        }

        // Create temp file path
        let tmp_path = std::env::temp_dir().join(format!(
            "screenshot_recording_{}.mp4",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ));

        eprintln!("Starting recording: {}x{} -> {:?}", pixel_w, pixel_h, tmp_path);

        // Create encoder
        let encoder = match crate::encoder::VideoEncoder::new(&tmp_path, pixel_w, pixel_h, 30) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Failed to create encoder: {}", e);
                return;
            }
        };

        // Start encoder
        let mut recording = RecordingState::new(encoder, selection, scale_factor);
        if let Err(e) = recording.encoder.start() {
            eprintln!("Failed to start recording: {}", e);
            return;
        }

        // Start ~30fps timer for frame capture
        let target: &AnyObject = unsafe { &*(self as *const Self as *const AnyObject) };
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                1.0 / 30.0,
                target,
                sel!(captureRecordingFrame:),
                None,
                true,
            )
        };
        recording.timer = Some(timer);

        // Store temp path for later save dialog
        recording.output_path = Some(tmp_path);

        // Show the border window around the recording region and exclude it from capture
        if let Some(border) = self.ivars().recording_border.borrow().as_ref() {
            border.show(selection, mtm);
            recording.exclude_window_id = Some(border.window_number());
        }

        *self.ivars().recording_state.borrow_mut() = Some(recording);

        // Update status bar
        if let Some(sb) = self.ivars().status_bar.borrow().as_ref() {
            sb.enter_recording_mode(mtm);
        }

        eprintln!("Recording started");
    }

    fn stop_recording(&self) {
        let mtm = MainThreadMarker::from(self);

        let recording = self.ivars().recording_state.borrow_mut().take();
        let Some(mut recording) = recording else {
            return;
        };

        // Invalidate the timer
        if let Some(timer) = recording.timer.take() {
            timer.invalidate();
        }

        // Finish encoding
        recording.encoder.finish();

        // Hide the recording border
        if let Some(border) = self.ivars().recording_border.borrow().as_ref() {
            border.hide();
        }

        // Exit recording mode in status bar
        if let Some(sb) = self.ivars().status_bar.borrow().as_ref() {
            sb.exit_recording_mode(mtm);
        }

        // Show save dialog for the MP4
        if let Some(tmp_path) = &recording.output_path {
            self.show_save_dialog_for_recording(tmp_path, mtm);
        }

        eprintln!("Recording stopped");
    }

    fn show_save_dialog_for_recording(&self, tmp_path: &PathBuf, mtm: MainThreadMarker) {
        let panel = NSSavePanel::new(mtm);
        panel.setNameFieldStringValue(&NSString::from_str("recording.mp4"));

        let response = panel.runModal();
        if response == NSModalResponseOK {
            if let Some(url) = panel.URL() {
                if let Some(path) = url.path() {
                    let dest = path.to_string();
                    if let Err(e) = std::fs::rename(tmp_path, &dest) {
                        // rename may fail across filesystems, try copy
                        if let Err(e2) = std::fs::copy(tmp_path, &dest) {
                            eprintln!("Failed to save recording: rename={}, copy={}", e, e2);
                        } else {
                            let _ = std::fs::remove_file(tmp_path);
                            eprintln!("Recording saved to {}", dest);
                        }
                    } else {
                        eprintln!("Recording saved to {}", dest);
                    }
                }
            }
        }
        // Clean up temp file if not saved
        let _ = std::fs::remove_file(tmp_path);
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
                    let _: () = unsafe {
                        msg_send![&*overlay.window, addChildWindow: &*toolbar.panel, ordered: 1i64]
                    };
                }
            }
        }
    }
}
