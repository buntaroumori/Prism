//! Lock-free sample ring between the decode/DSP producer and the device sink.
//!
//! The ring stores interleaved `f32` frames at the *device* format. Capacity is
//! a whole number of frames so a consumer request can never split a frame.
//!
//! The producer side is a normal thread; the consumer is the device callback
//! (WASAPI/ASIO/PipeWire) or a dedicated ALSA writer thread. The SPSC
//! `ringbuf` implementation gives us wait-free reads/writes with a single
//! atomic pair, no locks and no allocation after construction.
//!
//! Diagnostics counters are atomics read by the UI at low frequency:
//! * `underruns` — a consumer request was not fully satisfied (device starved);
//! * `overruns` — the producer attempted to write into a full ring and dropped
//!   frames (the engine treats this as a hard error and resynchronises).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Producer, Split};

/// Producer half handed to the decode/DSP thread.
pub struct SampleProducer {
    inner: ringbuf::HeapProd<f32>,
    channels: usize,
    capacity_frames: usize,
    stats: Arc<RingStats>,
    /// Bumped by [`SampleProducer::clear`]; the consumer discards everything
    /// it still holds when it observes a change. This is how a seek flushes
    /// audio without either side taking a lock.
    generation: Arc<AtomicU64>,
}

/// Consumer half moved into the device callback.
pub struct SampleConsumer {
    inner: ringbuf::HeapCons<f32>,
    channels: usize,
    capacity_frames: usize,
    stats: Arc<RingStats>,
    generation: Arc<AtomicU64>,
    seen_generation: u64,
}

impl std::fmt::Debug for SampleProducer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SampleProducer")
            .field("channels", &self.channels)
            .field("capacity_frames", &self.capacity_frames)
            .field("occupied_frames", &self.occupied_frames())
            .finish()
    }
}

impl std::fmt::Debug for SampleConsumer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SampleConsumer")
            .field("channels", &self.channels)
            .field("capacity_frames", &self.capacity_frames)
            .field("occupied_frames", &self.occupied_frames())
            .finish()
    }
}

/// Shared diagnostics readable from any thread.
#[derive(Debug, Default)]
pub struct RingStats {
    pub underruns: AtomicU64,
    pub overruns: AtomicU64,
    pub pushed_frames: AtomicU64,
    pub popped_frames: AtomicU64,
}

/// Create a ring sized in frames.
///
/// # Panics
/// Panics if `channels == 0` or `frames == 0`; both are programmer errors and
/// are validated earlier by [`crate::audio::format::AudioFormat`].
#[must_use]
pub fn create_ring(
    frames: usize,
    channels: usize,
) -> (SampleProducer, SampleConsumer, Arc<RingStats>) {
    assert!(channels > 0, "ring requires at least one channel");
    assert!(frames > 0, "ring requires at least one frame");

    let capacity = frames * channels;
    let ring = HeapRb::<f32>::new(capacity);
    let (producer, consumer) = ring.split();

    let stats = Arc::new(RingStats::default());
    let generation = Arc::new(AtomicU64::new(0));

    let producer = SampleProducer {
        inner: producer,
        channels,
        capacity_frames: frames,
        stats: Arc::clone(&stats),
        generation: Arc::clone(&generation),
    };
    let consumer = SampleConsumer {
        inner: consumer,
        channels,
        capacity_frames: frames,
        stats: Arc::clone(&stats),
        generation,
        seen_generation: 0,
    };

    (producer, consumer, stats)
}

impl SampleProducer {
    /// Number of frames currently readable by the consumer.
    #[must_use]
    pub fn occupied_frames(&self) -> usize {
        self.inner.occupied_len() / self.channels
    }

    /// Free space in frames.
    #[must_use]
    pub fn free_frames(&self) -> usize {
        self.inner.vacant_len() / self.channels
    }

    #[must_use]
    pub const fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }

    #[must_use]
    pub const fn channels(&self) -> usize {
        self.channels
    }

    /// Push interleaved samples, returning the number of frames written.
    ///
    /// A partial write is reported, never silently padded. The engine retries
    /// after the consumer drains; the dropped-frame counter is only incremented
    /// by [`Self::push_or_drop`], which is used for destructive flushes.
    pub fn push(&mut self, samples: &[f32]) -> usize {
        let frames = samples.len() / self.channels;
        let written = self.inner.push_slice(samples);
        let written_frames = written / self.channels;
        debug_assert!(written % self.channels == 0, "frame split across channels");
        if written_frames > 0 {
            self.stats
                .pushed_frames
                .fetch_add(written_frames as u64, Ordering::Relaxed);
        }
        frames.min(written_frames)
    }

    /// Push as much as fits and count what did not.
    pub fn push_or_drop(&mut self, samples: &[f32]) -> usize {
        let frames = samples.len() / self.channels;
        let written = self.push(samples);
        if written < frames {
            self.stats
                .overruns
                .fetch_add((frames - written) as u64, Ordering::Relaxed);
        }
        written
    }

    /// Ask the consumer to discard everything it has not taken yet.
    ///
    /// The producer cannot read the ring, so the flush is deferred: the
    /// generation counter is bumped and the next device callback drains the
    /// stale frames before pulling new ones. Lock-free and allocation-free on
    /// both sides.
    pub fn clear(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }
}

impl SampleConsumer {
    /// Pull frames into an interleaved device buffer.
    ///
    /// The callback contract requires a full buffer: any shortfall is filled
    /// with silence and counted as an underrun, which is exactly what the
    /// diagnostics panel reports after a device hiccup.
    pub fn pop_or_silence(&mut self, out: &mut [f32]) -> usize {
        let generation = self.generation.load(Ordering::Acquire);
        if generation != self.seen_generation {
            self.seen_generation = generation;
            self.inner.clear();
        }

        let wanted = out.len();
        let read = self.inner.pop_slice(out);
        let read_frames = read / self.channels;
        let wanted_frames = wanted / self.channels;

        if read < wanted {
            out[read..].fill(0.0);
            self.stats
                .underruns
                .fetch_add((wanted_frames - read_frames) as u64, Ordering::Relaxed);
        }
        if read_frames > 0 {
            self.stats
                .popped_frames
                .fetch_add(read_frames as u64, Ordering::Relaxed);
        }

        read_frames
    }

    /// Pull frames only when the ring holds a full buffer, otherwise silence.
    ///
    /// Used during the initial pre-roll so a half-filled ring does not produce
    /// a burst of underruns at start-up.
    pub fn pop_prerolled(&mut self, out: &mut [f32]) -> usize {
        if self.inner.occupied_len() < out.len() {
            out.fill(0.0);
            return 0;
        }
        self.pop_or_silence(out)
    }

    #[must_use]
    pub fn occupied_frames(&self) -> usize {
        self.inner.occupied_len() / self.channels
    }

    #[must_use]
    pub const fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_frames_are_never_split() {
        let (mut producer, mut consumer, stats) = create_ring(4, 2);
        let frames = producer.push(&[1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
        assert_eq!(frames, 3);
        assert_eq!(producer.occupied_frames(), 3);
        assert_eq!(producer.free_frames(), 1);

        let mut out = [0.0f32; 6];
        let read = consumer.pop_or_silence(&mut out);
        assert_eq!(read, 3);
        assert_eq!(out, [1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
        assert_eq!(stats.underruns.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn overrun_drops_whole_frames_and_counts() {
        let (mut producer, _consumer, stats) = create_ring(2, 2);
        let written = producer.push_or_drop(&[1.0, 1.0, 2.0, 2.0, 3.0, 3.0]);
        assert_eq!(written, 2);
        assert_eq!(stats.overruns.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn underrun_is_padded_with_silence_and_counted() {
        let (mut producer, mut consumer, stats) = create_ring(4, 2);
        producer.push(&[0.5, 0.5]);

        let mut out = [9.0f32; 8];
        let read = consumer.pop_or_silence(&mut out);
        assert_eq!(read, 1);
        assert_eq!(out[0..2], [0.5, 0.5]);
        assert_eq!(out[2..], [0.0; 6]);
        assert_eq!(stats.underruns.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn preroll_returns_silence_until_full() {
        let (mut producer, mut consumer, stats) = create_ring(2, 2);
        producer.push(&[1.0, 1.0]);
        let mut out = [7.0f32; 4];
        assert_eq!(consumer.pop_prerolled(&mut out), 0);
        assert_eq!(out, [0.0; 4]);
        assert_eq!(
            stats.underruns.load(Ordering::Relaxed),
            0,
            "preroll is not an underrun"
        );

        producer.push(&[2.0, 2.0, 3.0, 3.0]);
        assert_eq!(consumer.pop_prerolled(&mut out), 2);
        assert_eq!(out, [1.0, 1.0, 2.0, 2.0]);
    }

    #[test]
    fn clear_discards_unconsumed_audio() {
        let (mut producer, mut consumer, _stats) = create_ring(8, 1);
        producer.push(&[1.0, 2.0, 3.0]);
        producer.clear();

        // The flush is deferred to the consumer (the producer cannot read the
        // ring): the next pull discards the stale frames instead of playing
        // them, and reports silence rather than an underrun.
        let mut out = [9.0f32; 3];
        assert_eq!(consumer.pop_or_silence(&mut out), 0);
        assert_eq!(out, [0.0; 3]);
        assert_eq!(consumer.occupied_frames(), 0);

        // New audio pushed after the flush plays normally.
        producer.push(&[4.0, 5.0]);
        assert_eq!(consumer.pop_or_silence(&mut out[..2]), 2);
        assert_eq!(out[..2], [4.0, 5.0]);
    }

    #[test]
    fn moved_frames_are_counted_for_each_side() {
        let (mut producer, mut consumer, stats) = create_ring(8, 2);
        producer.push(&[0.1, 0.2, 0.3, 0.4]);
        let mut out = [0.0f32; 4];
        consumer.pop_or_silence(&mut out);
        assert_eq!(stats.pushed_frames.load(Ordering::Relaxed), 2);
        assert_eq!(stats.popped_frames.load(Ordering::Relaxed), 2);
    }
}
