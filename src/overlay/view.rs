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

use crate::annotation::model::Annotation;

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
                for ann in self.ivars().annotations.borrow().iter() {
                    crate::annotation::renderer::draw_annotation(&cg, ann);
                }
                if let Some(ref ann) = *self.ivars().current_annotation.borrow() {
                    crate::annotation::renderer::draw_annotation(&cg, ann);
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

            let active_tool = self.ivars().active_tool.get();

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
            if active_tool != ActiveTool::Select {
                if let Some(ann) = self.ivars().current_annotation.borrow_mut().take() {
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
            // Escape = 53
            if key_code == 53 {
                self.dismiss();
            }
            // Cmd+Z = undo (keyCode 6 = Z)
            let flags = event.modifierFlags();
            if key_code == 6
                && flags.contains(objc2_app_kit::NSEventModifierFlags::Command)
            {
                self.ivars().annotations.borrow_mut().pop();
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

    fn show_text_field(&self, point: CGPoint) {
        // Commit any existing text field first
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

    /// Commit the current text field content as a Text annotation.
    pub fn commit_text_field(&self) {
        let field = self.ivars().text_field.borrow_mut().take();
        if let Some(field) = field {
            let text = field.stringValue().to_string();
            if !text.is_empty() {
                let color = self.ivars().annotation_color.get();
                let position = self.ivars().text_position.get();
                self.ivars().annotations.borrow_mut().push(Annotation::Text {
                    position,
                    text,
                    color,
                    font_size: 16.0,
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
