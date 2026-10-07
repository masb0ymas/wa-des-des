//! Native notifications on macOS, through the modern `UNUserNotificationCenter` API.
//!
//! The default Tauri stack (`tauri-plugin-notification` → `notify-rust` → `mac-notification-sys`)
//! delivers through the deprecated `NSUserNotification` API. That has two defects for this app:
//! the system never asks for authorization, so the app never appears in Notification Settings,
//! and — decisive for a monitor — the system suppresses notifications while the sending app is
//! frontmost, which is exactly when a message for another account matters. The one override,
//! `shouldPresentNotification:`, is not implemented by mac-notification-sys and cannot be
//! retrofitted from outside.
//!
//! This module therefore talks to `UNUserNotificationCenter` directly: it asks for authorization
//! on the first notification (the standard system prompt), registers the app in Notification
//! Settings, and presents banners through the `willPresent` delegate even while WaDesk is the
//! active app. Windows and Linux keep the plugin, which speaks the modern API there already.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use block2::{Block, RcBlock};
use objc2::{define_class, extern_methods};
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use tauri::{AppHandle, Emitter as _};

use crate::MAIN_LABEL;

// Presents banners even while WaDesk is frontmost. Stateless, so it is freely shareable; the
// system calls it on the main thread's run loop.
define_class!(
    // SAFETY: NSObject has no subclassing requirements, and the delegate adds no state.
    #[unsafe(super(NSObject))]
    #[name = "WaDeskNotificationDelegate"]
    struct Delegate;

    impl Delegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            handler: &Block<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            // Banner plus a slot in Notification Center's history. Without this override the
            // system default for a frontmost app is to present nothing.
            handler.call((
                UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,
            ));
        }
    }

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {}
);

// The delegate carries no ivars, so the inherited `+new` fully initializes it.
impl Delegate {
    extern_methods!(
        #[unsafe(method(new))]
        fn new() -> Retained<Self>;
    );
}

// The delegate has no state and only forwards its handler call, so moving it across threads is
// sound. Required because `show` may run on a command or event thread.
unsafe impl Send for Delegate {}
unsafe impl Sync for Delegate {}

/// Keeps the delegate alive for the process; the center holds a weak reference.
static DELEGATE: OnceLock<Retained<Delegate>> = OnceLock::new();

/// A denial has been reported to the user once per run; every message must not repeat it.
static DENIED_REPORTED: AtomicBool = AtomicBool::new(false);

/// The notification center, or an explanation of why there is none.
///
/// `currentNotificationCenter` raises an Objective-C exception for a bare binary — one without a
/// bundle identifier — which Rust cannot catch, so the identifier is checked first. (`tauri dev`
/// runs such a binary; `pin_dev_identity` swaps in Terminal's identifier before any of this, so a
/// dev run lands past the check, attributed to Terminal like its notifications always were.)
fn center() -> Result<Retained<UNUserNotificationCenter>, String> {
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return Err("notifications unavailable: the app is not running from a bundle".into());
    }
    Ok(UNUserNotificationCenter::currentNotificationCenter())
}

fn install_delegate(center: &UNUserNotificationCenter) {
    let delegate = DELEGATE.get_or_init(Delegate::new);
    let object =
        ProtocolObject::<dyn UNUserNotificationCenterDelegate>::from_retained(delegate.clone());
    center.setDelegate(Some(&object));
}

/// Builds and schedules one banner. Any thread; the system hops to the main thread itself.
fn schedule(center: &UNUserNotificationCenter, title: &str, body: &str) {
    let identifier = NSString::from_str(&format!(
        "wadesk-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    ));
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    let request =
        UNNotificationRequest::requestWithIdentifier_content_trigger(&identifier, &content, None);
    // The completion block only reports scheduling errors, which for plain text banners are
    // limited to a disabled system state that the authorization flow already surfaces.
    center.addNotificationRequest_withCompletionHandler(&request, None);
}

/// Shows one notification, asking for authorization first if the user was never asked.
///
/// Non-blocking: the request's completion handler does the actual scheduling, so on the very
/// first call the banner appears right after the user approves the system prompt, and on later
/// calls — where the system answers immediately — it appears straight away. A denied permission
/// is reported once per run to the main window, since the callers cannot know it synchronously.
pub fn show(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    let center = center()?;
    install_delegate(&center);

    let handler: RcBlock<dyn Fn(Bool, *mut NSError)> = RcBlock::new({
        let app = app.clone();
        let title = title.to_string();
        let body = body.to_string();
        move |granted: Bool, _error: *mut NSError| {
            if bool::from(granted) {
                schedule(&UNUserNotificationCenter::currentNotificationCenter(), &title, &body);
            } else if !DENIED_REPORTED.swap(true, Ordering::Relaxed) {
                let _ = app.emit_to(
                    MAIN_LABEL,
                    "notifications://denied",
                    "Notifications are turned off for WaDesk in System Settings › Notifications",
                );
            }
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &handler,
    );
    Ok(())
}
