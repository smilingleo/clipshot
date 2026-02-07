use objc2_core_foundation::{CGFloat, CGPoint, CGSize};

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
