//! XY Oscilloscope display widget
//!
//! Identical to osci-rs version - in a real project, this would be a shared crate.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};

use crate::audio::XYSample;

/// Display settings for the oscilloscope
#[derive(Clone)]
pub struct OscilloscopeSettings {
    pub color: Color32,
    pub background: Color32,
    pub line_width: f32,
    pub draw_lines: bool,
    pub intensity: f32,
    pub sample_count: usize,
    pub zoom: f32,
    pub show_graticule: bool,
    pub persistence: f32,
}

impl Default for OscilloscopeSettings {
    fn default() -> Self {
        Self {
            color: Color32::from_rgb(100, 255, 100),
            background: Color32::from_rgb(10, 20, 10),
            line_width: 1.5,
            draw_lines: true,
            intensity: 1.0,
            sample_count: 2048,
            zoom: 1.0,
            show_graticule: true,
            persistence: 0.85,
        }
    }
}

/// XY Oscilloscope widget
pub struct Oscilloscope {
    pub settings: OscilloscopeSettings,
    persistence_buffer: Vec<(Pos2, f32)>,
}

impl Default for Oscilloscope {
    fn default() -> Self {
        Self::new()
    }
}

impl Oscilloscope {
    pub fn new() -> Self {
        Self {
            settings: OscilloscopeSettings::default(),
            persistence_buffer: Vec::with_capacity(8192),
        }
    }

    fn sample_to_screen(&self, sample: XYSample, rect: Rect) -> Pos2 {
        let zoom = self.settings.zoom;
        let norm_x = (sample.x / zoom + 1.0) / 2.0;
        let norm_y = (sample.y / zoom + 1.0) / 2.0;

        Pos2::new(
            rect.left() + norm_x * rect.width(),
            rect.bottom() - norm_y * rect.height(),
        )
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        samples: &[XYSample],
        size: Option<Vec2>,
    ) -> egui::Response {
        let size = size.unwrap_or_else(|| {
            let available = ui.available_size();
            let side = available.x.min(available.y).min(400.0);
            Vec2::new(side, side)
        });

        let (response, painter) = ui.allocate_painter(size, egui::Sense::hover());
        let rect = response.rect;

        painter.rect_filled(rect, 4.0, self.settings.background);

        if self.settings.show_graticule {
            self.draw_graticule(&painter, rect);
        }

        self.update_persistence(samples, rect);
        self.draw_persistence(&painter, rect);
        self.draw_samples(&painter, rect, samples);

        response
    }

    fn draw_graticule(&self, painter: &egui::Painter, rect: Rect) {
        let grid_color = Color32::from_rgba_unmultiplied(60, 80, 60, 100);
        let axis_color = Color32::from_rgba_unmultiplied(80, 100, 80, 150);

        let stroke_grid = Stroke::new(0.5, grid_color);
        let stroke_axis = Stroke::new(1.0, axis_color);

        for i in 0..=10 {
            let t = i as f32 / 10.0;
            let x = rect.left() + t * rect.width();
            let y = rect.top() + t * rect.height();
            let stroke = if i == 5 { stroke_axis } else { stroke_grid };

            painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], stroke);
            painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], stroke);
        }
    }

    fn update_persistence(&mut self, samples: &[XYSample], rect: Rect) {
        let decay = self.settings.persistence;

        self.persistence_buffer.retain_mut(|(_, alpha)| {
            *alpha *= decay;
            *alpha > 0.01
        });

        for sample in samples.iter().take(self.settings.sample_count) {
            let pos = self.sample_to_screen(*sample, rect);
            if rect.contains(pos) {
                self.persistence_buffer.push((pos, self.settings.intensity));
            }
        }

        const MAX_POINTS: usize = 50000;
        if self.persistence_buffer.len() > MAX_POINTS {
            let excess = self.persistence_buffer.len() - MAX_POINTS;
            self.persistence_buffer.drain(0..excess);
        }
    }

    fn draw_persistence(&self, painter: &egui::Painter, rect: Rect) {
        let base_color = self.settings.color;

        for (pos, alpha) in &self.persistence_buffer {
            if !rect.contains(*pos) {
                continue;
            }

            let color = Color32::from_rgba_unmultiplied(
                base_color.r(),
                base_color.g(),
                base_color.b(),
                (alpha * 255.0 * 0.3) as u8,
            );

            painter.circle_filled(*pos, self.settings.line_width * 0.5, color);
        }
    }

    fn draw_samples(&self, painter: &egui::Painter, rect: Rect, samples: &[XYSample]) {
        if samples.is_empty() {
            return;
        }

        let color = Color32::from_rgba_unmultiplied(
            self.settings.color.r(),
            self.settings.color.g(),
            self.settings.color.b(),
            (self.settings.intensity * 255.0) as u8,
        );

        let stroke = Stroke::new(self.settings.line_width, color);

        let points: Vec<Pos2> = samples
            .iter()
            .take(self.settings.sample_count)
            .map(|s| self.sample_to_screen(*s, rect))
            .collect();

        if self.settings.draw_lines && points.len() >= 2 {
            for window in points.windows(2) {
                let p1 = window[0];
                let p2 = window[1];
                let dist_sq = (p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2);
                let max_dist_sq = (rect.width() * 0.5).powi(2);

                if dist_sq < max_dist_sq {
                    painter.line_segment([p1, p2], stroke);
                }
            }
        } else {
            for pos in points {
                if rect.contains(pos) {
                    painter.circle_filled(pos, self.settings.line_width, color);
                }
            }
        }
    }

    pub fn clear_persistence(&mut self) {
        self.persistence_buffer.clear();
    }
}
