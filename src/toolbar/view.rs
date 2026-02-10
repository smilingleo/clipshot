use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSButton, NSColor, NSEvent, NSFont, NSView};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_foundation::{MainThreadMarker, NSRect, NSString};

const BUTTON_W: CGFloat = 28.0;
const BUTTON_H: CGFloat = 24.0;
const BUTTON_SPACING: CGFloat = 2.0;
const TOOLBAR_PADDING: CGFloat = 4.0;
const COLOR_WELL_W: CGFloat = 28.0;

const TOOL_BUTTONS: &[(&str, &str, &str)] = &[
    ("\u{2196}", "toolSelect:",    "Select (S)"),
    ("\u{2192}", "toolArrow:",     "Arrow (A)"),
    ("\u{25A1}", "toolRect:",      "Rectangle (R)"),
    ("\u{25CB}", "toolEllipse:",   "Ellipse (E)"),
    ("\u{270E}", "toolPencil:",    "Pencil (P)"),
    ("T",        "toolText:",      "Text (T)"),
    ("\u{25A8}", "toolHighlight:", "Highlight (H)"),
    ("\u{2460}", "toolStep:",      "Step (N)"),
    ("\u{2591}", "toolBlur:",      "Blur (B)"),
    ("\u{2702}", "toolCrop:",      "Crop (C)"),
];

const STROKE_BUTTONS: &[(&str, &str, &str)] = &[
    ("\u{2500}", "strokeThin:",   "Thin (1)"),
    ("\u{2501}", "strokeMedium:", "Medium (2)"),
    ("\u{2588}", "strokeThick:",  "Thick (3)"),
];

const PLAYBACK_BUTTONS: &[(&str, &str, &str)] = &[
    ("\u{25C0}", "editorReverse:",   "Reverse Play"),
    ("\u{25B6}", "editorPlayPause:", "Play / Pause (Space)"),
];

const ACTION_BUTTONS: &[(&str, &str, &str)] = &[
    ("\u{21A9}", "actionUndo:",    "Undo (Cmd+Z)"),
    ("\u{21AA}", "actionRedo:",    "Redo (Cmd+Shift+Z)"),
    ("\u{2715}", "actionCancel:",  "Cancel (Esc)"),
    ("S",        "actionSave:",    "Save to File"),
    ("\u{2713}", "actionConfirm:", "Confirm"),
];

pub struct ToolbarViewIvars {
    /// All buttons except Confirm, for enabling/disabling.
    non_confirm_buttons: RefCell<Vec<Retained<NSButton>>>,
    /// Tool buttons for active state tracking.
    tool_buttons: RefCell<Vec<Retained<NSButton>>>,
    /// Index of the currently active tool button.
    active_tool_index: Cell<usize>,
    /// Stroke width buttons for active state tracking.
    stroke_buttons: RefCell<Vec<Retained<NSButton>>>,
    /// Index of the currently active stroke button.
    active_stroke_index: Cell<usize>,
    /// Color button for toggling the color picker.
    color_button: RefCell<Option<Retained<NSButton>>>,
    /// Whether the color picker is currently shown.
    color_picker_active: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarView"]
    #[ivars = ToolbarViewIvars]
    pub struct ToolbarView;

    impl ToolbarView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }
    }
);

impl ToolbarView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // tools + 1 (color well) + strokes + playback + actions + 4 gaps
        let total_slots = TOOL_BUTTONS.len()
            + 1 // color well
            + STROKE_BUTTONS.len()
            + PLAYBACK_BUTTONS.len()
            + ACTION_BUTTONS.len()
            + 4;
        let width =
            TOOLBAR_PADDING * 2.0 + total_slots as CGFloat * (BUTTON_W + BUTTON_SPACING);
        let height = TOOLBAR_PADDING * 2.0 + BUTTON_H;

        let frame = NSRect::new(CGPoint::ZERO, CGSize::new(width, height));
        let this = mtm.alloc().set_ivars(ToolbarViewIvars {
            non_confirm_buttons: RefCell::new(Vec::new()),
            tool_buttons: RefCell::new(Vec::new()),
            active_tool_index: Cell::new(0),
            stroke_buttons: RefCell::new(Vec::new()),
            active_stroke_index: Cell::new(1), // Medium is default
            color_button: RefCell::new(None),
            color_picker_active: Cell::new(false),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };

        let mut x = TOOLBAR_PADDING;
        let mut non_confirm = Vec::new();
        let mut tool_btns = Vec::new();

        for (label, sel_name, tooltip) in TOOL_BUTTONS {
            let btn = create_button(mtm, label, sel_name, tooltip, x, TOOLBAR_PADDING);
            view.addSubview(&btn);
            tool_btns.push(btn.clone());
            non_confirm.push(btn);
            x += BUTTON_W + BUTTON_SPACING;
        }

        x += BUTTON_SPACING * 2.0;

        // Color button (toggles the color picker)
        let color_btn = create_button(mtm, "\u{25A0}", "toggleColorPicker:", "Color", x, TOOLBAR_PADDING);
        // Set initial color (red) via attributed title
        set_button_title_color(&color_btn, "\u{25A0}", &NSColor::redColor());
        view.addSubview(&color_btn);
        non_confirm.push(color_btn.clone());
        *view.ivars().color_button.borrow_mut() = Some(color_btn);
        x += COLOR_WELL_W + BUTTON_SPACING;

        x += BUTTON_SPACING * 2.0;

        let mut stroke_btns = Vec::new();
        for (label, sel_name, tooltip) in STROKE_BUTTONS {
            let btn = create_button(mtm, label, sel_name, tooltip, x, TOOLBAR_PADDING);
            view.addSubview(&btn);
            stroke_btns.push(btn.clone());
            non_confirm.push(btn);
            x += BUTTON_W + BUTTON_SPACING;
        }

        x += BUTTON_SPACING * 2.0;

        for (label, sel_name, tooltip) in PLAYBACK_BUTTONS {
            let btn = create_button(mtm, label, sel_name, tooltip, x, TOOLBAR_PADDING);
            view.addSubview(&btn);
            non_confirm.push(btn);
            x += BUTTON_W + BUTTON_SPACING;
        }

        x += BUTTON_SPACING * 2.0;

        for (i, (label, sel_name, tooltip)) in ACTION_BUTTONS.iter().enumerate() {
            let btn = create_button(mtm, label, sel_name, tooltip, x, TOOLBAR_PADDING);
            view.addSubview(&btn);
            // Last button in ACTION_BUTTONS is Confirm — don't add it to non_confirm
            if i < ACTION_BUTTONS.len() - 1 {
                non_confirm.push(btn);
            }
            x += BUTTON_W + BUTTON_SPACING;
        }

        *view.ivars().non_confirm_buttons.borrow_mut() = non_confirm;
        *view.ivars().tool_buttons.borrow_mut() = tool_btns;
        *view.ivars().stroke_buttons.borrow_mut() = stroke_btns;

        // Set initial active tool state (Select = index 0)
        view.set_active_tool(0);
        // Set initial active stroke state (Medium = index 1)
        view.set_active_stroke(1);

        view
    }

    /// Enable or disable all buttons except the Confirm button.
    pub fn set_non_confirm_buttons_enabled(&self, enabled: bool) {
        let buttons = self.ivars().non_confirm_buttons.borrow();
        for btn in buttons.iter() {
            let btn: &NSButton = btn;
            btn.setEnabled(enabled);
        }
    }

    /// Set the active tool button by index. Updates visual state.
    pub fn set_active_tool(&self, index: usize) {
        let tool_buttons = self.ivars().tool_buttons.borrow();
        let prev = self.ivars().active_tool_index.get();
        // Reset previous
        if let Some(btn) = tool_buttons.get(prev) {
            #[allow(deprecated)]
            btn.setBezelStyle(objc2_app_kit::NSBezelStyle::Inline);
        }
        // Highlight current
        if let Some(btn) = tool_buttons.get(index) {
            #[allow(deprecated)]
            btn.setBezelStyle(objc2_app_kit::NSBezelStyle::SmallSquare);
        }
        self.ivars().active_tool_index.set(index);
    }

    /// Set the active stroke button by index. Updates visual state.
    pub fn set_active_stroke(&self, index: usize) {
        let stroke_buttons = self.ivars().stroke_buttons.borrow();
        let prev = self.ivars().active_stroke_index.get();
        if let Some(btn) = stroke_buttons.get(prev) {
            #[allow(deprecated)]
            btn.setBezelStyle(objc2_app_kit::NSBezelStyle::Inline);
        }
        if let Some(btn) = stroke_buttons.get(index) {
            #[allow(deprecated)]
            btn.setBezelStyle(objc2_app_kit::NSBezelStyle::SmallSquare);
        }
        self.ivars().active_stroke_index.set(index);
    }

    /// Set the color button's displayed color programmatically.
    pub fn set_color(&self, r: CGFloat, g: CGFloat, b: CGFloat) {
        if let Some(ref btn) = *self.ivars().color_button.borrow() {
            let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0);
            set_button_title_color(btn, "\u{25A0}", &color);
        }
    }

    /// Get the frame of the color button (for positioning the color panel).
    pub fn color_button_frame(&self) -> Option<objc2_foundation::NSRect> {
        self.ivars().color_button.borrow().as_ref().map(|btn| {
            use objc2_app_kit::NSView;
            NSView::frame(btn)
        })
    }

    /// Set the color button's active/inactive visual state.
    pub fn set_color_picker_active(&self, active: bool) {
        self.ivars().color_picker_active.set(active);
        if let Some(ref btn) = *self.ivars().color_button.borrow() {
            #[allow(deprecated)]
            if active {
                btn.setBezelStyle(objc2_app_kit::NSBezelStyle::SmallSquare);
            } else {
                btn.setBezelStyle(objc2_app_kit::NSBezelStyle::Inline);
            }
        }
    }

    /// Whether the color picker is currently active.
    pub fn is_color_picker_active(&self) -> bool {
        self.ivars().color_picker_active.get()
    }
}

fn create_button(
    mtm: MainThreadMarker,
    label: &str,
    sel_name: &str,
    tooltip: &str,
    x: CGFloat,
    y: CGFloat,
) -> Retained<NSButton> {
    let frame = NSRect::new(CGPoint::new(x, y), CGSize::new(BUTTON_W, BUTTON_H));
    let sel_cstr = std::ffi::CString::new(sel_name).unwrap();
    let button: Retained<NSButton> = unsafe { msg_send![mtm.alloc(), initWithFrame: frame] };
    button.setTitle(&NSString::from_str(label));
    unsafe {
        button.setAction(Some(Sel::register(&sel_cstr)));
        button.setTarget(None);
        button.setToolTip(Some(&NSString::from_str(tooltip)));
    }
    button.setFont(Some(&NSFont::systemFontOfSize(12.0)));
    #[allow(deprecated)]
    button.setBezelStyle(objc2_app_kit::NSBezelStyle::Inline);
    button
}

/// Set a button's title with a specific foreground color using NSAttributedString.
fn set_button_title_color(button: &NSButton, title: &str, color: &NSColor) {
    unsafe {
        let ns_title = NSString::from_str(title);
        let font = NSFont::systemFontOfSize(12.0);

        let fg_key = NSString::from_str("NSColor");
        let font_key = NSString::from_str("NSFont");

        let color_ptr = color as *const NSColor as *const objc2::runtime::AnyObject;
        let font_ptr = &*font as *const NSFont as *const objc2::runtime::AnyObject;
        let fg_key_ptr = &*fg_key as *const NSString as *const objc2::runtime::AnyObject;
        let font_key_ptr = &*font_key as *const NSString as *const objc2::runtime::AnyObject;

        let keys: [*const objc2::runtime::AnyObject; 2] = [fg_key_ptr, font_key_ptr];
        let vals: [*const objc2::runtime::AnyObject; 2] = [color_ptr, font_ptr];

        let dict: *mut objc2::runtime::AnyObject = msg_send![
            objc2::class!(NSDictionary),
            dictionaryWithObjects: vals.as_ptr(),
            forKeys: keys.as_ptr(),
            count: 2usize
        ];

        let attr_str: *mut objc2::runtime::AnyObject = msg_send![
            objc2::class!(NSAttributedString), alloc
        ];
        let attr_str: *mut objc2::runtime::AnyObject = msg_send![
            attr_str,
            initWithString: &*ns_title,
            attributes: dict
        ];

        let _: () = msg_send![button, setAttributedTitle: attr_str];
    }
}
