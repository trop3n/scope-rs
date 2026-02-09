//! Sample buffer for sharing audio data between threads
//!
//! This is essentially identical to osci-rs/src/audio/buffer.rs
//! In a real project, we'd use a shared crate for common components.

use std::sync::{Arc, Mutex};

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

/// Thread-safe circular buffer for XY audio samples
pub struct SampleBuffer {
    inner: Arc<Mutex<BufferInner>>,
}

struct BufferInner {
    samples: Vec<XYSample>,
    write_pos: usize,
    samples_written: u64,
}

impl SampleBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferInner {
                samples: vec![XYSample::default(); capacity],
                write_pos: 0,
                samples_written: 0,
            })),
        }
    }

    pub fn push(&self, sample: XYSample) -> bool {
        if let Ok(mut inner) = self.inner.try_lock() {
            let len = inner.samples.len();
            let pos = inner.write_pos;
            inner.samples[pos] = sample;
            inner.write_pos = (pos + 1) % len;
            inner.samples_written += 1;
            true
        } else {
            false
        }
    }

    pub fn get_samples(&self) -> Vec<XYSample> {
        let inner = self.inner.lock().unwrap();
        let len = inner.samples.len();
        let mut result = Vec::with_capacity(len);

        for i in 0..len {
            let idx = (inner.write_pos + i) % len;
            result.push(inner.samples[idx]);
        }

        result
    }

    pub fn samples_written(&self) -> u64 {
        self.inner.lock().unwrap().samples_written
    }

    pub fn clone_ref(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Clone for SampleBuffer {
    fn clone(&self) -> Self {
        self.clone_ref()
    }
}
