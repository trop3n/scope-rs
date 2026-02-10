//! Audio input capture
//!
//! This module handles capturing audio from input devices (microphones, etc.)

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use super::buffer::{SampleBuffer, XYSample};

/// Audio input capture engine
pub struct AudioInput {
    /// Whether capture is active
    is_capturing: Arc<AtomicBool>,

    /// The audio input stream
    stream: Option<cpal::Stream>,

    /// Shared sample buffer
    buffer: SampleBuffer,

    /// Available input devices
    pub devices: Vec<String>,

    /// Selected device index
    pub selected_device: usize,

    /// Gain multiplier (shared atomically with audio thread)
    gain_atomic: Arc<AtomicU32>,

    /// Gain value for UI binding
    pub gain: f32,

    /// Status message
    pub status: String,
}

impl AudioInput {
    /// Create a new audio input handler
    pub fn new(buffer: SampleBuffer) -> Self {
        // Enumerate input devices
        let host = cpal::default_host();
        let devices: Vec<String> = host
            .input_devices()
            .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default();

        let device_count = devices.len();

        Self {
            is_capturing: Arc::new(AtomicBool::new(false)),
            stream: None,
            buffer,
            devices,
            selected_device: 0,
            gain_atomic: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
            gain: 1.0,
            status: if device_count > 0 {
                format!("Found {} input device(s)", device_count)
            } else {
                "No input devices found".to_string()
            },
        }
    }

    /// Check if currently capturing
    pub fn is_capturing(&self) -> bool {
        self.is_capturing.load(Ordering::Relaxed)
    }

    /// Start audio capture
    pub fn start(&mut self) {
        if self.stream.is_some() {
            return;
        }

        log::info!("Starting audio capture...");

        let host = cpal::default_host();

        // Get selected device
        let device = match host.input_devices() {
            Ok(mut devices) => match devices.nth(self.selected_device) {
                Some(d) => d,
                None => {
                    self.status = "Error: Device not found".to_string();
                    return;
                }
            },
            Err(e) => {
                self.status = format!("Error: {}", e);
                return;
            }
        };

        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        log::info!("Using input device: {}", device_name);

        let config = match device.default_input_config() {
            Ok(c) => c,
            Err(e) => {
                self.status = format!("Error: {}", e);
                return;
            }
        };

        log::info!("Audio config: {:?}", config);

        let channels = config.channels() as usize;
        let buffer = self.buffer.clone_ref();
        let is_capturing = Arc::clone(&self.is_capturing);
        // Sync current UI gain to atomic before starting
        self.gain_atomic.store(self.gain.to_bits(), Ordering::Relaxed);
        let gain_atomic = Arc::clone(&self.gain_atomic);

        let stream_result = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if !is_capturing.load(Ordering::Relaxed) {
                        return;
                    }

                    let gain = f32::from_bits(gain_atomic.load(Ordering::Relaxed));
                    for frame in data.chunks(channels) {
                        let x = frame[0] * gain;
                        let y = if channels > 1 {
                            frame[1] * gain
                        } else {
                            x
                        };
                        buffer.push(XYSample::new(x, y));
                    }
                },
                |err| log::error!("Audio error: {}", err),
                None,
            ),
            cpal::SampleFormat::I16 => {
                let is_capturing = Arc::clone(&self.is_capturing);
                let buffer = self.buffer.clone_ref();
                let gain_atomic = Arc::clone(&self.gain_atomic);
                device.build_input_stream(
                &config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if !is_capturing.load(Ordering::Relaxed) {
                        return;
                    }

                    let gain = f32::from_bits(gain_atomic.load(Ordering::Relaxed));
                    for frame in data.chunks(channels) {
                        let x = (frame[0] as f32 / 32768.0) * gain;
                        let y = if channels > 1 {
                            (frame[1] as f32 / 32768.0) * gain
                        } else {
                            x
                        };
                        buffer.push(XYSample::new(x, y));
                    }
                },
                |err| log::error!("Audio error: {}", err),
                None,
            )},
            format => {
                self.status = format!("Unsupported format: {:?}", format);
                return;
            }
        };

        match stream_result {
            Ok(s) => {
                if let Err(e) = s.play() {
                    self.status = format!("Error: {}", e);
                    return;
                }

                self.is_capturing.store(true, Ordering::Relaxed);
                self.stream = Some(s);
                self.status = format!("Capturing: {}", device_name);
                log::info!("Capture started");
            }
            Err(e) => {
                self.status = format!("Error: {}", e);
            }
        }
    }

    /// Stop audio capture
    pub fn stop(&mut self) {
        self.is_capturing.store(false, Ordering::Relaxed);
        self.stream = None;
        self.status = "Stopped".to_string();
        log::info!("Capture stopped");
    }

    /// Sync the UI gain value to the audio thread
    /// Call this after the gain slider changes
    pub fn sync_gain(&self) {
        self.gain_atomic.store(self.gain.to_bits(), Ordering::Relaxed);
    }

    /// Toggle capture state
    pub fn toggle(&mut self) {
        if self.is_capturing() {
            self.stop();
        } else {
            self.start();
        }
    }
}
