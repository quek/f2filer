use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use parking_lot::Mutex;
use std::time::{Duration, Instant};

use eframe::egui;
use rodio::Source;

const SILENCE_THRESHOLD: f32 = 0.01;

pub struct AudioPreview {
    pub title: String,
    path: PathBuf,
    waveform: Arc<Mutex<Vec<f32>>>, // populated by background thread
    waveform_ready: bool,
    sample_rate: u32,
    duration_secs: f32,
    silence_skip_secs: f32,
    sink: Option<rodio::Player>,
    _stream: Option<rodio::MixerDeviceSink>,
    playing: bool,
    play_start: Option<Instant>,
    pause_offset: f32,
}

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_lowercase().as_str(), "wav" | "ogg" | "aif" | "aiff"))
        .unwrap_or(false)
}

fn is_wav(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("wav"))
        .unwrap_or(false)
}

/// Scan audio file header and detect initial silence duration.
/// Returns (silence_secs, sample_rate, channels, duration_secs).
fn scan_header_and_silence(path: &Path) -> Option<(f32, u32, u16, f32)> {
    if is_wav(path) {
        scan_wav_header_and_silence(path)
    } else {
        scan_generic_header_and_silence(path)
    }
}

/// WAV-specific scan using hound (fast header reading).
fn scan_wav_header_and_silence(path: &Path) -> Option<(f32, u32, u16, f32)> {
    let reader = hound::WavReader::open(path).ok()?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let channels = spec.channels;
    if sample_rate == 0 || channels == 0 {
        return None;
    }
    let total_samples = reader.duration(); // per channel
    let duration_secs = total_samples as f32 / sample_rate as f32;

    // Scan for first non-silent sample (read at most first 5 seconds)
    let max_scan = (sample_rate as usize * channels as usize * 5).min(total_samples as usize * channels as usize);
    let mut silent_frames = 0u64;

    match spec.sample_format {
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            if bits == 0 || bits > 32 {
                return None;
            }
            let max_val = (1u32 << (bits - 1)) as f32;
            let mut count = 0usize;
            let mut frame_max: f32 = 0.0;
            for sample in reader.into_samples::<i32>() {
                if count >= max_scan {
                    break;
                }
                let s = sample.ok()? as f32 / max_val;
                frame_max = frame_max.max(s.abs());
                count += 1;
                if count % channels as usize == 0 {
                    if frame_max > SILENCE_THRESHOLD {
                        break;
                    }
                    silent_frames += 1;
                    frame_max = 0.0;
                }
            }
        }
        hound::SampleFormat::Float => {
            let mut count = 0usize;
            let mut frame_max: f32 = 0.0;
            for sample in reader.into_samples::<f32>() {
                if count >= max_scan {
                    break;
                }
                let s = sample.ok()?;
                frame_max = frame_max.max(s.abs());
                count += 1;
                if count % channels as usize == 0 {
                    if frame_max > SILENCE_THRESHOLD {
                        break;
                    }
                    silent_frames += 1;
                    frame_max = 0.0;
                }
            }
        }
    }

    let silence_secs = silent_frames as f32 / sample_rate as f32;
    Some((silence_secs, sample_rate, channels, duration_secs))
}

/// Generic scan using rodio::Decoder for OGG/AIFF etc.
/// Only scans first 5 seconds for silence detection (avoids iterating entire file on UI thread).
/// Duration is computed later from waveform data when background loading completes.
fn scan_generic_header_and_silence(path: &Path) -> Option<(f32, u32, u16, f32)> {
    let file = std::fs::File::open(path).ok()?;
    let decoder = rodio::Decoder::new(BufReader::new(file)).ok()?;
    let sample_rate = decoder.sample_rate().get();
    let channels = decoder.channels().get();

    // Only scan first 5 seconds to detect initial silence.
    let max_scan = sample_rate as usize * channels as usize * 5;
    let mut silent_frames = 0u64;
    let mut count = 0usize;
    let mut frame_max: f32 = 0.0;

    for sample in decoder {
        if count >= max_scan {
            break;
        }
        frame_max = frame_max.max(sample.abs());
        count += 1;
        if count % channels as usize == 0 {
            if frame_max > SILENCE_THRESHOLD {
                break;
            }
            silent_frames += 1;
            frame_max = 0.0;
        }
    }

    let silence_secs = silent_frames as f32 / sample_rate as f32;
    // Duration unknown — will be computed from waveform in background thread
    Some((silence_secs, sample_rate, channels, 0.0))
}

/// Load samples for waveform display in background thread.
fn load_waveform_background(
    path: PathBuf,
    waveform: Arc<Mutex<Vec<f32>>>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        if is_wav(&path) {
            load_waveform_wav(&path, &waveform);
        } else {
            load_waveform_generic(&path, &waveform);
        }
        ctx.request_repaint();
    });
}

/// WAV waveform loading using hound.
fn load_waveform_wav(path: &Path, waveform: &Arc<Mutex<Vec<f32>>>) {
    let Ok(reader) = hound::WavReader::open(path) else { return };
    let spec = reader.spec();
    let channels = spec.channels as usize;
    if channels == 0 {
        return;
    }

    let raw_samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            if bits == 0 || bits > 32 {
                return;
            }
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

    let mono: Vec<f32> = if channels > 1 {
        raw_samples
            .chunks(channels)
            .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        raw_samples
    };

    *waveform.lock() = mono;
}

/// Generic waveform loading using rodio::Decoder for OGG/AIFF etc.
fn load_waveform_generic(path: &Path, waveform: &Arc<Mutex<Vec<f32>>>) {
    let Ok(file) = std::fs::File::open(path) else { return };
    let Ok(decoder) = rodio::Decoder::new(BufReader::new(file)) else { return };
    let channels = decoder.channels().get() as usize;

    let raw_samples: Vec<f32> = decoder.collect();

    let mono: Vec<f32> = if channels > 1 {
        raw_samples
            .chunks(channels)
            .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        raw_samples
    };

    *waveform.lock() = mono;
}

pub fn load(path: &Path, ctx: &egui::Context) -> Option<AudioPreview> {
    // Quick scan: header + silence detection (only reads beginning of file)
    let (silence_secs, sample_rate, _channels, duration_secs) =
        scan_header_and_silence(path)?;

    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Initialize audio output stream
    let stream = rodio::DeviceSinkBuilder::open_default_sink().ok();

    // Start background waveform loading
    let waveform = Arc::new(Mutex::new(Vec::new()));
    load_waveform_background(path.to_path_buf(), Arc::clone(&waveform), ctx.clone());

    let mut preview = AudioPreview {
        title,
        path: path.to_path_buf(),
        waveform,
        waveform_ready: false,
        sample_rate,
        duration_secs,
        silence_skip_secs: silence_secs,
        sink: None,
        _stream: stream,
        playing: false,
        play_start: None,
        pause_offset: 0.0,
    };

    // Auto-play immediately (streams from file, skips silence)
    preview.start_playback();

    Some(preview)
}

impl AudioPreview {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        // Check if waveform data arrived from background thread
        if !self.waveform_ready {
            let len = self.waveform.lock().len();
            if len > 0 {
                self.waveform_ready = true;
                // Compute exact duration from waveform samples (mono, so len = total frames)
                if self.sample_rate > 0 {
                    self.duration_secs = len as f32 / self.sample_rate as f32;
                }
            }
        }

        ui.vertical(|ui| {
            ui.strong(&self.title);
            let skip_info = if self.silence_skip_secs > 0.01 {
                format!(" (skip {:.2}s silence)", self.silence_skip_secs)
            } else {
                String::new()
            };
            let dur_str = if self.duration_secs > 0.0 {
                format!("{:.1}s", self.duration_secs)
            } else {
                "---".to_string()
            };
            ui.label(format!("{}Hz / {}{}", self.sample_rate, dur_str, skip_info));
            ui.separator();

            // Waveform area
            let available = ui.available_size();
            let waveform_height = (available.y - 50.0).max(60.0);
            let waveform_width = available.x;

            let (rect, _response) = ui.allocate_exact_size(
                egui::vec2(waveform_width, waveform_height),
                egui::Sense::click(),
            );

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
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(30));
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
            egui::Stroke::new(0.5_f32, egui::Color32::from_rgb(60, 60, 80)),
        );

        let samples = {
            let lock = self.waveform.lock();
            if lock.is_empty() {
                // Still loading - show message
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Loading waveform...",
                    egui::FontId::monospace(12.0),
                    egui::Color32::GRAY,
                );
                return;
            }
            lock.clone()
        };

        let width = rect.width() as usize;
        if width == 0 {
            return;
        }

        let half_height = rect.height() / 2.0;
        let waveform_color = egui::Color32::from_rgb(80, 200, 120);

        let samples_per_pixel = samples.len() as f32 / width as f32;

        for px in 0..width {
            let start_idx = (px as f32 * samples_per_pixel) as usize;
            let end_idx =
                (((px + 1) as f32 * samples_per_pixel) as usize).min(samples.len());

            if start_idx >= end_idx {
                continue;
            }

            let mut min_val = f32::MAX;
            let mut max_val = f32::MIN;
            for i in start_idx..end_idx {
                let s = samples[i];
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
                egui::Stroke::new(1.0_f32, waveform_color),
            );
        }

        // Silence skip marker
        if self.silence_skip_secs > 0.01 && self.duration_secs > 0.0 {
            let ratio = (self.silence_skip_secs / self.duration_secs).clamp(0.0, 1.0);
            let x = rect.left() + ratio * rect.width();
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(255, 200, 50)),
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
                    egui::Stroke::new(2.0_f32, egui::Color32::from_rgb(255, 80, 80)),
                );
            }
        }
    }

    fn toggle_play(&mut self) {
        if self.playing {
            if let Some(sink) = &self.sink {
                sink.stop();
            }
            self.pause_offset = self.current_position();
            self.playing = false;
            self.play_start = None;
            self.sink = None;
        } else {
            self.start_playback();
        }
    }

    fn start_playback(&mut self) {
        let Some(stream) = &self._stream else { return };
        let Ok(file) = std::fs::File::open(&self.path) else { return };
        let Ok(source) = rodio::Decoder::new(BufReader::new(file)) else { return };
        let sink = rodio::Player::connect_new(stream.mixer());

        // Skip initial silence
        if self.silence_skip_secs > 0.01 {
            let skip = Duration::from_secs_f32(self.silence_skip_secs);
            sink.append(source.skip_duration(skip));
        } else {
            sink.append(source);
        }

        self.sink = Some(sink);
        self.playing = true;
        self.play_start = Some(Instant::now());
        self.pause_offset = self.silence_skip_secs;
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
