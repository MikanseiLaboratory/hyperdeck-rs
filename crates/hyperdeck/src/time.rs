//! Clock used for timeouts and the server watchdog.
//!
//! The core never reads a clock. Callers pass milliseconds in, and this trait
//! is how the async driver obtains them and waits.

use core::future::Future;

/// A millisecond clock and a cancellable sleep.
pub trait Clock: Send + Sync {
    /// Monotonic milliseconds. The origin is arbitrary but must not go backwards.
    fn now_ms(&self) -> u64;

    /// Suspends the current task for `ms` milliseconds.
    ///
    /// The future is `Send` so a connection task can run on tokio's multi-thread
    /// runtime and on Embassy's thread-mode executor.
    fn sleep(&self, ms: u64) -> impl Future<Output = ()> + Send + '_;
}

/// [`embassy_time`] clock.
///
/// Available with the `embassy-time` feature. On the host, enable Embassy's
/// std driver. On RP2040, enable `embassy-rp`'s `time-driver`.
#[cfg(feature = "embassy-time")]
#[derive(Debug, Default, Clone, Copy)]
pub struct EmbassyClock;

#[cfg(feature = "embassy-time")]
impl Clock for EmbassyClock {
    fn now_ms(&self) -> u64 {
        embassy_time::Instant::now().as_millis()
    }

    fn sleep(&self, ms: u64) -> impl Future<Output = ()> + Send + '_ {
        embassy_time::Timer::after(embassy_time::Duration::from_millis(ms))
    }
}

/// Placeholder so the name resolves in documentation when the feature is off.
#[cfg(not(feature = "embassy-time"))]
#[derive(Debug, Default, Clone, Copy)]
pub struct EmbassyClock;
