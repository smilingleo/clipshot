use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};

/// Padding added to bounding rects for hit-testing tolerance.
const HIT_TEST_PADDING: CGFloat = 4.0;
/// Tolerance for hitting a resize handle.
const HANDLE_HIT_TOLERANCE: CGFloat = 6.0;

/// Identifies a specific resize handle on an annotation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandleKind {
    /// Arrow start control point.
    ArrowStart,
    /// Arrow end control point.
    ArrowEnd,
    /// Rect/Ellipse corners and edges.
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

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
    Highlight {
        origin: CGPoint,
        size: CGSize,
        color: (CGFloat, CGFloat, CGFloat),
        opacity: CGFloat,
    },
    Step {
        center: CGPoint,
        number: u32,
        color: (CGFloat, CGFloat, CGFloat),
        radius: CGFloat,
    },
    Blur {
        origin: CGPoint,
        size: CGSize,
        block_size: usize,
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
            Annotation::Highlight { origin, size, .. } => {
                normalize_annotation_rect(*origin, *size)
            }
            Annotation::Step { center, radius, .. } => {
                CGRect::new(
                    CGPoint::new(center.x - radius, center.y - radius),
                    CGSize::new(radius * 2.0, radius * 2.0),
                )
            }
            Annotation::Blur { origin, size, .. } => {
                normalize_annotation_rect(*origin, *size)
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

    /// Translate the annotation by (dx, dy).
    pub fn translate(&mut self, dx: CGFloat, dy: CGFloat) {
        match self {
            Annotation::Arrow { start, end, .. } => {
                start.x += dx;
                start.y += dy;
                end.x += dx;
                end.y += dy;
            }
            Annotation::Rect { origin, .. }
            | Annotation::Ellipse { origin, .. }
            | Annotation::Highlight { origin, .. } => {
                origin.x += dx;
                origin.y += dy;
            }
            Annotation::Pencil { points, .. } => {
                for p in points.iter_mut() {
                    p.x += dx;
                    p.y += dy;
                }
            }
            Annotation::Text { position, .. } => {
                position.x += dx;
                position.y += dy;
            }
            Annotation::Step { center, .. } => {
                center.x += dx;
                center.y += dy;
            }
            Annotation::Blur { origin, .. } => {
                origin.x += dx;
                origin.y += dy;
            }
        }
    }

    /// Return the resize handle positions for this annotation.
    /// Returns an empty vec for types that don't support resizing (Pencil, Text).
    pub fn resize_handles(&self) -> Vec<(HandleKind, CGPoint)> {
        match self {
            Annotation::Arrow { start, end, .. } => {
                vec![
                    (HandleKind::ArrowStart, *start),
                    (HandleKind::ArrowEnd, *end),
                ]
            }
            Annotation::Rect { origin, size, .. }
            | Annotation::Ellipse { origin, size, .. }
            | Annotation::Highlight { origin, size, .. }
            | Annotation::Blur { origin, size, .. } => {
                let r = normalize_annotation_rect(*origin, *size);
                rect_handles(r)
            }
            _ => vec![],
        }
    }

    /// Hit-test against this annotation's resize handles.
    /// Returns the handle kind if a handle is within tolerance of the point.
    pub fn hit_test_handle(&self, point: CGPoint) -> Option<HandleKind> {
        for (kind, hp) in self.resize_handles() {
            if (point.x - hp.x).abs() <= HANDLE_HIT_TOLERANCE
                && (point.y - hp.y).abs() <= HANDLE_HIT_TOLERANCE
            {
                return Some(kind);
            }
        }
        None
    }

    /// Apply a resize operation by moving a specific handle to a new point.
    pub fn apply_resize(&mut self, handle: HandleKind, point: CGPoint) {
        match self {
            Annotation::Arrow { start, end, .. } => match handle {
                HandleKind::ArrowStart => *start = point,
                HandleKind::ArrowEnd => *end = point,
                _ => {}
            },
            Annotation::Rect { origin, size, .. }
            | Annotation::Ellipse { origin, size, .. }
            | Annotation::Highlight { origin, size, .. }
            | Annotation::Blur { origin, size, .. } => {
                let r = normalize_annotation_rect(*origin, *size);
                let new_r = apply_rect_resize(r, handle, point);
                *origin = new_r.origin;
                *size = new_r.size;
            }
            _ => {}
        }
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
        Annotation::Highlight {
            origin, size, ..
        } => {
            size.width = point.x - origin.x;
            size.height = point.y - origin.y;
        }
        Annotation::Pencil { points, .. } => {
            points.push(point);
        }
        Annotation::Text { .. } => {}
        Annotation::Step { .. } => {}
        Annotation::Blur {
            origin, size, ..
        } => {
            size.width = point.x - origin.x;
            size.height = point.y - origin.y;
        }
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

/// Return 8 resize handles (4 corners + 4 edges) for a normalized rect.
fn rect_handles(r: CGRect) -> Vec<(HandleKind, CGPoint)> {
    let (x, y, w, h) = (r.origin.x, r.origin.y, r.size.width, r.size.height);
    vec![
        (HandleKind::TopLeft, CGPoint::new(x, y)),
        (HandleKind::Top, CGPoint::new(x + w / 2.0, y)),
        (HandleKind::TopRight, CGPoint::new(x + w, y)),
        (HandleKind::Left, CGPoint::new(x, y + h / 2.0)),
        (HandleKind::Right, CGPoint::new(x + w, y + h / 2.0)),
        (HandleKind::BottomLeft, CGPoint::new(x, y + h)),
        (HandleKind::Bottom, CGPoint::new(x + w / 2.0, y + h)),
        (HandleKind::BottomRight, CGPoint::new(x + w, y + h)),
    ]
}

/// Apply a rect resize by moving a specific handle to a new point.
fn apply_rect_resize(r: CGRect, handle: HandleKind, point: CGPoint) -> CGRect {
    let (x, y, w, h) = (r.origin.x, r.origin.y, r.size.width, r.size.height);
    let (nx, ny, nw, nh) = match handle {
        HandleKind::TopLeft => (point.x, point.y, x + w - point.x, y + h - point.y),
        HandleKind::Top => (x, point.y, w, y + h - point.y),
        HandleKind::TopRight => (x, point.y, point.x - x, y + h - point.y),
        HandleKind::Left => (point.x, y, x + w - point.x, h),
        HandleKind::Right => (x, y, point.x - x, h),
        HandleKind::BottomLeft => (point.x, y, x + w - point.x, point.y - y),
        HandleKind::Bottom => (x, y, w, point.y - y),
        HandleKind::BottomRight => (x, y, point.x - x, point.y - y),
        _ => (x, y, w, h),
    };
    CGRect::new(CGPoint::new(nx, ny), CGSize::new(nw, nh))
}

/// Inflate a rect by a given amount on all sides.
fn inflate_rect(rect: CGRect, amount: CGFloat) -> CGRect {
    CGRect::new(
        CGPoint::new(rect.origin.x - amount, rect.origin.y - amount),
        CGSize::new(rect.size.width + amount * 2.0, rect.size.height + amount * 2.0),
    )
}
