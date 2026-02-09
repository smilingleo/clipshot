mod app;
mod statusbar;
mod hotkey;
mod capture;
mod overlay;
mod toolbar;
mod annotation;
mod actions;
mod encoder;
mod recording;

use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate};
use objc2_foundation::MainThreadMarker;

fn main() {
    let mtm = MainThreadMarker::new().expect("must run on main thread");

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let delegate = app::AppDelegate::new(mtm);
    let delegate_proto: &ProtocolObject<dyn NSApplicationDelegate> =
        ProtocolObject::from_ref(&*delegate);
    app.setDelegate(Some(delegate_proto));

    app.run();
}
