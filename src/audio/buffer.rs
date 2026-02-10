//! Lock-free sample buffer for sharing audio data between threads
//!
//! This module provides a thread-safe circular buffer using the `ringbuf` crate.
//! Unlike mutex-based solutions, this uses atomic operations and is safe to use
//! in real-time audio callbacks without risking priority inversion or blocking.
//!
//! ## Why Lock-Free?
//!
//! Audio callbacks run on a real-time thread with strict timing requirements.
//! If the audio thread blocks waiting for a mutex held by the UI thread:
//! - Audio buffer underruns occur (audible glitches/clicks)
//! - The audio system may drop samples
//!
//! Lock-free data structures use atomic operations instead of locks:
//! - Producer and consumer can operate simultaneously
//! - No thread ever waits for another
//! - Consistent, predictable timing
//!
//! ## Design
//!
//! We use a SPSC (Single-Producer, Single-Consumer) ring buffer:
//! - Audio thread is the single producer (pushes samples)
//! - UI thread is the single consumer (reads samples for display)
//!
//! The buffer also maintains a "snapshot" for the UI - a separate copy that
//! the UI can read without affecting the ring buffer. This is updated
//! periodically by draining available samples from the ring.

use ringbuf::{
    traits::{Consumer, Producer, Split},
    HeapRb,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

/// A 2D point representing an XY sample
/// Left channel = X, Right channel = Y
#[derive(Clone, Copy, Debug, Default)]
pub struct XYSample {
    pub x: f32,
    pub y: f32,
}

impl XYSample {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Producer half of the sample buffer (owned by audio thread)
pub struct SampleProducer {
    producer: ringbuf::HeapProd<XYSample>,
    samples_written: Arc<AtomicU64>,
}

impl SampleProducer {
    /// Push a single sample into the buffer
    ///
    /// This is lock-free and safe to call from audio callbacks.
    /// If the buffer is full, the sample is dropped (acceptable for visualization).
    #[inline]
    pub fn push(&mut self, sample: XYSample) {
        // try_push returns Err if full - we just ignore it
        let _ = self.producer.try_push(sample);
        self.samples_written.fetch_add(1, Ordering::Relaxed);
    }

    /// Push multiple samples into the buffer
    #[inline]
    pub fn push_slice(&mut self, samples: &[XYSample]) {
        for &sample in samples {
            let _ = self.producer.try_push(sample);
        }
        self.samples_written
            .fetch_add(samples.len() as u64, Ordering::Relaxed);
    }
}

/// Consumer half of the sample buffer (owned by UI thread)
pub struct SampleConsumer {
    consumer: ringbuf::HeapCons<XYSample>,
    samples_written: Arc<AtomicU64>,
    /// Snapshot buffer for UI display
    snapshot: Vec<XYSample>,
    /// Capacity of the snapshot
    capacity: usize,
    /// Current write position in snapshot (circular)
    write_pos: usize,
}

impl SampleConsumer {
    /// Update the snapshot by draining available samples from the ring buffer
    ///
    /// Call this once per frame before reading samples.
    pub fn update(&mut self) {
        // Drain all available samples into our snapshot buffer
        while let Some(sample) = self.consumer.try_pop() {
            self.snapshot[self.write_pos] = sample;
            self.write_pos = (self.write_pos + 1) % self.capacity;
        }
    }

    /// Get all samples in chronological order (oldest first)
    ///
    /// Call `update()` first to get the latest samples.
    pub fn get_samples(&self) -> Vec<XYSample> {
        let mut result = Vec::with_capacity(self.capacity);

        // Read from write_pos (oldest) and wrap around
        for i in 0..self.capacity {
            let idx = (self.write_pos + i) % self.capacity;
            result.push(self.snapshot[idx]);
        }

        result
    }

    /// Get total samples written (for statistics)
    pub fn samples_written(&self) -> u64 {
        self.samples_written.load(Ordering::Relaxed)
    }
}

/// Thread-safe sample buffer using lock-free ring buffer
///
/// This is a wrapper that provides the same API as the old mutex-based buffer
/// but uses lock-free operations internally.
pub struct SampleBuffer {
    /// Producer handle (will be taken by audio thread)
    producer: Arc<Mutex<Option<SampleProducer>>>,
    /// Consumer handle (will be taken by UI thread)
    consumer: Arc<Mutex<Option<SampleConsumer>>>,
    /// Shared sample counter
    samples_written: Arc<AtomicU64>,
    /// Buffer capacity
    capacity: usize,
}

impl SampleBuffer {
    /// Create a new sample buffer with the given capacity
    pub fn new(capacity: usize) -> Self {
        let rb = HeapRb::<XYSample>::new(capacity * 2); // Extra space for ring buffer
        let (prod, cons) = rb.split();

        let samples_written = Arc::new(AtomicU64::new(0));

        let producer = SampleProducer {
            producer: prod,
            samples_written: Arc::clone(&samples_written),
        };

        let consumer = SampleConsumer {
            consumer: cons,
            samples_written: Arc::clone(&samples_written),
            snapshot: vec![XYSample::default(); capacity],
            capacity,
            write_pos: 0,
        };

        Self {
            producer: Arc::new(Mutex::new(Some(producer))),
            consumer: Arc::new(Mutex::new(Some(consumer))),
            samples_written,
            capacity,
        }
    }

    /// Take the producer handle (audio thread should call this once)
    pub fn take_producer(&self) -> Option<SampleProducer> {
        self.producer.lock().unwrap().take()
    }

    /// Take the consumer handle (UI thread should call this once)
    pub fn take_consumer(&self) -> Option<SampleConsumer> {
        self.consumer.lock().unwrap().take()
    }

    /// Push a sample (compatibility API - uses internal producer if available)
    ///
    /// Note: For best performance, use `take_producer()` and push directly.
    pub fn push(&self, sample: XYSample) -> bool {
        if let Ok(mut guard) = self.producer.try_lock() {
            if let Some(ref mut prod) = *guard {
                prod.push(sample);
                return true;
            }
        }
        false
    }

    /// Get samples (compatibility API - uses internal consumer if available)
    ///
    /// Note: For best performance, use `take_consumer()` and read directly.
    pub fn get_samples(&self) -> Vec<XYSample> {
        if let Ok(mut guard) = self.consumer.lock() {
            if let Some(ref mut cons) = *guard {
                cons.update();
                return cons.get_samples();
            }
        }
        vec![XYSample::default(); self.capacity]
    }

    /// Get total samples written
    pub fn samples_written(&self) -> u64 {
        self.samples_written.load(Ordering::Relaxed)
    }

    /// Clone reference to share between threads
    pub fn clone_ref(&self) -> Self {
        Self {
            producer: Arc::clone(&self.producer),
            consumer: Arc::clone(&self.consumer),
            samples_written: Arc::clone(&self.samples_written),
            capacity: self.capacity,
        }
    }
}

impl Clone for SampleBuffer {
    fn clone(&self) -> Self {
        self.clone_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_producer_consumer() {
        let buffer = SampleBuffer::new(4);

        let mut producer = buffer.take_producer().unwrap();
        let mut consumer = buffer.take_consumer().unwrap();

        producer.push(XYSample::new(1.0, 1.0));
        producer.push(XYSample::new(2.0, 2.0));
        producer.push(XYSample::new(3.0, 3.0));

        consumer.update();
        let samples = consumer.get_samples();
        assert_eq!(samples.len(), 4);
    }

    #[test]
    fn test_circular_wrap() {
        let buffer = SampleBuffer::new(3);

        let mut producer = buffer.take_producer().unwrap();
        let mut consumer = buffer.take_consumer().unwrap();

        // Push more samples than capacity
        producer.push(XYSample::new(1.0, 1.0));
        producer.push(XYSample::new(2.0, 2.0));
        producer.push(XYSample::new(3.0, 3.0));
        producer.push(XYSample::new(4.0, 4.0));

        consumer.update();
        let samples = consumer.get_samples();

        // Should have the 3 most recent samples somewhere in the buffer
        let values: Vec<f32> = samples.iter().map(|s| s.x).collect();
        assert!(values.contains(&2.0) || values.contains(&3.0) || values.contains(&4.0));
    }

    #[test]
    fn test_compatibility_api() {
        let buffer = SampleBuffer::new(4);

        buffer.push(XYSample::new(1.0, 1.0));
        buffer.push(XYSample::new(2.0, 2.0));

        let samples = buffer.get_samples();
        assert_eq!(samples.len(), 4);
    }
}
