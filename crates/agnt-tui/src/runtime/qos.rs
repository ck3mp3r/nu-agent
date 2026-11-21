//! Thread priority for the surface's latency-critical threads.
//!
//! The scheduling policy has two halves. Tool children are demoted below the
//! agent (`agnt-runtime`'s `child_priority`), and the threads that read keys and
//! paint are promoted above ordinary work. This module is the second half, and it
//! exists only where the OS offers the knob: macOS Quality of Service. Every other
//! host keeps its default priority and relies on the demotion alone.

/// Ask the OS to rank the calling thread with user-interactive work.
///
/// macOS only: the call sets the thread's QoS class, so the scheduler gives it a
/// timeslice ahead of background work even while every core is busy — the case
/// the demoted tool children cannot cover, because the agent's own threads would
/// still wait behind them at the default class.
///
/// A failure is logged and never fatal: the thread keeps its default class, which
/// is exactly how it runs on the other hosts.
#[cfg(target_os = "macos")]
pub(crate) fn promote_to_user_interactive() {
    // SAFETY: the call reads only its arguments, promotes the calling thread, and
    // reports through its return value.
    let rc = unsafe { ffi::pthread_set_qos_class_self_np(ffi::QOS_CLASS_USER_INTERACTIVE, 0) };
    if rc != 0 {
        log::warn!(
            "thread QoS promotion failed with code {rc}; the thread keeps its default class"
        );
    }
}

/// No-op off macOS: the child demotion carries the protection there.
#[cfg(not(target_os = "macos"))]
pub(crate) fn promote_to_user_interactive() {}

// region:    --- Support

/// The `<pthread/qos.h>` seam.
///
/// Declared here because the workspace's libc exposes these only `pub(crate)`:
/// the names are present in its sources and unreachable from outside the crate.
/// The symbol and both values are libSystem's own (`<sys/qos.h>` and
/// `<pthread/qos.h>`), so no dependency changes and no shim library is needed.
#[cfg(target_os = "macos")]
mod ffi {
    use std::os::raw::c_int;

    /// `qos_class_t`: an unsigned enum in `<sys/qos.h>`.
    pub(super) type QosClass = u32;

    /// `QOS_CLASS_USER_INTERACTIVE`.
    pub(super) const QOS_CLASS_USER_INTERACTIVE: QosClass = 0x21;

    unsafe extern "C" {
        /// `int pthread_set_qos_class_self_np(qos_class_t, int)`.
        pub(super) fn pthread_set_qos_class_self_np(class: QosClass, priority: c_int) -> c_int;
    }
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/runtime/qos.rs"]
mod qos_test;

// endregion: --- Tests
