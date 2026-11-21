#![cfg(target_os = "macos")]
//! The macOS thread-promotion seam: the call must actually move the calling
//! thread's class, not merely return.

use crate::runtime::qos::promote_to_user_interactive;
use std::os::raw::c_int;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// `QOS_CLASS_USER_INTERACTIVE` from `<sys/qos.h>`.
const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;

unsafe extern "C" {
    /// `int pthread_get_qos_class_np(pthread_t, qos_class_t *, int *)`.
    fn pthread_get_qos_class_np(
        thread: libc::pthread_t,
        class: *mut u32,
        priority: *mut c_int,
    ) -> c_int;
}

/// WHEN the module promotes the calling thread, THE OS SHALL report that thread
/// at the user-interactive class. The seam is declared locally, so this pins the
/// declaration's class value and its call shape against libSystem.
#[test]
fn test_runtime_qos_promotion_sets_the_user_interactive_class() -> Result<()> {
    // -- Setup & Fixtures
    let mut class: u32 = 0;
    let mut priority: c_int = 0;

    // -- Exec
    promote_to_user_interactive();
    // SAFETY: both out-pointers are valid for the call, which writes only
    // through them.
    let rc = unsafe { pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut priority) };

    // -- Check
    assert_eq!(rc, 0, "reading the thread's class must succeed");
    assert_eq!(
        class, QOS_CLASS_USER_INTERACTIVE,
        "the calling thread must rank as user-interactive after the promotion"
    );
    Ok(())
}
