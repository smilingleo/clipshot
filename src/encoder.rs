use std::ffi::c_void;
use std::path::Path;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVFileTypeMPEG4,
    AVMediaTypeVideo, AVVideoCodecKey, AVVideoCodecTypeH264, AVVideoHeightKey, AVVideoWidthKey,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGImage};
use objc2_core_media::CMTime;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

pub struct VideoEncoder {
    writer: Retained<AVAssetWriter>,
    input: Retained<AVAssetWriterInput>,
    adaptor: Retained<AVAssetWriterInputPixelBufferAdaptor>,
    width: usize,
    height: usize,
    frame_count: u64,
    fps: i32,
}

impl VideoEncoder {
    pub fn new(path: &Path, width: usize, height: usize, fps: i32) -> Result<Self, String> {
        let url = {
            let path_str = path.to_str().ok_or("Invalid path")?;
            let ns_path = NSString::from_str(path_str);
            NSURL::fileURLWithPath(&ns_path)
        };

        let file_type = unsafe { AVFileTypeMPEG4.ok_or("AVFileTypeMPEG4 not available")? };

        let writer = unsafe {
            AVAssetWriter::assetWriterWithURL_fileType_error(&url, file_type)
                .map_err(|e| format!("Failed to create AVAssetWriter: {}", e))?
        };

        let output_settings = build_video_settings(width, height)?;

        let media_type =
            unsafe { AVMediaTypeVideo.ok_or("AVMediaTypeVideo not available")? };

        let input = unsafe {
            AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(
                media_type,
                Some(&output_settings),
            )
        };
        unsafe { input.setExpectsMediaDataInRealTime(true) };

        let adaptor = unsafe {
            AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
                &input,
                None,
            )
        };

        unsafe { writer.addInput(&input) };

        Ok(VideoEncoder {
            writer,
            input,
            adaptor,
            width,
            height,
            frame_count: 0,
            fps,
        })
    }

    pub fn start(&self) -> Result<(), String> {
        let ok = unsafe { self.writer.startWriting() };
        if !ok {
            let err = unsafe { self.writer.error() };
            return Err(format!(
                "startWriting failed: {}",
                err.map(|e| e.to_string()).unwrap_or_default()
            ));
        }
        let zero = unsafe { CMTime::new(0, self.fps) };
        unsafe { self.writer.startSessionAtSourceTime(zero) };
        eprintln!("VideoEncoder: started writing");
        Ok(())
    }

    /// Append a cropped CGImage as a frame. Renders directly into CVPixelBuffer.
    pub fn append_frame(&mut self, cropped_image: &CGImage) -> bool {
        if !unsafe { self.input.isReadyForMoreMediaData() } {
            return false;
        }

        // Create a CVPixelBuffer from the CGImage
        let mut pixel_buffer_ptr: *mut CVPixelBuffer = std::ptr::null_mut();
        let status = unsafe {
            CVPixelBufferCreate(
                None,
                self.width,
                self.height,
                kCVPixelFormatType_32BGRA,
                None,
                NonNull::new(&mut pixel_buffer_ptr).unwrap(),
            )
        };

        if status != kCVReturnSuccess || pixel_buffer_ptr.is_null() {
            eprintln!("VideoEncoder: CVPixelBufferCreate failed: {}", status);
            return false;
        }

        // SAFETY: CVPixelBufferCreate succeeded; pixel_buffer_ptr is a valid CF object
        // with retain count 1. We must CFRelease it when done.
        let pb_ref: &CVPixelBuffer = unsafe { &*pixel_buffer_ptr };

        // Render the CGImage into the pixel buffer memory
        let rendered = self.render_cgimage_to_pixel_buffer(pb_ref, cropped_image);
        if !rendered {
            unsafe { CFRelease(pixel_buffer_ptr as *const c_void) };
            return false;
        }

        let presentation_time = unsafe { CMTime::new(self.frame_count as i64, self.fps) };

        let ok = unsafe {
            self.adaptor
                .appendPixelBuffer_withPresentationTime(pb_ref, presentation_time)
        };

        // Release our ownership of the pixel buffer (adaptor retains internally if needed)
        unsafe { CFRelease(pixel_buffer_ptr as *const c_void) };

        if ok {
            self.frame_count += 1;
        } else {
            eprintln!("VideoEncoder: appendPixelBuffer failed");
        }

        ok
    }

    /// Finish writing synchronously.
    pub fn finish(&self) {
        unsafe { self.input.markAsFinished() };

        #[allow(deprecated)]
        let ok = unsafe { self.writer.finishWriting() };

        if ok {
            eprintln!(
                "VideoEncoder: finished writing ({} frames)",
                self.frame_count
            );
        } else {
            let err = unsafe { self.writer.error() };
            eprintln!(
                "VideoEncoder: finishWriting failed: {}",
                err.map(|e| e.to_string()).unwrap_or_default()
            );
        }
    }

    /// Render a CGImage into an already-created CVPixelBuffer via CGBitmapContext.
    fn render_cgimage_to_pixel_buffer(
        &self,
        pixel_buffer: &CVPixelBuffer,
        image: &CGImage,
    ) -> bool {
        unsafe {
            CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0));
        }

        let base_address = CVPixelBufferGetBaseAddress(pixel_buffer);
        let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);

        if base_address.is_null() {
            unsafe { CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0)) };
            return false;
        }

        // kCGBitmapByteOrder32Little | kCGImageAlphaPremultipliedFirst
        let bitmap_info: u32 = 0x2000 | 2;
        let color_space = match CGColorSpace::new_device_rgb() {
            Some(cs) => cs,
            None => {
                unsafe {
                    CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0));
                }
                return false;
            }
        };

        let ctx = unsafe {
            CGBitmapContextCreate(
                base_address,
                self.width,
                self.height,
                8,
                bytes_per_row,
                Some(&color_space),
                bitmap_info,
            )
        };

        let ok = if let Some(ref ctx) = ctx {
            let draw_rect = CGRect::new(
                CGPoint::ZERO,
                CGSize::new(self.width as CGFloat, self.height as CGFloat),
            );
            CGContext::draw_image(Some(ctx), draw_rect, Some(image));
            true
        } else {
            false
        };

        unsafe {
            CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags(0));
        }

        ok
    }
}

/// Build an NSDictionary with video codec, width and height settings.
fn build_video_settings(
    width: usize,
    height: usize,
) -> Result<Retained<NSDictionary<NSString, AnyObject>>, String> {
    let codec_key =
        unsafe { AVVideoCodecKey.ok_or("AVVideoCodecKey not available")? };
    let codec_value =
        unsafe { AVVideoCodecTypeH264.ok_or("AVVideoCodecTypeH264 not available")? };
    let width_key =
        unsafe { AVVideoWidthKey.ok_or("AVVideoWidthKey not available")? };
    let height_key =
        unsafe { AVVideoHeightKey.ok_or("AVVideoHeightKey not available")? };

    let width_num = NSNumber::numberWithInt(width as _);
    let height_num = NSNumber::numberWithInt(height as _);

    let codec_obj: &AnyObject =
        unsafe { &*(codec_value as *const NSString as *const AnyObject) };
    let width_obj: &AnyObject =
        unsafe { &*(&*width_num as *const NSNumber as *const AnyObject) };
    let height_obj: &AnyObject =
        unsafe { &*(&*height_num as *const NSNumber as *const AnyObject) };

    let keys: &[&NSString] = &[codec_key, width_key, height_key];
    let objects: &[&AnyObject] = &[codec_obj, width_obj, height_obj];

    Ok(NSDictionary::from_slices(keys, objects))
}
