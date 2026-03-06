use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::Instant;

use eframe::egui;

pub struct AudioPreview {
    pub title: String,
    path: PathBuf,
    samples: Vec<f32>,       // normalized mono samples (-1.0 to 1.0)
    sample_rate: u32,
    duration_secs: f32,
    sink: Option<rodio::Sink>,
    _stream: Option<rodio::OutputStream>,
    stream_handle: Option<rodio::OutputStreamHandle>,
    playing: bool,
    play_start: Option<Instant>,
    pause_offset: f32, // seconds elapsed when paused
}

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase() == "wav")
        .unwrap_or(false)
}

pub fn load(path: &Path) -> Option<AudioPreview> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels as usize;

    // Read all samples as f32
    let raw_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max_val = (1u32 << (bits - 1)) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / max_val)
                .collect()
        }
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
    };

    if raw_samples.is_empty() {
        return None;
    }

    // Convert to mono by averaging channels
    let mono_samples: Vec<f32> = if channels > 1 {
        raw_samples
            .chunks(channels)
            .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        raw_samples
    };

    let duration_secs = mono_samples.len() as f32 / sample_rate as f32;
    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Initialize audio output stream
    let (stream, stream_handle) = match rodio::OutputStream::try_default() {
        Ok((s, h)) => (Some(s), Some(h)),
        Err(_) => (None, None),
    };

    let mut preview = AudioPreview {
        title,
        path: path.to_path_buf(),
        samples: mono_samples,
        sample_rate,
        duration_secs,
        sink: None,
        _stream: stream,
        stream_handle,
        playing: false,
        play_start: None,
        pause_offset: 0.0,
    };

    // Auto-play on load
    preview.start_playback();

    Some(preview)
}

impl AudioPreview {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.strong(&self.title);
            ui.label(format!(
                "{}Hz / {:.1}s",
                self.sample_rate, self.duration_secs
            ));
            ui.separator();

            // Waveform area
            let available = ui.available_size();
            let waveform_height = (available.y - 50.0).max(60.0);
            let waveform_width = available.x;

            let (rect, _response) =
                ui.allocate_exact_size(egui::vec2(waveform_width, waveform_height), egui::Sense::click());

            self.draw_waveform(ui, rect);

            ui.add_space(8.0);

            // Controls
            ui.horizontal(|ui| {
                let btn_text = if self.playing { "■ Stop" } else { "▶ Play" };
                if ui.button(btn_text).clicked() {
                    self.toggle_play();
                }

                let current = self.current_position();
                let total = self.duration_secs;
                ui.label(format!(
                    "{} / {}",
                    format_time(current),
                    format_time(total)
                ));
            });

            // Check if playback finished
            if self.playing {
                if let Some(sink) = &self.sink {
                    if sink.empty() {
                        self.playing = false;
                        self.play_start = None;
                        self.pause_offset = 0.0;
                        self.sink = None;
                    }
                }
                // Request repaint for position update
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
            }
        });
    }

    fn draw_waveform(&self, ui: &egui::Ui, rect: egui::Rect) {
        let painter = ui.painter_at(rect);

        // Background
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 20, 30));

        // Center line
        let center_y = rect.center().y;
        painter.line_segment(
            [
                egui::pos2(rect.left(), center_y),
                egui::pos2(rect.right(), center_y),
            ],
            egui::Stroke::new(0.5, egui::Color32::from_rgb(60, 60, 80)),
        );

        if self.samples.is_empty() {
            return;
        }

        let width = rect.width() as usize;
        if width == 0 {
            return;
        }

        let half_height = rect.height() / 2.0;
        let waveform_color = egui::Color32::from_rgb(80, 200, 120);

        // Draw waveform: for each pixel column, find min/max sample
        let samples_per_pixel = self.samples.len() as f32 / width as f32;

        for px in 0..width {
            let start_idx = (px as f32 * samples_per_pixel) as usize;
            let end_idx = (((px + 1) as f32 * samples_per_pixel) as usize).min(self.samples.len());

            if start_idx >= end_idx {
                continue;
            }

            let mut min_val = f32::MAX;
            let mut max_val = f32::MIN;
            for i in start_idx..end_idx {
                let s = self.samples[i];
                if s < min_val {
                    min_val = s;
                }
                if s > max_val {
                    max_val = s;
                }
            }

            let x = rect.left() + px as f32;
            let y_top = center_y - max_val * half_height;
            let y_bottom = center_y - min_val * half_height;

            painter.line_segment(
                [egui::pos2(x, y_top), egui::pos2(x, y_bottom)],
                egui::Stroke::new(1.0, waveform_color),
            );
        }

        // Playback position indicator
        if self.playing || self.pause_offset > 0.0 {
            let pos = self.current_position();
            if self.duration_secs > 0.0 {
                let ratio = (pos / self.duration_secs).clamp(0.0, 1.0);
                let x = rect.left() + ratio * rect.width();
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(255, 80, 80)),
                );
            }
        }
    }

    fn toggle_play(&mut self) {
        if self.playing {
            // Pause
            if let Some(sink) = &self.sink {
                sink.stop();
            }
            self.pause_offset = self.current_position();
            self.playing = false;
            self.play_start = None;
            self.sink = None;
        } else {
            // Play from beginning (or resume not supported with rodio file source)
            self.start_playback();
        }
    }

    fn start_playback(&mut self) {
        let handle = match &self.stream_handle {
            Some(h) => h,
            None => return,
        };

        let file = match std::fs::File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return,
        };

        let source = match rodio::Decoder::new(BufReader::new(file)) {
            Ok(s) => s,
            Err(_) => return,
        };

        let sink = match rodio::Sink::try_new(handle) {
            Ok(s) => s,
            Err(_) => return,
        };

        sink.append(source);
        self.sink = Some(sink);
        self.playing = true;
        self.play_start = Some(Instant::now());
        self.pause_offset = 0.0;
    }

    fn current_position(&self) -> f32 {
        if self.playing {
            if let Some(start) = self.play_start {
                let elapsed = start.elapsed().as_secs_f32() + self.pause_offset;
                return elapsed.min(self.duration_secs);
            }
        }
        self.pause_offset
    }

    pub fn stop(&mut self) {
        if let Some(sink) = &self.sink {
            sink.stop();
        }
        self.sink = None;
        self.playing = false;
        self.play_start = None;
        self.pause_offset = 0.0;
    }
}

fn format_time(secs: f32) -> String {
    let total = secs as u32;
    let m = total / 60;
    let s = total % 60;
    format!("{}:{:02}", m, s)
}
