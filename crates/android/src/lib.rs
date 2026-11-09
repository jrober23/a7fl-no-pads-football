//! The Android entry point.
//!
//! Android starts a `NativeActivity`, which loads this library and calls
//! `android_main` on a thread of its own. Everything after that is the same game
//! as the desktop client: this crate exists only so the link is a `cdylib`.

#![cfg(target_os = "android")]

use android_activity::AndroidApp;

/// Called by `android-activity`'s glue once the activity is up.
#[no_mangle]
fn android_main(app: AndroidApp) {
    nopads_cli::view::android::run(app);
}
