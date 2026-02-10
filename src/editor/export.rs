use std::path::Path;

use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
    CGImageAlphaInfo,
};

use super::decoder::VideoDecoder;
use super::model::TimedAnnotation;
use crate::encoder::VideoEncoder;

/// Export the video with timed annotations composited onto frames.
pub fn export_with_annotations(
    decoder: &VideoDecoder,
    annotations: &[TimedAnnotation],
    output_path: &Path,
) -> Result<(), String> {
    let width = decoder.width();
    let height = decoder.height();
    let total_frames = decoder.total_frames();
    let fps = decoder.fps().round() as i32;

    if total_frames == 0 {
        return Err("No frames to export".to_string());
    }

    let mut encoder = VideoEncoder::new(output_path, width, height, fps)?;
    encoder.start()?;

    for frame_idx in 0..total_frames {
        let Some(source_image) = decoder.frame_at(frame_idx) else {
            continue;
        };

        // Collect annotations visible at this frame
        let visible_annotations: Vec<_> = annotations
            .iter()
            .filter(|ta| {
                frame_idx >= ta.start_frame
                    && ta.end_frame.map_or(true, |end| frame_idx < end)
            })
            .map(|ta| &ta.annotation)
            .collect();

        if visible_annotations.is_empty() {
            // No annotations: encode the source frame directly
            encoder.append_frame(source_image);
        } else {
            // Composite annotations onto the frame
            let composited = composite_frame(source_image, &visible_annotations, width, height);
            match composited {
                Some(ref img) => {
                    encoder.append_frame(img);
                }
                None => {
                    // Fallback: encode without annotations
                    encoder.append_frame(source_image);
                }
            }
        }
    }

    encoder.finish();
    eprintln!("Export complete: {} frames -> {:?}", total_frames, output_path);
    Ok(())
}

/// Draw annotations onto a source frame, producing a new CGImage.
fn composite_frame(
    source: &CGImage,
    annotations: &[&crate::annotation::model::Annotation],
    width: usize,
    height: usize,
) -> Option<objc2_core_foundation::CFRetained<CGImage>> {
    let color_space = CGColorSpace::new_device_rgb()?;
    let bitmap_info = CGImageAlphaInfo::PremultipliedLast.0;

    let ctx = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            width,
            height,
            8,
            width * 4,
            Some(&color_space),
            bitmap_info,
        )
    }?;

    // Draw the source image
    let draw_rect = CGRect::new(
        CGPoint::ZERO,
        CGSize::new(width as CGFloat, height as CGFloat),
    );
    CGContext::draw_image(Some(&ctx), draw_rect, Some(source));

    // The source image is drawn in a bottom-left coordinate system (bitmap context).
    // Annotations were drawn in a flipped (top-left) coordinate system in the editor view.
    // We need to flip the context so annotations render correctly.
    CGContext::translate_ctm(Some(&ctx), 0.0, height as CGFloat);
    CGContext::scale_ctm(Some(&ctx), 1.0, -1.0);

    for ann in annotations {
        crate::annotation::renderer::draw_annotation(&ctx, ann);
    }

    CGBitmapContextCreateImage(Some(&ctx))
}
