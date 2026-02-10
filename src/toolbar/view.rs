use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSButton, NSEvent, NSFont, NSView};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_foundation::{MainThreadMarker, NSRect, NSString};

const BUTTON_W: CGFloat = 28.0;
const BUTTON_H: CGFloat = 24.0;
const BUTTON_SPACING: CGFloat = 2.0;
const TOOLBAR_PADDING: CGFloat = 4.0;

const TOOL_BUTTONS: &[(&str, &str, &str)] = &[
    ("\u{2196}", "toolSelect:",  "Select"),
    ("\u{2192}", "toolArrow:",   "Arrow"),
    ("\u{25A1}", "toolRect:",    "Rectangle"),
    ("\u{25CB}", "toolEllipse:", "Ellipse"),
    ("\u{270E}", "toolPencil:",  "Pencil"),
    ("T",        "toolText:",    "Text"),
];

const COLOR_BUTTONS: &[(&str, &str, &str)] = &[
    ("R", "colorRed:",    "Red"),
    ("B", "colorBlue:",   "Blue"),
    ("G", "colorGreen:",  "Green"),
    ("Y", "colorYellow:", "Yellow"),
];

const PLAYBACK_BUTTONS: &[(&str, &str, &str)] = &[
    ("\u{25C0}", "editorReverse:",   "Reverse Play"),
    ("\u{25B6}", "editorPlayPause:", "Play / Pause (Space)"),
];

const ACTION_BUTTONS: &[(&str, &str, &str)] = &[
    ("\u{21A9}", "actionUndo:",    "Undo (Cmd+Z)"),
    ("\u{2715}", "actionCancel:",  "Cancel (Esc)"),
    ("S",        "actionSave:",    "Save to File"),
    ("\u{2713}", "actionConfirm:", "Confirm"),
];

pub struct ToolbarViewIvars {
    /// All buttons except Confirm, for enabling/disabling.
    non_confirm_buttons: RefCell<Vec<Retained<NSButton>>>,
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
        // +3 for the spacing gaps between groups (tools|colors|playback|actions)
        let total_buttons = TOOL_BUTTONS.len()
            + COLOR_BUTTONS.len()
            + PLAYBACK_BUTTONS.len()
            + ACTION_BUTTONS.len()
            + 3;
        let width =
            TOOLBAR_PADDING * 2.0 + total_buttons as CGFloat * (BUTTON_W + BUTTON_SPACING);
        let height = TOOLBAR_PADDING * 2.0 + BUTTON_H;

        let frame = NSRect::new(CGPoint::ZERO, CGSize::new(width, height));
        let this = mtm.alloc().set_ivars(ToolbarViewIvars {
            non_confirm_buttons: RefCell::new(Vec::new()),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };

        let mut x = TOOLBAR_PADDING;
        let mut non_confirm = Vec::new();

        for (label, sel_name, tooltip) in TOOL_BUTTONS {
            let btn = create_button(mtm, label, sel_name, tooltip, x, TOOLBAR_PADDING);
            view.addSubview(&btn);
            non_confirm.push(btn);
            x += BUTTON_W + BUTTON_SPACING;
        }

        x += BUTTON_SPACING * 2.0;

        for (label, sel_name, tooltip) in COLOR_BUTTONS {
            let btn = create_button(mtm, label, sel_name, tooltip, x, TOOLBAR_PADDING);
            view.addSubview(&btn);
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
