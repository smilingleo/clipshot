use std::path::PathBuf;

use objc2::rc::Retained;
use objc2_core_foundation::{CFRetained, CGFloat, CGRect};
use objc2_core_graphics::CGImage;
use objc2_foundation::NSTimer;

use crate::encoder::VideoEncoder;

pub struct RecordingState {
    pub encoder: VideoEncoder,
    pub selection_rect: CGRect,
    pub scale_factor: CGFloat,
    pub timer: Option<Retained<NSTimer>>,
    pub output_path: Option<PathBuf>,
    /// Window ID of the border overlay to exclude from screen capture.
    pub exclude_window_id: Option<u32>,
}

impl RecordingState {
    pub fn new(
        encoder: VideoEncoder,
        selection_rect: CGRect,
        scale_factor: CGFloat,
    ) -> Self {
        RecordingState {
            encoder,
            selection_rect,
            scale_factor,
            timer: None,
            output_path: None,
            exclude_window_id: None,
        }
    }

    /// Capture one frame: grab full screen, crop to selection, feed to encoder.
    pub fn capture_frame(&mut self) {
        let full_image =
            match crate::capture::capture_full_screen_excluding(self.exclude_window_id) {
                Some(img) => img,
                None => return,
            };

        let cropped = match self.crop_to_selection(&full_image) {
            Some(img) => img,
            None => return,
        };

        self.encoder.append_frame(&cropped);
    }

    fn crop_to_selection(&self, full_image: &CGImage) -> Option<CFRetained<CGImage>> {
        let sel = self.selection_rect;
        let s = self.scale_factor;

        let pixel_rect = CGRect::new(
            objc2_core_foundation::CGPoint::new(sel.origin.x * s, sel.origin.y * s),
            objc2_core_foundation::CGSize::new(sel.size.width * s, sel.size.height * s),
        );

        CGImage::with_image_in_rect(Some(full_image), pixel_rect)
    }
}
