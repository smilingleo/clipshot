use objc2_app_kit::{NSModalResponseOK, NSSavePanel};
use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
    CGImageAlphaInfo,
};
use objc2_foundation::{MainThreadMarker, NSString};

use crate::annotation::model::Annotation;

/// Crop the captured CGImage to the selection area, compositing annotations on top.
pub fn crop_and_composite(
    full_image: &CGImage,
    selection: CGRect,
    scale_factor: CGFloat,
    annotations: &[Annotation],
) -> Option<CFRetained<CGImage>> {
    let pixel_x = (selection.origin.x * scale_factor) as usize;
    let pixel_y = (selection.origin.y * scale_factor) as usize;
    let pixel_w = (selection.size.width * scale_factor) as usize;
    let pixel_h = (selection.size.height * scale_factor) as usize;

    if pixel_w == 0 || pixel_h == 0 {
        return None;
    }

    let crop_rect = CGRect::new(
        CGPoint::new(pixel_x as CGFloat, pixel_y as CGFloat),
        CGSize::new(pixel_w as CGFloat, pixel_h as CGFloat),
    );
    let cropped = CGImage::with_image_in_rect(Some(full_image), crop_rect)?;

    if annotations.is_empty() {
        return Some(cropped);
    }

    // Create a bitmap context to composite annotations
    let color_space = CGColorSpace::new_device_rgb()?;
    let bitmap_info = CGImageAlphaInfo::PremultipliedLast.0;
    let ctx = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            pixel_w,
            pixel_h,
            8,
            pixel_w * 4,
            Some(&color_space),
            bitmap_info,
        )
    }?;

    let draw_rect = CGRect::new(
        CGPoint::ZERO,
        CGSize::new(pixel_w as CGFloat, pixel_h as CGFloat),
    );
    CGContext::draw_image(Some(&ctx), draw_rect, Some(&cropped));

    // Scale context for annotations (logical coords -> pixel coords)
    CGContext::scale_ctm(Some(&ctx), scale_factor, scale_factor);
    CGContext::translate_ctm(Some(&ctx), -selection.origin.x, -selection.origin.y);

    // Flip coordinate system (bitmap context is bottom-left, annotations are top-left)
    CGContext::translate_ctm(Some(&ctx), 0.0, selection.origin.y + selection.size.height);
    CGContext::scale_ctm(Some(&ctx), 1.0, -1.0);
    CGContext::translate_ctm(Some(&ctx), 0.0, -selection.origin.y);

    for ann in annotations {
        crate::annotation::renderer::draw_annotation(&ctx, ann, Some(&cropped));
    }

    CGBitmapContextCreateImage(Some(&ctx))
}

/// Copy a CGImage to the system clipboard using arboard.
pub fn copy_to_clipboard(image: &CGImage) -> Result<(), String> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));

    let rgba = cgimage_to_rgba(image)?;

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let img_data = arboard::ImageData {
        width,
        height,
        bytes: std::borrow::Cow::Borrowed(&rgba),
    };
    clipboard.set_image(img_data).map_err(|e| e.to_string())?;

    eprintln!("Image copied to clipboard ({}x{})", width, height);
    Ok(())
}

/// Save a CGImage to a file via NSSavePanel.
pub fn save_to_file(image: &CGImage, mtm: MainThreadMarker) {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));

    let rgba = match cgimage_to_rgba(image) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to convert image: {}", e);
            return;
        }
    };

    let img_buf = match image::RgbaImage::from_raw(width as u32, height as u32, rgba) {
        Some(buf) => buf,
        None => {
            eprintln!("Failed to create image buffer");
            return;
        }
    };

    let panel = NSSavePanel::new(mtm);
    panel.setNameFieldStringValue(&NSString::from_str("clipshot.png"));

    let response = panel.runModal();
    if response == NSModalResponseOK {
        if let Some(url) = panel.URL() {
            if let Some(path) = url.path() {
                let path_str = path.to_string();
                if let Err(e) = img_buf.save(&path_str) {
                    eprintln!("Failed to save: {}", e);
                } else {
                    eprintln!("Saved to {}", path_str);
                }
            }
        }
    }
}

/// Convert a CGImage to an RGBA byte buffer.
pub(crate) fn cgimage_to_rgba(image: &CGImage) -> Result<Vec<u8>, String> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    let bytes_per_row = width * 4;
    let total_bytes = bytes_per_row * height;

    let color_space =
        CGColorSpace::new_device_rgb().ok_or("Failed to create color space")?;

    let bitmap_info = CGImageAlphaInfo::PremultipliedLast.0;
    let mut buffer = vec![0u8; total_bytes];
    let ctx = unsafe {
        CGBitmapContextCreate(
            buffer.as_mut_ptr() as *mut _,
            width,
            height,
            8,
            bytes_per_row,
            Some(&color_space),
            bitmap_info,
        )
    }
    .ok_or("Failed to create bitmap context")?;

    let draw_rect = CGRect::new(
        CGPoint::ZERO,
        CGSize::new(width as CGFloat, height as CGFloat),
    );
    CGContext::draw_image(Some(&ctx), draw_rect, Some(image));

    Ok(buffer)
}
