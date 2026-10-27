//! macOS: notifications through `UNUserNotificationCenter`, with one
//! delegate for every click (#234).
//!
//! A notification is posted and forgotten: nothing waits on it. Its click
//! comes to [`Delegate`], which looks up what the notification was for by
//! its identifier and opens it, as a Windows toast's link does. Before
//! this, each notification waited for its click on a thread of its own,
//! and `mac-notification-sys` checked whether it was still shown with a
//! timer on the main run loop, a synchronous call to `usernoted` every half
//! second for as long as it stayed in Notification Center: two days of
//! notices left the app answering clicks seconds late.
//!
//! Only an app bundle has a notification center: run outside one (a dev
//! build), a notification is shown without its click.

use std::{
    collections::VecDeque,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use block2::RcBlock;
use objc2::{
    AllocAnyThread, define_class, msg_send,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationDefaultActionIdentifier,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use tauri::AppHandle;

use crate::Click;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// What the latest notifications were for, by identifier. A click on an
/// older one (or on one from before the app last started) brings the app
/// forward and nothing more.
static RECENT: Mutex<VecDeque<(String, Click)>> = Mutex::new(VecDeque::new());
const RECENT_MAX: usize = 64;

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and this has no Drop.
    #[unsafe(super(NSObject))]
    #[name = "ArugulaNotifications"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        // Shown while the app is in front too, as before.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            done: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            done.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            done: &block2::DynBlock<dyn Fn()>,
        ) {
            // A click on the notification itself, not a dismiss.
            if &*response.actionIdentifier() == unsafe { UNNotificationDefaultActionIdentifier } {
                let id = response.notification().request().identifier().to_string();
                if let (Some(app), Some(click)) = (APP.get(), recall(&id)) {
                    let a = app.clone();
                    let _ = app.run_on_main_thread(move || click.open(&a));
                }
            }
            done.call(());
        }
    }
);

/// Whether this runs as an app bundle, which a notification center needs
/// (outside one, `currentNotificationCenter` raises).
fn bundled() -> bool {
    let main = NSBundle::mainBundle();
    main.bundleIdentifier().is_some() && main.bundlePath().to_string().ends_with(".app")
}

/// Set the delegate and ask to notify. On the main thread, at launch, so a
/// click that starts the app finds the delegate there.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    if !bundled() {
        return;
    }
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center holds its delegate weakly: keep it for the app's life.
    std::mem::forget(delegate);
    let asked = RcBlock::new(|granted: Bool, _error: *mut NSError| {
        if !granted.as_bool() {
            eprintln!("arugula: notifications are off for Arugula (System Settings → Notifications)");
        }
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &asked,
    );
}

/// Show a notification whose click opens `click`. Returns at once.
pub fn show(click: Click, title: &str, body: &str) {
    if !bundled() {
        let _ = mac_notification_sys::set_application(arugula_proto::service::APP_ID);
        let _ = mac_notification_sys::send_notification(
            title,
            None,
            body,
            Some(mac_notification_sys::Notification::new().asynchronous(true)),
        );
        return;
    }
    let id = remember(click);
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    let request =
        UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&id), &content, None);
    let added = RcBlock::new(|error: *mut NSError| {
        if let Some(e) = unsafe { error.as_ref() } {
            eprintln!("arugula: a notification: {}", e.localizedDescription());
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, Some(&added));
}

fn remember(click: Click) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = format!("arugula-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst));
    let mut r = RECENT.lock().unwrap();
    r.push_back((id.clone(), click));
    while r.len() > RECENT_MAX {
        r.pop_front();
    }
    id
}

fn recall(id: &str) -> Option<Click> {
    let mut r = RECENT.lock().unwrap();
    let at = r.iter().position(|(n, _)| n == id)?;
    r.remove(at).map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_finds_what_its_notification_was_for_once() {
        let id = remember(Click::Pane(7));
        assert!(matches!(recall(&id), Some(Click::Pane(7))));
        assert!(recall(&id).is_none());
    }

    #[test]
    fn only_the_latest_are_kept() {
        let first = remember(Click::Pane(1));
        for p in 0..RECENT_MAX as u32 {
            remember(Click::Pane(p));
        }
        assert!(recall(&first).is_none());
    }
}
