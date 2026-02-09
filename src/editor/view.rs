use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSEvent, NSFont, NSGraphicsContext, NSImage, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_foundation::{MainThreadMarker, NSRect, NSString};

use crate::annotation::model::{Annotation, update_annotation};
use crate::overlay::view::ActiveTool;

pub struct EditorViewIvars {
    pub current_image: RefCell<Option<Retained<NSImage>>>,
    pub active_tool: Cell<ActiveTool>,
    pub annotation_color: Cell<(CGFloat, CGFloat, CGFloat)>,
    pub current_annotation: RefCell<Option<Annotation>>,
    /// All annotations to draw on the current frame (from active sessions).
    pub annotations_to_draw: RefCell<Vec<Annotation>>,
    /// Completed annotation waiting to be picked up by the editor window.
    pub pending_annotation: RefCell<Option<Annotation>>,
    pub tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    pub text_field: RefCell<Option<Retained<NSTextField>>>,
    pub text_position: Cell<CGPoint>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "EditorView"]
    #[ivars = EditorViewIvars]
    pub struct EditorView;

    impl EditorView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(canBecomeKeyView))]
        fn can_become_key_view(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let Some(context) = NSGraphicsContext::currentContext() else {
                return;
            };
            let cg = context.CGContext();
            let bounds = self.bounds();

            // Draw the current video frame as background
            if let Some(ref image) = *self.ivars().current_image.borrow() {
                image.drawInRect(bounds);
            }

            // Draw all annotations visible at this frame
            for ann in self.ivars().annotations_to_draw.borrow().iter() {
                crate::annotation::renderer::draw_annotation(&cg, ann);
            }

            // Draw the in-progress annotation
            if let Some(ref ann) = *self.ivars().current_annotation.borrow() {
                crate::annotation::renderer::draw_annotation(&cg, ann);
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convert_event_point(event);
            let active_tool = self.ivars().active_tool.get();

            if active_tool == ActiveTool::Select {
                return;
            }

            self.start_annotation(point);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let point = self.convert_event_point(event);

            if let Some(ref mut ann) = *self.ivars().current_annotation.borrow_mut() {
                update_annotation(ann, point);
                self.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            if let Some(ann) = self.ivars().current_annotation.borrow_mut().take() {
                self.finish_annotation(ann);
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let key_code = event.keyCode();

            // Space bar = 49 -> toggle play/pause
            if key_code == 49 {
                self.notify_delegate_play_pause();
                return;
            }

            // Escape = 53 -> cancel
            if key_code == 53 {
                self.notify_delegate_cancel();
                return;
            }

            // Cmd+Z = undo
            let flags = event.modifierFlags();
            if key_code == 6
                && flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
            {
                self.notify_delegate_undo();
            }
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            if let Some(old_area) = self.ivars().tracking_area.borrow_mut().take() {
                self.removeTrackingArea(&old_area);
            }

            let options = NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::CursorUpdate;
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    MainThreadMarker::from(self).alloc(),
                    self.bounds(),
                    options,
                    Some(self),
                    None,
                )
            };
            self.addTrackingArea(&area);
            *self.ivars().tracking_area.borrow_mut() = Some(area);
        }
    }
);

impl EditorView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(EditorViewIvars {
            current_image: RefCell::new(None),
            active_tool: Cell::new(ActiveTool::Arrow),
            annotation_color: Cell::new((1.0, 0.0, 0.0)),
            current_annotation: RefCell::new(None),
            annotations_to_draw: RefCell::new(Vec::new()),
            pending_annotation: RefCell::new(None),
            tracking_area: RefCell::new(None),
            text_field: RefCell::new(None),
            text_position: Cell::new(CGPoint::ZERO),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view
    }

    /// Set the current frame image and the annotations visible at this frame.
    pub fn display_frame(&self, image: Retained<NSImage>, annotations: Vec<Annotation>) {
        *self.ivars().current_image.borrow_mut() = Some(image);
        *self.ivars().annotations_to_draw.borrow_mut() = annotations;
        self.setNeedsDisplay(true);
    }

    /// Take the pending annotation (if any). Called by the app delegate after
    /// receiving editorAnnotationAdded: notification.
    pub fn take_pending_annotation(&self) -> Option<Annotation> {
        self.ivars().pending_annotation.borrow_mut().take()
    }

    fn convert_event_point(&self, event: &NSEvent) -> CGPoint {
        let window_point = event.locationInWindow();
        self.convertPoint_fromView(window_point, None)
    }

    fn start_annotation(&self, point: CGPoint) {
        let color = self.ivars().annotation_color.get();
        let tool = self.ivars().active_tool.get();

        let ann = match tool {
            ActiveTool::Arrow => Annotation::Arrow {
                start: point,
                end: point,
                color,
                width: 2.0,
            },
            ActiveTool::Rectangle => Annotation::Rect {
                origin: point,
                size: CGSize::ZERO,
                color,
                width: 2.0,
            },
            ActiveTool::Ellipse => Annotation::Ellipse {
                origin: point,
                size: CGSize::ZERO,
                color,
                width: 2.0,
            },
            ActiveTool::Pencil => Annotation::Pencil {
                points: vec![point],
                color,
                width: 2.0,
            },
            ActiveTool::Text => {
                self.show_text_field(point);
                return;
            }
            ActiveTool::Select => return,
        };

        *self.ivars().current_annotation.borrow_mut() = Some(ann);
    }

    fn finish_annotation(&self, ann: Annotation) {
        // Store in pending slot for the app delegate to pick up
        *self.ivars().pending_annotation.borrow_mut() = Some(ann.clone());
        // Also add to annotations_to_draw for immediate visual feedback
        self.ivars().annotations_to_draw.borrow_mut().push(ann);
        self.setNeedsDisplay(true);
        // Notify app delegate
        self.notify_delegate_annotation();
    }

    fn show_text_field(&self, point: CGPoint) {
        self.commit_text_field();

        let mtm = MainThreadMarker::from(self);
        let frame = NSRect::new(point, CGSize::new(200.0, 24.0));
        let field = NSTextField::new(mtm);
        field.setFrame(frame);
        field.setFont(Some(&NSFont::systemFontOfSize(16.0)));
        field.setDrawsBackground(true);
        field.setBordered(true);
        field.setStringValue(&NSString::from_str(""));

        self.addSubview(&field);
        if let Some(w) = self.window() {
            w.makeFirstResponder(Some(&*field));
        }

        self.ivars().text_position.set(point);
        *self.ivars().text_field.borrow_mut() = Some(field);
    }

    pub fn commit_text_field(&self) {
        let field = self.ivars().text_field.borrow_mut().take();
        if let Some(field) = field {
            let text = field.stringValue().to_string();
            if !text.is_empty() {
                let color = self.ivars().annotation_color.get();
                let position = self.ivars().text_position.get();
                let ann = Annotation::Text {
                    position,
                    text,
                    color,
                    font_size: 16.0,
                };
                self.finish_annotation(ann);
            }
            field.removeFromSuperview();
            self.setNeedsDisplay(true);
            if let Some(window) = self.window() {
                let _ = window.makeFirstResponder(Some(self));
            }
        }
    }

    fn notify_delegate_play_pause(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorPlayPause: self] };
        }
    }

    fn notify_delegate_cancel(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, actionCancel: self] };
        }
    }

    fn notify_delegate_undo(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, actionUndo: self] };
        }
    }

    fn notify_delegate_annotation(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorAnnotationAdded: self] };
        }
    }
}
