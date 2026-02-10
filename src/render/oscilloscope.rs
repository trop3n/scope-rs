//! XY Oscilloscope display widget
//!
//! Enhanced version with multiple display modes and channel controls.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use serde::{Deserialize, Serialize};

use crate::audio::XYSample;

/// Display mode for the oscilloscope
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum DisplayMode {
    /// Draw individual dots at each sample point
    Dots,
    /// Connect samples with lines (default)
    #[default]
    Lines,
    /// Gradient effect with varying intensity based on speed
    Gradient,
    /// Points only, no persistence
    Points,
}

impl DisplayMode {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Dots => "Dots",
            Self::Lines => "Lines",
            Self::Gradient => "Gradient",
            Self::Points => "Points",
        }
    }

    pub fn all() -> &'static [DisplayMode] {
        &[Self::Dots, Self::Lines, Self::Gradient, Self::Points]
    }
}

/// Color theme preset
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum ColorTheme {
    #[default]
    Green,
    Amber,
    Blue,
    White,
    Purple,
    Cyan,
    Red,
}

impl ColorTheme {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Green => "Green",
            Self::Amber => "Amber",
            Self::Blue => "Blue",
            Self::White => "White",
            Self::Purple => "Purple",
            Self::Cyan => "Cyan",
            Self::Red => "Red",
        }
    }

    pub fn colors(&self) -> (Color32, Color32) {
        match self {
            Self::Green => (
                Color32::from_rgb(100, 255, 100),
                Color32::from_rgb(10, 20, 10),
            ),
            Self::Amber => (Color32::from_rgb(255, 176, 0), Color32::from_rgb(20, 15, 5)),
            Self::Blue => (
                Color32::from_rgb(100, 150, 255),
                Color32::from_rgb(10, 10, 20),
            ),
            Self::White => (
                Color32::from_rgb(220, 220, 220),
                Color32::from_rgb(15, 15, 15),
            ),
            Self::Purple => (
                Color32::from_rgb(200, 100, 255),
                Color32::from_rgb(15, 10, 20),
            ),
            Self::Cyan => (
                Color32::from_rgb(100, 255, 255),
                Color32::from_rgb(10, 20, 20),
            ),
            Self::Red => (
                Color32::from_rgb(255, 100, 100),
                Color32::from_rgb(20, 10, 10),
            ),
        }
    }

    pub fn all() -> &'static [ColorTheme] {
        &[
            Self::Green,
            Self::Amber,
            Self::Blue,
            Self::White,
            Self::Purple,
            Self::Cyan,
            Self::Red,
        ]
    }
}

/// Display settings for the oscilloscope
#[derive(Clone)]
pub struct OscilloscopeSettings {
    pub color: Color32,
    pub background: Color32,
    pub line_width: f32,
    pub display_mode: DisplayMode,
    pub intensity: f32,
    pub sample_count: usize,
    pub zoom: f32,
    pub show_graticule: bool,
    pub persistence: f32,
    pub theme: ColorTheme,
    // Channel controls
    pub swap_xy: bool,
    pub invert_x: bool,
    pub invert_y: bool,
    pub dc_offset_x: f32,
    pub dc_offset_y: f32,
}

impl Default for OscilloscopeSettings {
    fn default() -> Self {
        let theme = ColorTheme::default();
        let (color, background) = theme.colors();
        Self {
            color,
            background,
            line_width: 1.5,
            display_mode: DisplayMode::default(),
            intensity: 1.0,
            sample_count: 2048,
            zoom: 1.0,
            show_graticule: true,
            persistence: 0.85,
            theme,
            swap_xy: false,
            invert_x: false,
            invert_y: false,
            dc_offset_x: 0.0,
            dc_offset_y: 0.0,
        }
    }
}

impl OscilloscopeSettings {
    /// Apply a color theme
    pub fn apply_theme(&mut self, theme: ColorTheme) {
        self.theme = theme;
        let (color, background) = theme.colors();
        self.color = color;
        self.background = background;
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

    /// Apply channel controls to a sample
    fn process_sample(&self, sample: XYSample) -> XYSample {
        let mut x = sample.x;
        let mut y = sample.y;

        // Apply DC offset
        x += self.settings.dc_offset_x;
        y += self.settings.dc_offset_y;

        // Apply invert
        if self.settings.invert_x {
            x = -x;
        }
        if self.settings.invert_y {
            y = -y;
        }

        // Apply swap
        if self.settings.swap_xy {
            std::mem::swap(&mut x, &mut y);
        }

        XYSample::new(x, y)
    }

    fn sample_to_screen(&self, sample: XYSample, rect: Rect) -> Pos2 {
        let processed = self.process_sample(sample);
        let zoom = self.settings.zoom;
        let norm_x = (processed.x / zoom + 1.0) / 2.0;
        let norm_y = (processed.y / zoom + 1.0) / 2.0;

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

            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                stroke,
            );
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

        let base_color = self.settings.color;
        let intensity = self.settings.intensity;

        let color = Color32::from_rgba_unmultiplied(
            base_color.r(),
            base_color.g(),
            base_color.b(),
            (intensity * 255.0) as u8,
        );

        let points: Vec<Pos2> = samples
            .iter()
            .take(self.settings.sample_count)
            .map(|s| self.sample_to_screen(*s, rect))
            .collect();

        match self.settings.display_mode {
            DisplayMode::Dots => {
                // Draw small dots at each sample point
                for pos in &points {
                    if rect.contains(*pos) {
                        painter.circle_filled(*pos, self.settings.line_width * 0.5, color);
                    }
                }
            }
            DisplayMode::Lines => {
                // Connect samples with lines
                let stroke = Stroke::new(self.settings.line_width, color);
                if points.len() >= 2 {
                    for window in points.windows(2) {
                        let p1 = window[0];
                        let p2 = window[1];
                        // Skip long jumps (likely discontinuities)
                        let dist_sq = (p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2);
                        let max_dist_sq = (rect.width() * 0.5).powi(2);

                        if dist_sq < max_dist_sq {
                            painter.line_segment([p1, p2], stroke);
                        }
                    }
                }
            }
            DisplayMode::Gradient => {
                // Gradient effect - intensity varies based on velocity
                if points.len() >= 2 {
                    for window in points.windows(2) {
                        let p1 = window[0];
                        let p2 = window[1];

                        let dist = ((p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2)).sqrt();
                        let max_dist = rect.width() * 0.5;

                        if dist < max_dist {
                            // Slower movement = brighter (more time spent at location)
                            let velocity_factor = 1.0 - (dist / max_dist).min(1.0);
                            let alpha = (intensity * velocity_factor * 255.0) as u8;

                            let gradient_color = Color32::from_rgba_unmultiplied(
                                base_color.r(),
                                base_color.g(),
                                base_color.b(),
                                alpha.max(30), // Minimum visibility
                            );

                            let stroke = Stroke::new(
                                self.settings.line_width * (0.5 + velocity_factor),
                                gradient_color,
                            );
                            painter.line_segment([p1, p2], stroke);
                        }
                    }
                }
            }
            DisplayMode::Points => {
                // Just points, no lines, no persistence effect
                for pos in &points {
                    if rect.contains(*pos) {
                        painter.circle_filled(*pos, self.settings.line_width, color);
                    }
                }
            }
        }
    }

    pub fn clear_persistence(&mut self) {
        self.persistence_buffer.clear();
    }
}
