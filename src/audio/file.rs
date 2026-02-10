//! Audio file playback
//!
//! This module handles loading and playing audio files using symphonia.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{
    traits::{Consumer, Observer, Producer, Split},
    HeapRb,
};
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};
use thiserror::Error;

use super::buffer::{SampleBuffer, XYSample};

/// Errors that can occur during audio file operations
#[derive(Error, Debug)]
pub enum FileError {
    #[error("Failed to open file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to probe audio format: {0}")]
    ProbeError(String),

    #[error("No audio tracks found")]
    NoTracks,

    #[error("Unsupported codec")]
    UnsupportedCodec,

    #[error("Decoder error: {0}")]
    DecoderError(String),
}

/// Playback state
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// Audio file metadata
#[derive(Debug, Clone)]
pub struct AudioFileInfo {
    pub path: PathBuf,
    pub filename: String,
    pub duration: Duration,
    pub sample_rate: u32,
    pub channels: u32,
    pub format: String,
}

/// Audio file player
pub struct AudioFilePlayer {
    /// Current file info
    pub info: Option<AudioFileInfo>,

    /// Playback state
    state: Arc<Mutex<PlaybackState>>,

    /// Current position in samples
    position: Arc<AtomicU64>,

    /// Total samples
    total_samples: u64,

    /// Sample rate
    sample_rate: u32,

    /// Whether playback thread is running
    is_running: Arc<AtomicBool>,

    /// Playback thread handle
    thread_handle: Option<thread::JoinHandle<()>>,

    /// Sample buffer for visualization
    buffer: SampleBuffer,

    /// Audio output ring buffer producer (for feeding cpal)
    audio_producer: Arc<Mutex<Option<ringbuf::HeapProd<f32>>>>,

    /// cpal output stream for audio playback
    output_stream: Option<cpal::Stream>,

    /// Shared volume for audio thread (AtomicU32 with f32 bits)
    volume_atomic: Arc<AtomicU32>,

    /// Playback speed multiplier
    pub speed: f32,

    /// Volume/gain
    pub volume: f32,

    /// Loop playback
    pub loop_playback: bool,

    /// Status message
    pub status: String,

    /// Waveform overview (downsampled)
    pub waveform: Vec<(f32, f32)>,
}

impl AudioFilePlayer {
    /// Create a new audio file player
    pub fn new(buffer: SampleBuffer) -> Self {
        Self {
            info: None,
            state: Arc::new(Mutex::new(PlaybackState::Stopped)),
            position: Arc::new(AtomicU64::new(0)),
            total_samples: 0,
            sample_rate: 44100,
            is_running: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
            buffer,
            audio_producer: Arc::new(Mutex::new(None)),
            output_stream: None,
            volume_atomic: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
            speed: 1.0,
            volume: 1.0,
            loop_playback: false,
            status: "No file loaded".to_string(),
            waveform: Vec::new(),
        }
    }

    /// Load an audio file
    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<(), FileError> {
        // Stop any current playback
        self.stop();

        let path = path.as_ref();
        let file = File::open(path)?;

        // Create media source stream
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        // Create hint from file extension
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        // Probe the file
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| FileError::ProbeError(e.to_string()))?;

        let format = probed.format;

        // Get the default track
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(FileError::NoTracks)?;

        let codec_params = &track.codec_params;
        let sample_rate = codec_params.sample_rate.unwrap_or(44100);
        let channels = codec_params.channels.map(|c| c.count() as u32).unwrap_or(2);

        // Calculate duration
        let duration = if let Some(n_frames) = codec_params.n_frames {
            let time_base = codec_params.time_base.unwrap_or(TimeBase::new(1, sample_rate));
            let time = time_base.calc_time(n_frames);
            Duration::from_secs_f64(time.seconds as f64 + time.frac)
        } else {
            Duration::ZERO
        };

        let total_samples = codec_params.n_frames.unwrap_or(0);

        // Get format name from codec
        let format_name = format!("{:?}", codec_params.codec).replace("CODEC_TYPE_", "");

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        self.info = Some(AudioFileInfo {
            path: path.to_path_buf(),
            filename: filename.clone(),
            duration,
            sample_rate,
            channels,
            format: format_name,
        });

        self.total_samples = total_samples;
        self.sample_rate = sample_rate;
        self.position.store(0, Ordering::Relaxed);

        // Generate waveform overview
        self.generate_waveform(path)?;

        self.status = format!("Loaded: {}", filename);
        log::info!("Loaded audio file: {:?}", path);

        Ok(())
    }

    /// Generate waveform overview by reading the file
    fn generate_waveform(&mut self, path: &Path) -> Result<(), FileError> {
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| FileError::ProbeError(e.to_string()))?;

        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or(FileError::NoTracks)?;

        let track_id = track.id;

        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(|e| FileError::DecoderError(e.to_string()))?;

        // Collect samples for waveform (downsample to ~1000 points)
        let target_points = 1000;
        let mut all_samples: Vec<(f32, f32)> = Vec::new();

        loop {
            let packet = match format.next_packet() {
                Ok(p) => p,
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(_) => break,
            };

            if packet.track_id() != track_id {
                continue;
            }

            match decoder.decode(&packet) {
                Ok(decoded) => {
                    let samples = extract_samples(&decoded);
                    all_samples.extend(samples);
                }
                Err(_) => continue,
            }
        }

        // Downsample for overview
        if all_samples.is_empty() {
            self.waveform = Vec::new();
        } else {
            let step = (all_samples.len() / target_points).max(1);
            self.waveform = all_samples
                .chunks(step)
                .map(|chunk| {
                    let (sum_x, sum_y) = chunk.iter().fold((0.0, 0.0), |acc, s| (acc.0 + s.0, acc.1 + s.1));
                    (sum_x / chunk.len() as f32, sum_y / chunk.len() as f32)
                })
                .collect();
        }

        Ok(())
    }

    /// Start playback
    pub fn play(&mut self) {
        if self.info.is_none() {
            return;
        }

        // If paused, just resume
        {
            let mut state = self.state.lock().unwrap();
            if *state == PlaybackState::Paused {
                *state = PlaybackState::Playing;
                self.status = "Playing".to_string();
                return;
            }
        }

        // Set up cpal audio output
        self.start_audio_output();

        // Sync volume to atomic
        self.volume_atomic.store(self.volume.to_bits(), Ordering::Relaxed);

        // Start new playback thread
        self.is_running.store(true, Ordering::Relaxed);

        let path = self.info.as_ref().unwrap().path.clone();
        let buffer = self.buffer.clone_ref();
        let audio_producer = Arc::clone(&self.audio_producer);
        let state = Arc::clone(&self.state);
        let position = Arc::clone(&self.position);
        let is_running = Arc::clone(&self.is_running);
        let volume_atomic = Arc::clone(&self.volume_atomic);
        let sample_rate = self.sample_rate;
        let speed = self.speed;
        let loop_playback = self.loop_playback;

        *self.state.lock().unwrap() = PlaybackState::Playing;
        self.status = "Playing".to_string();

        self.thread_handle = Some(thread::spawn(move || {
            if let Err(e) = playback_thread(
                &path,
                buffer,
                audio_producer,
                state,
                position,
                is_running,
                volume_atomic,
                sample_rate,
                speed,
                loop_playback,
            ) {
                log::error!("Playback error: {}", e);
            }
        }));
    }

    /// Set up cpal audio output stream
    fn start_audio_output(&mut self) {
        // Create audio ring buffer (stereo interleaved: L R L R ...)
        let rb = HeapRb::<f32>::new(48000 * 2); // ~1 second of stereo audio
        let (prod, mut cons) = rb.split();

        // Store producer for the playback thread
        *self.audio_producer.lock().unwrap() = Some(prod);

        // Open cpal output
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
            None => {
                log::warn!("No output device for file playback audio");
                return;
            }
        };

        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to get output config: {}", e);
                return;
            }
        };

        let channels = config.channels() as usize;

        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_mut(channels) {
                    let left = cons.try_pop().unwrap_or(0.0);
                    let right = cons.try_pop().unwrap_or(0.0);
                    if channels >= 2 {
                        frame[0] = left;
                        frame[1] = right;
                        for ch in frame.iter_mut().skip(2) {
                            *ch = 0.0;
                        }
                    } else {
                        frame[0] = (left + right) / 2.0;
                    }
                }
            },
            |err| log::error!("Audio output error: {}", err),
            None,
        );

        match stream {
            Ok(s) => {
                if let Err(e) = s.play() {
                    log::warn!("Failed to start output stream: {}", e);
                    return;
                }
                self.output_stream = Some(s);
            }
            Err(e) => {
                log::warn!("Failed to build output stream: {}", e);
            }
        }
    }

    /// Pause playback
    pub fn pause(&mut self) {
        let mut state = self.state.lock().unwrap();
        if *state == PlaybackState::Playing {
            *state = PlaybackState::Paused;
            self.status = "Paused".to_string();
        }
    }

    /// Stop playback
    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::Relaxed);
        *self.state.lock().unwrap() = PlaybackState::Stopped;

        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }

        // Clean up audio output
        self.output_stream = None;
        *self.audio_producer.lock().unwrap() = None;

        self.position.store(0, Ordering::Relaxed);
        self.status = if self.info.is_some() {
            "Stopped".to_string()
        } else {
            "No file loaded".to_string()
        };
    }

    /// Sync UI volume to audio thread
    pub fn sync_volume(&self) {
        self.volume_atomic.store(self.volume.to_bits(), Ordering::Relaxed);
    }

    /// Toggle play/pause
    pub fn toggle(&mut self) {
        let state = *self.state.lock().unwrap();
        match state {
            PlaybackState::Stopped => self.play(),
            PlaybackState::Playing => self.pause(),
            PlaybackState::Paused => self.play(),
        }
    }

    /// Get current playback state
    pub fn state(&self) -> PlaybackState {
        *self.state.lock().unwrap()
    }

    /// Get current position as fraction (0.0 - 1.0)
    pub fn position_fraction(&self) -> f32 {
        if self.total_samples == 0 {
            return 0.0;
        }
        self.position.load(Ordering::Relaxed) as f32 / self.total_samples as f32
    }

    /// Get current position as duration
    pub fn position_duration(&self) -> Duration {
        let samples = self.position.load(Ordering::Relaxed);
        Duration::from_secs_f64(samples as f64 / self.sample_rate as f64)
    }

    /// Seek to position (0.0 - 1.0)
    pub fn seek(&mut self, fraction: f32) {
        let fraction = fraction.clamp(0.0, 1.0);
        let target_sample = (self.total_samples as f32 * fraction) as u64;
        self.position.store(target_sample, Ordering::Relaxed);
    }

    /// Check if a file is loaded
    pub fn has_file(&self) -> bool {
        self.info.is_some()
    }
}

/// Extract XY samples from decoded audio buffer
fn extract_samples(buffer: &AudioBufferRef<'_>) -> Vec<(f32, f32)> {
    let mut samples = Vec::new();

    match buffer {
        AudioBufferRef::F32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();

            for frame in 0..frames {
                let x = buf.chan(0)[frame];
                let y = if channels > 1 {
                    buf.chan(1)[frame]
                } else {
                    x
                };
                samples.push((x, y));
            }
        }
        AudioBufferRef::S16(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();

            for frame in 0..frames {
                let x = buf.chan(0)[frame] as f32 / 32768.0;
                let y = if channels > 1 {
                    buf.chan(1)[frame] as f32 / 32768.0
                } else {
                    x
                };
                samples.push((x, y));
            }
        }
        AudioBufferRef::S32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();

            for frame in 0..frames {
                let x = buf.chan(0)[frame] as f32 / 2147483648.0;
                let y = if channels > 1 {
                    buf.chan(1)[frame] as f32 / 2147483648.0
                } else {
                    x
                };
                samples.push((x, y));
            }
        }
        _ => {}
    }

    samples
}

/// Playback thread function
fn playback_thread(
    path: &Path,
    buffer: SampleBuffer,
    audio_producer: Arc<Mutex<Option<ringbuf::HeapProd<f32>>>>,
    state: Arc<Mutex<PlaybackState>>,
    position: Arc<AtomicU64>,
    is_running: Arc<AtomicBool>,
    volume_atomic: Arc<AtomicU32>,
    sample_rate: u32,
    _speed: f32,
    loop_playback: bool,
) -> Result<(), FileError> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| FileError::ProbeError(e.to_string()))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(FileError::NoTracks)?;

    let track_id = track.id;

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| FileError::DecoderError(e.to_string()))?;

    // Seek to current position if needed
    let start_sample = position.load(Ordering::Relaxed);
    if start_sample > 0 {
        let _ = format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: Time::from(start_sample as f64 / sample_rate as f64),
                track_id: Some(track_id),
            },
        );
    }

    // Sleep duration for pacing the decoder (slightly faster than real-time,
    // cpal output callback drives actual timing)
    let packet_sleep = Duration::from_millis(5);

    let mut current_sample = start_sample;

    loop {
        if !is_running.load(Ordering::Relaxed) {
            break;
        }

        // Check if paused
        {
            let s = state.lock().unwrap();
            if *s == PlaybackState::Paused {
                thread::sleep(Duration::from_millis(10));
                continue;
            }
            if *s == PlaybackState::Stopped {
                break;
            }
        }

        // Read and decode a packet
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // End of file
                if loop_playback {
                    // Seek back to start
                    let _ = format.seek(
                        SeekMode::Accurate,
                        SeekTo::Time {
                            time: Time::from(0.0),
                            track_id: Some(track_id),
                        },
                    );
                    current_sample = 0;
                    position.store(0, Ordering::Relaxed);
                    continue;
                } else {
                    *state.lock().unwrap() = PlaybackState::Stopped;
                    break;
                }
            }
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let samples = extract_samples(&decoded);
                let num_samples = samples.len();
                let volume = f32::from_bits(volume_atomic.load(Ordering::Relaxed));

                // Push samples to visualization buffer
                for &(x, y) in &samples {
                    buffer.push(XYSample::new(x * volume, y * volume));
                }

                // Push interleaved stereo samples to audio output
                if let Ok(mut guard) = audio_producer.try_lock() {
                    if let Some(ref mut prod) = *guard {
                        for &(x, y) in &samples {
                            let _ = prod.try_push(x * volume);
                            let _ = prod.try_push(y * volume);
                        }
                    }
                }

                current_sample += num_samples as u64;
                position.store(current_sample, Ordering::Relaxed);

                // Pace the decoder - wait if audio buffer is getting full
                // This prevents decoding too far ahead while cpal drains at real-time
                let should_wait = audio_producer.try_lock()
                    .map(|guard| guard.as_ref().map(|p| p.is_full()).unwrap_or(false))
                    .unwrap_or(false);
                if should_wait {
                    thread::sleep(packet_sleep);
                }
            }
            Err(_) => continue,
        }
    }

    Ok(())
}
