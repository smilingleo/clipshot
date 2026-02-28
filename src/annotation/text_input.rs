use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSFont, NSGraphicsContext, NSTextView, NSView};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSRect, NSString};

const MIN_WIDTH: CGFloat = 80.0;
const PADDING: CGFloat = 4.0;

// --- TextContainerView: NSView with dotted border ---

pub struct TextContainerViewIvars;

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "TextContainerView"]
    #[ivars = TextContainerViewIvars]
    pub struct TextContainerView;

    impl TextContainerView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty_rect: NSRect) {
            let Some(context) = NSGraphicsContext::currentContext() else {
                return;
            };
            let cg = context.CGContext();
            let bounds = self.bounds();

            objc2_core_graphics::CGContext::save_g_state(Some(&cg));
            objc2_core_graphics::CGContext::set_rgb_stroke_color(Some(&cg), 0.6, 0.6, 0.6, 1.0);
            objc2_core_graphics::CGContext::set_line_width(Some(&cg), 1.0);
            let dash_lengths: [CGFloat; 2] = [3.0, 3.0];
            unsafe {
                objc2_core_graphics::CGContext::set_line_dash(
                    Some(&cg),
                    0.0,
                    dash_lengths.as_ptr(),
                    dash_lengths.len(),
                );
            }
            objc2_core_graphics::CGContext::stroke_rect(Some(&cg), bounds);
            objc2_core_graphics::CGContext::restore_g_state(Some(&cg));
        }
    }
);

// --- AnnotationTextView: NSTextView subclass with Enter-to-commit ---

pub struct AnnotationTextViewIvars;

define_class!(
    #[unsafe(super(NSTextView))]
    #[thread_kind = MainThreadOnly]
    #[name = "AnnotationTextView"]
    #[ivars = AnnotationTextViewIvars]
    pub struct AnnotationTextView;

    impl AnnotationTextView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let key_code = event.keyCode();
            let flags = event.modifierFlags();

            if key_code == 36 {
                // Return key
                if flags.contains(objc2_app_kit::NSEventModifierFlags::Shift) {
                    // Shift+Enter: let NSTextView insert newline
                    let _: () = unsafe { msg_send![super(self), keyDown: event] };
                } else {
                    // Enter: commit text input
                    unsafe {
                        if let Some(container) = self.superview() {
                            if let Some(parent_view) = container.superview() {
                                let _: () =
                                    msg_send![&*parent_view, commitTextInput];
                            }
                        }
                    }
                }
                return;
            }

            if key_code == 53 {
                // Escape: cancel text input
                unsafe {
                    if let Some(container) = self.superview() {
                        if let Some(parent_view) = container.superview() {
                            let _: () =
                                msg_send![&*parent_view, cancelTextInput];
                        }
                    }
                }
                return;
            }

            // All other keys: let NSTextView handle normally
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        #[unsafe(method(didChangeText))]
        fn did_change_text(&self) {
            let _: () = unsafe { msg_send![super(self), didChangeText] };
            self.resize_to_fit();
        }
    }
);

impl AnnotationTextView {
    fn resize_to_fit(&self) {
        let text: Retained<NSString> = unsafe { msg_send![self, string] };
        let font: *mut AnyObject = unsafe { msg_send![self, font] };

        let measured = measure_text_size(&text.to_string(), font);

        let new_width = (measured.width + PADDING * 2.0).max(MIN_WIDTH + PADDING * 2.0);
        let new_height = measured.height.max(18.0) + PADDING * 2.0;

        // Resize text view
        self.setFrame(CGRect::new(
            CGPoint::new(PADDING, PADDING),
            CGSize::new(new_width - PADDING * 2.0, new_height - PADDING * 2.0),
        ));

        // Resize container (superview)
        unsafe {
            if let Some(container) = self.superview() {
                let origin = container.frame().origin;
                container.setFrame(CGRect::new(
                    origin,
                    CGSize::new(new_width, new_height),
                ));
                container.setNeedsDisplay(true);
            }
        }
    }
}

fn measure_text_size(text: &str, font: *mut AnyObject) -> CGSize {
    if text.is_empty() {
        return CGSize::new(MIN_WIDTH, 18.0);
    }

    unsafe {
        let ns_str = NSString::from_str(text);
        let font_attr_key = NSString::from_str("NSFont");
        let dict: *mut AnyObject = msg_send![
            objc2::class!(NSDictionary),
            dictionaryWithObject: font,
            forKey: &*font_attr_key
        ];

        // Use unconstrained width so text only breaks on explicit newlines
        let max_size = CGSize::new(1.0e7, 1.0e7);
        // NSStringDrawingUsesLineFragmentOrigin (1) | NSStringDrawingUsesFontLeading (2)
        let options: usize = 3;
        let context: *mut AnyObject = std::ptr::null_mut();
        let result: CGRect = msg_send![
            &*ns_str,
            boundingRectWithSize: max_size,
            options: options,
            attributes: dict,
            context: context
        ];

        CGSize::new(result.size.width.ceil(), result.size.height.ceil())
    }
}

/// Create a text input with dotted-border container and transparent NSTextView.
/// Returns (container, text_view) both as `Retained<NSView>`.
///
/// The parent view (superview of container) MUST implement Obj-C methods
/// `commitTextInput` and `cancelTextInput` for Enter/Escape handling.
pub fn create_text_input(
    mtm: MainThreadMarker,
    position: CGPoint,
    initial_text: &str,
    font_size: CGFloat,
    color: (CGFloat, CGFloat, CGFloat),
) -> (Retained<NSView>, Retained<NSView>) {
    let font = NSFont::systemFontOfSize(font_size);
    let font_ptr: *mut AnyObject = unsafe { std::mem::transmute_copy(&font) };

    let initial_size = if initial_text.is_empty() {
        CGSize::new(MIN_WIDTH + PADDING * 2.0, font_size * 1.5 + PADDING * 2.0)
    } else {
        let m = measure_text_size(initial_text, font_ptr);
        CGSize::new(
            (m.width + PADDING * 2.0).max(MIN_WIDTH + PADDING * 2.0),
            (m.height + PADDING * 2.0).max(font_size * 1.5 + PADDING * 2.0),
        )
    };

    let container_frame = CGRect::new(position, initial_size);

    // Create container view (dotted border)
    let container_alloc = mtm.alloc().set_ivars(TextContainerViewIvars);
    let container: Retained<TextContainerView> =
        unsafe { msg_send![super(container_alloc), initWithFrame: container_frame] };
    let container: Retained<NSView> = Retained::into_super(container);

    // Create AnnotationTextView (NSTextView subclass)
    let inner_frame = CGRect::new(
        CGPoint::new(PADDING, PADDING),
        CGSize::new(
            container_frame.size.width - PADDING * 2.0,
            container_frame.size.height - PADDING * 2.0,
        ),
    );

    let tv_alloc = mtm.alloc().set_ivars(AnnotationTextViewIvars);
    let text_view: Retained<AnnotationTextView> =
        unsafe { msg_send![super(tv_alloc), initWithFrame: inner_frame] };

    // Configure NSTextView properties
    let _: () = unsafe { msg_send![&*text_view, setDrawsBackground: false] };
    let _: () = unsafe { msg_send![&*text_view, setRichText: false] };
    let _: () = unsafe { msg_send![&*text_view, setFont: &*font] };
    let _: () = unsafe { msg_send![&*text_view, setAllowsUndo: true] };

    // Set text color
    let ns_color: *mut AnyObject = unsafe {
        msg_send![
            objc2::class!(NSColor),
            colorWithRed: color.0,
            green: color.1,
            blue: color.2,
            alpha: 1.0 as CGFloat
        ]
    };
    let _: () = unsafe { msg_send![&*text_view, setTextColor: ns_color] };

    // Disable word wrapping: text only breaks on explicit newlines (Shift+Enter)
    let _: () = unsafe { msg_send![&*text_view, setHorizontallyResizable: true] };
    let big = CGSize::new(1.0e7, 1.0e7);
    let _: () = unsafe { msg_send![&*text_view, setMaxSize: big] };
    let tc: *mut AnyObject = unsafe { msg_send![&*text_view, textContainer] };
    if !tc.is_null() {
        let _: () = unsafe { msg_send![tc, setContainerSize: big] };
        let _: () = unsafe { msg_send![tc, setWidthTracksTextView: false] };
    }

    // Set initial text
    if !initial_text.is_empty() {
        let ns_text = NSString::from_str(initial_text);
        let _: () = unsafe { msg_send![&*text_view, setString: &*ns_text] };
    }

    // Convert AnnotationTextView -> NSTextView -> NSText -> NSView
    let tv_nsview: Retained<NSView> =
        Retained::into_super(Retained::into_super(Retained::into_super(text_view)));
    container.addSubview(&tv_nsview);

    (container, tv_nsview)
}

/// Commit the text input: extract text, remove container from superview.
/// Returns the text if non-empty.
pub fn commit_text_input(container: &NSView, text_view: &NSView) -> Option<String> {
    let text: Retained<NSString> = unsafe { msg_send![text_view, string] };
    let s = text.to_string();
    container.removeFromSuperview();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
