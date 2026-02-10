use objc2::rc::Retained;
use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGImage;
use objc2_foundation::NSTimer;

/// Phase within each timer tick: scroll first, capture on the next tick.
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Scroll,
    Capture,
}

/// State machine for scrolling capture.
///
/// Alternates between two phases driven by NSTimer ticks:
///   Scroll → (timer interval for settle) → Capture → Scroll → ...
///
/// The timer interval between ticks gives the scrolled content time to render.
/// Because each tick returns immediately (no blocking sleep), the main run loop
/// stays responsive for hotkey events (Ctrl+Cmd+S to stop).
pub struct ScrollCaptureState {
    /// Selection rectangle in overlay logical coordinates (top-left origin, flipped).
    pub selection: CGRect,
    /// Display scale factor (e.g. 2.0 for Retina).
    pub scale_factor: CGFloat,
    /// CG global origin of the display (from CGDisplayBounds, top-left origin).
    pub screen_origin: CGPoint,
    /// Maximum number of scroll steps before auto-stop.
    pub max_steps: usize,
    /// Delay between timer ticks in seconds.
    pub settle_delay: f64,
    /// Captured frames (in pixel coordinates, cropped to selection).
    pub frames: Vec<CFRetained<CGImage>>,
    /// RGBA pixel data captured at the same time as each frame.
    /// Stored immediately at capture time to avoid CGImage copy-on-write issues
    /// where backing data becomes stale after subsequent screen captures.
    pub frame_rgba: Vec<Vec<u8>>,
    /// Timer driving the capture loop.
    pub timer: Option<Retained<NSTimer>>,
    /// Number of scroll steps performed so far.
    step_count: usize,
    /// Current phase in the tick cycle.
    phase: Phase,
    /// Window ID of the border window to exclude from capture.
    border_window_id: Option<u32>,
    /// The display ID to capture frames from (locked at capture start).
    display_id: u32,
}

impl ScrollCaptureState {
    pub fn new(
        selection: CGRect,
        scale_factor: CGFloat,
        screen_origin: CGPoint,
        display_id: u32,
    ) -> Self {
        ScrollCaptureState {
            selection,
            scale_factor,
            screen_origin,
            max_steps: 50,
            settle_delay: 0.5,
            frames: Vec::new(),
            frame_rgba: Vec::new(),
            timer: None,
            step_count: 0,
            phase: Phase::Capture,
            border_window_id: None,
            display_id,
        }
    }

    /// Set the border window ID to exclude from screen captures.
    pub fn set_border_window_id(&mut self, id: u32) {
        self.border_window_id = Some(id);
    }

    /// Called by the timer on each tick. Returns false to signal stop.
    ///
    /// Two-phase cycle (non-blocking):
    ///   Phase::Scroll  → simulate scroll, switch to Capture
    ///   Phase::Capture → capture frame, check overlap, switch to Scroll
    ///
    /// The NSTimer interval between ticks lets the scroll render before capture.
    pub fn tick(&mut self) -> bool {
        match self.phase {
            Phase::Scroll => {
                self.step_count += 1;
                if self.step_count > self.max_steps {
                    eprintln!("Scroll capture: max steps reached");
                    return false;
                }

                // Scroll down by 2/3 of selection height — guarantees at least 1/3 overlap
                let center_x = self.selection.origin.x + self.selection.size.width / 2.0;
                let center_y = self.selection.origin.y + self.selection.size.height / 2.0;
                let screen_point =
                    crate::scroll::overlay_to_cg_global(center_x, center_y, self.screen_origin);
                let scroll_amount = -(self.selection.size.height * 2.0 / 3.0) as i32;
                crate::scroll::simulate_scroll(screen_point, scroll_amount);

                self.phase = Phase::Capture;
                true
            }
            Phase::Capture => {
                let Some(frame) = self.capture_and_crop() else {
                    // Capture failed, try scrolling again
                    self.phase = Phase::Scroll;
                    return true;
                };

                // Convert to RGBA immediately while the screen capture data is fresh.
                // CGImage backing data can become stale due to copy-on-write semantics,
                // so we capture the bytes now for reliable overlap detection later.
                let rgba = match crate::actions::cgimage_to_rgba(&frame) {
                    Ok(data) => data,
                    Err(e) => {
                        eprintln!("Scroll capture: RGBA conversion failed: {}", e);
                        self.phase = Phase::Scroll;
                        return true;
                    }
                };

                // Check overlap with previous frame to detect end of scrollable content
                if let Some(prev) = self.frames.last() {
                    let frame_height = CGImage::height(Some(prev));
                    let overlap = estimate_overlap(prev, &frame, frame_height);

                    // 95%+ overlap means content didn't scroll — duplicate frame, stop without pushing
                    if overlap >= frame_height * 19 / 20 {
                        eprintln!("Scroll capture: content stopped scrolling (overlap={} / height={})", overlap, frame_height);
                        return false;
                    }

                    // 80%+ overlap means near end of scrollable content — push frame and stop
                    if overlap > frame_height * 4 / 5 {
                        eprintln!(
                            "Scroll capture: near end of content (overlap={} / height={}), stopping",
                            overlap, frame_height
                        );
                        self.frames.push(frame);
                        self.frame_rgba.push(rgba);
                        return false;
                    }
                }

                self.frames.push(frame);
                self.frame_rgba.push(rgba);
                eprintln!("Scroll capture: frame {} captured", self.frames.len());
                self.phase = Phase::Scroll;
                true
            }
        }
    }

    /// Capture the target display and crop to the selection area.
    fn capture_and_crop(&self) -> Option<CFRetained<CGImage>> {
        let full = crate::capture::capture_display_excluding(self.display_id, self.border_window_id)?;

        // Convert selection from logical coords to pixel coords
        let pixel_x = (self.selection.origin.x * self.scale_factor) as usize;
        let pixel_y = (self.selection.origin.y * self.scale_factor) as usize;
        let pixel_w = (self.selection.size.width * self.scale_factor) as usize;
        let pixel_h = (self.selection.size.height * self.scale_factor) as usize;

        if pixel_w == 0 || pixel_h == 0 {
            return None;
        }

        let crop_rect = CGRect::new(
            CGPoint::new(pixel_x as CGFloat, pixel_y as CGFloat),
            CGSize::new(pixel_w as CGFloat, pixel_h as CGFloat),
        );

        CGImage::with_image_in_rect(Some(&full), crop_rect)
    }
}

/// Estimate the vertical overlap between two consecutive frames.
///
/// Crops a small strip from the bottom of `prev` and searches for it in
/// `curr`. Returns approximate overlap in pixels, or 0 if no overlap.
fn estimate_overlap(prev: &CGImage, curr: &CGImage, frame_height: usize) -> usize {
    let width = CGImage::width(Some(prev));
    if width == 0 || frame_height < 15 {
        return 0;
    }

    // Crop prev to just the bottom 15 rows
    let strip_height = 15.min(frame_height);
    let strip_start = frame_height - strip_height;
    let prev_crop_rect = CGRect::new(
        CGPoint::new(0.0, strip_start as CGFloat),
        CGSize::new(width as CGFloat, strip_height as CGFloat),
    );
    let Some(prev_strip) = CGImage::with_image_in_rect(Some(prev), prev_crop_rect) else {
        return 0;
    };
    let Ok(data_prev) = crate::actions::cgimage_to_rgba(&prev_strip) else {
        return 0;
    };

    // Search the full curr image
    let search_height = frame_height;
    let curr_crop_rect = CGRect::new(
        CGPoint::ZERO,
        CGSize::new(width as CGFloat, search_height as CGFloat),
    );
    let Some(curr_top) = CGImage::with_image_in_rect(Some(curr), curr_crop_rect) else {
        return 0;
    };
    let Ok(data_curr) = crate::actions::cgimage_to_rgba(&curr_top) else {
        return 0;
    };

    let bpr = width * 4;

    // Reference strip: 5 rows starting 10 rows from the bottom of prev
    let ref_local_start = strip_height.saturating_sub(10);
    let ref_count = 5.min(strip_height - ref_local_start);
    let dist_from_bottom = strip_height - ref_local_start;

    let search_limit = search_height.saturating_sub(ref_count);

    for candidate in 0..=search_limit {
        let mut sad_sum: u64 = 0;
        let mut count: u64 = 0;

        for r in 0..ref_count {
            let prev_row = ref_local_start + r;
            let curr_row = candidate + r;

            let prev_offset = prev_row * bpr;
            let curr_offset = curr_row * bpr;

            let mut col = 0;
            while col < width {
                let pp = prev_offset + col * 4;
                let cp = curr_offset + col * 4;
                if pp + 2 < data_prev.len() && cp + 2 < data_curr.len() {
                    for c in 0..3 {
                        sad_sum += (data_prev[pp + c] as i32 - data_curr[cp + c] as i32)
                            .unsigned_abs() as u64;
                    }
                    count += 1;
                }
                col += 8;
            }
        }

        if count > 0 {
            let avg = sad_sum as f64 / (count as f64 * 3.0);
            if avg < 5.0 {
                return dist_from_bottom + candidate;
            }
        }
    }

    0
}
