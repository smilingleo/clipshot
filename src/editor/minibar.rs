use std::cell::Cell;
use std::ffi::CString;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSView};
use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGSize};
use objc2_core_graphics::CGContext;
use objc2_foundation::{MainThreadMarker, NSRect};

/// What the user is currently dragging on the mini bar.
#[derive(Clone, Copy, PartialEq)]
enum MiniBarDragTarget {
    None,
    StartHandle,
    EndHandle,
}

pub struct MiniBarViewIvars {
    total_frames: Cell<usize>,
    start_frame: Cell<usize>,
    /// None = annotation persists to end of video.
    end_frame: Cell<Option<usize>>,
    current_frame: Cell<usize>,
    dragging: Cell<MiniBarDragTarget>,
    /// Frame the delegate should seek to (set during drag).
    pending_seek_frame: Cell<Option<usize>>,
}

const HANDLE_WIDTH: CGFloat = 6.0;
const TRACK_HEIGHT: CGFloat = 10.0;
const TRACK_Y_OFFSET: CGFloat = 5.0;
const BUTTON_WIDTH: CGFloat = 42.0;
const BUTTON_GAP: CGFloat = 4.0;

/// Total height of the mini bar view (track + padding).
pub const MINI_BAR_HEIGHT: CGFloat = 20.0;
/// Width of the mini bar view (track + button).
pub const MINI_BAR_WIDTH: CGFloat = 220.0;
/// Vertical gap between annotation bounding rect and the mini bar.
pub const MINI_BAR_GAP: CGFloat = 6.0;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "MiniBarView"]
    #[ivars = MiniBarViewIvars]
    pub struct MiniBarView;

    impl MiniBarView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let Some(context) = objc2_app_kit::NSGraphicsContext::currentContext() else {
                return;
            };
            let ctx = context.CGContext();
            let bounds = self.bounds();

            // Draw semi-transparent background pill
            CGContext::save_g_state(Some(&ctx));
            CGContext::set_rgb_fill_color(Some(&ctx), 0.0, 0.0, 0.0, 0.75);
            fill_rounded_rect(&ctx, bounds, bounds.size.height / 2.0);

            let total = self.ivars().total_frames.get();
            if total == 0 {
                CGContext::restore_g_state(Some(&ctx));
                return;
            }

            // Track area is everything left of the button
            let track_right = bounds.size.width - BUTTON_WIDTH - BUTTON_GAP;
            let track_left = HANDLE_WIDTH + 4.0;
            let track_width = track_right - track_left;

            if track_width <= 0.0 {
                CGContext::restore_g_state(Some(&ctx));
                return;
            }

            let track_rect = NSRect::new(
                CGPoint::new(track_left, TRACK_Y_OFFSET),
                CGSize::new(track_width, TRACK_HEIGHT),
            );

            // Draw track background
            CGContext::set_rgb_fill_color(Some(&ctx), 0.3, 0.3, 0.3, 1.0);
            let corner_radius = TRACK_HEIGHT / 2.0;
            fill_rounded_rect(&ctx, track_rect, corner_radius);

            // Draw the colored range between start and end
            let start = self.ivars().start_frame.get();
            let end = self.ivars().end_frame.get().unwrap_or(total);
            let start_x = self.x_for_frame(start);
            let end_x = self.x_for_frame(end);

            if end_x > start_x {
                let range_rect = NSRect::new(
                    CGPoint::new(start_x, TRACK_Y_OFFSET),
                    CGSize::new(end_x - start_x, TRACK_HEIGHT),
                );
                CGContext::set_rgb_fill_color(Some(&ctx), 0.3, 0.6, 1.0, 0.7);
                fill_rounded_rect(&ctx, range_rect, corner_radius);
            }

            // Draw start handle
            CGContext::set_rgb_fill_color(Some(&ctx), 0.4, 0.7, 1.0, 1.0);
            let start_handle = NSRect::new(
                CGPoint::new(start_x - HANDLE_WIDTH / 2.0, TRACK_Y_OFFSET - 2.0),
                CGSize::new(HANDLE_WIDTH, TRACK_HEIGHT + 4.0),
            );
            fill_rounded_rect(&ctx, start_handle, 2.0);

            // Draw end handle
            CGContext::set_rgb_fill_color(Some(&ctx), 0.4, 0.7, 1.0, 1.0);
            let end_handle = NSRect::new(
                CGPoint::new(end_x - HANDLE_WIDTH / 2.0, TRACK_Y_OFFSET - 2.0),
                CGSize::new(HANDLE_WIDTH, TRACK_HEIGHT + 4.0),
            );
            fill_rounded_rect(&ctx, end_handle, 2.0);

            // Draw playhead (thin white line at current frame)
            let current = self.ivars().current_frame.get();
            let playhead_x = self.x_for_frame(current);
            CGContext::set_rgb_stroke_color(Some(&ctx), 1.0, 1.0, 1.0, 0.9);
            CGContext::set_line_width(Some(&ctx), 1.5);
            CGContext::move_to_point(Some(&ctx), playhead_x, TRACK_Y_OFFSET - 2.0);
            CGContext::add_line_to_point(Some(&ctx), playhead_x, TRACK_Y_OFFSET + TRACK_HEIGHT + 2.0);
            CGContext::stroke_path(Some(&ctx));

            // Draw "Done" button on the right
            let button_x = bounds.size.width - BUTTON_WIDTH - 2.0;
            let button_rect = NSRect::new(
                CGPoint::new(button_x, 2.0),
                CGSize::new(BUTTON_WIDTH, bounds.size.height - 4.0),
            );
            CGContext::set_rgb_fill_color(Some(&ctx), 0.3, 0.6, 1.0, 1.0);
            fill_rounded_rect(&ctx, button_rect, (bounds.size.height - 4.0) / 2.0);

            // Draw "Done" text — flipped coordinate system, so use negative d in text matrix
            CGContext::set_rgb_fill_color(Some(&ctx), 1.0, 1.0, 1.0, 1.0);
            let font_name = CString::new("Helvetica-Bold").unwrap();
            let font_size: CGFloat = 11.0;
            #[allow(deprecated)]
            unsafe {
                CGContext::select_font(
                    Some(&ctx),
                    font_name.as_ptr(),
                    font_size,
                    objc2_core_graphics::CGTextEncoding::EncodingMacRoman,
                );
            }
            CGContext::set_text_matrix(
                Some(&ctx),
                CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: -1.0, tx: 0.0, ty: 0.0 },
            );
            let label = CString::new("Done").unwrap();
            let text_w: CGFloat = 30.0; // approximate width of "Done" at 11pt
            let text_x = button_x + (BUTTON_WIDTH - text_w) / 2.0;
            let text_y = bounds.size.height / 2.0 + font_size * 0.35;
            #[allow(deprecated)]
            unsafe {
                CGContext::show_text_at_point(
                    Some(&ctx),
                    text_x,
                    text_y,
                    label.as_ptr(),
                    4, // "Done".len()
                );
            }

            CGContext::restore_g_state(Some(&ctx));
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let point = {
                let window_point = event.locationInWindow();
                self.convertPoint_fromView(window_point, None)
            };

            let bounds = self.bounds();

            // Check if click is in the "Done" button area
            let button_x = bounds.size.width - BUTTON_WIDTH - 2.0;
            if point.x >= button_x {
                self.notify_delegate_confirm();
                return;
            }

            // Otherwise check handle hit-testing
            let start_x = self.x_for_frame(self.ivars().start_frame.get());
            let total = self.ivars().total_frames.get();
            let end = self.ivars().end_frame.get().unwrap_or(total);
            let end_x = self.x_for_frame(end);

            let hit_tolerance = HANDLE_WIDTH + 4.0;

            // Check end handle first (takes priority if handles overlap)
            if (point.x - end_x).abs() <= hit_tolerance {
                self.ivars().dragging.set(MiniBarDragTarget::EndHandle);
            } else if (point.x - start_x).abs() <= hit_tolerance {
                self.ivars().dragging.set(MiniBarDragTarget::StartHandle);
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let drag = self.ivars().dragging.get();
            if drag == MiniBarDragTarget::None {
                return;
            }

            let point = {
                let window_point = event.locationInWindow();
                self.convertPoint_fromView(window_point, None)
            };

            let frame = self.frame_for_x(point.x);
            let total = self.ivars().total_frames.get();

            match drag {
                MiniBarDragTarget::StartHandle => {
                    let end = self.ivars().end_frame.get().unwrap_or(total);
                    let clamped = frame.min(end.saturating_sub(1));
                    self.ivars().start_frame.set(clamped);
                    self.ivars().pending_seek_frame.set(Some(clamped));
                }
                MiniBarDragTarget::EndHandle => {
                    let start = self.ivars().start_frame.get();
                    let clamped = frame.max(start + 1).min(total);
                    self.ivars().end_frame.set(Some(clamped));
                    // Seek to end - 1 so user sees the last visible frame
                    self.ivars().pending_seek_frame.set(Some(clamped.saturating_sub(1)));
                }
                MiniBarDragTarget::None => {}
            }

            self.setNeedsDisplay(true);
            self.notify_delegate_changed();
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            let was_dragging = self.ivars().dragging.get();
            self.ivars().dragging.set(MiniBarDragTarget::None);

            if was_dragging != MiniBarDragTarget::None {
                self.notify_delegate_drag_ended();
            }
        }
    }
);

impl MiniBarView {
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(MiniBarViewIvars {
            total_frames: Cell::new(0),
            start_frame: Cell::new(0),
            end_frame: Cell::new(None),
            current_frame: Cell::new(0),
            dragging: Cell::new(MiniBarDragTarget::None),
            pending_seek_frame: Cell::new(None),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        view
    }

    /// Update the mini bar's state from external source.
    pub fn update_state(
        &self,
        start: usize,
        end: Option<usize>,
        current: usize,
        total: usize,
    ) {
        self.ivars().start_frame.set(start);
        self.ivars().end_frame.set(end);
        self.ivars().current_frame.set(current);
        self.ivars().total_frames.set(total);
        self.setNeedsDisplay(true);
    }

    /// Take the pending seek frame (consumed by delegate).
    pub fn take_pending_seek_frame(&self) -> Option<usize> {
        self.ivars().pending_seek_frame.take()
    }

    /// Get the current start frame value.
    pub fn start_frame(&self) -> usize {
        self.ivars().start_frame.get()
    }

    /// Get the current end frame value.
    pub fn end_frame(&self) -> Option<usize> {
        self.ivars().end_frame.get()
    }

    /// Convert an x coordinate to a frame index.
    fn frame_for_x(&self, x: CGFloat) -> usize {
        let bounds = self.bounds();
        let track_right = bounds.size.width - BUTTON_WIDTH - BUTTON_GAP;
        let track_left = HANDLE_WIDTH + 4.0;
        let track_width = track_right - track_left;
        let total = self.ivars().total_frames.get();
        if track_width <= 0.0 || total == 0 {
            return 0;
        }
        let relative_x = (x - track_left).max(0.0).min(track_width);
        let fraction = relative_x / track_width;
        let frame = (fraction * total as CGFloat).round() as usize;
        frame.min(total)
    }

    /// Convert a frame index to an x coordinate.
    fn x_for_frame(&self, frame: usize) -> CGFloat {
        let bounds = self.bounds();
        let track_right = bounds.size.width - BUTTON_WIDTH - BUTTON_GAP;
        let track_left = HANDLE_WIDTH + 4.0;
        let track_width = track_right - track_left;
        let total = self.ivars().total_frames.get();
        if total == 0 {
            return track_left;
        }
        let fraction = frame as CGFloat / total as CGFloat;
        track_left + fraction * track_width
    }

    fn notify_delegate_changed(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorMiniBarChanged: self] };
        }
    }

    fn notify_delegate_drag_ended(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorMiniBarDragEnded: self] };
        }
    }

    fn notify_delegate_confirm(&self) {
        let mtm = MainThreadMarker::from(self);
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        if let Some(delegate) = app.delegate() {
            let _: () = unsafe { msg_send![&*delegate, editorConfirmAnnotation: self] };
        }
    }
}

/// Fill a rounded rectangle path.
fn fill_rounded_rect(ctx: &CGContext, rect: NSRect, radius: CGFloat) {
    let min_x = rect.origin.x;
    let min_y = rect.origin.y;
    let max_x = min_x + rect.size.width;
    let max_y = min_y + rect.size.height;
    let r = radius.min(rect.size.width / 2.0).min(rect.size.height / 2.0);

    CGContext::move_to_point(Some(ctx), min_x + r, min_y);
    CGContext::add_line_to_point(Some(ctx), max_x - r, min_y);
    CGContext::add_arc_to_point(Some(ctx), max_x, min_y, max_x, min_y + r, r);
    CGContext::add_line_to_point(Some(ctx), max_x, max_y - r);
    CGContext::add_arc_to_point(Some(ctx), max_x, max_y, max_x - r, max_y, r);
    CGContext::add_line_to_point(Some(ctx), min_x + r, max_y);
    CGContext::add_arc_to_point(Some(ctx), min_x, max_y, min_x, max_y - r, r);
    CGContext::add_line_to_point(Some(ctx), min_x, min_y + r);
    CGContext::add_arc_to_point(Some(ctx), min_x, min_y, min_x + r, min_y, r);
    CGContext::close_path(Some(ctx));
    CGContext::fill_path(Some(ctx));
}
