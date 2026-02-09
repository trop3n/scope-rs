//! scope-rs - Oscilloscope Audio Visualizer
//!
//! This application visualizes audio input as XY oscilloscope graphics.
//!
//! ## Milestone 3: Oscilloscope Display
//! This version adds:
//! - Modular code structure (audio/, render/)
//! - XY oscilloscope visualization
//! - Persistence/afterglow effect

use eframe::egui;

mod audio;
mod render;

use audio::{AudioInput, SampleBuffer};
use render::Oscilloscope;

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
    oscilloscope: Oscilloscope,
    show_settings: bool,
}

impl ScopeApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let buffer = SampleBuffer::new(BUFFER_SIZE);
        let audio = AudioInput::new(buffer.clone_ref());

        Self {
            buffer,
            audio,
            oscilloscope: Oscilloscope::new(),
            show_settings: false,
        }
    }
}

impl eframe::App for ScopeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();

        // Top panel
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("scope-rs");
                ui.separator();

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
                if ui.add_enabled(enabled, egui::Button::new(button_text)).clicked() {
                    self.audio.toggle();
                }

                ui.separator();
                ui.toggle_value(&mut self.show_settings, "⚙ Settings");
                ui.separator();
                ui.label(&self.audio.status);
            });
        });

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
                            ui.add(
                                egui::Slider::new(&mut self.audio.gain, 0.1..=10.0)
                                    .logarithmic(true),
                            );
                        });
                    });

                    ui.separator();

                    ui.collapsing("Display", |ui| {
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
                        ui.checkbox(&mut self.oscilloscope.settings.draw_lines, "Draw lines");

                        if ui.button("Clear persistence").clicked() {
                            self.oscilloscope.clear_persistence();
                        }
                    });

                    ui.separator();

                    ui.collapsing("Color", |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Green").clicked() {
                                self.oscilloscope.settings.color =
                                    egui::Color32::from_rgb(100, 255, 100);
                                self.oscilloscope.settings.background =
                                    egui::Color32::from_rgb(10, 20, 10);
                            }
                            if ui.button("Amber").clicked() {
                                self.oscilloscope.settings.color =
                                    egui::Color32::from_rgb(255, 176, 0);
                                self.oscilloscope.settings.background =
                                    egui::Color32::from_rgb(20, 15, 5);
                            }
                            if ui.button("Blue").clicked() {
                                self.oscilloscope.settings.color =
                                    egui::Color32::from_rgb(100, 150, 255);
                                self.oscilloscope.settings.background =
                                    egui::Color32::from_rgb(10, 10, 20);
                            }
                        });
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
                    ui.small("Milestone 3: XY Oscilloscope Display");
                });
            });
        });
    }
}
