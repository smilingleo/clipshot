use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSCursor, NSEvent, NSFont, NSGraphicsContext, NSImage, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSView,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGContext;
use objc2_foundation::{MainThreadMarker, NSRect, NSString};

use crate::annotation::model::{Annotation, HandleKind};

/// Tracks which part of the selection the user is interacting with.
#[derive(Clone, Copy, PartialEq)]
pub enum DragMode {
    None,
    Creating,
    Moving,
    ResizeTopLeft,
    ResizeTopRight,
    ResizeBottomLeft,
    ResizeBottomRight,
    ResizeTop,
    ResizeBottom,
    ResizeLeft,
    ResizeRight,
}

/// Which annotation tool is currently active.
#[derive(Clone, Copy, PartialEq)]
pub enum ActiveTool {
    Select,
    Arrow,
    Rectangle,
    Ellipse,
    Pencil,
    Text,
    Highlight,
    Step,
    Blur,
    Crop,
}

/// Tracks what the Select tool is currently dragging.
#[derive(Clone, Copy, PartialEq)]
pub enum SelectDragMode {
    None,
    MovingAnnotation,
    ResizingHandle(HandleKind),
}

pub struct OverlayViewIvars {
    pub screenshot: RefCell<Option<Retained<NSImage>>>,
    pub scale_factor: Cell<CGFloat>,
    pub selection: Cell<Option<CGRect>>,
    pub drag_start: Cell<CGPoint>,
    pub drag_mode: Cell<DragMode>,
    pub original_selection: Cell<Option<CGRect>>,
    pub active_tool: Cell<ActiveTool>,
    pub annotations: RefCell<Vec<Annotation>>,
    pub current_annotation: RefCell<Option<Annotation>>,
    pub annotation_color: Cell<(CGFloat, CGFloat, CGFloat)>,
    pub tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    /// Active text field for text tool input
    pub text_field: RefCell<Option<Retained<NSTextField>>>,
    /// Position where the text tool was clicked
    pub text_position: Cell<CGPoint>,
    /// Index of the currently selected annotation (for highlight + delete).
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
    /// Redo stack for undone annotations.
    pub redo_stack: RefCell<Vec<Annotation>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "OverlayView"]
    #[ivars = OverlayViewIvars]
    pub struct OverlayView;

    impl OverlayView {
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

            // Draw the captured screenshot as background
            if let Some(ref screenshot) = *self.ivars().screenshot.borrow() {
                screenshot.drawInRect(bounds);
            }

            // Draw semi-transparent dark overlay over everything
            CGContext::set_rgb_fill_color(Some(&cg), 0.0, 0.0, 0.0, 0.5);
            CGContext::fill_rect(Some(&cg), bounds);

            // If there's a selection, clear the overlay within it to show the bright image
            if let Some(sel_rect) = self.ivars().selection.get() {
                let norm = normalize_rect(sel_rect);
                if norm.size.width < 1.0 || norm.size.height < 1.0 {
                    return;
                }

                // Clear the dark overlay in the selection area
                CGContext::save_g_state(Some(&cg));
                CGContext::set_blend_mode(
                    Some(&cg),
                    objc2_core_graphics::CGBlendMode::Clear,
                );
                CGContext::fill_rect(Some(&cg), norm);
                CGContext::restore_g_state(Some(&cg));

                // Redraw the screenshot in the selection area (since Clear removed it)
                CGContext::save_g_state(Some(&cg));
                CGContext::clip_to_rect(Some(&cg), norm);
                if let Some(ref screenshot) = *self.ivars().screenshot.borrow() {
                    screenshot.drawInRect(bounds);
                }
                CGContext::restore_g_state(Some(&cg));

                // Draw annotations within the selection area
                CGContext::save_g_state(Some(&cg));
                CGContext::clip_to_rect(Some(&cg), norm);
                let active_idx = self.ivars().active_annotation_index.get();
                for (i, ann) in self.ivars().annotations.borrow().iter().enumerate() {
                    crate::annotation::renderer::draw_annotation(&cg, ann, None);
                    if active_idx == Some(i) {
                        draw_annotation_highlight(&cg, ann);
                        draw_annotation_handles(&cg, ann);
                    }
                }
                if let Some(ref ann) = *self.ivars().current_annotation.borrow() {
                    crate::annotation::renderer::draw_annotation(&cg, ann, None);
                }
                CGContext::restore_g_state(Some(&cg));

                // Draw selection border (dashed blue line)
                CGContext::save_g_state(Some(&cg));
                CGContext::set_rgb_stroke_color(Some(&cg), 0.2, 0.6, 1.0, 1.0);
                CGContext::set_line_width(Some(&cg), 1.0);
                let dash_lengths: [CGFloat; 2] = [4.0, 4.0];
                unsafe {
                    CGContext::set_line_dash(
                        Some(&cg),
                        0.0,
                        dash_lengths.as_ptr(),
                        dash_lengths.len(),
                    );
                }
                CGContext::stroke_rect(Some(&cg), norm);
                CGContext::restore_g_state(Some(&cg));

                // Draw resize handles
                self.draw_resize_handles(&cg, norm);
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = self.convert_event_point(event);
            self.ivars().drag_start.set(point);
            self.commit_text_field();

            let active_tool = self.ivars().active_tool.get();

            // Select tool with existing selection: handle resize, move, or select/deselect
            if active_tool == ActiveTool::Select {
                if self.ivars().selection.get().is_some() {
                    let annotations = self.ivars().annotations.borrow();

                    // 1. Check if clicking on a handle of the currently selected annotation
                    if let Some(idx) = self.ivars().active_annotation_index.get() {
                        if let Some(ann) = annotations.get(idx) {
                            if let Some(handle) = ann.hit_test_handle(point) {
                                drop(annotations);
                                self.ivars().select_drag_mode.set(SelectDragMode::ResizingHandle(handle));
                                self.ivars().select_drag_start.set(point);
                                return;
                            }
                        }
                    }

                    // 2. Check if clicking on any annotation body
                    let mut hit_idx = None;
                    for (i, ann) in annotations.iter().enumerate().rev() {
                        if ann.hit_test(point) {
                            hit_idx = Some(i);
                            break;
                        }
                    }
                    drop(annotations);

                    if let Some(idx) = hit_idx {
                        // Double-click on Text annotation: re-edit it
                        if event.clickCount() >= 2 {
                            let annotations = self.ivars().annotations.borrow();
                            if let Some(ann) = annotations.get(idx) {
                                if let Annotation::Text { position, text, color, font_size } = ann {
                                    let pos = *position;
                                    let txt = text.clone();
                                    let clr = *color;
                                    let fs = *font_size;
                                    drop(annotations);
                                    // Remove the annotation
                                    self.ivars().annotations.borrow_mut().remove(idx);
                                    self.ivars().active_annotation_index.set(None);
                                    // Show text field pre-filled
                                    self.ivars().annotation_color.set(clr);
                                    self.ivars().annotation_font_size.set(fs);
                                    self.show_text_field_with_text(pos, &txt, fs);
                                    self.setNeedsDisplay(true);
                                    return;
                                }
                            }
                            drop(annotations);
                        }
                        self.ivars().active_annotation_index.set(Some(idx));
                        self.ivars().select_drag_mode.set(SelectDragMode::MovingAnnotation);
                        self.ivars().select_drag_start.set(point);
                    } else {
                        // Clicked empty space — deselect
                        self.ivars().active_annotation_index.set(None);
                        self.ivars().select_drag_mode.set(SelectDragMode::None);
                    }
                    self.setNeedsDisplay(true);
                    return;
                }
            }

            // Annotation tools: draw inside the selection
            if active_tool != ActiveTool::Select {
                if let Some(sel_rect) = self.ivars().selection.get() {
                    let norm = normalize_rect(sel_rect);
                    if rect_contains(norm, point) {
                        self.start_annotation(point);
                        return;
                    }
                }
            }

            // Check if clicking on resize handle or inside existing selection
            if let Some(sel_rect) = self.ivars().selection.get() {
                let norm = normalize_rect(sel_rect);
                if let Some(handle) = self.hit_test_handle(norm, point) {
                    self.ivars().drag_mode.set(handle);
                    self.ivars().original_selection.set(Some(norm));
                    return;
                }
                if rect_contains(norm, point) {
                    self.ivars().drag_mode.set(DragMode::Moving);
                    self.ivars().original_selection.set(Some(norm));
                    return;
                }
            }

            // Start a new selection
            self.ivars().drag_mode.set(DragMode::Creating);
            self.ivars().selection.set(Some(CGRect::new(point, CGSize::ZERO)));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let point = self.convert_event_point(event);
            let start = self.ivars().drag_start.get();

            let active_tool = self.ivars().active_tool.get();

            // Handle Select tool drag (move or resize annotation)
            if active_tool == ActiveTool::Select {
                let drag_mode = self.ivars().select_drag_mode.get();
                match drag_mode {
                    SelectDragMode::MovingAnnotation => {
                        if let Some(idx) = self.ivars().active_annotation_index.get() {
                            let drag_start = self.ivars().select_drag_start.get();
                            let dx = point.x - drag_start.x;
                            let dy = point.y - drag_start.y;
                            let mut annotations = self.ivars().annotations.borrow_mut();
                            if let Some(ann) = annotations.get_mut(idx) {
                                ann.translate(dx, dy);
                            }
                            drop(annotations);
                            self.ivars().select_drag_start.set(point);
                            self.setNeedsDisplay(true);
                        }
                        return;
                    }
                    SelectDragMode::ResizingHandle(handle) => {
                        if let Some(idx) = self.ivars().active_annotation_index.get() {
                            let mut annotations = self.ivars().annotations.borrow_mut();
                            if let Some(ann) = annotations.get_mut(idx) {
                                ann.apply_resize(handle, point);
                            }
                            drop(annotations);
                            self.setNeedsDisplay(true);
                        }
                        return;
                    }
                    SelectDragMode::None => {}
                }
            }

            if active_tool != ActiveTool::Select {
                if let Some(ref mut ann) = *self.ivars().current_annotation.borrow_mut() {
                    crate::annotation::model::update_annotation(ann, point);
                    self.setNeedsDisplay(true);
                    return;
                }
            }

            match self.ivars().drag_mode.get() {
                DragMode::Creating => {
                    let rect = CGRect::new(
                        CGPoint::new(start.x.min(point.x), start.y.min(point.y)),
                        CGSize::new((point.x - start.x).abs(), (point.y - start.y).abs()),
                    );
                    self.ivars().selection.set(Some(rect));
                }
                DragMode::Moving => {
                    if let Some(orig) = self.ivars().original_selection.get() {
                        let dx = point.x - start.x;
                        let dy = point.y - start.y;
                        let moved = CGRect::new(
                            CGPoint::new(orig.origin.x + dx, orig.origin.y + dy),
                            orig.size,
                        );
                        self.ivars().selection.set(Some(moved));
                    }
                }
                mode => {
                    if let Some(orig) = self.ivars().original_selection.get() {
                        let resized = resize_rect(orig, mode, start, point);
                        self.ivars().selection.set(Some(resized));
                    }
                }
            }

            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            let active_tool = self.ivars().active_tool.get();

            // Reset select drag mode
            if active_tool == ActiveTool::Select {
                self.ivars().select_drag_mode.set(SelectDragMode::None);
            }

            if active_tool != ActiveTool::Select {
                if let Some(ann) = self.ivars().current_annotation.borrow_mut().take() {
                    self.ivars().redo_stack.borrow_mut().clear();
                    self.ivars().annotations.borrow_mut().push(ann);
                    self.setNeedsDisplay(true);
                }
            }

            // Normalize the selection rectangle
            if let Some(sel_rect) = self.ivars().selection.get() {
                self.ivars().selection.set(Some(normalize_rect(sel_rect)));
            }

            self.ivars().drag_mode.set(DragMode::None);
            self.ivars().original_selection.set(None);

            self.notify_selection_changed();
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

            // Escape = 53
            if key_code == 53 {
                self.dismiss();
                return;
            }

            // Delete (backspace=51, forward delete=117) -> delete selected annotation
            if key_code == 51 || key_code == 117 {
                if let Some(idx) = self.ivars().active_annotation_index.get() {
                    let mut annotations = self.ivars().annotations.borrow_mut();
                    if idx < annotations.len() {
                        annotations.remove(idx);
                    }
                    drop(annotations);
                    self.ivars().active_annotation_index.set(None);
                    self.setNeedsDisplay(true);
                    return;
                }
            }

            // Cmd+Shift+Z = redo (keyCode 6 = Z with Shift)
            if key_code == 6
                && flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
                && flags.contains(objc2_app_kit::NSEventModifierFlags::Shift)
            {
                if let Some(ann) = self.ivars().redo_stack.borrow_mut().pop() {
                    self.ivars().annotations.borrow_mut().push(ann);
                    self.setNeedsDisplay(true);
                }
                return;
            }

            // Cmd+Z = undo (keyCode 6 = Z)
            if key_code == 6
                && flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
            {
                if let Some(ann) = self.ivars().annotations.borrow_mut().pop() {
                    self.ivars().redo_stack.borrow_mut().push(ann);
                }
                self.setNeedsDisplay(true);
            }
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            let bounds = self.bounds();
            self.addCursorRect_cursor(bounds, &NSCursor::crosshairCursor());
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

impl OverlayView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(OverlayViewIvars {
            screenshot: RefCell::new(None),
            scale_factor: Cell::new(1.0),
            selection: Cell::new(None),
            drag_start: Cell::new(CGPoint::ZERO),
            drag_mode: Cell::new(DragMode::None),
            original_selection: Cell::new(None),
            active_tool: Cell::new(ActiveTool::Select),
            annotations: RefCell::new(Vec::new()),
            current_annotation: RefCell::new(None),
            annotation_color: Cell::new((1.0, 0.0, 0.0)),
            tracking_area: RefCell::new(None),
            text_field: RefCell::new(None),
            text_position: Cell::new(CGPoint::ZERO),
            active_annotation_index: Cell::new(None),
            select_drag_mode: Cell::new(SelectDragMode::None),
            select_drag_start: Cell::new(CGPoint::ZERO),
            annotation_width: Cell::new(3.0),
            annotation_font_size: Cell::new(18.0),
            next_step_number: Cell::new(1),
            redo_stack: RefCell::new(Vec::new()),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view
    }

    pub fn set_screenshot(&self, image: Retained<NSImage>, scale_factor: CGFloat) {
        *self.ivars().screenshot.borrow_mut() = Some(image);
        self.ivars().scale_factor.set(scale_factor);
    }

    pub fn reset(&self) {
        self.ivars().selection.set(None);
        self.ivars().drag_mode.set(DragMode::None);
        self.ivars().original_selection.set(None);
        self.ivars().active_tool.set(ActiveTool::Select);
        self.ivars().annotations.borrow_mut().clear();
        *self.ivars().current_annotation.borrow_mut() = None;
        self.ivars().active_annotation_index.set(None);
        self.ivars().select_drag_mode.set(SelectDragMode::None);
        self.commit_text_field();
        self.setNeedsDisplay(true);
    }

    fn convert_event_point(&self, event: &NSEvent) -> CGPoint {
        let window_point = event.locationInWindow();
        self.convertPoint_fromView(window_point, None)
    }

    fn hit_test_handle(&self, rect: CGRect, point: CGPoint) -> Option<DragMode> {
        let hs: CGFloat = 4.0;

        let corners = [
            (rect.origin.x, rect.origin.y, DragMode::ResizeTopLeft),
            (rect.origin.x + rect.size.width, rect.origin.y, DragMode::ResizeTopRight),
            (rect.origin.x, rect.origin.y + rect.size.height, DragMode::ResizeBottomLeft),
            (
                rect.origin.x + rect.size.width,
                rect.origin.y + rect.size.height,
                DragMode::ResizeBottomRight,
            ),
        ];
        for (cx, cy, mode) in corners {
            if (point.x - cx).abs() <= hs && (point.y - cy).abs() <= hs {
                return Some(mode);
            }
        }

        let edges = [
            (rect.origin.x + rect.size.width / 2.0, rect.origin.y, DragMode::ResizeTop),
            (
                rect.origin.x + rect.size.width / 2.0,
                rect.origin.y + rect.size.height,
                DragMode::ResizeBottom,
            ),
            (rect.origin.x, rect.origin.y + rect.size.height / 2.0, DragMode::ResizeLeft),
            (
                rect.origin.x + rect.size.width,
                rect.origin.y + rect.size.height / 2.0,
                DragMode::ResizeRight,
            ),
        ];
        for (cx, cy, mode) in edges {
            if (point.x - cx).abs() <= hs && (point.y - cy).abs() <= hs {
                return Some(mode);
            }
        }

        None
    }

    fn draw_resize_handles(&self, cg: &CGContext, rect: CGRect) {
        let handle_size: CGFloat = 6.0;
        let hs = handle_size / 2.0;

        let points = [
            (rect.origin.x, rect.origin.y),
            (rect.origin.x + rect.size.width, rect.origin.y),
            (rect.origin.x, rect.origin.y + rect.size.height),
            (rect.origin.x + rect.size.width, rect.origin.y + rect.size.height),
            (rect.origin.x + rect.size.width / 2.0, rect.origin.y),
            (rect.origin.x + rect.size.width / 2.0, rect.origin.y + rect.size.height),
            (rect.origin.x, rect.origin.y + rect.size.height / 2.0),
            (rect.origin.x + rect.size.width, rect.origin.y + rect.size.height / 2.0),
        ];

        CGContext::set_rgb_fill_color(Some(cg), 1.0, 1.0, 1.0, 1.0);
        CGContext::set_rgb_stroke_color(Some(cg), 0.2, 0.6, 1.0, 1.0);
        CGContext::set_line_width(Some(cg), 1.0);
        // Reset to solid line
        unsafe { CGContext::set_line_dash(Some(cg), 0.0, std::ptr::null(), 0) };

        for (x, y) in points {
            let handle_rect = CGRect::new(
                CGPoint::new(x - hs, y - hs),
                CGSize::new(handle_size, handle_size),
            );
            CGContext::fill_rect(Some(cg), handle_rect);
            CGContext::stroke_rect(Some(cg), handle_rect);
        }
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
                self.ivars().redo_stack.borrow_mut().clear();
                self.ivars().annotations.borrow_mut().push(ann);
                self.setNeedsDisplay(true);
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

    fn show_text_field(&self, point: CGPoint) {
        self.show_text_field_with_text(point, "", self.ivars().annotation_font_size.get());
    }

    fn show_text_field_with_text(&self, point: CGPoint, initial_text: &str, font_size: CGFloat) {
        // Commit any existing text field first
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

    /// Commit the current text field content as a Text annotation.
    pub fn commit_text_field(&self) {
        let field = self.ivars().text_field.borrow_mut().take();
        if let Some(field) = field {
            let text = field.stringValue().to_string();
            if !text.is_empty() {
                let color = self.ivars().annotation_color.get();
                let position = self.ivars().text_position.get();
                let font_size = self.ivars().annotation_font_size.get();
                self.ivars().redo_stack.borrow_mut().clear();
                self.ivars().annotations.borrow_mut().push(Annotation::Text {
                    position,
                    text,
                    color,
                    font_size,
                });
            }
            field.removeFromSuperview();
            self.setNeedsDisplay(true);
            // Restore key responder to self
            if let Some(window) = self.window() {
                let _ = window.makeFirstResponder(Some(self));
            }
        }
    }

    fn dismiss(&self) {
        if let Some(window) = self.window() {
            window.orderOut(None);
        }
    }

    fn notify_selection_changed(&self) {
        // Notify the app delegate to reposition the toolbar
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { objc2::msg_send![&*delegate, selectionChanged: self] };
        }
    }

    /// Notify the app delegate that the tool changed (from keyboard shortcut),
    /// so the toolbar visual state can be updated.
    fn notify_tool_changed(&self) {
        let tool = self.ivars().active_tool.get();
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let d = &*delegate;
            match tool {
                ActiveTool::Select => { let _: () = unsafe { objc2::msg_send![d, toolSelect: self] }; }
                ActiveTool::Arrow => { let _: () = unsafe { objc2::msg_send![d, toolArrow: self] }; }
                ActiveTool::Rectangle => { let _: () = unsafe { objc2::msg_send![d, toolRect: self] }; }
                ActiveTool::Ellipse => { let _: () = unsafe { objc2::msg_send![d, toolEllipse: self] }; }
                ActiveTool::Pencil => { let _: () = unsafe { objc2::msg_send![d, toolPencil: self] }; }
                ActiveTool::Text => { let _: () = unsafe { objc2::msg_send![d, toolText: self] }; }
                ActiveTool::Highlight => { let _: () = unsafe { objc2::msg_send![d, toolHighlight: self] }; }
                ActiveTool::Step => { let _: () = unsafe { objc2::msg_send![d, toolStep: self] }; }
                ActiveTool::Blur => { let _: () = unsafe { objc2::msg_send![d, toolBlur: self] }; }
                ActiveTool::Crop => { let _: () = unsafe { objc2::msg_send![d, toolCrop: self] }; }
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
                "strokeThin:" => { let _: () = unsafe { objc2::msg_send![d, strokeThin: self] }; }
                "strokeMedium:" => { let _: () = unsafe { objc2::msg_send![d, strokeMedium: self] }; }
                "strokeThick:" => { let _: () = unsafe { objc2::msg_send![d, strokeThick: self] }; }
                _ => {}
            }
        }
    }
}

// --- Geometry helpers ---

pub fn normalize_rect(r: CGRect) -> CGRect {
    CGRect::new(
        CGPoint::new(
            if r.size.width < 0.0 { r.origin.x + r.size.width } else { r.origin.x },
            if r.size.height < 0.0 { r.origin.y + r.size.height } else { r.origin.y },
        ),
        CGSize::new(r.size.width.abs(), r.size.height.abs()),
    )
}

fn rect_contains(r: CGRect, p: CGPoint) -> bool {
    p.x >= r.origin.x
        && p.x <= r.origin.x + r.size.width
        && p.y >= r.origin.y
        && p.y <= r.origin.y + r.size.height
}

fn resize_rect(orig: CGRect, mode: DragMode, start: CGPoint, current: CGPoint) -> CGRect {
    let dx = current.x - start.x;
    let dy = current.y - start.y;
    let (x, y, w, h) = (orig.origin.x, orig.origin.y, orig.size.width, orig.size.height);

    let (nx, ny, nw, nh) = match mode {
        DragMode::ResizeTopLeft => (x + dx, y + dy, w - dx, h - dy),
        DragMode::ResizeTopRight => (x, y + dy, w + dx, h - dy),
        DragMode::ResizeBottomLeft => (x + dx, y, w - dx, h + dy),
        DragMode::ResizeBottomRight => (x, y, w + dx, h + dy),
        DragMode::ResizeTop => (x, y + dy, w, h - dy),
        DragMode::ResizeBottom => (x, y, w, h + dy),
        DragMode::ResizeLeft => (x + dx, y, w - dx, h),
        DragMode::ResizeRight => (x, y, w + dx, h),
        _ => (x, y, w, h),
    };

    CGRect::new(CGPoint::new(nx, ny), CGSize::new(nw, nh))
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
    // Solid line for handles
    unsafe { CGContext::set_line_dash(Some(ctx), 0.0, std::ptr::null(), 0) };

    for (_kind, point) in handles {
        let handle_rect = CGRect::new(
            CGPoint::new(point.x - hs, point.y - hs),
            CGSize::new(handle_size, handle_size),
        );
        CGContext::fill_rect(Some(ctx), handle_rect);
        CGContext::stroke_rect(Some(ctx), handle_rect);
    }

    CGContext::restore_g_state(Some(ctx));
}

/// Draw a dashed highlight border around a selected annotation.
fn draw_annotation_highlight(ctx: &CGContext, ann: &Annotation) {
    let rect = ann.bounding_rect();
    let highlight = CGRect::new(
        CGPoint::new(rect.origin.x - 2.0, rect.origin.y - 2.0),
        CGSize::new(rect.size.width + 4.0, rect.size.height + 4.0),
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

/// Map a macOS keyCode to an ActiveTool (for keyboard shortcuts).
pub fn tool_for_key(key_code: u16) -> Option<ActiveTool> {
    match key_code {
        1 => Some(ActiveTool::Select),     // S
        0 => Some(ActiveTool::Arrow),      // A
        15 => Some(ActiveTool::Rectangle), // R
        14 => Some(ActiveTool::Ellipse),   // E
        35 => Some(ActiveTool::Pencil),    // P
        17 => Some(ActiveTool::Text),      // T
        4 => Some(ActiveTool::Highlight),  // H
        45 => Some(ActiveTool::Step),     // N
        11 => Some(ActiveTool::Blur),     // B
        8 => Some(ActiveTool::Crop),      // C
        _ => None,
    }
}

/// Map a macOS keyCode to a stroke action selector name and index.
/// Returns (selector_name, index) for 1=thin, 2=medium, 3=thick.
pub fn stroke_for_key(key_code: u16) -> Option<(&'static str, usize)> {
    match key_code {
        18 => Some(("strokeThin:", 0)),   // 1
        19 => Some(("strokeMedium:", 1)), // 2
        20 => Some(("strokeThick:", 2)),  // 3
        _ => None,
    }
}
