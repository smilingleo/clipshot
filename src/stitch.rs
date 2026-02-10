use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
    CGImageAlphaInfo,
};

/// Stitch multiple captured frames into a single tall image by detecting overlapping regions.
pub fn stitch_frames(frames: &[CFRetained<CGImage>]) -> Option<CFRetained<CGImage>> {
    if frames.is_empty() {
        return None;
    }
    if frames.len() == 1 {
        return copy_cgimage(&frames[0]);
    }

    let frame_width = CGImage::width(Some(&frames[0]));
    let frame_height = CGImage::height(Some(&frames[0]));

    // Pre-convert all frames to RGBA once (avoids redundant conversions per pair)
    let start = std::time::Instant::now();
    let mut rgba_data: Vec<Vec<u8>> = Vec::with_capacity(frames.len());
    for (i, f) in frames.iter().enumerate() {
        match crate::actions::cgimage_to_rgba(f) {
            Ok(data) => rgba_data.push(data),
            Err(e) => {
                eprintln!("Stitch: RGBA conversion failed for frame {}: {}", i, e);
                return None;
            }
        }
    }
    eprintln!("Stitch: RGBA conversion took {:?}", start.elapsed());

    // Find overlap between each consecutive pair
    let start = std::time::Instant::now();
    let mut overlaps = Vec::with_capacity(frames.len() - 1);
    for i in 0..frames.len() - 1 {
        let overlap =
            find_overlap_fast(&rgba_data[i], &rgba_data[i + 1], frame_width, frame_height);
        overlaps.push(overlap);
    }
    eprintln!(
        "Stitch: overlap detection took {:?}, overlaps: {:?}",
        start.elapsed(),
        overlaps
    );

    // Compute total canvas height
    let mut total_height = frame_height;
    for &overlap in &overlaps {
        let addition = frame_height.saturating_sub(overlap);
        if addition == 0 {
            continue;
        }
        total_height += addition;
    }

    if total_height == 0 || frame_width == 0 {
        return None;
    }

    // Create the output canvas
    let start = std::time::Instant::now();
    let color_space = CGColorSpace::new_device_rgb()?;
    let bitmap_info = CGImageAlphaInfo::PremultipliedLast.0;
    let ctx = unsafe {
        CGBitmapContextCreate(
            std::ptr::null_mut(),
            frame_width,
            total_height,
            8,
            frame_width * 4,
            Some(&color_space),
            bitmap_info,
        )
    }?;

    // Draw frames top to bottom (CGBitmapContext origin = bottom-left)
    let mut current_y = total_height;

    // First frame
    current_y -= frame_height;
    let draw_rect = CGRect::new(
        CGPoint::new(0.0, current_y as CGFloat),
        CGSize::new(frame_width as CGFloat, frame_height as CGFloat),
    );
    CGContext::draw_image(Some(&ctx), draw_rect, Some(&frames[0]));

    // Subsequent frames — crop out overlapping portion
    for (i, &overlap) in overlaps.iter().enumerate() {
        let addition = frame_height.saturating_sub(overlap);
        if addition == 0 {
            continue;
        }

        let crop_rect = CGRect::new(
            CGPoint::new(0.0, overlap as CGFloat),
            CGSize::new(frame_width as CGFloat, addition as CGFloat),
        );
        if let Some(cropped) = CGImage::with_image_in_rect(Some(&frames[i + 1]), crop_rect) {
            current_y -= addition;
            let draw_rect = CGRect::new(
                CGPoint::new(0.0, current_y as CGFloat),
                CGSize::new(frame_width as CGFloat, addition as CGFloat),
            );
            CGContext::draw_image(Some(&ctx), draw_rect, Some(&cropped));
        }
    }

    eprintln!("Stitch: canvas compositing took {:?}", start.elapsed());

    CGBitmapContextCreateImage(Some(&ctx))
}

/// Fast overlap detection using strip matching:
///
/// Instead of comparing variable-size overlap regions (which biases toward small K
/// where fewer pixels make accidental matches likely), we take a fixed-size reference
/// strip from frame A and slide it through frame B. The constant strip size ensures
/// equal signal for all candidate positions, eliminating the small-K false match problem.
///
/// 1. Sparse probe: ~48 positions, 8x column subsampling
/// 2. Refine: ±probe_step around the best, 2x column subsampling
/// 3. Verify: full overlap region with moderate subsampling
fn find_overlap_fast(
    data_a: &[u8],
    data_b: &[u8],
    width: usize,
    height: usize,
) -> usize {
    if data_a.is_empty() || data_b.is_empty() || width == 0 || height < 32 {
        return 0;
    }

    let bpr = width * 4;
    let max_overlap = height * 19 / 20;
    let margin = width / 20;
    let col_start = margin;
    let col_end = width.saturating_sub(margin);
    if col_end <= col_start {
        return 0;
    }

    // Reference strip: 16 rows from the bottom sixth of frame A.
    // Positioned to be roughly centered in the expected overlap region
    // (we scroll 2/3 of height, so overlap ≈ 1/3 of height).
    let strip_rows = 16.min(height / 4);
    let strip_offset = height / 6; // distance from bottom of A to start of strip
    let strip_a_start = height - strip_offset;

    // With overlap of k rows, the strip appears at row (k - strip_offset) in B.
    // Valid when k >= strip_offset. Max position = max_overlap - strip_offset - strip_rows.
    let max_pos_b = max_overlap
        .saturating_sub(strip_offset)
        .saturating_sub(strip_rows);

    if strip_rows < 4 || max_pos_b == 0 {
        return 0;
    }

    // Phase 1: Sparse probe — ~48 positions with 8x column subsampling
    let probe_step = (max_pos_b / 48).max(1);
    let mut best_pos: usize = 0;
    let mut best_sad = f64::MAX;

    let mut pos = 0;
    while pos <= max_pos_b {
        let sad = strip_sad(
            data_a, data_b, bpr, col_start, col_end, strip_a_start, pos, strip_rows, 8,
        );
        if sad < best_sad {
            best_sad = sad;
            best_pos = pos;
        }
        pos += probe_step;
    }

    if best_sad > 30.0 {
        return 0;
    }

    // Phase 2: Refine — ±probe_step around best with 2x column subsampling
    let lo = best_pos.saturating_sub(probe_step);
    let hi = (best_pos + probe_step).min(max_pos_b);
    best_sad = f64::MAX;

    for pos in lo..=hi {
        let sad = strip_sad(
            data_a, data_b, bpr, col_start, col_end, strip_a_start, pos, strip_rows, 2,
        );
        if sad < best_sad {
            best_sad = sad;
            best_pos = pos;
        }
    }

    if best_sad > 20.0 {
        return 0;
    }

    let overlap = best_pos + strip_offset;

    // Phase 3: Verify — check the full overlap region with moderate subsampling
    let verify_sad = compute_sad(data_a, data_b, bpr, width, height, overlap, 4);

    if verify_sad > 20.0 {
        eprintln!(
            "No reliable overlap (verified SAD={:.1} at {} rows)",
            verify_sad, overlap
        );
        return 0;
    }

    if overlap >= height {
        return height;
    }

    eprintln!("Overlap: {} rows (SAD={:.2})", overlap, verify_sad);
    overlap
}

/// Compute average per-channel SAD between a strip of rows in A and a strip of rows in B.
/// Only considers columns in `[col_start, col_end)` with the given step.
#[inline]
fn strip_sad(
    data_a: &[u8],
    data_b: &[u8],
    bpr: usize,
    col_start: usize,
    col_end: usize,
    a_start: usize,
    b_start: usize,
    rows: usize,
    col_step: usize,
) -> f64 {
    let mut sad_sum: u64 = 0;
    let mut count: u64 = 0;

    for r in 0..rows {
        let a_base = (a_start + r) * bpr;
        let b_base = (b_start + r) * bpr;

        let mut col = col_start;
        while col < col_end {
            let ap = a_base + col * 4;
            let bp = b_base + col * 4;
            if ap + 2 < data_a.len() && bp + 2 < data_b.len() {
                sad_sum += (data_a[ap] as i32 - data_b[bp] as i32).unsigned_abs() as u64;
                sad_sum +=
                    (data_a[ap + 1] as i32 - data_b[bp + 1] as i32).unsigned_abs() as u64;
                sad_sum +=
                    (data_a[ap + 2] as i32 - data_b[bp + 2] as i32).unsigned_abs() as u64;
                count += 1;
            }
            col += col_step;
        }
    }

    if count == 0 {
        return f64::MAX;
    }
    sad_sum as f64 / (count as f64 * 3.0)
}

/// Compute average per-channel SAD between bottom K rows of A and top K rows of B.
/// `step`: subsample factor for both rows and columns.
#[inline]
fn compute_sad(
    data_a: &[u8],
    data_b: &[u8],
    bpr: usize,
    width: usize,
    height: usize,
    k: usize,
    step: usize,
) -> f64 {
    let mut sad_sum: u64 = 0;
    let mut count: u64 = 0;

    // Skip leftmost and rightmost 5% of columns to avoid scrollbar/UI-overlay interference
    let margin = width / 20;
    let col_start = margin;
    let col_end = width.saturating_sub(margin);

    let mut row = 0;
    while row < k {
        let a_row = height - k + row;
        let b_row = row;
        let a_base = a_row * bpr;
        let b_base = b_row * bpr;

        let mut col = col_start;
        while col < col_end {
            let ap = a_base + col * 4;
            let bp = b_base + col * 4;
            if ap + 2 < data_a.len() && bp + 2 < data_b.len() {
                sad_sum += (data_a[ap] as i32 - data_b[bp] as i32).unsigned_abs() as u64;
                sad_sum +=
                    (data_a[ap + 1] as i32 - data_b[bp + 1] as i32).unsigned_abs() as u64;
                sad_sum +=
                    (data_a[ap + 2] as i32 - data_b[bp + 2] as i32).unsigned_abs() as u64;
                count += 1;
            }
            col += step;
        }
        row += step;
    }

    if count == 0 {
        return f64::MAX;
    }
    sad_sum as f64 / (count as f64 * 3.0)
}

/// Create a copy of a CGImage via bitmap context.
fn copy_cgimage(image: &CGImage) -> Option<CFRetained<CGImage>> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
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

    let draw_rect = CGRect::new(
        CGPoint::ZERO,
        CGSize::new(width as CGFloat, height as CGFloat),
    );
    CGContext::draw_image(Some(&ctx), draw_rect, Some(image));

    CGBitmapContextCreateImage(Some(&ctx))
}
