use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};

/// Padding added to bounding rects for hit-testing tolerance.
const HIT_TEST_PADDING: CGFloat = 4.0;

#[derive(Clone)]
pub enum Annotation {
    Arrow {
        start: CGPoint,
        end: CGPoint,
        color: (CGFloat, CGFloat, CGFloat),
        width: CGFloat,
    },
    Rect {
        origin: CGPoint,
        size: CGSize,
        color: (CGFloat, CGFloat, CGFloat),
        width: CGFloat,
    },
    Ellipse {
        origin: CGPoint,
        size: CGSize,
        color: (CGFloat, CGFloat, CGFloat),
        width: CGFloat,
    },
    Pencil {
        points: Vec<CGPoint>,
        color: (CGFloat, CGFloat, CGFloat),
        width: CGFloat,
    },
    Text {
        position: CGPoint,
        text: String,
        color: (CGFloat, CGFloat, CGFloat),
        font_size: CGFloat,
    },
}

impl Annotation {
    /// Compute the bounding rectangle of this annotation.
    pub fn bounding_rect(&self) -> CGRect {
        match self {
            Annotation::Arrow { start, end, width, .. } => {
                let min_x = start.x.min(end.x);
                let min_y = start.y.min(end.y);
                let max_x = start.x.max(end.x);
                let max_y = start.y.max(end.y);
                inflate_rect(CGRect::new(
                    CGPoint::new(min_x, min_y),
                    CGSize::new(max_x - min_x, max_y - min_y),
                ), *width)
            }
            Annotation::Rect { origin, size, width, .. } => {
                inflate_rect(normalize_annotation_rect(*origin, *size), *width)
            }
            Annotation::Ellipse { origin, size, width, .. } => {
                inflate_rect(normalize_annotation_rect(*origin, *size), *width)
            }
            Annotation::Pencil { points, width, .. } => {
                if points.is_empty() {
                    return CGRect::new(CGPoint::ZERO, CGSize::ZERO);
                }
                let mut min_x = points[0].x;
                let mut min_y = points[0].y;
                let mut max_x = points[0].x;
                let mut max_y = points[0].y;
                for p in &points[1..] {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                inflate_rect(CGRect::new(
                    CGPoint::new(min_x, min_y),
                    CGSize::new(max_x - min_x, max_y - min_y),
                ), *width)
            }
            Annotation::Text { position, text, font_size, .. } => {
                // Estimate text bounding box: ~0.6 * font_size per character width
                let char_width = *font_size * 0.6;
                let estimated_width = char_width * text.len() as CGFloat;
                let estimated_height = *font_size * 1.2;
                CGRect::new(
                    *position,
                    CGSize::new(estimated_width, estimated_height),
                )
            }
        }
    }

    /// Test whether a point falls within this annotation's bounding rect (with padding).
    pub fn hit_test(&self, point: CGPoint) -> bool {
        let rect = inflate_rect(self.bounding_rect(), HIT_TEST_PADDING);
        point.x >= rect.origin.x
            && point.x <= rect.origin.x + rect.size.width
            && point.y >= rect.origin.y
            && point.y <= rect.origin.y + rect.size.height
    }
}

/// Update an in-progress annotation with a new mouse position.
pub fn update_annotation(ann: &mut Annotation, point: CGPoint) {
    match ann {
        Annotation::Arrow { end, .. } => {
            *end = point;
        }
        Annotation::Rect {
            origin, size, ..
        } => {
            size.width = point.x - origin.x;
            size.height = point.y - origin.y;
        }
        Annotation::Ellipse {
            origin, size, ..
        } => {
            size.width = point.x - origin.x;
            size.height = point.y - origin.y;
        }
        Annotation::Pencil { points, .. } => {
            points.push(point);
        }
        Annotation::Text { .. } => {}
    }
}

/// Normalize a rect that may have negative width/height.
fn normalize_annotation_rect(origin: CGPoint, size: CGSize) -> CGRect {
    CGRect::new(
        CGPoint::new(
            if size.width < 0.0 { origin.x + size.width } else { origin.x },
            if size.height < 0.0 { origin.y + size.height } else { origin.y },
        ),
        CGSize::new(size.width.abs(), size.height.abs()),
    )
}

/// Inflate a rect by a given amount on all sides.
fn inflate_rect(rect: CGRect, amount: CGFloat) -> CGRect {
    CGRect::new(
        CGPoint::new(rect.origin.x - amount, rect.origin.y - amount),
        CGSize::new(rect.size.width + amount * 2.0, rect.size.height + amount * 2.0),
    )
}
