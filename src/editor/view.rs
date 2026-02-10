use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSEvent, NSFont, NSGraphicsContext, NSImage, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView,
};
use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGContext, CGImage};
use objc2_foundation::{MainThreadMarker, NSRect, NSString};

use crate::annotation::model::{Annotation, HandleKind, update_annotation};
use crate::overlay::view::{ActiveTool, SelectDragMode, stroke_for_key, tool_for_key};

/// Tracks the current drag operation for the Crop tool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CropDragMode {
    None,
    Drawing,
    Moving,
    Resizing(HandleKind),
}

pub struct EditorViewIvars {
    pub current_image: RefCell<Option<Retained<NSImage>>>,
    pub active_tool: Cell<ActiveTool>,
    pub annotation_color: Cell<(CGFloat, CGFloat, CGFloat)>,
    pub current_annotation: RefCell<Option<Annotation>>,
    /// Annotations to draw on the current frame, with their indices in EditorState.
    pub annotations_to_draw: RefCell<Vec<(usize, Annotation)>>,
    /// Completed annotation waiting to be picked up by the editor window.
    pub pending_annotation: RefCell<Option<Annotation>>,
    pub tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    pub text_field: RefCell<Option<Retained<NSTextField>>>,
    pub text_position: Cell<CGPoint>,
    /// Click point stored when Select tool is used, for the delegate to read.
    pub selection_click_point: Cell<Option<CGPoint>>,
    /// Index of the currently selected/active annotation (for visual highlight).
    pub active_annotation_index: Cell<Option<usize>>,
    /// What the Select tool is currently dragging.
    pub select_drag_mode: Cell<SelectDragMode>,
    /// Mouse position at the start of a select drag.
    pub select_drag_start: Cell<CGPoint>,
    /// Current stroke width for annotations.
    pub annotation_width: Cell<CGFloat>,
    /// Current font size for text annotations.
    pub annotation_font_size: Cell<CGFloat>,
    /// Next step number for the Step tool.
    pub next_step_number: Cell<u32>,
    /// Current frame as CGImage (for blur annotation rendering).
    pub current_cgimage: RefCell<Option<CFRetained<CGImage>>>,
    /// Crop rectangle (in view coordinates). None = no crop.
    pub crop_rect: Cell<Option<CGRect>>,
    /// Current crop drag operation.
    pub crop_drag_mode: Cell<CropDragMode>,
    /// Mouse position at the start of a crop drag.
    pub crop_drag_start: Cell<CGPoint>,
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
            let active_idx = self.ivars().active_annotation_index.get();
            let cgimage_ref = self.ivars().current_cgimage.borrow();
            let screenshot = cgimage_ref.as_ref().map(|img| &**img);
            for (idx, ann) in self.ivars().annotations_to_draw.borrow().iter() {
                crate::annotation::renderer::draw_annotation(&cg, ann, screenshot);

                // Draw highlight and handles around the active/selected annotation
                if active_idx == Some(*idx) {
                    draw_selection_highlight(&cg, ann);
                    draw_annotation_handles(&cg, ann);
                }
            }

            // Draw the in-progress annotation
            if let Some(ref ann) = *self.ivars().current_annotation.borrow() {
                crate::annotation::renderer::draw_annotation(&cg, ann, screenshot);
            }
            drop(cgimage_ref);

            // Draw crop overlay if present
            if let Some(crop) = self.ivars().crop_rect.get() {
                let norm_crop = crate::overlay::view::normalize_rect(crop);
                draw_crop_overlay(&cg, bounds, norm_crop, self.ivars().active_tool.get() == ActiveTool::Crop);
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            // Commit any open text field (removes it if empty)
            self.commit_text_field();

            let point = self.convert_event_point(event);
            let active_tool = self.ivars().active_tool.get();

            if active_tool == ActiveTool::Crop {
                // Crop tool: draw a new crop rect or interact with existing one
                if let Some(crop) = self.ivars().crop_rect.get() {
                    let norm = crate::overlay::view::normalize_rect(crop);
                    // Hit-test crop handles first
                    if let Some(handle) = hit_test_crop_handle(norm, point) {
                        self.ivars().crop_drag_mode.set(CropDragMode::Resizing(handle));
                        self.ivars().crop_drag_start.set(point);
                        return;
                    }
                    // Hit-test inside crop rect → move
                    if point.x >= norm.origin.x
                        && point.x <= norm.origin.x + norm.size.width
                        && point.y >= norm.origin.y
                        && point.y <= norm.origin.y + norm.size.height
                    {
                        self.ivars().crop_drag_mode.set(CropDragMode::Moving);
                        self.ivars().crop_drag_start.set(point);
                        return;
                    }
                }
                // Start drawing a new crop rect
                self.ivars().crop_rect.set(Some(CGRect::new(point, CGSize::ZERO)));
                self.ivars().crop_drag_mode.set(CropDragMode::Drawing);
                self.ivars().crop_drag_start.set(point);
                return;
            }

            if active_tool == ActiveTool::Select {
                // 1. Check if clicking on a handle of the currently selected annotation
                if let Some(idx) = self.ivars().active_annotation_index.get() {
                    let annotations = self.ivars().annotations_to_draw.borrow();
                    if let Some((_, ann)) = annotations.iter().find(|(i, _)| *i == idx) {
                        if let Some(handle) = ann.hit_test_handle(point) {
                            drop(annotations);
                            self.ivars().select_drag_mode.set(SelectDragMode::ResizingHandle(handle));
                            self.ivars().select_drag_start.set(point);
                            return;
                        }
                    }
                    drop(annotations);
                }

                // 2. Check if clicking on any annotation body (for move or select)
                let annotations = self.ivars().annotations_to_draw.borrow();
                let mut hit_idx = None;
                for (idx, ann) in annotations.iter().rev() {
                    if ann.hit_test(point) {
                        hit_idx = Some(*idx);
                        break;
                    }
                }
                drop(annotations);

                if let Some(idx) = hit_idx {
                    // Double-click on Text annotation: re-edit it
                    if event.clickCount() >= 2 {
                        let annotations = self.ivars().annotations_to_draw.borrow();
                        if let Some((_, ann)) = annotations.iter().find(|(i, _)| *i == idx) {
                            if let Annotation::Text { position, text, color, font_size } = ann {
                                let pos = *position;
                                let txt = text.clone();
                                let clr = *color;
                                let fs = *font_size;
                                drop(annotations);
                                // Select and delete the annotation via delegate
                                self.ivars().selection_click_point.set(Some(point));
                                self.notify_delegate_selection_click();
                                self.notify_delegate_delete_annotation();
                                // Show text field pre-filled
                                self.ivars().annotation_color.set(clr);
                                self.ivars().annotation_font_size.set(fs);
                                self.show_text_field_with_text(pos, &txt, fs);
                                return;
                            }
                        }
                        drop(annotations);
                    }
                    // Start moving this annotation
                    self.ivars().select_drag_mode.set(SelectDragMode::MovingAnnotation);
                    self.ivars().select_drag_start.set(point);
                    // Notify delegate to select this annotation
                    self.ivars().selection_click_point.set(Some(point));
                    self.notify_delegate_selection_click();
                } else {
                    // Clicked empty space — deselect
                    self.ivars().select_drag_mode.set(SelectDragMode::None);
                    self.ivars().selection_click_point.set(Some(point));
                    self.notify_delegate_selection_click();
                }
                return;
            }

            self.start_annotation(point);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let point = self.convert_event_point(event);

            // Handle Crop tool drag
            let crop_mode = self.ivars().crop_drag_mode.get();
            match crop_mode {
                CropDragMode::Drawing => {
                    let start = self.ivars().crop_drag_start.get();
                    let new_rect = CGRect::new(
                        start,
                        CGSize::new(point.x - start.x, point.y - start.y),
                    );
                    self.ivars().crop_rect.set(Some(new_rect));
                    self.setNeedsDisplay(true);
                    return;
                }
                CropDragMode::Moving => {
                    let drag_start = self.ivars().crop_drag_start.get();
                    let dx = point.x - drag_start.x;
                    let dy = point.y - drag_start.y;
                    self.ivars().crop_drag_start.set(point);
                    if let Some(mut crop) = self.ivars().crop_rect.get() {
                        crop.origin.x += dx;
                        crop.origin.y += dy;
                        self.ivars().crop_rect.set(Some(crop));
                    }
                    self.setNeedsDisplay(true);
                    return;
                }
                CropDragMode::Resizing(handle) => {
                    if let Some(crop) = self.ivars().crop_rect.get() {
                        let new_crop = apply_crop_resize(crop, handle, point);
                        self.ivars().crop_rect.set(Some(new_crop));
                    }
                    self.setNeedsDisplay(true);
                    return;
                }
                CropDragMode::None => {}
            }

            // Handle Select tool drag (move or resize annotation)
            let drag_mode = self.ivars().select_drag_mode.get();
            match drag_mode {
                SelectDragMode::MovingAnnotation => {
                    let drag_start = self.ivars().select_drag_start.get();
                    let dx = point.x - drag_start.x;
                    let dy = point.y - drag_start.y;
                    self.ivars().select_drag_start.set(point);
                    self.notify_delegate_move_annotation(dx, dy);
                    self.setNeedsDisplay(true);
                    return;
                }
                SelectDragMode::ResizingHandle(handle) => {
                    self.notify_delegate_resize_annotation(handle, point);
                    self.setNeedsDisplay(true);
                    return;
                }
                SelectDragMode::None => {}
            }

            if let Some(ref mut ann) = *self.ivars().current_annotation.borrow_mut() {
                update_annotation(ann, point);
                self.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.ivars().crop_drag_mode.set(CropDragMode::None);
            self.ivars().select_drag_mode.set(SelectDragMode::None);

            if let Some(ann) = self.ivars().current_annotation.borrow_mut().take() {
                self.finish_annotation(ann);
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let key_code = event.keyCode();
            let flags = event.modifierFlags();
            let has_modifiers = flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
                || flags.contains(objc2_app_kit::NSEventModifierFlags::Control)
                || flags.contains(objc2_app_kit::NSEventModifierFlags::Option);

            // Tool shortcuts (only when no text field active and no modifiers)
            if !has_modifiers && self.ivars().text_field.borrow().is_none() {
                if let Some(tool) = tool_for_key(key_code) {
                    self.commit_text_field();
                    self.ivars().active_tool.set(tool);
                    self.notify_tool_changed();
                    return;
                }
                // Stroke width shortcuts: 1=thin, 2=medium, 3=thick
                if let Some((sel_name, _)) = stroke_for_key(key_code) {
                    self.notify_stroke_changed(sel_name);
                    return;
                }
            }

            // Enter = 36 -> apply crop if crop tool is active with a crop rect
            if key_code == 36 {
                if self.ivars().active_tool.get() == ActiveTool::Crop
                    && self.ivars().crop_rect.get().is_some()
                {
                    self.notify_delegate_apply_crop();
                    return;
                }
            }

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

            // Delete (backspace=51, forward delete=117) -> delete selected annotation
            if key_code == 51 || key_code == 117 {
                if self.ivars().active_annotation_index.get().is_some() {
                    self.notify_delegate_delete_annotation();
                    return;
                }
            }

            // Cmd+Shift+Z = redo
            if key_code == 6
                && flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
                && flags.contains(objc2_app_kit::NSEventModifierFlags::Shift)
            {
                self.notify_delegate_redo();
                return;
            }

            // Cmd+Z = undo (crop undo takes priority when crop is set)
            if key_code == 6
                && flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
            {
                if self.ivars().crop_rect.get().is_some() {
                    self.ivars().crop_rect.set(None);
                    // Switch back to Select tool and notify delegate
                    self.ivars().active_tool.set(ActiveTool::Select);
                    self.notify_tool_changed();
                    self.setNeedsDisplay(true);
                    return;
                }
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
            selection_click_point: Cell::new(None),
            active_annotation_index: Cell::new(None),
            select_drag_mode: Cell::new(SelectDragMode::None),
            select_drag_start: Cell::new(CGPoint::ZERO),
            annotation_width: Cell::new(3.0),
            annotation_font_size: Cell::new(18.0),
            next_step_number: Cell::new(1),
            current_cgimage: RefCell::new(None),
            crop_rect: Cell::new(None),
            crop_drag_mode: Cell::new(CropDragMode::None),
            crop_drag_start: Cell::new(CGPoint::ZERO),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view
    }

    /// Set the current frame image and the indexed annotations visible at this frame.
    pub fn display_frame(&self, image: Retained<NSImage>, cgimage: &CGImage, annotations: Vec<(usize, Annotation)>) {
        *self.ivars().current_image.borrow_mut() = Some(image);
        *self.ivars().current_cgimage.borrow_mut() = Some(unsafe { CFRetained::retain(cgimage.into()) });
        *self.ivars().annotations_to_draw.borrow_mut() = annotations;
        self.setNeedsDisplay(true);
    }

    /// Take the pending annotation (if any). Called by the app delegate after
    /// receiving editorAnnotationAdded: notification.
    pub fn take_pending_annotation(&self) -> Option<Annotation> {
        self.ivars().pending_annotation.borrow_mut().take()
    }

    /// Take the selection click point. Called by the app delegate after
    /// receiving editorSelectionClick: notification.
    pub fn take_selection_click_point(&self) -> Option<CGPoint> {
        self.ivars().selection_click_point.take()
    }

    /// Set the active annotation index for visual highlight.
    pub fn set_active_annotation_index(&self, idx: Option<usize>) {
        self.ivars().active_annotation_index.set(idx);
        self.setNeedsDisplay(true);
    }

    fn convert_event_point(&self, event: &NSEvent) -> CGPoint {
        let window_point = event.locationInWindow();
        self.convertPoint_fromView(window_point, None)
    }

    fn start_annotation(&self, point: CGPoint) {
        let color = self.ivars().annotation_color.get();
        let tool = self.ivars().active_tool.get();
        let width = self.ivars().annotation_width.get();

        let ann = match tool {
            ActiveTool::Arrow => Annotation::Arrow {
                start: point,
                end: point,
                color,
                width,
            },
            ActiveTool::Rectangle => Annotation::Rect {
                origin: point,
                size: CGSize::ZERO,
                color,
                width,
            },
            ActiveTool::Ellipse => Annotation::Ellipse {
                origin: point,
                size: CGSize::ZERO,
                color,
                width,
            },
            ActiveTool::Pencil => Annotation::Pencil {
                points: vec![point],
                color,
                width,
            },
            ActiveTool::Highlight => Annotation::Highlight {
                origin: point,
                size: CGSize::ZERO,
                color,
                opacity: 0.35,
            },
            ActiveTool::Blur => Annotation::Blur {
                origin: point,
                size: CGSize::ZERO,
                block_size: 10,
            },
            ActiveTool::Step => {
                let number = self.ivars().next_step_number.get();
                self.ivars().next_step_number.set(number + 1);
                let ann = Annotation::Step {
                    center: point,
                    number,
                    color,
                    radius: 14.0,
                };
                self.finish_annotation(ann);
                return;
            }
            ActiveTool::Text => {
                self.show_text_field(point);
                return;
            }
            ActiveTool::Select | ActiveTool::Crop => return,
        };

        *self.ivars().current_annotation.borrow_mut() = Some(ann);
    }

    fn finish_annotation(&self, ann: Annotation) {
        // Store in pending slot for the app delegate to pick up
        *self.ivars().pending_annotation.borrow_mut() = Some(ann.clone());
        // Also add to annotations_to_draw for immediate visual feedback (index 0 is placeholder)
        self.ivars().annotations_to_draw.borrow_mut().push((usize::MAX, ann));
        self.setNeedsDisplay(true);
        // Notify app delegate
        self.notify_delegate_annotation();
    }

    fn show_text_field(&self, point: CGPoint) {
        self.show_text_field_with_text(point, "", self.ivars().annotation_font_size.get());
    }

    fn show_text_field_with_text(&self, point: CGPoint, initial_text: &str, font_size: CGFloat) {
        self.commit_text_field();

        let mtm = MainThreadMarker::from(self);
        let field_height = font_size * 1.5;
        let frame = NSRect::new(point, CGSize::new(200.0, field_height));
        let field = NSTextField::new(mtm);
        field.setFrame(frame);
        field.setFont(Some(&NSFont::systemFontOfSize(font_size)));
        field.setDrawsBackground(true);
        field.setBordered(true);
        field.setStringValue(&NSString::from_str(initial_text));

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
                let font_size = self.ivars().annotation_font_size.get();
                let ann = Annotation::Text {
                    position,
                    text,
                    color,
                    font_size,
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

    fn notify_delegate_redo(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, actionRedo: self] };
        }
    }

    fn notify_delegate_annotation(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorAnnotationAdded: self] };
        }
    }

    fn notify_delegate_selection_click(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorSelectionClick: self] };
        }
    }

    fn notify_delegate_delete_annotation(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorDeleteAnnotation: self] };
        }
    }

    fn notify_delegate_move_annotation(&self, dx: CGFloat, dy: CGFloat) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorMoveAnnotation: dx, y: dy] };
        }
    }

    fn notify_delegate_resize_annotation(&self, handle: HandleKind, point: CGPoint) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            // Pack handle as u32 and point components as CGFloat
            let handle_val = handle as u32;
            let _: () = unsafe { msg_send![&*delegate, editorResizeAnnotation: handle_val, x: point.x, y: point.y] };
        }
    }

    fn notify_delegate_apply_crop(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorApplyCrop: self] };
        }
    }

    /// Notify the app delegate that the tool changed (from keyboard shortcut).
    fn notify_tool_changed(&self) {
        let tool = self.ivars().active_tool.get();
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let d = &*delegate;
            match tool {
                ActiveTool::Select => { let _: () = unsafe { msg_send![d, toolSelect: self] }; }
                ActiveTool::Arrow => { let _: () = unsafe { msg_send![d, toolArrow: self] }; }
                ActiveTool::Rectangle => { let _: () = unsafe { msg_send![d, toolRect: self] }; }
                ActiveTool::Ellipse => { let _: () = unsafe { msg_send![d, toolEllipse: self] }; }
                ActiveTool::Pencil => { let _: () = unsafe { msg_send![d, toolPencil: self] }; }
                ActiveTool::Text => { let _: () = unsafe { msg_send![d, toolText: self] }; }
                ActiveTool::Highlight => { let _: () = unsafe { msg_send![d, toolHighlight: self] }; }
                ActiveTool::Step => { let _: () = unsafe { msg_send![d, toolStep: self] }; }
                ActiveTool::Blur => { let _: () = unsafe { msg_send![d, toolBlur: self] }; }
                ActiveTool::Crop => { let _: () = unsafe { msg_send![d, toolCrop: self] }; }
            }
        }
    }

    /// Notify the app delegate that the stroke width changed (from keyboard shortcut).
    fn notify_stroke_changed(&self, sel_name: &str) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let d = &*delegate;
            match sel_name {
                "strokeThin:" => { let _: () = unsafe { msg_send![d, strokeThin: self] }; }
                "strokeMedium:" => { let _: () = unsafe { msg_send![d, strokeMedium: self] }; }
                "strokeThick:" => { let _: () = unsafe { msg_send![d, strokeThick: self] }; }
                _ => {}
            }
        }
    }
}

/// Draw resize handles on a selected annotation.
fn draw_annotation_handles(ctx: &CGContext, ann: &Annotation) {
    let handles = ann.resize_handles();
    if handles.is_empty() {
        return;
    }

    let handle_size: CGFloat = 6.0;
    let hs = handle_size / 2.0;

    CGContext::save_g_state(Some(ctx));
    CGContext::set_rgb_fill_color(Some(ctx), 1.0, 1.0, 1.0, 1.0);
    CGContext::set_rgb_stroke_color(Some(ctx), 0.2, 0.5, 1.0, 1.0);
    CGContext::set_line_width(Some(ctx), 1.0);
    unsafe { CGContext::set_line_dash(Some(ctx), 0.0, std::ptr::null(), 0) };

    for (_kind, point) in handles {
        let handle_rect = objc2_core_foundation::CGRect::new(
            CGPoint::new(point.x - hs, point.y - hs),
            CGSize::new(handle_size, handle_size),
        );
        CGContext::fill_rect(Some(ctx), handle_rect);
        CGContext::stroke_rect(Some(ctx), handle_rect);
    }

    CGContext::restore_g_state(Some(ctx));
}

/// Draw a dashed highlight border around a selected annotation.
fn draw_selection_highlight(ctx: &CGContext, ann: &Annotation) {
    let rect = ann.bounding_rect();
    // Inflate slightly for visual clarity
    let highlight = objc2_core_foundation::CGRect::new(
        CGPoint::new(rect.origin.x - 2.0, rect.origin.y - 2.0),
        objc2_core_foundation::CGSize::new(rect.size.width + 4.0, rect.size.height + 4.0),
    );

    CGContext::save_g_state(Some(ctx));
    CGContext::set_rgb_stroke_color(Some(ctx), 0.2, 0.5, 1.0, 0.8);
    CGContext::set_line_width(Some(ctx), 1.5);
    let dash_lengths: [CGFloat; 2] = [4.0, 3.0];
    unsafe {
        CGContext::set_line_dash(Some(ctx), 0.0, dash_lengths.as_ptr(), dash_lengths.len());
    }
    CGContext::stroke_rect(Some(ctx), highlight);
    CGContext::restore_g_state(Some(ctx));
}

/// Draw the crop overlay: dim everything outside the crop rect, draw border and handles.
fn draw_crop_overlay(
    ctx: &CGContext,
    bounds: objc2_foundation::NSRect,
    crop: CGRect,
    is_active: bool,
) {
    CGContext::save_g_state(Some(ctx));

    // Draw semi-transparent dark overlay on the 4 strips outside the crop rect.
    // We avoid CGContext::clear_rect because it erases the image underneath.
    let bx = bounds.origin.x;
    let by = bounds.origin.y;
    let bw = bounds.size.width;
    let bh = bounds.size.height;
    let cx = crop.origin.x;
    let cy = crop.origin.y;
    let cw = crop.size.width;
    let ch = crop.size.height;

    CGContext::set_rgb_fill_color(Some(ctx), 0.0, 0.0, 0.0, 0.5);
    // Top strip
    if cy > by {
        CGContext::fill_rect(Some(ctx), CGRect::new(CGPoint::new(bx, by), CGSize::new(bw, cy - by)));
    }
    // Bottom strip
    let crop_bottom = cy + ch;
    let bounds_bottom = by + bh;
    if crop_bottom < bounds_bottom {
        CGContext::fill_rect(Some(ctx), CGRect::new(CGPoint::new(bx, crop_bottom), CGSize::new(bw, bounds_bottom - crop_bottom)));
    }
    // Left strip (between top and bottom strips)
    if cx > bx {
        CGContext::fill_rect(Some(ctx), CGRect::new(CGPoint::new(bx, cy), CGSize::new(cx - bx, ch)));
    }
    // Right strip (between top and bottom strips)
    let crop_right = cx + cw;
    let bounds_right = bx + bw;
    if crop_right < bounds_right {
        CGContext::fill_rect(Some(ctx), CGRect::new(CGPoint::new(crop_right, cy), CGSize::new(bounds_right - crop_right, ch)));
    }

    // Draw dashed border around the crop rect
    CGContext::set_rgb_stroke_color(Some(ctx), 1.0, 1.0, 1.0, 0.9);
    CGContext::set_line_width(Some(ctx), 1.5);
    let dash_lengths: [CGFloat; 2] = [6.0, 4.0];
    unsafe {
        CGContext::set_line_dash(Some(ctx), 0.0, dash_lengths.as_ptr(), dash_lengths.len());
    }
    CGContext::stroke_rect(Some(ctx), crop);

    // Draw resize handles when crop tool is active
    if is_active {
        let handle_size: CGFloat = 6.0;
        let hs = handle_size / 2.0;
        CGContext::set_rgb_fill_color(Some(ctx), 1.0, 1.0, 1.0, 1.0);
        CGContext::set_rgb_stroke_color(Some(ctx), 0.2, 0.5, 1.0, 1.0);
        CGContext::set_line_width(Some(ctx), 1.0);
        unsafe { CGContext::set_line_dash(Some(ctx), 0.0, std::ptr::null(), 0) };

        for (_kind, pt) in crop_resize_handles(crop) {
            let handle_rect = CGRect::new(
                CGPoint::new(pt.x - hs, pt.y - hs),
                CGSize::new(handle_size, handle_size),
            );
            CGContext::fill_rect(Some(ctx), handle_rect);
            CGContext::stroke_rect(Some(ctx), handle_rect);
        }
    }

    CGContext::restore_g_state(Some(ctx));
}

/// Return the 8 resize handles for a crop rectangle (same positions as annotation handles).
fn crop_resize_handles(r: CGRect) -> Vec<(HandleKind, CGPoint)> {
    let x0 = r.origin.x;
    let y0 = r.origin.y;
    let x1 = r.origin.x + r.size.width;
    let y1 = r.origin.y + r.size.height;
    let mx = (x0 + x1) / 2.0;
    let my = (y0 + y1) / 2.0;
    vec![
        (HandleKind::TopLeft, CGPoint::new(x0, y0)),
        (HandleKind::Top, CGPoint::new(mx, y0)),
        (HandleKind::TopRight, CGPoint::new(x1, y0)),
        (HandleKind::Left, CGPoint::new(x0, my)),
        (HandleKind::Right, CGPoint::new(x1, my)),
        (HandleKind::BottomLeft, CGPoint::new(x0, y1)),
        (HandleKind::Bottom, CGPoint::new(mx, y1)),
        (HandleKind::BottomRight, CGPoint::new(x1, y1)),
    ]
}

/// Hit-test the crop handles. Returns the handle kind if the point is near one.
fn hit_test_crop_handle(crop: CGRect, point: CGPoint) -> Option<HandleKind> {
    let tolerance: CGFloat = 6.0;
    for (kind, pt) in crop_resize_handles(crop) {
        if (point.x - pt.x).abs() <= tolerance && (point.y - pt.y).abs() <= tolerance {
            return Some(kind);
        }
    }
    None
}

/// Apply a resize operation to the crop rect based on which handle is being dragged.
fn apply_crop_resize(crop: CGRect, handle: HandleKind, point: CGPoint) -> CGRect {
    let mut x0 = crop.origin.x;
    let mut y0 = crop.origin.y;
    let mut x1 = crop.origin.x + crop.size.width;
    let mut y1 = crop.origin.y + crop.size.height;

    match handle {
        HandleKind::TopLeft => { x0 = point.x; y0 = point.y; }
        HandleKind::Top => { y0 = point.y; }
        HandleKind::TopRight => { x1 = point.x; y0 = point.y; }
        HandleKind::Left => { x0 = point.x; }
        HandleKind::Right => { x1 = point.x; }
        HandleKind::BottomLeft => { x0 = point.x; y1 = point.y; }
        HandleKind::Bottom => { y1 = point.y; }
        HandleKind::BottomRight => { x1 = point.x; y1 = point.y; }
        _ => {}
    }

    CGRect::new(
        CGPoint::new(x0, y0),
        CGSize::new(x1 - x0, y1 - y0),
    )
}
