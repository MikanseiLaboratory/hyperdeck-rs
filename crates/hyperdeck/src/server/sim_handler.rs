//! [`DeckHandler`] adapter around [`hyperdeck_core::DeckSim`].

use core::fmt::Write;

use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::mutex::Mutex;
use hyperdeck_core::DeckSim;
use hyperdeck_core::ethernet::Command;

use super::DeckHandler;
use super::handler::{Outcome, ReplyWriter, outcome_from_sim};

/// Shared in-memory deck.
pub struct SharedSim<M: RawMutex> {
    sim: Mutex<M, DeckSim>,
}

impl<M: RawMutex> SharedSim<M> {
    /// Wraps `sim`.
    pub const fn new(sim: DeckSim) -> Self {
        Self {
            sim: Mutex::new(sim),
        }
    }

    /// The demo timeline from [`DeckSim::demo`].
    pub fn demo() -> Self {
        Self::new(DeckSim::demo())
    }

    /// Locks the deck and advances playback.
    ///
    /// Returns a transport notice when the state changed.
    pub async fn tick(&self, frames: u32) -> Option<heapless::Vec<u8, 768>> {
        let mut sim = self.sim.lock().await;
        if !sim.tick(frames) {
            return None;
        }
        let mut text = heapless::String::<768>::new();
        sim.write_transport_notice(&mut text).ok()?;
        let mut bytes = heapless::Vec::new();
        bytes.extend_from_slice(text.as_bytes()).ok()?;
        Some(bytes)
    }
}

impl<M> DeckHandler for SharedSim<M>
where
    M: RawMutex + Send + Sync + 'static,
{
    // `async fn` would drop the `Send` bound the trait promises.
    #[allow(clippy::manual_async_fn)]
    fn handle_command<'a>(
        &'a self,
        cmd: &'a Command<'_>,
        body: &'a mut ReplyWriter<'_>,
    ) -> impl core::future::Future<Output = Outcome> + Send + 'a {
        async move {
            let mut sim = self.sim.lock().await;
            let mut text = heapless::String::<768>::new();
            let mut notify = heapless::String::<768>::new();
            let result = sim.apply(cmd, &mut text, &mut notify);
            let _ = body.write_str(&text);
            outcome_from_sim(result, &notify)
                .unwrap_or_else(|_| Outcome::status(108, "internal error"))
        }
    }
}
