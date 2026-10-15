//! Fan-out of asynchronous notices to a fixed set of connections.

use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use hyperdeck_core::session::NotifyKind;

/// One encoded `5xx` message.
#[derive(Clone)]
pub struct Notice<const N: usize = 768> {
    /// Which subscription this notice belongs to.
    pub kind: NotifyKind,
    /// Complete message, including the terminating blank line.
    pub bytes: heapless::Vec<u8, N>,
}

/// Connection slots and the deck-wide remote-control switch.
///
/// Each slot owns an [`embassy_sync::channel::Channel`]. Publishing clones the
/// notice into every slot, which avoids holding a subscriber borrow across an
/// `.await` — that borrow cannot live in a spawned task that also needs the
/// rest of this struct.
pub struct Hub<M: RawMutex, const CONNS: usize> {
    queues: [Channel<M, Notice, 4>; CONNS],
    used: Mutex<M, [bool; CONNS]>,
    remote_enabled: Mutex<M, bool>,
}

impl<M: RawMutex, const CONNS: usize> Hub<M, CONNS> {
    /// `remote_enabled` is the deck-wide switch (`remote: enable`).
    pub const fn new(remote_enabled: bool) -> Self {
        Self {
            queues: [const { Channel::new() }; CONNS],
            used: Mutex::new([false; CONNS]),
            remote_enabled: Mutex::new(remote_enabled),
        }
    }

    /// Reserves a connection slot, or `None` when every slot is taken.
    pub async fn acquire(&self) -> Option<usize> {
        let mut used = self.used.lock().await;
        let id = used.iter().position(|slot| !*slot)?;
        used[id] = true;
        drop(used);
        while self.queues[id].try_receive().is_ok() {}
        Some(id)
    }

    /// Returns a slot reserved with [`Hub::acquire`].
    pub async fn release(&self, id: usize) {
        if let Ok(mut used) = self.used.try_lock() {
            if id < CONNS {
                used[id] = false;
            }
        } else {
            let mut used = self.used.lock().await;
            if id < CONNS {
                used[id] = false;
            }
        }
    }

    /// Queues `notice` for every slot. A full queue drops the oldest notice.
    pub fn publish(&self, notice: Notice) {
        for queue in &self.queues {
            if let Err(embassy_sync::channel::TrySendError::Full(rejected)) =
                queue.try_send(notice.clone())
            {
                let _ = queue.try_receive();
                let _ = queue.try_send(rejected);
            }
        }
    }

    /// Next notice for `id`, if one is waiting.
    pub fn poll(&self, id: usize) -> Option<Notice> {
        self.queues.get(id)?.try_receive().ok()
    }

    /// Deck-wide remote enable flag.
    pub async fn remote_enabled(&self) -> bool {
        *self.remote_enabled.lock().await
    }

    /// Sets the deck-wide remote enable flag.
    pub async fn set_remote_enabled(&self, enabled: bool) {
        *self.remote_enabled.lock().await = enabled;
    }
}
