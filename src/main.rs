#![allow(dead_code)]

//! scope-rs - Oscilloscope Audio Visualizer
//!
//! This application visualizes audio input as XY oscilloscope graphics.
//!
//! ## Milestone 10 & 11: Audio Visualization Polish + File Playback
//! This version adds:
//! - Multiple display modes (Dots, Lines, Gradient, Points)
//! - Channel controls (swap, invert, DC offset)
//! - Color themes
//! - Audio file playback with symphonia
//! - Waveform overview display

use eframe::egui;
use std::time::Duration;

mod audio;
mod midi;
mod render;
mod settings;

use audio::{AudioFilePlayer, AudioInput, PlaybackState, SampleBuffer};
use render::{ColorTheme, DisplayMode, Oscilloscope};

/// Input source mode
#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum InputMode {
    #[default]
    Live,
    File,
}

const BUFFER_SIZE: usize = 2048;

fn main() -> eframe::Result<()> {
    env_logger::init();
    log::info!("Starting scope-rs");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_title("scope-rs"),
        ..Default::default()
    };

    eframe::run_native(
        "scope-rs",
        options,
        Box::new(|cc| Ok(Box::new(ScopeApp::new(cc)))),
    )
}

struct ScopeApp {
    buffer: SampleBuffer,
    audio: AudioInput,
    file_player: AudioFilePlayer,
    oscilloscope: Oscilloscope,
    midi: midi::MidiController,
    show_settings: bool,
    input_mode: InputMode,
}

impl ScopeApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let buffer = SampleBuffer::new(BUFFER_SIZE);
        let audio = AudioInput::new(buffer.clone_ref());
        let file_player = AudioFilePlayer::new(buffer.clone_ref());

        let mut app = Self {
            buffer,
            audio,
            file_player,
            oscilloscope: Oscilloscope::new(),
            midi: midi::MidiController::new(),
            show_settings: false,
            input_mode: InputMode::default(),
        };

        let settings = settings::AppSettings::load();
        settings.apply(&mut app);

        app
    }
}

impl Drop for ScopeApp {
    fn drop(&mut self) {
        settings::AppSettings::from_app(self).save();
    }
}

impl eframe::App for ScopeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();

        // Poll MIDI and apply parameter updates
        let midi_updates = self.midi.poll();
        if !midi_updates.is_empty() {
            midi::apply_updates(
                &midi_updates,
                &mut self.oscilloscope,
                &mut self.audio,
                &mut self.file_player,
            );
        }

        // Top panel
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("scope-rs");
                ui.separator();

                // Input mode selector
                ui.selectable_value(&mut self.input_mode, InputMode::Live, "Live");
                ui.selectable_value(&mut self.input_mode, InputMode::File, "File");
                ui.separator();

                match self.input_mode {
                    InputMode::Live => {
                        // Device selector
                        egui::ComboBox::from_id_salt("device")
                            .selected_text(
                                self.audio
                                    .devices
                                    .get(self.audio.selected_device)
                                    .cloned()
                                    .unwrap_or_else(|| "None".to_string()),
                            )
                            .show_ui(ui, |ui| {
                                for (i, name) in self.audio.devices.iter().enumerate() {
                                    ui.selectable_value(&mut self.audio.selected_device, i, name);
                                }
                            });

                        ui.separator();

                        // Capture button
                        let button_text = if self.audio.is_capturing() {
                            "⏹ Stop"
                        } else {
                            "▶ Capture"
                        };

                        let enabled = !self.audio.devices.is_empty() || self.audio.is_capturing();
                        if ui
                            .add_enabled(enabled, egui::Button::new(button_text))
                            .clicked()
                        {
                            self.audio.toggle();
                        }

                        ui.separator();
                        ui.label(&self.audio.status);
                    }
                    InputMode::File => {
                        // File open button
                        if ui.button("📂 Open").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter(
                                    "Audio",
                                    &["wav", "mp3", "flac", "ogg", "m4a", "aac", "aiff"],
                                )
                                .pick_file()
                            {
                                if let Err(e) = self.file_player.load(&path) {
                                    log::error!("Failed to load file: {}", e);
                                    self.file_player.status = format!("Error: {}", e);
                                }
                            }
                        }

                        ui.separator();

                        // File info
                        if let Some(info) = &self.file_player.info {
                            ui.label(&info.filename);
                            ui.separator();
                        }

                        ui.label(&self.file_player.status);
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.toggle_value(&mut self.show_settings, "⚙ Settings");
                });
            });
        });

        // Bottom panel for file playback controls
        if self.input_mode == InputMode::File && self.file_player.has_file() {
            egui::TopBottomPanel::bottom("playback_panel").show(ctx, |ui| {
                ui.add_space(4.0);

                // Waveform overview / seek bar
                let available_width = ui.available_width();
                let (response, painter) = ui.allocate_painter(
                    egui::vec2(available_width, 40.0),
                    egui::Sense::click_and_drag(),
                );
                let rect = response.rect;

                // Draw background
                painter.rect_filled(rect, 4.0, egui::Color32::from_gray(30));

                // Draw waveform
                if !self.file_player.waveform.is_empty() {
                    let waveform = &self.file_player.waveform;
                    let center_y = rect.center().y;
                    let height = rect.height() * 0.4;

                    for (i, (x, y)) in waveform.iter().enumerate() {
                        let t = i as f32 / waveform.len() as f32;
                        let screen_x = rect.left() + t * rect.width();

                        // Draw both channels
                        let amp_x = x.abs().min(1.0) * height;
                        let amp_y = y.abs().min(1.0) * height;

                        painter.line_segment(
                            [
                                egui::pos2(screen_x, center_y - amp_x),
                                egui::pos2(screen_x, center_y + amp_y),
                            ],
                            egui::Stroke::new(1.0, egui::Color32::from_rgb(80, 120, 80)),
                        );
                    }
                }

                // Draw playhead
                let position = self.file_player.position_fraction();
                let playhead_x = rect.left() + position * rect.width();
                painter.line_segment(
                    [
                        egui::pos2(playhead_x, rect.top()),
                        egui::pos2(playhead_x, rect.bottom()),
                    ],
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                );

                // Handle seeking
                if response.dragged() || response.clicked() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let seek_fraction = (pos.x - rect.left()) / rect.width();
                        self.file_player.seek(seek_fraction);
                    }
                }

                ui.add_space(4.0);

                // Playback controls
                ui.horizontal(|ui| {
                    // Play/Pause button
                    let play_text = match self.file_player.state() {
                        PlaybackState::Playing => "⏸",
                        _ => "▶",
                    };
                    if ui.button(play_text).clicked() {
                        self.file_player.toggle();
                    }

                    // Stop button
                    if ui.button("⏹").clicked() {
                        self.file_player.stop();
                    }

                    ui.separator();

                    // Time display
                    let current = self.file_player.position_duration();
                    let total = self
                        .file_player
                        .info
                        .as_ref()
                        .map(|i| i.duration)
                        .unwrap_or(Duration::ZERO);
                    ui.label(format!(
                        "{} / {}",
                        format_duration(current),
                        format_duration(total)
                    ));

                    ui.separator();

                    // Volume
                    ui.label("Vol:");
                    if ui
                        .add(
                            egui::Slider::new(&mut self.file_player.volume, 0.0..=2.0)
                                .show_value(false),
                        )
                        .changed()
                    {
                        self.file_player.sync_volume();
                    }

                    ui.separator();

                    // Speed
                    ui.label("Speed:");
                    ui.add(
                        egui::Slider::new(&mut self.file_player.speed, 0.25..=2.0)
                            .show_value(false),
                    );
                    ui.label(format!("{:.1}x", self.file_player.speed));

                    ui.separator();

                    // Loop toggle
                    ui.checkbox(&mut self.file_player.loop_playback, "Loop");
                });

                ui.add_space(4.0);
            });
        }

        // Settings panel
        if self.show_settings {
            egui::SidePanel::right("settings_panel")
                .min_width(200.0)
                .show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.separator();

                    ui.collapsing("Audio", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Gain:");
                            if ui
                                .add(
                                    egui::Slider::new(&mut self.audio.gain, 0.1..=10.0)
                                        .logarithmic(true),
                                )
                                .changed()
                            {
                                self.audio.sync_gain();
                            }
                        });
                    });

                    ui.separator();

                    ui.collapsing("Display", |ui| {
                        // Display mode selector
                        ui.horizontal(|ui| {
                            ui.label("Mode:");
                            egui::ComboBox::from_id_salt("display_mode")
                                .selected_text(self.oscilloscope.settings.display_mode.name())
                                .show_ui(ui, |ui| {
                                    for mode in DisplayMode::all() {
                                        ui.selectable_value(
                                            &mut self.oscilloscope.settings.display_mode,
                                            *mode,
                                            mode.name(),
                                        );
                                    }
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label("Zoom:");
                            ui.add(egui::Slider::new(
                                &mut self.oscilloscope.settings.zoom,
                                0.1..=2.0,
                            ));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Line width:");
                            ui.add(egui::Slider::new(
                                &mut self.oscilloscope.settings.line_width,
                                0.5..=5.0,
                            ));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Intensity:");
                            ui.add(egui::Slider::new(
                                &mut self.oscilloscope.settings.intensity,
                                0.1..=1.0,
                            ));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Persistence:");
                            ui.add(egui::Slider::new(
                                &mut self.oscilloscope.settings.persistence,
                                0.0..=0.99,
                            ));
                        });

                        ui.checkbox(&mut self.oscilloscope.settings.show_graticule, "Show grid");

                        if ui.button("Clear persistence").clicked() {
                            self.oscilloscope.clear_persistence();
                        }
                    });

                    ui.separator();

                    ui.collapsing("Channel", |ui| {
                        ui.checkbox(&mut self.oscilloscope.settings.swap_xy, "Swap X/Y");
                        ui.checkbox(&mut self.oscilloscope.settings.invert_x, "Invert X");
                        ui.checkbox(&mut self.oscilloscope.settings.invert_y, "Invert Y");

                        ui.separator();

                        ui.horizontal(|ui| {
                            ui.label("X offset:");
                            ui.add(egui::Slider::new(
                                &mut self.oscilloscope.settings.dc_offset_x,
                                -1.0..=1.0,
                            ));
                        });

                        ui.horizontal(|ui| {
                            ui.label("Y offset:");
                            ui.add(egui::Slider::new(
                                &mut self.oscilloscope.settings.dc_offset_y,
                                -1.0..=1.0,
                            ));
                        });

                        if ui.button("Reset offsets").clicked() {
                            self.oscilloscope.settings.dc_offset_x = 0.0;
                            self.oscilloscope.settings.dc_offset_y = 0.0;
                        }
                    });

                    ui.separator();

                    ui.collapsing("Color", |ui| {
                        // Theme selector
                        ui.horizontal(|ui| {
                            ui.label("Theme:");
                            egui::ComboBox::from_id_salt("color_theme")
                                .selected_text(self.oscilloscope.settings.theme.name())
                                .show_ui(ui, |ui| {
                                    for theme in ColorTheme::all() {
                                        if ui
                                            .selectable_label(
                                                self.oscilloscope.settings.theme == *theme,
                                                theme.name(),
                                            )
                                            .clicked()
                                        {
                                            self.oscilloscope.settings.apply_theme(*theme);
                                        }
                                    }
                                });
                        });
                    });

                    ui.separator();

                    ui.collapsing("MIDI", |ui| {
                        // Port selector
                        ui.horizontal(|ui| {
                            ui.label("Port:");
                            egui::ComboBox::from_id_salt("midi_port")
                                .selected_text(
                                    self.midi
                                        .ports
                                        .get(self.midi.selected_port)
                                        .cloned()
                                        .unwrap_or_else(|| "None".to_string()),
                                )
                                .show_ui(ui, |ui| {
                                    for (i, name) in self.midi.ports.iter().enumerate() {
                                        ui.selectable_value(&mut self.midi.selected_port, i, name);
                                    }
                                });
                        });

                        ui.horizontal(|ui| {
                            let button_text = if self.midi.is_connected {
                                "Disconnect"
                            } else {
                                "Connect"
                            };
                            if ui.button(button_text).clicked() {
                                self.midi.toggle();
                            }
                            if ui.button("Refresh").clicked() {
                                self.midi.scan_ports();
                            }
                        });

                        ui.small(&self.midi.status);
                        ui.separator();

                        // Mappings
                        ui.label("Mappings:");

                        // Snapshot mapping info to avoid borrow conflicts
                        let mapping_info: Vec<_> = self
                            .midi
                            .mappings
                            .iter()
                            .enumerate()
                            .map(|(i, m)| (i, m.cc, m.param.name()))
                            .collect();
                        let learning = self.midi.learning;

                        let mut remove_idx = None;
                        let mut learn_idx = None;
                        let mut cancel_learn = false;

                        for (i, cc, param_name) in &mapping_info {
                            ui.horizontal(|ui| {
                                let is_learning = learning == Some(*i);
                                let label = if is_learning {
                                    format!("CC ? -> {}", param_name)
                                } else {
                                    format!("CC {:>3} -> {}", cc, param_name)
                                };
                                ui.monospace(&label);

                                if is_learning {
                                    if ui.small_button("Cancel").clicked() {
                                        cancel_learn = true;
                                    }
                                } else if ui.small_button("Learn").clicked() {
                                    learn_idx = Some(*i);
                                }

                                if ui.small_button("X").clicked() {
                                    remove_idx = Some(*i);
                                }
                            });
                        }

                        // Apply deferred actions
                        if cancel_learn {
                            self.midi.cancel_learn();
                        }
                        if let Some(idx) = learn_idx {
                            self.midi.start_learn(idx);
                        }
                        if let Some(idx) = remove_idx {
                            self.midi.remove_mapping(idx);
                        }

                        // Add new mapping
                        let unmapped = self.midi.unmapped_params();
                        if !unmapped.is_empty() && ui.button("+ Add").clicked() {
                            self.midi.add_mapping(0, unmapped[0]);
                        }
                    });
                });
        }

        // Main oscilloscope display
        egui::CentralPanel::default().show(ctx, |ui| {
            let samples = self.buffer.get_samples();
            self.oscilloscope.show(ui, &samples, None);

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.small(format!("Samples: {}", samples.len()));
                    ui.separator();
                    ui.small(format!("Total: {}", self.buffer.samples_written()));
                    ui.separator();
                    let mode_str = match self.input_mode {
                        InputMode::Live => "Live Input",
                        InputMode::File => "File Playback",
                    };
                    ui.small(format!(
                        "Mode: {} | Display: {}",
                        mode_str,
                        self.oscilloscope.settings.display_mode.name()
                    ));
                });
            });
        });
    }
}

/// Format a duration as MM:SS
fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let mins = secs / 60;
    let secs = secs % 60;
    format!("{:02}:{:02}", mins, secs)
}
