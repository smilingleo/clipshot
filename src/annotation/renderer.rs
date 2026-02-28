use std::ffi::CString;

use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGContext, CGImage};

use super::model::Annotation;

/// Draw an annotation onto a CGContext.
/// `screenshot` is required for Blur annotations to sample underlying pixels.
/// For non-Blur annotations, it is ignored.
pub fn draw_annotation(ctx: &CGContext, ann: &Annotation, screenshot: Option<&CGImage>) {
    match ann {
        Annotation::Arrow {
            start,
            end,
            color,
            width,
        } => draw_arrow(ctx, *start, *end, *color, *width),
        Annotation::Rect {
            origin,
            size,
            color,
            width,
        } => draw_rect(ctx, *origin, *size, *color, *width),
        Annotation::Ellipse {
            origin,
            size,
            color,
            width,
        } => draw_ellipse(ctx, *origin, *size, *color, *width),
        Annotation::Pencil {
            points,
            color,
            width,
        } => draw_pencil(ctx, points, *color, *width),
        Annotation::Text {
            position,
            text,
            color,
            font_size,
        } => draw_text(ctx, *position, text, *color, *font_size),
        Annotation::Highlight {
            origin,
            size,
            color,
            opacity,
        } => draw_highlight(ctx, *origin, *size, *color, *opacity),
        Annotation::Step {
            center,
            number,
            color,
            radius,
        } => draw_step(ctx, *center, *number, *color, *radius),
        Annotation::Blur {
            origin,
            size,
            block_size,
        } => draw_blur(ctx, *origin, *size, *block_size, screenshot),
    }
}

fn draw_arrow(
    ctx: &CGContext,
    start: CGPoint,
    end: CGPoint,
    color: (CGFloat, CGFloat, CGFloat),
    width: CGFloat,
) {
    CGContext::save_g_state(Some(ctx));
    CGContext::set_rgb_stroke_color(Some(ctx), color.0, color.1, color.2, 1.0);
    CGContext::set_line_width(Some(ctx), width);
    CGContext::set_line_cap(Some(ctx), objc2_core_graphics::CGLineCap::Round);

    CGContext::move_to_point(Some(ctx), start.x, start.y);
    CGContext::add_line_to_point(Some(ctx), end.x, end.y);
    CGContext::stroke_path(Some(ctx));

    // Arrowhead
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let len = (dx * dx + dy * dy).sqrt();
    if len > 1.0 {
        let arrow_len = 12.0_f64.min(len * 0.3);
        let arrow_angle = 0.4;
        let angle = dy.atan2(dx);

        let p1 = CGPoint::new(
            end.x - arrow_len * (angle - arrow_angle).cos(),
            end.y - arrow_len * (angle - arrow_angle).sin(),
        );
        let p2 = CGPoint::new(
            end.x - arrow_len * (angle + arrow_angle).cos(),
            end.y - arrow_len * (angle + arrow_angle).sin(),
        );

        CGContext::set_rgb_fill_color(Some(ctx), color.0, color.1, color.2, 1.0);
        CGContext::move_to_point(Some(ctx), end.x, end.y);
        CGContext::add_line_to_point(Some(ctx), p1.x, p1.y);
        CGContext::add_line_to_point(Some(ctx), p2.x, p2.y);
        CGContext::close_path(Some(ctx));
        CGContext::fill_path(Some(ctx));
    }

    CGContext::restore_g_state(Some(ctx));
}

fn draw_rect(
    ctx: &CGContext,
    origin: CGPoint,
    size: CGSize,
    color: (CGFloat, CGFloat, CGFloat),
    width: CGFloat,
) {
    let norm = normalize_rect(CGRect::new(origin, size));
    CGContext::save_g_state(Some(ctx));
    CGContext::set_rgb_stroke_color(Some(ctx), color.0, color.1, color.2, 1.0);
    CGContext::set_line_width(Some(ctx), width);
    CGContext::stroke_rect(Some(ctx), norm);
    CGContext::restore_g_state(Some(ctx));
}

fn draw_ellipse(
    ctx: &CGContext,
    origin: CGPoint,
    size: CGSize,
    color: (CGFloat, CGFloat, CGFloat),
    width: CGFloat,
) {
    let norm = normalize_rect(CGRect::new(origin, size));
    CGContext::save_g_state(Some(ctx));
    CGContext::set_rgb_stroke_color(Some(ctx), color.0, color.1, color.2, 1.0);
    CGContext::set_line_width(Some(ctx), width);
    CGContext::stroke_ellipse_in_rect(Some(ctx), norm);
    CGContext::restore_g_state(Some(ctx));
}

fn draw_pencil(
    ctx: &CGContext,
    points: &[CGPoint],
    color: (CGFloat, CGFloat, CGFloat),
    width: CGFloat,
) {
    if points.len() < 2 {
        return;
    }

    CGContext::save_g_state(Some(ctx));
    CGContext::set_rgb_stroke_color(Some(ctx), color.0, color.1, color.2, 1.0);
    CGContext::set_line_width(Some(ctx), width);
    CGContext::set_line_cap(Some(ctx), objc2_core_graphics::CGLineCap::Round);
    CGContext::set_line_join(Some(ctx), objc2_core_graphics::CGLineJoin::Round);

    CGContext::move_to_point(Some(ctx), points[0].x, points[0].y);
    for p in &points[1..] {
        CGContext::add_line_to_point(Some(ctx), p.x, p.y);
    }
    CGContext::stroke_path(Some(ctx));
    CGContext::restore_g_state(Some(ctx));
}

fn draw_text(
    ctx: &CGContext,
    position: CGPoint,
    text: &str,
    color: (CGFloat, CGFloat, CGFloat),
    font_size: CGFloat,
) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSString;

    CGContext::save_g_state(Some(ctx));

    unsafe {
        // Wrap the CGContext in an NSGraphicsContext so NSString drawing works
        // (needed for export paths that don't have a current NSGraphicsContext)
        let prev_ctx: *mut AnyObject =
            msg_send![objc2::class!(NSGraphicsContext), currentContext];
        let ns_ctx: *mut AnyObject = msg_send![
            objc2::class!(NSGraphicsContext),
            graphicsContextWithCGContext: ctx,
            flipped: true
        ];
        let _: () = msg_send![
            objc2::class!(NSGraphicsContext),
            setCurrentContext: ns_ctx
        ];

        // Create font and color
        let font: *mut AnyObject =
            msg_send![objc2::class!(NSFont), systemFontOfSize: font_size];
        let ns_color: *mut AnyObject = msg_send![
            objc2::class!(NSColor),
            colorWithRed: color.0,
            green: color.1,
            blue: color.2,
            alpha: 1.0 as CGFloat
        ];

        // Build attributes dictionary
        let font_key = NSString::from_str("NSFont");
        let color_key = NSString::from_str("NSColor");
        let keys: [*const AnyObject; 2] =
            [&*font_key as *const _ as *const _, &*color_key as *const _ as *const _];
        let vals: [*const AnyObject; 2] = [font as *const _, ns_color as *const _];
        let dict: *mut AnyObject = msg_send![
            objc2::class!(NSDictionary),
            dictionaryWithObjects: vals.as_ptr(),
            forKeys: keys.as_ptr(),
            count: 2usize
        ];

        let line_height = font_size * 1.3;
        for (i, line) in text.split('\n').enumerate() {
            let ns_line = NSString::from_str(line);
            let point = CGPoint::new(
                position.x,
                position.y + (i as CGFloat) * line_height,
            );
            let _: () = msg_send![
                &*ns_line,
                drawAtPoint: point,
                withAttributes: dict
            ];
        }

        // Restore previous NSGraphicsContext
        let _: () = msg_send![
            objc2::class!(NSGraphicsContext),
            setCurrentContext: prev_ctx
        ];
    }

    CGContext::restore_g_state(Some(ctx));
}

fn draw_step(
    ctx: &CGContext,
    center: CGPoint,
    number: u32,
    color: (CGFloat, CGFloat, CGFloat),
    radius: CGFloat,
) {
    CGContext::save_g_state(Some(ctx));

    // Fill circle
    let circle_rect = CGRect::new(
        CGPoint::new(center.x - radius, center.y - radius),
        CGSize::new(radius * 2.0, radius * 2.0),
    );
    CGContext::set_rgb_fill_color(Some(ctx), color.0, color.1, color.2, 1.0);
    CGContext::fill_ellipse_in_rect(Some(ctx), circle_rect);

    // Draw white number centered in circle
    CGContext::set_rgb_fill_color(Some(ctx), 1.0, 1.0, 1.0, 1.0);
    let num_str = format!("{}", number);
    let font_size = if number >= 10 { radius * 0.9 } else { radius * 1.2 };

    let font_name = CString::new("Helvetica-Bold").unwrap();
    #[allow(deprecated)]
    unsafe {
        CGContext::select_font(
            Some(ctx),
            font_name.as_ptr(),
            font_size,
            objc2_core_graphics::CGTextEncoding::EncodingMacRoman,
        );
    }

    // Flip text matrix for our flipped coordinate system
    CGContext::set_text_matrix(
        Some(ctx),
        CGAffineTransform {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            tx: 0.0,
            ty: 0.0,
        },
    );

    // Estimate text width for centering (~0.6 * font_size per char)
    let char_width = font_size * 0.6;
    let text_width = char_width * num_str.len() as CGFloat;
    let text_x = center.x - text_width / 2.0;
    let text_y = center.y + font_size * 0.35; // vertical center

    let c_text = CString::new(num_str).unwrap();
    let len = c_text.as_bytes().len();
    #[allow(deprecated)]
    unsafe {
        CGContext::show_text_at_point(Some(ctx), text_x, text_y, c_text.as_ptr(), len);
    }

    CGContext::restore_g_state(Some(ctx));
}

fn draw_highlight(
    ctx: &CGContext,
    origin: CGPoint,
    size: CGSize,
    color: (CGFloat, CGFloat, CGFloat),
    opacity: CGFloat,
) {
    let norm = normalize_rect(CGRect::new(origin, size));
    CGContext::save_g_state(Some(ctx));
    CGContext::set_rgb_fill_color(Some(ctx), color.0, color.1, color.2, opacity);
    CGContext::fill_rect(Some(ctx), norm);
    CGContext::restore_g_state(Some(ctx));
}

fn draw_blur(
    ctx: &CGContext,
    origin: CGPoint,
    size: CGSize,
    block_size: usize,
    screenshot: Option<&CGImage>,
) {
    let norm = normalize_rect(CGRect::new(origin, size));
    if norm.size.width < 1.0 || norm.size.height < 1.0 {
        return;
    }

    let screenshot = match screenshot {
        Some(img) => img,
        None => {
            // No screenshot available — draw a gray placeholder
            CGContext::save_g_state(Some(ctx));
            CGContext::set_rgb_fill_color(Some(ctx), 0.5, 0.5, 0.5, 0.8);
            CGContext::fill_rect(Some(ctx), norm);
            CGContext::restore_g_state(Some(ctx));
            return;
        }
    };

    let img_w = CGImage::width(Some(screenshot)) as CGFloat;
    let img_h = CGImage::height(Some(screenshot)) as CGFloat;
    if img_w < 1.0 || img_h < 1.0 {
        return;
    }

    // Use the current CTM to transform the annotation rect to user-space pixel coords.
    // The CTM maps from annotation coords to the bitmap coords.
    let ctm = CGContext::ctm(Some(ctx));
    // Transform the origin and corner of the blur rect
    let p0 = apply_transform(ctm, norm.origin);
    let p1 = apply_transform(ctm, CGPoint::new(
        norm.origin.x + norm.size.width,
        norm.origin.y + norm.size.height,
    ));
    let px = p0.x.min(p1.x).max(0.0);
    let py = p0.y.min(p1.y).max(0.0);
    let pw = (p0.x.max(p1.x) - px).min(img_w - px);
    let ph = (p0.y.max(p1.y) - py).min(img_h - py);

    if pw < 1.0 || ph < 1.0 {
        return;
    }

    let crop_rect = CGRect::new(CGPoint::new(px, py), CGSize::new(pw, ph));
    let cropped = CGImage::with_image_in_rect(Some(screenshot), crop_rect);
    let cropped = match cropped {
        Some(img) => img,
        None => return,
    };

    let crop_w = CGImage::width(Some(&cropped));
    let crop_h = CGImage::height(Some(&cropped));
    if crop_w == 0 || crop_h == 0 {
        return;
    }

    let rgba = match crate::actions::cgimage_to_rgba(&cropped) {
        Ok(data) => data,
        Err(_) => return,
    };

    // Draw pixelated blocks in view coordinates
    CGContext::save_g_state(Some(ctx));
    let bs = block_size.max(2);
    let blocks_x = ((crop_w + bs - 1) / bs) as CGFloat;
    let blocks_y = ((crop_h + bs - 1) / bs) as CGFloat;
    let view_block_w = norm.size.width / blocks_x;
    let view_block_h = norm.size.height / blocks_y;

    let mut by = 0;
    let mut vy = norm.origin.y;
    while by < crop_h {
        let bh = bs.min(crop_h - by);
        let mut bx = 0;
        let mut vx = norm.origin.x;
        while bx < crop_w {
            let bw = bs.min(crop_w - bx);
            let mut r_sum: u64 = 0;
            let mut g_sum: u64 = 0;
            let mut b_sum: u64 = 0;
            let mut count: u64 = 0;
            for row in by..(by + bh) {
                for col in bx..(bx + bw) {
                    let idx = (row * crop_w + col) * 4;
                    if idx + 2 < rgba.len() {
                        r_sum += rgba[idx] as u64;
                        g_sum += rgba[idx + 1] as u64;
                        b_sum += rgba[idx + 2] as u64;
                        count += 1;
                    }
                }
            }
            if count > 0 {
                let r = r_sum as CGFloat / count as CGFloat / 255.0;
                let g = g_sum as CGFloat / count as CGFloat / 255.0;
                let b = b_sum as CGFloat / count as CGFloat / 255.0;
                CGContext::set_rgb_fill_color(Some(ctx), r, g, b, 1.0);
                CGContext::fill_rect(Some(ctx), CGRect::new(
                    CGPoint::new(vx, vy),
                    CGSize::new(view_block_w, view_block_h),
                ));
            }
            bx += bs;
            vx += view_block_w;
        }
        by += bs;
        vy += view_block_h;
    }

    CGContext::restore_g_state(Some(ctx));
}

fn apply_transform(t: CGAffineTransform, p: CGPoint) -> CGPoint {
    CGPoint::new(
        t.a * p.x + t.c * p.y + t.tx,
        t.b * p.x + t.d * p.y + t.ty,
    )
}

fn normalize_rect(r: CGRect) -> CGRect {
    CGRect::new(
        CGPoint::new(
            if r.size.width < 0.0 { r.origin.x + r.size.width } else { r.origin.x },
            if r.size.height < 0.0 { r.origin.y + r.size.height } else { r.origin.y },
        ),
        CGSize::new(r.size.width.abs(), r.size.height.abs()),
    )
}
