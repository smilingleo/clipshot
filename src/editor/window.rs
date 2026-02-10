use std::cell::{Cell, RefCell};
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSImage, NSSlider, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSRect, NSSize, NSString, NSTimer};

use super::decoder::VideoDecoder;
use super::minibar::{MiniBarView, MINI_BAR_GAP, MINI_BAR_HEIGHT, MINI_BAR_WIDTH};
use super::model::EditorState;
use super::view::EditorView;
use crate::annotation::model::Annotation;

const SLIDER_HEIGHT: CGFloat = 24.0;
const SLIDER_PADDING: CGFloat = 8.0;
const PROGRESS_BAR_HEIGHT: CGFloat = SLIDER_HEIGHT + SLIDER_PADDING * 2.0;

pub struct EditorWindow {
    pub window: Retained<NSWindow>,
    pub view: Retained<EditorView>,
    pub slider: Retained<NSSlider>,
    pub minibar_view: Retained<MiniBarView>,
    pub state: RefCell<EditorState>,
    pub decoder: VideoDecoder,
    pub timer: RefCell<Option<Retained<NSTimer>>>,
    /// True when playing in reverse direction.
    pub reversing: Cell<bool>,
    /// True for single-frame images (screenshots), false for video recordings.
    pub is_single_frame: bool,
}

impl EditorWindow {
    /// Open the editor with a recorded video file.
    pub fn open(video_path: &Path, mtm: MainThreadMarker) -> Result<Self, String> {
        let decoder = VideoDecoder::open(video_path)?;
        Self::open_with_decoder(decoder, "Edit Recording", video_path, mtm)
    }

    /// Open the editor with an already-constructed decoder.
    pub fn open_with_decoder(
        decoder: VideoDecoder,
        title: &str,
        source_path: &Path,
        mtm: MainThreadMarker,
    ) -> Result<Self, String> {
        let dec_width = decoder.width();
        let dec_height = decoder.height();
        let total_frames = decoder.total_frames();
        let fps = decoder.fps();

        if total_frames == 0 {
            return Err("No frames to display".to_string());
        }

        let is_single_frame = total_frames == 1;

        // Scale up if video height is less than 800px
        const MIN_HEIGHT: CGFloat = 800.0;
        let native_w = dec_width as CGFloat;
        let native_h = dec_height as CGFloat;
        let (view_w, view_h) = if native_h < MIN_HEIGHT {
            let scale = MIN_HEIGHT / native_h;
            ((native_w * scale).round(), MIN_HEIGHT)
        } else {
            (native_w, native_h)
        };

        // For single-frame (stitched image) mode, cap height at screen height - 100
        let screen = crate::screen::screen_with_mouse(mtm);
        let screen_height = screen.frame().size.height;
        let max_view_h = screen_height - 100.0;
        let (view_w, view_h) = if is_single_frame && view_h > max_view_h {
            let scale = max_view_h / view_h;
            ((view_w * scale).round(), max_view_h)
        } else {
            (view_w, view_h)
        };

        // Window: video + progress slider (slider hidden in single-frame mode)
        let progress_height = if is_single_frame { 0.0 } else { PROGRESS_BAR_HEIGHT };
        let window_w = view_w;
        let window_h = view_h + progress_height;

        let state = EditorState::new(
            source_path.to_path_buf(),
            total_frames,
            fps,
        );

        // Create the window
        let content_rect = NSRect::new(CGPoint::ZERO, CGSize::new(window_w, window_h));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                content_rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&NSString::from_str(title));
        unsafe { window.setReleasedWhenClosed(false) };
        window.center();

        // Non-flipped layout (origin bottom-left):
        // y=SLIDER_PADDING: main slider
        // y=PROGRESS_BAR_HEIGHT: video view

        // Create the editor view (video area)
        let view_frame = NSRect::new(
            CGPoint::new(0.0, progress_height),
            CGSize::new(view_w, view_h),
        );
        let view = EditorView::new(mtm, view_frame);

        // Create the progress slider at the bottom
        let slider_frame = NSRect::new(
            CGPoint::new(SLIDER_PADDING, SLIDER_PADDING),
            CGSize::new(window_w - SLIDER_PADDING * 2.0, SLIDER_HEIGHT),
        );
        let slider: Retained<NSSlider> =
            unsafe { msg_send![mtm.alloc(), initWithFrame: slider_frame] };
        slider.setMinValue(0.0);
        slider.setMaxValue((total_frames.saturating_sub(1)) as f64);
        slider.setDoubleValue(0.0);
        #[allow(deprecated)]
        slider.setContinuous(true);
        let sel_cstr = std::ffi::CString::new("editorSliderChanged:").unwrap();
        unsafe {
            slider.setAction(Some(Sel::register(&sel_cstr)));
            slider.setTarget(None);
        }

        // Hide slider in single-frame mode
        if is_single_frame {
            slider.setHidden(true);
        }

        // Create the floating mini bar (starts hidden, is a subview of the editor view)
        let minibar_frame = NSRect::new(
            CGPoint::ZERO,
            CGSize::new(MINI_BAR_WIDTH, MINI_BAR_HEIGHT),
        );
        let minibar_view = MiniBarView::new(mtm, minibar_frame);
        minibar_view.setHidden(true);
        view.addSubview(&minibar_view);

        // Add views to the window's content view
        if let Some(content_view) = window.contentView() {
            content_view.addSubview(&view);
            if !is_single_frame {
                content_view.addSubview(&slider);
            }
        }

        let editor = EditorWindow {
            window,
            view,
            slider,
            minibar_view,
            state: RefCell::new(state),
            decoder,
            timer: RefCell::new(None),
            reversing: Cell::new(false),
            is_single_frame,
        };

        // Display the first frame
        editor.display_current_frame(mtm);

        // Activate and show
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        editor.window.makeKeyAndOrderFront(None);
        editor.window.makeFirstResponder(Some(&*editor.view));

        eprintln!("Editor opened: {} frames, {:.1}fps, {}x{}", total_frames, fps, dec_width, dec_height);

        Ok(editor)
    }

    /// Display the frame at the current position, with all visible annotations.
    pub fn display_current_frame(&self, mtm: MainThreadMarker) {
        let state = self.state.borrow();
        let frame_idx = state.current_frame;

        let Some(cg_image) = self.decoder.frame_at(frame_idx) else {
            return;
        };

        // Convert CGImage to NSImage at view size
        let ns_image = NSImage::initWithCGImage_size(
            mtm.alloc(),
            cg_image,
            NSSize::new(
                self.decoder.width() as CGFloat,
                self.decoder.height() as CGFloat,
            ),
        );

        // Collect visible annotations with their indices
        let visible: Vec<(usize, Annotation)> = state
            .annotations_at_frame(frame_idx)
            .into_iter()
            .map(|(idx, ann)| (idx, ann.clone()))
            .collect();

        // Update active annotation highlight on the view
        self.view.set_active_annotation_index(state.active_annotation);

        self.view.display_frame(ns_image, cg_image, visible);

        // Update slider position
        self.slider.setDoubleValue(frame_idx as f64);

        // Update mini bar state and position if there's an active annotation
        if let Some((active_idx, range)) = state.active_annotation.and_then(|idx| {
            state.active_annotation_range().map(|r| (idx, r))
        }) {
            self.minibar_view.update_state(
                range.0,
                range.1,
                frame_idx,
                state.total_frames,
            );
            // Position the mini bar under the annotation
            let ann_rect = state.annotations.get(active_idx)
                .map(|ta| ta.annotation.bounding_rect());
            if let Some(ann_rect) = ann_rect {
                self.position_mini_bar_under(ann_rect);
            }
        }
    }

    /// Position the mini bar centered below an annotation's bounding rect,
    /// clamped to stay within the editor view bounds.
    fn position_mini_bar_under(&self, ann_rect: CGRect) {
        let view_bounds = self.view.bounds();
        let bar_w = MINI_BAR_WIDTH;
        let bar_h = MINI_BAR_HEIGHT;

        // Center horizontally under the annotation
        let ann_center_x = ann_rect.origin.x + ann_rect.size.width / 2.0;
        let mut bar_x = ann_center_x - bar_w / 2.0;

        // Place just below the annotation
        let mut bar_y = ann_rect.origin.y + ann_rect.size.height + MINI_BAR_GAP;

        // Clamp to view bounds
        bar_x = bar_x.max(2.0).min(view_bounds.size.width - bar_w - 2.0);
        bar_y = bar_y.max(2.0).min(view_bounds.size.height - bar_h - 2.0);

        self.minibar_view.setFrame(NSRect::new(
            CGPoint::new(bar_x, bar_y),
            CGSize::new(bar_w, bar_h),
        ));
    }

    /// Seek to a specific frame (called when slider is dragged).
    pub fn seek_to_frame(&self, frame: usize, mtm: MainThreadMarker) {
        let total = self.state.borrow().total_frames;
        let clamped = frame.min(total.saturating_sub(1));
        self.state.borrow_mut().current_frame = clamped;
        self.display_current_frame(mtm);
    }

    /// Toggle forward play/pause.
    pub fn toggle_playback(&self, timer_target: &AnyObject, mtm: MainThreadMarker) {
        let is_playing = self.state.borrow().is_playing;
        if is_playing && !self.reversing.get() {
            self.pause(mtm);
        } else {
            self.stop_timer();
            self.reversing.set(false);
            self.start_play(timer_target);
        }
    }

    /// Toggle reverse play/pause.
    pub fn toggle_reverse(&self, timer_target: &AnyObject, mtm: MainThreadMarker) {
        let is_playing = self.state.borrow().is_playing;
        if is_playing && self.reversing.get() {
            self.pause(mtm);
        } else {
            self.stop_timer();
            self.reversing.set(true);
            self.start_play(timer_target);
        }
    }

    /// Start playback timer (direction determined by `self.reversing`).
    fn start_play(&self, timer_target: &AnyObject) {
        self.view.commit_text_field();
        self.state.borrow_mut().play();

        let fps = self.state.borrow().fps;
        let interval = 1.0 / fps;

        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                interval,
                timer_target,
                sel!(editorTimerTick:),
                None,
                true,
            )
        };
        *self.timer.borrow_mut() = Some(timer);

        let dir = if self.reversing.get() { "reverse" } else { "forward" };
        eprintln!("Editor: playing {}", dir);
    }

    /// Stop the playback timer without any side effects.
    fn stop_timer(&self) {
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.invalidate();
        }
    }

    /// Pause playback: stop timer, set is_playing = false.
    fn pause(&self, mtm: MainThreadMarker) {
        self.stop_timer();
        self.reversing.set(false);

        self.state.borrow_mut().is_playing = false;

        let current_frame = self.state.borrow().current_frame;
        self.display_current_frame(mtm);

        eprintln!("Editor: paused at frame {}", current_frame);
    }

    /// Step one frame forward or backward (called by timer).
    pub fn advance_frame(&self, mtm: MainThreadMarker) {
        let mut state = self.state.borrow_mut();
        if !state.is_playing {
            return;
        }

        if self.reversing.get() {
            if state.current_frame == 0 {
                drop(state);
                self.pause(mtm);
                return;
            }
            state.current_frame -= 1;
        } else {
            state.current_frame += 1;
            if state.current_frame >= state.total_frames {
                state.current_frame = state.total_frames - 1;
                drop(state);
                self.pause(mtm);
                return;
            }
        }
        drop(state);

        self.display_current_frame(mtm);
    }

    /// Add an annotation at the current frame. Shows the mini bar.
    pub fn add_annotation(&self, annotation: Annotation, mtm: MainThreadMarker) {
        let frame = self.state.borrow().current_frame;
        self.state.borrow_mut().add_annotation(annotation, frame);
        self.show_mini_bar(mtm);
        self.display_current_frame(mtm);
    }

    /// Remove the last/active annotation.
    pub fn undo_annotation(&self, mtm: MainThreadMarker) {
        let had_active = self.state.borrow().active_annotation.is_some();
        self.state.borrow_mut().undo_annotation();
        if had_active {
            self.hide_mini_bar();
        }
        self.display_current_frame(mtm);
    }

    /// Redo the last undone annotation.
    pub fn redo_annotation(&self, mtm: MainThreadMarker) {
        if self.state.borrow_mut().redo_annotation() {
            self.show_mini_bar(mtm);
            self.display_current_frame(mtm);
        }
    }

    /// Confirm the active annotation's end frame at the current position.
    pub fn confirm_active_annotation(&self, mtm: MainThreadMarker) {
        let frame = self.state.borrow().current_frame;
        self.state.borrow_mut().confirm_active(frame);
        self.hide_mini_bar();
        self.view.set_active_annotation_index(None);
        self.display_current_frame(mtm);
    }

    /// Select an annotation by index and show the mini bar.
    pub fn select_annotation_at_index(&self, idx: usize, mtm: MainThreadMarker) {
        self.state.borrow_mut().select_annotation(idx);
        self.view.set_active_annotation_index(Some(idx));
        self.show_mini_bar(mtm);
        self.display_current_frame(mtm);
    }

    /// Delete the active (selected) annotation.
    pub fn delete_active_annotation(&self, mtm: MainThreadMarker) {
        let active = self.state.borrow().active_annotation;
        if let Some(idx) = active {
            self.state.borrow_mut().delete_annotation(idx);
            self.hide_mini_bar();
            self.view.set_active_annotation_index(None);
            self.display_current_frame(mtm);
        }
    }

    /// Deselect the active annotation and hide the mini bar.
    pub fn deselect_and_hide_mini_bar(&self) {
        self.state.borrow_mut().deselect_annotation();
        self.view.set_active_annotation_index(None);
        self.hide_mini_bar();
    }

    /// Show the mini bar positioned under the active annotation.
    /// Skipped for single-frame images (no timeline to edit).
    fn show_mini_bar(&self, _mtm: MainThreadMarker) {
        if self.is_single_frame {
            return;
        }
        let state = self.state.borrow();
        if let Some(range) = state.active_annotation_range() {
            self.minibar_view.update_state(
                range.0,
                range.1,
                state.current_frame,
                state.total_frames,
            );
            // Position under the annotation
            if let Some(active_idx) = state.active_annotation {
                if let Some(ta) = state.annotations.get(active_idx) {
                    let ann_rect = ta.annotation.bounding_rect();
                    drop(state);
                    self.position_mini_bar_under(ann_rect);
                    self.minibar_view.setHidden(false);
                    return;
                }
            }
            drop(state);
            self.minibar_view.setHidden(false);
        }
    }

    /// Hide the mini bar.
    fn hide_mini_bar(&self) {
        self.minibar_view.setHidden(true);
    }

    /// Close the editor window and clean up.
    pub fn close(&self) {
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.invalidate();
        }
        self.window.orderOut(None);
    }

    /// Get a reference to the editor state for export.
    pub fn sessions(&self) -> std::cell::Ref<'_, EditorState> {
        self.state.borrow()
    }
}
