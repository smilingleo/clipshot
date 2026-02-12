use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSColorPanel, NSModalResponseOK, NSSavePanel};
use objc2_core_foundation::{CFRetained, CGFloat, CGPoint};
use objc2_core_graphics::{CGDisplayBounds, CGImage};
use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSString, NSTimer,
};

use crate::border::RecordingBorder;
use crate::editor::window::EditorWindow;
use crate::hotkey::HotkeyManager;
use crate::overlay::view::ActiveTool;
use crate::overlay::OverlayWindow;
use crate::recording::RecordingState;
use crate::scroll_capture::ScrollCaptureState;
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
    /// Post-recording video editor
    editor_window: RefCell<Option<EditorWindow>>,
    /// True when the overlay is being used for scroll capture region selection
    scroll_capture_mode: Cell<bool>,
    /// Active scroll capture state (frames + timer)
    scroll_capture_state: RefCell<Option<ScrollCaptureState>>,
    /// True when the editor is being closed via cancel (discard without saving)
    editor_cancelled: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AppDelegate"]
    #[ivars = AppDelegateIvars]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn application_should_terminate_after_last_window_closed(
            &self,
            _sender: &NSApplication,
        ) -> bool {
            false
        }

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

            eprintln!("ClipShot started. Ctrl+Cmd+A=capture, Ctrl+Cmd+Z=record, Ctrl+Cmd+S=scroll capture.");
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
                        // Don't allow screenshot while recording, editing, or scroll capturing
                        if self.ivars().recording_state.borrow().is_some() {
                            eprintln!("Cannot capture screenshot while recording");
                            return;
                        }
                        if self.ivars().editor_window.borrow().is_some() {
                            eprintln!("Cannot capture screenshot while editing");
                            return;
                        }
                        if self.ivars().scroll_capture_state.borrow().is_some() {
                            eprintln!("Cannot capture screenshot while scroll capturing");
                            return;
                        }
                        eprintln!("Capture hotkey pressed");
                        self.do_capture();
                    } else if event.id() == hk.record_hotkey_id {
                        self.handle_record_hotkey();
                    } else if event.id() == hk.scroll_capture_hotkey_id {
                        // If already capturing, stop and stitch
                        if self.ivars().scroll_capture_state.borrow().is_some() {
                            self.stop_scroll_capture();
                        } else {
                            self.handle_scroll_capture_hotkey();
                        }
                    }
                }
            }
        }
    }

    // --- Capture triggers (menu items) ---
    impl AppDelegate {
        #[unsafe(method(captureScreenshot:))]
        fn capture_screenshot(&self, _sender: &AnyObject) {
            if self.ivars().recording_state.borrow().is_some() {
                eprintln!("Cannot capture screenshot while recording");
                return;
            }
            if self.ivars().editor_window.borrow().is_some() {
                eprintln!("Cannot capture screenshot while editing");
                return;
            }
            if self.ivars().scroll_capture_state.borrow().is_some() {
                eprintln!("Cannot capture screenshot while scroll capturing");
                return;
            }
            eprintln!("Capture triggered from menu!");
            self.do_capture();
        }

        #[unsafe(method(startRecording:))]
        fn start_recording_menu(&self, _sender: &AnyObject) {
            self.handle_record_hotkey();
        }

        #[unsafe(method(startScrollCapture:))]
        fn start_scroll_capture_menu(&self, _sender: &AnyObject) {
            self.handle_scroll_capture_hotkey();
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

        #[unsafe(method(toolHighlight:))]
        fn tool_highlight(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Highlight);
        }

        #[unsafe(method(toolStep:))]
        fn tool_step(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Step);
        }

        #[unsafe(method(toolBlur:))]
        fn tool_blur(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Blur);
        }

        #[unsafe(method(toolCrop:))]
        fn tool_crop(&self, _sender: &AnyObject) {
            self.set_active_tool(ActiveTool::Crop);
        }
    }

    // --- Color selection ---
    impl AppDelegate {
        #[unsafe(method(toggleColorPicker:))]
        fn toggle_color_picker(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
                if toolbar.view.is_color_picker_active() {
                    // Apply the chosen color when closing via the toggle button
                    self.apply_color_from_panel(mtm);
                    toolbar.hide_color_panel(mtm);
                } else {
                    // Observe when the color panel is closed by the user
                    let color_panel = NSColorPanel::sharedColorPanel(mtm);
                    let center = objc2_foundation::NSNotificationCenter::defaultCenter();
                    let observer: &AnyObject =
                        unsafe { &*(self as *const Self as *const AnyObject) };
                    unsafe {
                        center.addObserver_selector_name_object(
                            observer,
                            sel!(colorPanelClosed:),
                            Some(objc2_app_kit::NSWindowWillCloseNotification),
                            Some(&*color_panel),
                        );
                    }
                    toolbar.show_color_panel(mtm);
                }
            }
        }

        #[unsafe(method(colorPanelClosed:))]
        fn color_panel_closed(&self, _notification: &NSNotification) {
            let mtm = MainThreadMarker::from(self);
            // Remove the observer to avoid duplicate registrations
            let center = objc2_foundation::NSNotificationCenter::defaultCenter();
            let observer: &AnyObject =
                unsafe { &*(self as *const Self as *const AnyObject) };
            let color_panel = NSColorPanel::sharedColorPanel(mtm);
            unsafe {
                center.removeObserver_name_object(
                    observer,
                    Some(objc2_app_kit::NSWindowWillCloseNotification),
                    Some(&*color_panel),
                );
            }
            // Apply the chosen color and update button state
            self.apply_color_from_panel(mtm);
            if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
                toolbar.view.set_color_picker_active(false);
            }
        }
    }

    // --- Stroke width selection ---
    impl AppDelegate {
        #[unsafe(method(strokeThin:))]
        fn stroke_thin(&self, _sender: &AnyObject) {
            self.set_stroke_width(1.5, 14.0, 0);
        }

        #[unsafe(method(strokeMedium:))]
        fn stroke_medium(&self, _sender: &AnyObject) {
            self.set_stroke_width(3.0, 18.0, 1);
        }

        #[unsafe(method(strokeThick:))]
        fn stroke_thick(&self, _sender: &AnyObject) {
            self.set_stroke_width(5.5, 24.0, 2);
        }
    }

    // --- Actions ---
    impl AppDelegate {
        #[unsafe(method(actionUndo:))]
        fn action_undo(&self, _sender: &AnyObject) {
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                let mtm = MainThreadMarker::from(self);
                editor.undo_annotation(mtm);
                return;
            }
            if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
                if let Some(ann) = overlay.view.ivars().annotations.borrow_mut().pop() {
                    overlay.view.ivars().redo_stack.borrow_mut().push(ann);
                }
                overlay.view.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(actionRedo:))]
        fn action_redo(&self, _sender: &AnyObject) {
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                let mtm = MainThreadMarker::from(self);
                editor.redo_annotation(mtm);
                return;
            }
            if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
                if let Some(ann) = overlay.view.ivars().redo_stack.borrow_mut().pop() {
                    overlay.view.ivars().annotations.borrow_mut().push(ann);
                }
                overlay.view.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(actionCancel:))]
        fn action_cancel(&self, _sender: &AnyObject) {
            // If editor is open, mark as cancelled (discard without saving) and close
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                self.ivars().editor_cancelled.set(true);
                editor.window.close();
                return;
            }
            self.ivars().recording_mode.set(false);
            self.ivars().scroll_capture_mode.set(false);
            self.dismiss_all();
        }

        #[unsafe(method(actionSave:))]
        fn action_save(&self, _sender: &AnyObject) {
            // If editor is open, export with annotations and save
            if self.ivars().editor_window.borrow().is_some() {
                self.export_editor();
                return;
            }

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
            // If editor is open, check for crop mode first
            if self.ivars().editor_window.borrow().is_some() {
                let has_crop = {
                    let editor_ref = self.ivars().editor_window.borrow();
                    let editor = editor_ref.as_ref().unwrap();
                    let is_crop_tool = editor.view.ivars().active_tool.get() == ActiveTool::Crop;
                    let has_crop_rect = editor.view.ivars().crop_rect.get().is_some();
                    is_crop_tool && has_crop_rect
                };
                if has_crop {
                    self.apply_crop();
                } else {
                    self.export_editor();
                }
                return;
            }

            if self.ivars().scroll_capture_mode.get() {
                self.start_scroll_capture_with_selection();
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
            // In scroll capture mode, auto-start once a valid selection is drawn
            if self.ivars().scroll_capture_mode.get() {
                if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
                    if let Some(sel) = overlay.view.ivars().selection.get() {
                        let norm = crate::overlay::view::normalize_rect(sel);
                        if norm.size.width > 5.0 && norm.size.height > 5.0 {
                            self.start_scroll_capture_with_selection();
                            return;
                        }
                    }
                }
            }

            // In recording mode, auto-start once a valid selection is drawn
            if self.ivars().recording_mode.get() {
                if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
                    if let Some(sel) = overlay.view.ivars().selection.get() {
                        let norm = crate::overlay::view::normalize_rect(sel);
                        if norm.size.width > 5.0 && norm.size.height > 5.0 {
                            self.start_recording_with_selection();
                            return;
                        }
                    }
                }
            }

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

    // --- Scroll capture timer (called by NSTimer) ---
    impl AppDelegate {
        #[unsafe(method(scrollCaptureTick:))]
        fn scroll_capture_tick(&self, _timer: &NSObject) {
            let mut state_ref = self.ivars().scroll_capture_state.borrow_mut();
            if let Some(ref mut state) = *state_ref {
                if !state.tick() {
                    drop(state_ref);
                    self.stop_scroll_capture();
                }
            }
        }
    }

    // --- Editor actions ---
    impl AppDelegate {
        #[unsafe(method(editorPlayPause:))]
        fn editor_play_pause(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            let target: &AnyObject = unsafe { &*(self as *const Self as *const AnyObject) };
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                editor.toggle_playback(target, mtm);
            }
        }

        #[unsafe(method(editorReverse:))]
        fn editor_reverse(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            let target: &AnyObject = unsafe { &*(self as *const Self as *const AnyObject) };
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                editor.toggle_reverse(target, mtm);
            }
        }

        #[unsafe(method(editorAnnotationAdded:))]
        fn editor_annotation_added(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                if let Some(ann) = editor.view.take_pending_annotation() {
                    editor.add_annotation(ann, mtm);
                }
            }
        }

        #[unsafe(method(editorTimerTick:))]
        fn editor_timer_tick(&self, _timer: &NSObject) {
            let mtm = MainThreadMarker::from(self);
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                editor.advance_frame(mtm);
            }
        }

        #[unsafe(method(editorSliderChanged:))]
        fn editor_slider_changed(&self, sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                let value: f64 = unsafe { msg_send![sender, doubleValue] };
                editor.seek_to_frame(value as usize, mtm);
            }
        }

        #[unsafe(method(editorWindowClosed:))]
        fn editor_window_closed(&self, _notification: &NSNotification) {
            // Defer cleanup to next run-loop iteration.  The EditorView that
            // triggered this (via keyDown → close, or the window's own close
            // button) is still on the call stack.  Dropping the editor now
            // would deallocate it and cause a use-after-free / segfault.
            let this: &AnyObject = unsafe { &*(self as *const Self as *const AnyObject) };
            unsafe {
                let _: () = msg_send![
                    this,
                    performSelector: sel!(deferredEditorCleanup:),
                    withObject: std::ptr::null::<AnyObject>(),
                    afterDelay: 0.0_f64
                ];
            }
        }

        #[unsafe(method(deferredEditorCleanup:))]
        fn deferred_editor_cleanup(&self, _sender: Option<&AnyObject>) {
            if self.ivars().editor_cancelled.get() {
                self.ivars().editor_cancelled.set(false);
                self.close_editor_discard();
            } else {
                self.close_editor_and_save_raw();
            }
        }

        #[unsafe(method(editorConfirmAnnotation:))]
        fn editor_confirm_annotation(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                editor.confirm_active_annotation(mtm);
            }
        }

        #[unsafe(method(editorSelectionClick:))]
        fn editor_selection_click(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                let point = editor.view.take_selection_click_point();
                if let Some(point) = point {
                    let state = editor.state.borrow();
                    let frame = state.current_frame;
                    let hit = state.hit_test_annotation(point, frame);
                    drop(state);

                    if let Some(idx) = hit {
                        editor.select_annotation_at_index(idx, mtm);
                    } else {
                        editor.deselect_and_hide_mini_bar();
                        editor.display_current_frame(mtm);
                    }
                }
            }
        }

        #[unsafe(method(editorDeleteAnnotation:))]
        fn editor_delete_annotation(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                editor.delete_active_annotation(mtm);
            }
        }

        #[unsafe(method(editorMoveAnnotation:y:))]
        fn editor_move_annotation(&self, dx: CGFloat, dy: CGFloat) {
            let mtm = MainThreadMarker::from(self);
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                let active = editor.state.borrow().active_annotation;
                if let Some(idx) = active {
                    let mut state = editor.state.borrow_mut();
                    if let Some(ta) = state.annotations.get_mut(idx) {
                        ta.annotation.translate(dx, dy);
                    }
                    drop(state);
                    editor.display_current_frame(mtm);
                }
            }
        }

        #[unsafe(method(editorResizeAnnotation:x:y:))]
        fn editor_resize_annotation(&self, handle_val: u32, x: CGFloat, y: CGFloat) {
            let mtm = MainThreadMarker::from(self);
            let handle = match handle_val {
                0 => crate::annotation::model::HandleKind::ArrowStart,
                1 => crate::annotation::model::HandleKind::ArrowEnd,
                2 => crate::annotation::model::HandleKind::TopLeft,
                3 => crate::annotation::model::HandleKind::Top,
                4 => crate::annotation::model::HandleKind::TopRight,
                5 => crate::annotation::model::HandleKind::Left,
                6 => crate::annotation::model::HandleKind::Right,
                7 => crate::annotation::model::HandleKind::BottomLeft,
                8 => crate::annotation::model::HandleKind::Bottom,
                9 => crate::annotation::model::HandleKind::BottomRight,
                _ => return,
            };
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                let active = editor.state.borrow().active_annotation;
                if let Some(idx) = active {
                    let mut state = editor.state.borrow_mut();
                    if let Some(ta) = state.annotations.get_mut(idx) {
                        ta.annotation.apply_resize(handle, CGPoint::new(x, y));
                    }
                    drop(state);
                    editor.display_current_frame(mtm);
                }
            }
        }

        #[unsafe(method(editorApplyCrop:))]
        fn editor_apply_crop(&self, _sender: &AnyObject) {
            self.apply_crop();
        }

        #[unsafe(method(editorMiniBarChanged:))]
        fn editor_mini_bar_changed(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                if let Some(seek_frame) = editor.minibar_view.take_pending_seek_frame() {
                    editor.seek_to_frame(seek_frame, mtm);
                }
            }
        }

        #[unsafe(method(editorMiniBarDragEnded:))]
        fn editor_mini_bar_drag_ended(&self, _sender: &AnyObject) {
            let mtm = MainThreadMarker::from(self);
            if let Some(ref editor) = *self.ivars().editor_window.borrow() {
                let active_idx = editor.state.borrow().active_annotation;
                if let Some(idx) = active_idx {
                    let start = editor.minibar_view.start_frame();
                    let end = editor.minibar_view.end_frame();
                    let mut state = editor.state.borrow_mut();
                    state.set_annotation_start(idx, start);
                    if let Some(end) = end {
                        state.set_annotation_end(idx, end);
                    }
                    drop(state);
                    editor.display_current_frame(mtm);
                }
            }
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
            editor_window: RefCell::new(None),
            scroll_capture_mode: Cell::new(false),
            scroll_capture_state: RefCell::new(None),
            editor_cancelled: Cell::new(false),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn do_capture(&self) {
        let mtm = MainThreadMarker::from(self);

        // Hide toolbar from previous session
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.hide();
        }

        // Find the screen containing the mouse cursor
        let screen = crate::screen::screen_with_mouse(mtm);

        if let Some(cg_image) = crate::capture::capture_full_screen() {
            if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
                overlay.show_with_screenshot(&cg_image, &screen, mtm);
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

        // Don't start recording while editing
        if self.ivars().editor_window.borrow().is_some() {
            eprintln!("Cannot start recording while editing");
            return;
        }

        // Set recording mode and show overlay for region selection
        self.ivars().recording_mode.set(true);
        eprintln!("Record hotkey pressed — select region to start recording");
        self.do_capture();
    }

    fn start_recording_with_selection(&self) {
        let mtm = MainThreadMarker::from(self);

        // Read selection from overlay, and get display info from the overlay's screen
        let (selection, scale_factor, display_id, screen_frame) = {
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
            let sf_frame = overlay.window.frame();
            let did = crate::screen::display_with_mouse();
            (sel, sf, did, sf_frame)
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
            "clipshot_recording_{}.mp4",
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
        let mut recording = RecordingState::new(encoder, selection, scale_factor, display_id);
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
            border.show(selection, screen_frame);
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

        eprintln!("Recording stopped");

        // Open the editor instead of showing save dialog directly
        if let Some(tmp_path) = &recording.output_path {
            self.open_editor(tmp_path, mtm);
        }
    }

    fn open_editor(&self, video_path: &PathBuf, mtm: MainThreadMarker) {
        match EditorWindow::open(video_path, mtm) {
            Ok(editor) => {
                // Observe window close to exit editing mode
                let center = objc2_foundation::NSNotificationCenter::defaultCenter();
                let observer: &AnyObject =
                    unsafe { &*(self as *const Self as *const AnyObject) };
                unsafe {
                    center.addObserver_selector_name_object(
                        observer,
                        sel!(editorWindowClosed:),
                        Some(objc2_app_kit::NSWindowWillCloseNotification),
                        Some(&*editor.window),
                    );
                }

                // Show the toolbar attached above the editor window
                self.show_editor_toolbar(&editor);
                *self.ivars().editor_window.borrow_mut() = Some(editor);
            }
            Err(e) => {
                eprintln!("Failed to open editor: {}", e);
                // Fall back to save dialog for raw video
                self.show_save_dialog_for_recording(video_path, mtm);
            }
        }
    }

    fn show_editor_toolbar(&self, editor: &EditorWindow) {
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            // Position toolbar centered above the editor window with a small gap
            let editor_frame = editor.window.frame();
            let toolbar_size = toolbar.view.frame().size;
            let gap: CGFloat = 4.0;
            let x = editor_frame.origin.x + (editor_frame.size.width - toolbar_size.width) / 2.0;
            let y = editor_frame.origin.y + editor_frame.size.height + gap;
            toolbar.panel.setFrameOrigin(objc2_core_foundation::CGPoint::new(x, y));
            toolbar.panel.orderFrontRegardless();

            // Attach toolbar as child window so it moves with the editor
            let _: () = unsafe {
                msg_send![&*editor.window, addChildWindow: &*toolbar.panel, ordered: 1i64]
            };
        }
    }

    fn export_editor(&self) {
        let mtm = MainThreadMarker::from(self);

        let editor_ref = self.ivars().editor_window.borrow();
        let Some(ref editor) = *editor_ref else {
            return;
        };

        // Single-frame mode: export as image instead of video
        let is_single_frame = editor.decoder.total_frames() == 1;
        if is_single_frame {
            drop(editor_ref);
            self.export_editor_as_image();
            return;
        }

        // Commit any pending text field
        editor.view.commit_text_field();

        let state = editor.sessions();
        let video_path = state.video_path.clone();
        let annotations = &state.annotations;

        // Create temp file for export
        let export_path = std::env::temp_dir().join(format!(
            "clipshot_export_{}.mp4",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ));

        // Check if there are any annotations at all
        let has_annotations = state.has_any_annotations();

        if has_annotations {
            if let Err(e) =
                crate::editor::export::export_with_annotations(
                    &editor.decoder,
                    annotations,
                    &export_path,
                    {
                        let vb = editor.view.bounds();
                        (vb.size.width, vb.size.height)
                    },
                )
            {
                eprintln!("Export failed: {}", e);
                drop(state);
                drop(editor_ref);
                // Fall back to saving raw video
                self.close_editor_and_save_raw();
                return;
            }
        }

        drop(state);
        drop(editor_ref);

        // Close editor
        if let Some(editor) = self.ivars().editor_window.borrow_mut().take() {
            // Remove notification observer
            let center = objc2_foundation::NSNotificationCenter::defaultCenter();
            let observer: &AnyObject =
                unsafe { &*(self as *const Self as *const AnyObject) };
            unsafe {
                center.removeObserver_name_object(
                    observer,
                    Some(objc2_app_kit::NSWindowWillCloseNotification),
                    Some(&*editor.window),
                );
            }
            editor.close();
        }
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.hide();
        }

        // Show save dialog
        if has_annotations {
            self.show_save_dialog_for_recording(&export_path, mtm);
            // Clean up raw video
            let _ = std::fs::remove_file(&video_path);
        } else {
            // No annotations: save the raw video directly
            self.show_save_dialog_for_recording(&video_path, mtm);
        }
    }

    /// Close the editor and discard everything (cancel). No save, no clipboard copy.
    fn close_editor_discard(&self) {
        let editor = self.ivars().editor_window.borrow_mut().take();
        if let Some(editor) = editor {
            // Remove notification observer
            let center = objc2_foundation::NSNotificationCenter::defaultCenter();
            let observer: &AnyObject =
                unsafe { &*(self as *const Self as *const AnyObject) };
            unsafe {
                center.removeObserver_name_object(
                    observer,
                    Some(objc2_app_kit::NSWindowWillCloseNotification),
                    Some(&*editor.window),
                );
            }

            // Clean up temp video file
            let video_path = editor.state.borrow().video_path.clone();
            let _ = std::fs::remove_file(&video_path);

            editor.close();
        }
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.hide();
        }
    }

    fn close_editor_and_save_raw(&self) {
        let mtm = MainThreadMarker::from(self);

        let editor = self.ivars().editor_window.borrow_mut().take();
        if let Some(editor) = editor {
            // Remove notification observer
            let center = objc2_foundation::NSNotificationCenter::defaultCenter();
            let observer: &AnyObject =
                unsafe { &*(self as *const Self as *const AnyObject) };
            unsafe {
                center.removeObserver_name_object(
                    observer,
                    Some(objc2_app_kit::NSWindowWillCloseNotification),
                    Some(&*editor.window),
                );
            }

            let is_single_frame = editor.decoder.total_frames() == 1;
            let video_path = editor.state.borrow().video_path.clone();

            // For single-frame mode (scroll capture), offer to save as PNG
            if is_single_frame {
                if let Some(source_image) = editor.decoder.frame_at(0) {
                    editor.close();
                    if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
                        toolbar.hide();
                    }
                    crate::actions::save_to_file(source_image, mtm);
                    return;
                }
            }

            editor.close();

            if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
                toolbar.hide();
            }

            // Offer to save the raw video
            self.show_save_dialog_for_recording(&video_path, mtm);
        }
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
        // Map ActiveTool to toolbar button index
        let tool_index = match tool {
            ActiveTool::Select => 0,
            ActiveTool::Arrow => 1,
            ActiveTool::Rectangle => 2,
            ActiveTool::Ellipse => 3,
            ActiveTool::Pencil => 4,
            ActiveTool::Text => 5,
            ActiveTool::Highlight => 6,
            ActiveTool::Step => 7,
            ActiveTool::Blur => 8,
            ActiveTool::Crop => 9,
        };
        // Update toolbar visual state
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.view.set_active_tool(tool_index);
        }
        if let Some(ref editor) = *self.ivars().editor_window.borrow() {
            editor.view.commit_text_field();
            editor.view.ivars().active_tool.set(tool);
            // Auto-set crop rect to full view bounds when activating crop tool
            if tool == ActiveTool::Crop && editor.view.ivars().crop_rect.get().is_none() {
                let bounds = editor.view.bounds();
                editor.view.ivars().crop_rect.set(Some(bounds));
                editor.view.setNeedsDisplay(true);
            }
            return;
        }
        if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
            overlay.view.commit_text_field();
            overlay.view.ivars().active_tool.set(tool);
        }
    }

    fn set_stroke_width(&self, width: CGFloat, font_size: CGFloat, stroke_index: usize) {
        // Update toolbar visual state
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.view.set_active_stroke(stroke_index);
        }
        if let Some(ref editor) = *self.ivars().editor_window.borrow() {
            editor.view.ivars().annotation_width.set(width);
            editor.view.ivars().annotation_font_size.set(font_size);
            return;
        }
        if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
            overlay.view.ivars().annotation_width.set(width);
            overlay.view.ivars().annotation_font_size.set(font_size);
        }
    }

    fn apply_color_from_panel(&self, mtm: MainThreadMarker) {
        let color_panel = NSColorPanel::sharedColorPanel(mtm);
        let color = color_panel.color();
        let r: CGFloat = unsafe { msg_send![&color, redComponent] };
        let g: CGFloat = unsafe { msg_send![&color, greenComponent] };
        let b: CGFloat = unsafe { msg_send![&color, blueComponent] };
        self.set_annotation_color((r, g, b));
        // Update the color button tint
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.view.set_color(r, g, b);
        }
    }

    fn set_annotation_color(&self, color: (f64, f64, f64)) {
        if let Some(ref editor) = *self.ivars().editor_window.borrow() {
            editor.view.ivars().annotation_color.set(color);
            return;
        }
        if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
            overlay.view.ivars().annotation_color.set(color);
        }
    }

    fn dismiss_all(&self) {
        if let Some(overlay) = self.ivars().overlay.borrow().as_ref() {
            overlay.hide();
        }
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            // Re-enable all buttons so toolbar is in a clean state for next use
            toolbar.set_non_confirm_buttons_enabled(true);
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

    fn update_toolbar_position(&self, _mtm: MainThreadMarker) {
        let overlay_ref = self.ivars().overlay.borrow();
        let overlay = overlay_ref.as_ref();
        let toolbar_ref = self.ivars().toolbar.borrow();
        let toolbar = toolbar_ref.as_ref();

        if let (Some(overlay), Some(toolbar)) = (overlay, toolbar) {
            if let Some(selection) = overlay.view.ivars().selection.get() {
                let norm = crate::overlay::view::normalize_rect(selection);
                if norm.size.width > 5.0 && norm.size.height > 5.0 {
                    // Use the overlay window's screen for toolbar positioning
                    let screen_frame = overlay.window.frame();
                    toolbar.show_near_selection(norm, screen_frame);
                    let _: () = unsafe {
                        msg_send![&*overlay.window, addChildWindow: &*toolbar.panel, ordered: 1i64]
                    };
                }
            }
        }
    }

    fn handle_scroll_capture_hotkey(&self) {
        // Don't start scroll capture while recording or editing
        if self.ivars().recording_state.borrow().is_some() {
            eprintln!("Cannot start scroll capture while recording");
            return;
        }
        if self.ivars().editor_window.borrow().is_some() {
            eprintln!("Cannot start scroll capture while editing");
            return;
        }

        // Set scroll capture mode and show overlay for region selection
        self.ivars().scroll_capture_mode.set(true);
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.set_non_confirm_buttons_enabled(false);
        }
        eprintln!("Scroll capture hotkey pressed — select region then confirm");
        self.do_capture();
    }

    fn start_scroll_capture_with_selection(&self) {
        let mtm = MainThreadMarker::from(self);

        // Read selection from overlay, and get display info from the overlay's screen
        let (selection, scale_factor, display_id, screen_frame) = {
            let overlay_ref = self.ivars().overlay.borrow();
            let overlay = match overlay_ref.as_ref() {
                Some(o) => o,
                None => return,
            };
            let sel = match overlay.view.ivars().selection.get() {
                Some(s) => crate::overlay::view::normalize_rect(s),
                None => {
                    eprintln!("No selection for scroll capture");
                    return;
                }
            };
            let sf = overlay.view.ivars().scale_factor.get();
            let sf_frame = overlay.window.frame();
            let did = crate::screen::display_with_mouse();
            (sel, sf, did, sf_frame)
        };

        // Dismiss overlay and clear mode
        self.ivars().scroll_capture_mode.set(false);
        self.dismiss_all();

        // Show the border window around the capture region so user can see the selected area
        if let Some(border) = self.ivars().recording_border.borrow().as_ref() {
            border.show(selection, screen_frame);
        }

        // Create scroll capture state — use CG display origin for coordinate conversion
        let cg_bounds = CGDisplayBounds(display_id);
        let screen_origin = CGPoint::new(cg_bounds.origin.x, cg_bounds.origin.y);
        let mut state = ScrollCaptureState::new(
            selection,
            scale_factor,
            screen_origin,
            display_id,
        );

        // Exclude the border window from screen captures
        if let Some(border) = self.ivars().recording_border.borrow().as_ref() {
            state.set_border_window_id(border.window_number());
        }

        // The first timer tick captures the initial frame (Phase::Capture).
        // We don't capture synchronously here because dismiss_all() above is
        // asynchronous — the overlay window is still visible until the run loop
        // processes its removal. The timer delay gives the UI time to settle.
        let target: &AnyObject = unsafe { &*(self as *const Self as *const AnyObject) };
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                state.settle_delay,
                target,
                sel!(scrollCaptureTick:),
                None,
                true,
            )
        };
        state.timer = Some(timer);

        *self.ivars().scroll_capture_state.borrow_mut() = Some(state);

        // Show status bar recording indicator
        if let Some(sb) = self.ivars().status_bar.borrow().as_ref() {
            sb.enter_recording_mode(mtm);
        }

        eprintln!("Scroll capture started");
    }

    fn stop_scroll_capture(&self) {
        let mtm = MainThreadMarker::from(self);

        let state = self.ivars().scroll_capture_state.borrow_mut().take();
        let Some(mut state) = state else {
            return;
        };

        // Invalidate timer
        if let Some(timer) = state.timer.take() {
            timer.invalidate();
        }

        // Hide the border window
        if let Some(border) = self.ivars().recording_border.borrow().as_ref() {
            border.hide();
        }

        // Exit recording mode in status bar
        if let Some(sb) = self.ivars().status_bar.borrow().as_ref() {
            sb.exit_recording_mode(mtm);
        }

        let frame_count = state.frames.len();
        eprintln!("Scroll capture stopped: {} frames captured", frame_count);

        if frame_count == 0 {
            eprintln!("No frames captured");
            return;
        }

        // Stitch frames using pre-captured RGBA data for overlap detection
        let stitched = crate::stitch::stitch_frames(&state.frames, &state.frame_rgba);
        let Some(stitched) = stitched else {
            eprintln!("Failed to stitch frames");
            return;
        };

        let width = CGImage::width(Some(&stitched));
        let height = CGImage::height(Some(&stitched));
        eprintln!("Stitched image: {}x{}", width, height);

        // Create decoder from stitched image and open editor
        let decoder = crate::editor::decoder::VideoDecoder::from_image(stitched);

        // Use a temporary path for the editor state
        let tmp_path = std::env::temp_dir().join(format!(
            "clipshot_scroll_{}.png",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ));

        match EditorWindow::open_with_decoder(decoder, "Edit Scroll Capture", &tmp_path, mtm) {
            Ok(editor) => {
                // Observe window close
                let center = objc2_foundation::NSNotificationCenter::defaultCenter();
                let observer: &AnyObject =
                    unsafe { &*(self as *const Self as *const AnyObject) };
                unsafe {
                    center.addObserver_selector_name_object(
                        observer,
                        sel!(editorWindowClosed:),
                        Some(objc2_app_kit::NSWindowWillCloseNotification),
                        Some(&*editor.window),
                    );
                }

                self.show_editor_toolbar(&editor);
                *self.ivars().editor_window.borrow_mut() = Some(editor);
            }
            Err(e) => {
                eprintln!("Failed to open editor for scroll capture: {}", e);
            }
        }
    }

    fn export_editor_as_image(&self) {
        let mtm = MainThreadMarker::from(self);

        let editor_ref = self.ivars().editor_window.borrow();
        let Some(ref editor) = *editor_ref else {
            return;
        };

        // Commit any pending text field
        editor.view.commit_text_field();

        let Some(source_image) = editor.decoder.frame_at(0) else {
            drop(editor_ref);
            return;
        };

        let state = editor.sessions();
        let annotations: Vec<&crate::annotation::model::Annotation> = state
            .annotations
            .iter()
            .map(|ta| &ta.annotation)
            .collect();

        let width = editor.decoder.width();
        let height = editor.decoder.height();
        let view_bounds = editor.view.bounds();
        let view_size = (view_bounds.size.width, view_bounds.size.height);
        let composited = crate::editor::export::composite_frame(source_image, &annotations, width, height, view_size);

        // Apply crop if present
        let crop_rect = editor.view.ivars().crop_rect.get();
        let final_image = if let (Some(img), Some(crop)) = (&composited, crop_rect) {
            let norm = crate::overlay::view::normalize_rect(crop);
            // Scale from view coords to pixel coords
            let view_bounds = editor.view.bounds();
            let sx = width as CGFloat / view_bounds.size.width;
            let sy = height as CGFloat / view_bounds.size.height;
            let pixel_crop = objc2_core_foundation::CGRect::new(
                CGPoint::new(norm.origin.x * sx, norm.origin.y * sy),
                objc2_core_foundation::CGSize::new(norm.size.width * sx, norm.size.height * sy),
            );
            objc2_core_graphics::CGImage::with_image_in_rect(Some(img), pixel_crop)
        } else {
            composited
        };

        drop(state);
        drop(editor_ref);

        // Close editor
        if let Some(editor) = self.ivars().editor_window.borrow_mut().take() {
            let center = objc2_foundation::NSNotificationCenter::defaultCenter();
            let observer: &AnyObject =
                unsafe { &*(self as *const Self as *const AnyObject) };
            unsafe {
                center.removeObserver_name_object(
                    observer,
                    Some(objc2_app_kit::NSWindowWillCloseNotification),
                    Some(&*editor.window),
                );
            }
            editor.close();
        }
        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.hide();
        }

        if let Some(image) = final_image {
            crate::actions::save_to_file(&image, mtm);
        }
    }

    fn apply_crop(&self) {
        let mtm = MainThreadMarker::from(self);

        let mut editor_ref = self.ivars().editor_window.borrow_mut();
        let Some(ref mut editor) = *editor_ref else {
            return;
        };

        // Commit any pending text field
        editor.view.commit_text_field();

        // Get the crop rect (in view coordinates)
        let Some(crop) = editor.view.ivars().crop_rect.get() else {
            return;
        };
        let norm_crop = crate::overlay::view::normalize_rect(crop);

        // Get the source image
        let Some(source_image) = editor.decoder.frame_at(0) else {
            return;
        };

        // Composite source image + all annotations
        let state = editor.sessions();
        let annotations: Vec<&crate::annotation::model::Annotation> = state
            .annotations
            .iter()
            .map(|ta| &ta.annotation)
            .collect();
        let width = editor.decoder.width();
        let height = editor.decoder.height();
        let view_bounds = editor.view.bounds();
        let view_size = (view_bounds.size.width, view_bounds.size.height);
        let composited = crate::editor::export::composite_frame(source_image, &annotations, width, height, view_size);
        drop(state);

        let Some(composited) = composited else {
            eprintln!("Failed to composite frame for crop");
            return;
        };

        // Scale crop rect from view coords to pixel coords
        let view_bounds = editor.view.bounds();
        let sx = width as CGFloat / view_bounds.size.width;
        let sy = height as CGFloat / view_bounds.size.height;
        let pixel_crop = objc2_core_foundation::CGRect::new(
            CGPoint::new(norm_crop.origin.x * sx, norm_crop.origin.y * sy),
            objc2_core_foundation::CGSize::new(norm_crop.size.width * sx, norm_crop.size.height * sy),
        );

        // Crop the composited image
        let Some(cropped) = objc2_core_graphics::CGImage::with_image_in_rect(Some(&composited), pixel_crop) else {
            eprintln!("Failed to crop image");
            return;
        };

        // Replace the decoder's image with the cropped result
        editor.decoder.replace_image(cropped);

        // Clear all annotations (they're now baked into the image)
        editor.state.borrow_mut().clear_all();

        // Clear crop rect
        editor.view.ivars().crop_rect.set(None);

        // Resize view/window for new image dimensions
        editor.resize_for_new_image(mtm);

        // Switch tool to Select and update toolbar
        editor.view.ivars().active_tool.set(ActiveTool::Select);
        drop(editor_ref);

        if let Some(toolbar) = self.ivars().toolbar.borrow().as_ref() {
            toolbar.view.set_active_tool(0); // 0 = Select
        }
    }
}
