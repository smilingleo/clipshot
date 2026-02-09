use std::cell::RefCell;
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSImage, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_foundation::{MainThreadMarker, NSRect, NSSize, NSString, NSTimer};

use super::decoder::VideoDecoder;
use super::model::EditorState;
use super::view::EditorView;
use crate::annotation::model::Annotation;

pub struct EditorWindow {
    pub window: Retained<NSWindow>,
    pub view: Retained<EditorView>,
    pub state: RefCell<EditorState>,
    pub decoder: VideoDecoder,
    pub timer: RefCell<Option<Retained<NSTimer>>>,
}

impl EditorWindow {
    /// Open the editor with a recorded video file.
    pub fn open(video_path: &Path, mtm: MainThreadMarker) -> Result<Self, String> {
        let decoder = VideoDecoder::open(video_path)?;

        let dec_width = decoder.width();
        let dec_height = decoder.height();
        let total_frames = decoder.total_frames();
        let fps = decoder.fps();
        let width = dec_width as CGFloat;
        let height = dec_height as CGFloat;

        if total_frames == 0 {
            return Err("Video has no frames".to_string());
        }

        let state = EditorState::new(
            video_path.to_path_buf(),
            total_frames,
            fps,
        );

        // Create the window
        let content_rect = NSRect::new(CGPoint::ZERO, CGSize::new(width, height));
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
        window.setTitle(&NSString::from_str("Edit Recording"));
        window.center();

        // Create the editor view
        let view_frame = NSRect::new(CGPoint::ZERO, CGSize::new(width, height));
        let view = EditorView::new(mtm, view_frame);
        window.setContentView(Some(&view));

        let editor = EditorWindow {
            window,
            view,
            state: RefCell::new(state),
            decoder,
            timer: RefCell::new(None),
        };

        // Pause at frame 0 to create the first annotation session
        editor.state.borrow_mut().pause_at(0);

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

        // Collect visible annotations for this frame
        let visible: Vec<Annotation> = state
            .annotations_at_frame(frame_idx)
            .into_iter()
            .cloned()
            .collect();

        self.view.display_frame(ns_image, visible);
    }

    /// Toggle play/pause.
    pub fn toggle_playback(&self, timer_target: &AnyObject, mtm: MainThreadMarker) {
        let is_playing = self.state.borrow().is_playing;
        if is_playing {
            self.pause(mtm);
        } else {
            self.play(timer_target);
        }
    }

    /// Start playback: create a timer that advances frames.
    fn play(&self, timer_target: &AnyObject) {
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

        eprintln!("Editor: playing");
    }

    /// Pause playback: stop timer and create new annotation session.
    fn pause(&self, mtm: MainThreadMarker) {
        // Stop timer
        if let Some(timer) = self.timer.borrow_mut().take() {
            timer.invalidate();
        }

        let current_frame = self.state.borrow().current_frame;
        self.state.borrow_mut().pause_at(current_frame);

        // Refresh display with annotations visible at current frame
        self.display_current_frame(mtm);

        eprintln!("Editor: paused at frame {}", current_frame);
    }

    /// Advance one frame (called by timer).
    pub fn advance_frame(&self, mtm: MainThreadMarker) {
        let mut state = self.state.borrow_mut();
        if !state.is_playing {
            return;
        }

        state.current_frame += 1;
        if state.current_frame >= state.total_frames {
            // Reached end: pause at last frame
            state.current_frame = state.total_frames - 1;
            drop(state);
            self.pause(mtm);
            return;
        }
        drop(state);

        self.display_current_frame(mtm);
    }

    /// Add an annotation to the active session.
    pub fn add_annotation(&self, annotation: Annotation) {
        self.state.borrow_mut().add_annotation(annotation);
    }

    /// Remove the last annotation from the active session.
    pub fn undo_annotation(&self, mtm: MainThreadMarker) {
        self.state.borrow_mut().undo_annotation();
        self.display_current_frame(mtm);
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
