use std::hash::{Hash, Hasher};

use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
    CGImageAlphaInfo,
};

/// Stitch multiple captured frames into a single tall image by detecting overlapping regions.
///
/// `rgba_data` contains pre-converted RGBA pixel data for each frame, captured at the same
/// time as the CGImages. This avoids CGImage copy-on-write issues where backing data becomes
/// stale between capture time and stitch time.
pub fn stitch_frames(
    frames: &[CFRetained<CGImage>],
    rgba_data: &[Vec<u8>],
) -> Option<CFRetained<CGImage>> {
    if frames.is_empty() || rgba_data.len() != frames.len() {
        return None;
    }
    if frames.len() == 1 {
        return copy_cgimage(&frames[0]);
    }

    let frame_width = CGImage::width(Some(&frames[0]));
    let frame_height = CGImage::height(Some(&frames[0]));

    // Find overlap between each consecutive pair
    let start = std::time::Instant::now();
    let mut overlaps = Vec::with_capacity(frames.len() - 1);
    for i in 0..frames.len() - 1 {
        let overlap =
            find_overlap(&rgba_data[i], &rgba_data[i + 1], frame_width, frame_height);
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

    // Compose output directly from RGBA byte data (avoids CGImage stale-data issues).
    // Both source and output use the same layout: RGBA 8bpc, top-down, row-major.
    let start = std::time::Instant::now();
    let bpr = frame_width * 4;
    let mut output = vec![0u8; bpr * total_height];
    let mut current_row: usize = 0;

    // First frame: copy all rows
    let copy_bytes = bpr * frame_height;
    output[..copy_bytes].copy_from_slice(&rgba_data[0][..copy_bytes]);
    current_row += frame_height;

    // Subsequent frames: copy only non-overlapping rows
    for (i, &overlap) in overlaps.iter().enumerate() {
        let addition = frame_height.saturating_sub(overlap);
        if addition == 0 {
            continue;
        }

        let src = &rgba_data[i + 1];
        let src_start = overlap * bpr;
        let src_end = src_start + addition * bpr;
        let dst_start = current_row * bpr;
        let dst_end = dst_start + addition * bpr;
        output[dst_start..dst_end].copy_from_slice(&src[src_start..src_end]);
        current_row += addition;
    }

    eprintln!("Stitch: canvas compositing took {:?}", start.elapsed());

    // Create CGImage from the composited buffer
    let color_space = CGColorSpace::new_device_rgb()?;
    let bitmap_info = CGImageAlphaInfo::PremultipliedLast.0;
    let ctx = unsafe {
        CGBitmapContextCreate(
            output.as_mut_ptr() as *mut _,
            frame_width,
            total_height,
            8,
            bpr,
            Some(&color_space),
            bitmap_info,
        )
    }?;

    let image = CGBitmapContextCreateImage(Some(&ctx));
    drop(ctx);
    drop(output);
    image
}

/// Hash one pixel row's RGB bytes over the column range `[byte_start, byte_end)`.
/// Uses `DefaultHasher` to produce a `u64` fingerprint.
#[inline]
fn hash_row(data: &[u8], row: usize, bpr: usize, byte_start: usize, byte_end: usize) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    let base = row * bpr;
    let start = base + byte_start;
    let end = base + byte_end;
    if end <= data.len() {
        data[start..end].hash(&mut hasher);
    }
    hasher.finish()
}

/// Detect overlap between two consecutive frames using row hashing.
///
/// Two-phase algorithm:
/// 1. Hash rows in the search regions of both frames.
/// 2. Pick a reference row from A, find candidate overlap values by matching its hash in B.
/// 3. For each candidate, count contiguous matching row hashes. The candidate with the
///    longest run wins. Requires a minimum run of 20 rows to accept.
fn find_overlap(
    data_a: &[u8],
    data_b: &[u8],
    width: usize,
    height: usize,
) -> usize {
    if data_a.is_empty() || data_b.is_empty() || width == 0 || height < 32 {
        return 0;
    }

    let bpr = width * 4;
    // Asymmetric margins: include left edge content (row numbers etc. make rows unique)
    // but exclude rightmost 5% to avoid scrollbar interference.
    let byte_start = 0;
    let right_margin = width / 20;
    let byte_end = width.saturating_sub(right_margin) * 4;
    if byte_end <= byte_start {
        return 0;
    }

    // Search range covers up to 95% of the frame height to handle high-overlap frames
    // near the end of scrollable content (scroll_capture stop conditions operate at 80-95%)
    let search_range = height * 19 / 20;

    // Hash bottom `search_range` rows of A
    let a_start_row = height - search_range;
    let hashes_a: Vec<u64> = (0..search_range)
        .map(|i| hash_row(data_a, a_start_row + i, bpr, byte_start, byte_end))
        .collect();

    // Hash top `search_range` rows of B
    let hashes_b: Vec<u64> = (0..search_range)
        .map(|i| hash_row(data_b, i, bpr, byte_start, byte_end))
        .collect();

    // Reference row: height/6 above the bottom of A, which sits roughly in the middle
    // of the expected overlap zone (overlap ≈ height/3)
    let ref_offset_from_bottom = height / 6;
    let ref_row_in_a = height - ref_offset_from_bottom; // absolute row in A
    let ref_idx_in_hashes_a = ref_row_in_a - a_start_row; // index into hashes_a
    let ref_hash = hashes_a[ref_idx_in_hashes_a];

    // Find candidate overlaps: for each position in B where the reference hash matches,
    // compute the implied overlap.
    //
    // If overlap is K rows, then:
    //   - bottom K rows of A (rows [height-K .. height)) align with top K rows of B (rows [0..K))
    //   - ref_row_in_a (absolute) maps to B row: ref_row_in_a - (height - K) = K - ref_offset_from_bottom
    //
    // So if hashes_b[j] matches, then j = K - ref_offset_from_bottom → K = j + ref_offset_from_bottom
    let mut best_overlap: usize = 0;
    let mut best_run: usize = 0;
    let mut best_matches: usize = 0;
    let mut best_rate: usize = 0; // match rate in permille (matches * 1000 / candidate_k)

    for (j, &h) in hashes_b.iter().enumerate() {
        if h != ref_hash {
            continue;
        }

        let candidate_k = j + ref_offset_from_bottom;
        if candidate_k == 0 || candidate_k > height {
            continue;
        }

        // Verify candidate by scanning ALL overlap rows for matching hashes.
        // Don't break on first mismatch — dynamic elements (cursor, selection border,
        // scrollbar) can cause isolated row differences in otherwise identical content.
        let a_overlap_start = height - candidate_k; // first overlapping row in A

        let mut longest_run: usize = 0;
        let mut current_run: usize = 0;
        let mut total_matches: usize = 0;

        for r in 0..candidate_k {
            let a_abs_row = a_overlap_start + r;
            let b_abs_row = r;

            let ha = if a_abs_row >= a_start_row {
                hashes_a[a_abs_row - a_start_row]
            } else {
                hash_row(data_a, a_abs_row, bpr, byte_start, byte_end)
            };

            let hb = if b_abs_row < search_range {
                hashes_b[b_abs_row]
            } else {
                hash_row(data_b, b_abs_row, bpr, byte_start, byte_end)
            };

            if ha == hb {
                current_run += 1;
                total_matches += 1;
                if current_run > longest_run {
                    longest_run = current_run;
                }
            } else {
                current_run = 0;
            }
        }

        // Score by match rate (not absolute count) to avoid large-overlap bias.
        // A correct overlap of 170 rows with 95% matches should beat a coincidental
        // overlap of 500 rows with 90% matches.
        let rate = if candidate_k > 0 {
            total_matches * 1000 / candidate_k
        } else {
            0
        };
        if rate > best_rate || (rate == best_rate && longest_run > best_run) {
            best_rate = rate;
            best_matches = total_matches;
            best_run = longest_run;
            best_overlap = candidate_k;
        }
    }

    // Accept if enough rows match: either a decent contiguous run or a large
    // fraction of the overlap. Even 8 consecutive exact row-hash matches is
    // unambiguous (8 × 64 = 512 bits of entropy).
    let min_run = 8;
    let min_match_fraction = 4; // at least 1/4 of overlap rows must match
    let min_matches = min_run.max(best_overlap / min_match_fraction);

    if best_matches < min_matches && best_run < min_run {
        eprintln!(
            "Hash matching failed (best run {}, matches {}/{}, need run≥{} or matches≥{}), trying SAD fallback",
            best_run, best_matches, best_overlap, min_run, min_matches
        );
        return find_overlap_sad(data_a, data_b, width, height);
    }

    if best_overlap >= height {
        return height;
    }

    eprintln!(
        "Overlap: {} rows (run: {}, matches: {}/{})",
        best_overlap, best_run, best_matches, best_overlap
    );
    best_overlap
}

/// SAD-based fallback for overlap detection when hash matching fails.
///
/// Used when pixel values aren't byte-identical between frames (e.g., subpixel rendering
/// differences with small selections). Slides a multi-row reference strip from A through
/// B, finding the position with minimum average per-channel SAD.
fn find_overlap_sad(
    data_a: &[u8],
    data_b: &[u8],
    width: usize,
    height: usize,
) -> usize {
    let bpr = width * 4;
    let byte_start = 0;
    let right_margin = width / 20;
    let byte_end = width.saturating_sub(right_margin) * 4;
    if byte_end <= byte_start {
        return 0;
    }

    // Reference strip: 10 rows centered at height/6 above A's bottom
    let strip_rows = 10.min(height / 4);
    if strip_rows == 0 {
        return 0;
    }
    let ref_center = height - height / 6;
    let ref_start = ref_center
        .saturating_sub(strip_rows / 2)
        .min(height - strip_rows);

    let search_range = height * 19 / 20;
    let search_limit = search_range.saturating_sub(strip_rows);

    let mut best_sad = f64::MAX;
    let mut best_pos: usize = 0;

    for candidate in 0..=search_limit {
        let mut sad_sum: u64 = 0;
        let mut pixel_count: u64 = 0;

        for r in 0..strip_rows {
            let a_base = (ref_start + r) * bpr;
            let b_base = (candidate + r) * bpr;

            // Sample every 4th pixel for speed
            let mut offset = byte_start;
            while offset + 3 < byte_end {
                let ai = a_base + offset;
                let bi = b_base + offset;
                if ai + 2 < data_a.len() && bi + 2 < data_b.len() {
                    for c in 0..3 {
                        sad_sum += (data_a[ai + c] as i32 - data_b[bi + c] as i32)
                            .unsigned_abs() as u64;
                    }
                    pixel_count += 1;
                }
                offset += 16; // every 4th pixel
            }
        }

        if pixel_count > 0 {
            let avg = sad_sum as f64 / (pixel_count as f64 * 3.0);
            if avg < best_sad {
                best_sad = avg;
                best_pos = candidate;
            }
        }
    }

    if best_sad > 15.0 {
        eprintln!(
            "SAD fallback: no match (best avg SAD {:.1} at pos {})",
            best_sad, best_pos
        );
        return 0;
    }

    // Convert position to overlap:
    // Row ref_start in A aligns with row best_pos in B.
    // If overlap is K, row ref_start = height - K + best_pos → K = height - ref_start + best_pos
    let overlap = height - ref_start + best_pos;

    if overlap >= height {
        return height;
    }

    eprintln!(
        "SAD fallback: overlap {} rows (avg SAD {:.1} at B row {})",
        overlap, best_sad, best_pos
    );
    overlap
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
