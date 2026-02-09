//! Audio module - handles audio input and sample buffering
//!
//! This module provides:
//! - Ring buffer for thread-safe sample sharing
//! - Audio input capture

mod buffer;
mod input;

pub use buffer::{SampleBuffer, XYSample};
pub use input::AudioInput;
