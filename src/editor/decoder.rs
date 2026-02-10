use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderTrackOutput, AVMediaTypeVideo, AVURLAsset,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGImage,
};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress, kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};

/// Pre-decodes all frames from an MP4 into CGImages for random access.
pub struct VideoDecoder {
    frames: Vec<CFRetained<CGImage>>,
    fps: f64,
    width: usize,
    height: usize,
}

impl VideoDecoder {
    /// Open a video file and decode all frames into memory.
    pub fn open(path: &Path) -> Result<Self, String> {
        let path_str = path.to_str().ok_or("Invalid path")?;
        let ns_path = NSString::from_str(path_str);
        let url = NSURL::fileURLWithPath(&ns_path);

        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };

        // Get video track
        let video_type =
            unsafe { AVMediaTypeVideo.ok_or("AVMediaTypeVideo not available")? };
        #[allow(deprecated)]
        let tracks = unsafe { asset.tracksWithMediaType(video_type) };
        if tracks.len() == 0 {
            return Err("No video tracks found".to_string());
        }
        let track = tracks.objectAtIndex(0);

        let fps = unsafe { track.nominalFrameRate() } as f64;
        let natural_size = unsafe { track.naturalSize() };
        let width = natural_size.width as usize;
        let height = natural_size.height as usize;

        if width == 0 || height == 0 {
            return Err(format!("Invalid video dimensions: {}x{}", width, height));
        }

        // Create output settings for BGRA pixel format
        let output_settings = build_pixel_format_settings()?;

        // Create AVAssetReaderTrackOutput
        let track_output = unsafe {
            AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
                &track,
                Some(&output_settings),
            )
        };
        unsafe { track_output.setAlwaysCopiesSampleData(false) };

        // Create AVAssetReader
        let reader = unsafe {
            AVAssetReader::assetReaderWithAsset_error(&asset)
                .map_err(|e| format!("Failed to create AVAssetReader: {}", e))?
        };

        unsafe { reader.addOutput(&track_output) };

        let ok = unsafe { reader.startReading() };
        if !ok {
            let err = unsafe { reader.error() };
            return Err(format!(
                "Failed to start reading: {}",
                err.map(|e| e.to_string()).unwrap_or_default()
            ));
        }

        // Read all frames
        let mut frames = Vec::new();
        loop {
            let sample = unsafe { track_output.copyNextSampleBuffer() };
            let Some(sample) = sample else {
                break;
            };

            // Get the pixel buffer from the sample
            let image_buffer = unsafe { sample.image_buffer() };
            let Some(pixel_buffer) = image_buffer else {
                continue;
            };

            // Convert CVPixelBuffer to CGImage
            if let Some(cg_image) = pixel_buffer_to_cgimage(&pixel_buffer) {
                frames.push(cg_image);
            }
        }

        eprintln!(
            "VideoDecoder: decoded {} frames ({}x{} @ {:.1}fps)",
            frames.len(),
            width,
            height,
            fps
        );

        Ok(VideoDecoder {
            frames,
            fps,
            width,
            height,
        })
    }

    /// Create a decoder from a single static image (for scroll capture stitched results).
    pub fn from_image(image: CFRetained<CGImage>) -> Self {
        let width = CGImage::width(Some(&image));
        let height = CGImage::height(Some(&image));
        eprintln!("VideoDecoder: static image {}x{}", width, height);
        VideoDecoder {
            frames: vec![image],
            fps: 1.0,
            width,
            height,
        }
    }

    pub fn frame_at(&self, index: usize) -> Option<&CGImage> {
        self.frames.get(index).map(|f| &**f)
    }

    pub fn total_frames(&self) -> usize {
        self.frames.len()
    }

    pub fn fps(&self) -> f64 {
        self.fps
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Replace the decoder's image with a new one (used for crop).
    pub fn replace_image(&mut self, image: CFRetained<CGImage>) {
        let width = CGImage::width(Some(&image));
        let height = CGImage::height(Some(&image));
        self.frames = vec![image];
        self.width = width;
        self.height = height;
    }
}

/// Convert a CVPixelBuffer (BGRA) to a CGImage via CGBitmapContext.
fn pixel_buffer_to_cgimage(
    pixel_buffer: &objc2_core_video::CVPixelBuffer,
) -> Option<CFRetained<CGImage>> {
    unsafe {
        CVPixelBufferLockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly);
    }

    let base_address = CVPixelBufferGetBaseAddress(pixel_buffer);
    let bytes_per_row = CVPixelBufferGetBytesPerRow(pixel_buffer);
    let width = CVPixelBufferGetWidth(pixel_buffer);
    let height = CVPixelBufferGetHeight(pixel_buffer);

    if base_address.is_null() || width == 0 || height == 0 {
        unsafe {
            CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly);
        }
        return None;
    }

    // kCGBitmapByteOrder32Little | kCGImageAlphaPremultipliedFirst (matches BGRA)
    let bitmap_info: u32 = 0x2000 | 2;
    let color_space = CGColorSpace::new_device_rgb()?;

    let ctx = unsafe {
        CGBitmapContextCreate(
            base_address,
            width,
            height,
            8,
            bytes_per_row,
            Some(&color_space),
            bitmap_info,
        )
    }?;

    let result = CGBitmapContextCreateImage(Some(&ctx));

    unsafe {
        CVPixelBufferUnlockBaseAddress(pixel_buffer, CVPixelBufferLockFlags::ReadOnly);
    }

    result
}

/// Build NSDictionary with kCVPixelBufferPixelFormatTypeKey -> kCVPixelFormatType_32BGRA.
fn build_pixel_format_settings(
) -> Result<Retained<NSDictionary<NSString, AnyObject>>, String> {
    // The key is a CFString; we need to bridge it to NSString for the dictionary.
    // kCVPixelBufferPixelFormatTypeKey = "PixelFormatType"
    let key = NSString::from_str("PixelFormatType");
    let value = NSNumber::numberWithInt(kCVPixelFormatType_32BGRA as i32);

    let key_ref: &NSString = &key;
    let value_ref: &AnyObject =
        unsafe { &*(&*value as *const NSNumber as *const AnyObject) };

    Ok(NSDictionary::from_slices(&[key_ref], &[value_ref]))
}
