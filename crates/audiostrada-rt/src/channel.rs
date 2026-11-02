//! Wait-free single-producer single-consumer channel, safe to use from the audio thread.

use std::fmt;

/// Creates a bounded channel holding at most `capacity` values.
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let (producer, consumer) = rtrb::RingBuffer::new(capacity);
    (Sender(producer), Receiver(consumer))
}

pub struct Sender<T>(rtrb::Producer<T>);

impl<T> Sender<T> {
    /// Sends `value`, or gives it back if the channel is full.
    #[inline]
    pub fn try_send(&mut self, value: T) -> Result<(), T> {
        self.0.push(value).map_err(|rtrb::PushError::Full(v)| v)
    }

    pub fn free_slots(&self) -> usize {
        self.0.slots()
    }

    pub fn is_disconnected(&self) -> bool {
        self.0.is_abandoned()
    }
}

impl<T> fmt::Debug for Sender<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sender")
            .field("free_slots", &self.free_slots())
            .finish()
    }
}

pub struct Receiver<T>(rtrb::Consumer<T>);

impl<T> Receiver<T> {
    #[inline]
    pub fn try_recv(&mut self) -> Option<T> {
        self.0.pop().ok()
    }

    /// Iterates over the values currently available.
    pub fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(|| self.try_recv())
    }

    pub fn len(&self) -> usize {
        self.0.slots()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_disconnected(&self) -> bool {
        self.0.is_abandoned()
    }
}

impl<T> fmt::Debug for Receiver<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Receiver")
            .field("len", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_channel_gives_value_back() {
        let (mut tx, mut rx) = channel(2);
        assert_eq!(tx.try_send(1), Ok(()));
        assert_eq!(tx.try_send(2), Ok(()));
        assert_eq!(tx.try_send(3), Err(3));
        assert_eq!(rx.drain().collect::<Vec<_>>(), vec![1, 2]);
        assert!(rx.is_empty());
    }

    #[test]
    fn disconnection_is_visible() {
        let (tx, rx) = channel::<u8>(1);
        drop(tx);
        assert!(rx.is_disconnected());
        let (tx, rx) = channel::<u8>(1);
        assert!(!tx.is_disconnected());
        drop(rx);
        assert!(tx.is_disconnected());
    }

    #[test]
    fn slots_are_counted_on_both_sides() {
        let (mut tx, mut rx) = channel(3);
        assert_eq!((tx.free_slots(), rx.len()), (3, 0));
        tx.try_send('a').unwrap();
        tx.try_send('b').unwrap();
        assert_eq!((tx.free_slots(), rx.len()), (1, 2));
        assert_eq!(
            format!("{tx:?} {rx:?}"),
            "Sender { free_slots: 1 } Receiver { len: 2 }"
        );
        assert_eq!(rx.try_recv(), Some('a'));
        assert_eq!(rx.len(), 1);
        assert!(!rx.is_empty());
    }
}
