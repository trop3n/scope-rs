//! Audio module - handles audio input and sample buffering
//!
//! This module provides:
//! - Ring buffer for thread-safe sample sharing
//! - Audio input capture
//! - Audio file playback

mod buffer;
mod file;
mod input;

pub use buffer::{SampleBuffer, XYSample};
pub use file::{AudioFileInfo, AudioFilePlayer, FileError, PlaybackState};
pub use input::AudioInput;
