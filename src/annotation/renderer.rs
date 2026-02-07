use std::ffi::CString;

use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGContext;

use super::model::Annotation;

pub fn draw_annotation(ctx: &CGContext, ann: &Annotation) {
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

#[allow(deprecated)]
fn draw_text(
    ctx: &CGContext,
    position: CGPoint,
    text: &str,
    color: (CGFloat, CGFloat, CGFloat),
    font_size: CGFloat,
) {
    CGContext::save_g_state(Some(ctx));
    CGContext::set_rgb_fill_color(Some(ctx), color.0, color.1, color.2, 1.0);

    let font_name = CString::new("Helvetica").unwrap();
    unsafe {
        CGContext::select_font(
            Some(ctx),
            font_name.as_ptr(),
            font_size,
            objc2_core_graphics::CGTextEncoding::EncodingMacRoman,
        );
    }

    // Flip the text matrix for our flipped coordinate system
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

    let c_text = CString::new(text).unwrap_or_else(|_| CString::new("?").unwrap());
    let len = c_text.as_bytes().len();
    unsafe {
        CGContext::show_text_at_point(
            Some(ctx),
            position.x,
            position.y + font_size,
            c_text.as_ptr(),
            len,
        );
    }
    CGContext::restore_g_state(Some(ctx));
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
