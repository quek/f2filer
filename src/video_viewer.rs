use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use eframe::egui;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use rodio::Source;
use std::time::Duration;

const MAX_PREVIEW_WIDTH: u32 = 800;
const FRAME_BUFFER_CAPACITY: usize = 30;

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "avi", "mkv", "webm", "mov", "wmv", "flv", "m4v", "mpg", "mpeg", "ts",
];

pub fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

struct VideoInfo {
    width: u32,
    height: u32,
    fps: f64,
    duration_secs: f64,
}

struct DecodedFrame {
    rgba: Vec<u8>,
    index: u64,
}

struct FrameBuffer {
    frames: VecDeque<DecodedFrame>,
    finished: bool,
}

pub struct VideoPreview {
    pub title: String,
    width: u32,
    height: u32,
    fps: f64,
    duration_secs: f64,
    frame_buffer: Arc<Mutex<FrameBuffer>>,
    current_texture: Option<egui::TextureHandle>,
    current_frame_idx: u64,
    start_time: Instant,
    playing: bool,
    // Audio
    sink: Option<rodio::Sink>,
    _stream: Option<rodio::OutputStream>,
    // Decoder management
    _decoder_handle: Option<JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,
    audio_child: Option<Child>,
}

fn probe_video(path: &Path) -> Option<VideoInfo> {
    let path_str = path.to_string_lossy();
    let output = Command::new("ffprobe")
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_streams",
            "-show_format",
            &path_str,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000) // CREATE_NO_WINDOW on Windows
        .output()
        .ok()?;

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;

    // Find video stream
    let streams = json.get("streams")?.as_array()?;
    let video_stream = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|v| v.as_str()) == Some("video"))?;

    let width = video_stream.get("width")?.as_u64()? as u32;
    let height = video_stream.get("height")?.as_u64()? as u32;

    // Parse fps from r_frame_rate (e.g., "30/1" or "24000/1001")
    let fps = parse_frame_rate(
        video_stream
            .get("r_frame_rate")
            .and_then(|v| v.as_str())
            .unwrap_or("24/1"),
    );

    // Duration from format or stream
    let duration_secs = json
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .or_else(|| {
            video_stream
                .get("duration")
                .and_then(|d| d.as_str())
                .and_then(|s| s.parse::<f64>().ok())
        })
        .unwrap_or(0.0);

    Some(VideoInfo {
        width,
        height,
        fps,
        duration_secs,
    })
}

fn parse_frame_rate(s: &str) -> f64 {
    if let Some((num, den)) = s.split_once('/') {
        let n: f64 = num.parse().unwrap_or(24.0);
        let d: f64 = den.parse().unwrap_or(1.0);
        if d > 0.0 { n / d } else { 24.0 }
    } else {
        s.parse().unwrap_or(24.0)
    }
}

/// Calculate scaled dimensions to fit within MAX_PREVIEW_WIDTH while maintaining aspect ratio.
/// Height is rounded to even number (required by ffmpeg).
fn scaled_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width <= MAX_PREVIEW_WIDTH {
        // Ensure even height
        let h = if height % 2 != 0 { height + 1 } else { height };
        return (width, h);
    }
    let scale = MAX_PREVIEW_WIDTH as f64 / width as f64;
    let new_w = MAX_PREVIEW_WIDTH;
    let mut new_h = (height as f64 * scale) as u32;
    if new_h % 2 != 0 {
        new_h += 1;
    }
    (new_w, new_h)
}

pub fn load(path: &Path, ctx: &egui::Context) -> Option<VideoPreview> {
    let info = probe_video(path)?;

    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let (scaled_w, scaled_h) = scaled_dimensions(info.width, info.height);
    let frame_size = (scaled_w * scaled_h * 4) as usize;

    let frame_buffer = Arc::new(Mutex::new(FrameBuffer {
        frames: VecDeque::new(),
        finished: false,
    }));

    let stop_flag = Arc::new(AtomicBool::new(false));

    // Start video frame decoder thread
    let decoder_handle = start_frame_decoder(
        path.to_path_buf(),
        scaled_w,
        scaled_h,
        frame_size,
        Arc::clone(&frame_buffer),
        Arc::clone(&stop_flag),
        ctx.clone(),
    );

    // Start audio playback
    let (stream, stream_handle) = match rodio::OutputStream::try_default() {
        Ok((s, h)) => (Some(s), Some(h)),
        Err(_) => (None, None),
    };

    let (sink, audio_child) = if let Some(ref handle) = stream_handle {
        start_audio_playback(path, handle)
    } else {
        (None, None)
    };

    Some(VideoPreview {
        title,
        width: scaled_w,
        height: scaled_h,
        fps: info.fps,
        duration_secs: info.duration_secs,
        frame_buffer,
        current_texture: None,
        current_frame_idx: 0,
        start_time: Instant::now(),
        playing: true,
        sink,
        _stream: stream,
        _decoder_handle: decoder_handle,
        stop_flag,
        audio_child,
    })
}

fn start_frame_decoder(
    path: PathBuf,
    width: u32,
    height: u32,
    frame_size: usize,
    buffer: Arc<Mutex<FrameBuffer>>,
    stop_flag: Arc<AtomicBool>,
    ctx: egui::Context,
) -> Option<JoinHandle<()>> {
    let path_str = path.to_string_lossy().to_string();

    let handle = thread::spawn(move || {
        let mut child = match Command::new("ffmpeg")
            .args([
                "-i", &path_str,
                "-f", "rawvideo",
                "-pix_fmt", "rgba",
                "-vf", &format!("scale={}:{}", width, height),
                "-v", "quiet",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return,
        };

        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => return,
        };

        let mut reader = std::io::BufReader::with_capacity(frame_size * 2, stdout);
        let mut frame_idx: u64 = 0;

        loop {
            if stop_flag.load(Ordering::Relaxed) {
                let _ = child.kill();
                break;
            }

            // Wait if buffer is full
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    let _ = child.kill();
                    return;
                }
                let len = buffer.lock().map(|b| b.frames.len()).unwrap_or(0);
                if len < FRAME_BUFFER_CAPACITY {
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(5));
            }

            // Read one frame
            let mut rgba = vec![0u8; frame_size];
            match reader.read_exact(&mut rgba) {
                Ok(()) => {}
                Err(_) => {
                    // EOF or error
                    if let Ok(mut buf) = buffer.lock() {
                        buf.finished = true;
                    }
                    break;
                }
            }

            if let Ok(mut buf) = buffer.lock() {
                buf.frames.push_back(DecodedFrame {
                    rgba,
                    index: frame_idx,
                });
            }

            frame_idx += 1;
            ctx.request_repaint();
        }

        let _ = child.wait();
    });

    Some(handle)
}

/// Custom rodio Source that reads raw PCM s16le from an ffmpeg pipe.
struct PcmPipeSource {
    reader: std::io::BufReader<std::process::ChildStdout>,
    channels: u16,
    sample_rate: u32,
}

impl Iterator for PcmPipeSource {
    type Item = i16;
    fn next(&mut self) -> Option<i16> {
        let mut buf = [0u8; 2];
        self.reader.read_exact(&mut buf).ok()?;
        Some(i16::from_le_bytes(buf))
    }
}

impl Source for PcmPipeSource {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { self.channels }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> { None }
}

fn start_audio_playback(
    path: &Path,
    stream_handle: &rodio::OutputStreamHandle,
) -> (Option<rodio::Sink>, Option<Child>) {
    let path_str = path.to_string_lossy().to_string();

    let mut child = match Command::new("ffmpeg")
        .args([
            "-i", &path_str,
            "-f", "s16le",
            "-acodec", "pcm_s16le",
            "-ac", "2",
            "-ar", "44100",
            "-v", "quiet",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return (None, None),
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => return (None, Some(child)),
    };

    let source = PcmPipeSource {
        reader: std::io::BufReader::with_capacity(16384, stdout),
        channels: 2,
        sample_rate: 44100,
    };

    let sink = match rodio::Sink::try_new(stream_handle) {
        Ok(s) => s,
        Err(_) => return (None, Some(child)),
    };

    sink.append(source);

    (Some(sink), Some(child))
}

impl VideoPreview {
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        // Try to advance to the correct frame
        self.advance_frame(ui.ctx());

        ui.vertical(|ui| {
            ui.strong(&self.title);
            ui.label(format!(
                "{}x{} {:.1}fps {:.1}s",
                self.width, self.height, self.fps, self.duration_secs
            ));
            ui.separator();

            // Video frame display
            if let Some(texture) = &self.current_texture {
                egui::ScrollArea::both().show(ui, |ui| {
                    let available = ui.available_size();
                    let img_w = self.width as f32;
                    let img_h = self.height as f32;
                    let scale_x = available.x / img_w;
                    let scale_y = (available.y - 50.0).max(60.0) / img_h;
                    let scale = scale_x.min(scale_y).min(1.0);

                    let display_size = egui::vec2(img_w * scale, img_h * scale);
                    ui.image(egui::load::SizedTexture::new(texture.id(), display_size));
                });
            } else {
                let available = ui.available_size();
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(available.x, (available.y - 50.0).max(60.0)),
                    egui::Sense::hover(),
                );
                ui.painter_at(rect).text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Loading...",
                    egui::FontId::monospace(14.0),
                    egui::Color32::GRAY,
                );
            }

            ui.add_space(4.0);

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

            // Request repaint while playing
            if self.playing {
                let frame_interval = if self.fps > 0.0 {
                    (1000.0 / self.fps) as u64
                } else {
                    33
                };
                ui.ctx().request_repaint_after(
                    std::time::Duration::from_millis(frame_interval.max(16)),
                );
            }
        });
    }

    fn advance_frame(&mut self, ctx: &egui::Context) {
        if !self.playing {
            return;
        }

        let elapsed = self.start_time.elapsed().as_secs_f64();
        let target_idx = (elapsed * self.fps) as u64;

        if let Ok(mut buf) = self.frame_buffer.lock() {
            // Drop frames that are behind
            while buf
                .frames
                .front()
                .is_some_and(|f| f.index < target_idx)
            {
                if buf.frames.len() > 1 {
                    buf.frames.pop_front();
                } else {
                    break;
                }
            }

            // Take the current frame
            if let Some(frame) = buf.frames.front() {
                if frame.index != self.current_frame_idx || self.current_texture.is_none() {
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [self.width as usize, self.height as usize],
                        &frame.rgba,
                    );

                    let texture = ctx.load_texture(
                        format!("video_frame_{}", frame.index),
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );

                    self.current_texture = Some(texture);
                    self.current_frame_idx = frame.index;
                }
            }

            // Check if finished
            if buf.finished && buf.frames.is_empty() {
                self.playing = false;
            }
        }
    }

    fn toggle_play(&mut self) {
        if self.playing {
            self.playing = false;
            if let Some(sink) = &self.sink {
                sink.pause();
            }
        } else {
            self.playing = true;
            self.start_time = Instant::now();
            if let Some(sink) = &self.sink {
                sink.play();
            }
        }
    }

    fn current_position(&self) -> f64 {
        if self.playing {
            self.start_time.elapsed().as_secs_f64().min(self.duration_secs)
        } else {
            (self.current_frame_idx as f64 / self.fps).min(self.duration_secs)
        }
    }

    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        self.playing = false;
        if let Some(sink) = &self.sink {
            sink.stop();
        }
        self.sink = None;
        if let Some(mut child) = self.audio_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for VideoPreview {
    fn drop(&mut self) {
        self.stop();
    }
}

fn format_time(secs: f64) -> String {
    let total = secs as u32;
    let m = total / 60;
    let s = total % 60;
    format!("{}:{:02}", m, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_video_file() {
        assert!(is_video_file(Path::new("movie.mp4")));
        assert!(is_video_file(Path::new("movie.MP4")));
        assert!(is_video_file(Path::new("movie.avi")));
        assert!(is_video_file(Path::new("movie.mkv")));
        assert!(is_video_file(Path::new("movie.webm")));
        assert!(!is_video_file(Path::new("image.png")));
        assert!(!is_video_file(Path::new("audio.wav")));
        assert!(!is_video_file(Path::new("file.txt")));
    }

    #[test]
    fn test_parse_frame_rate() {
        assert!((parse_frame_rate("30/1") - 30.0).abs() < 0.001);
        assert!((parse_frame_rate("24000/1001") - 23.976).abs() < 0.1);
        assert!((parse_frame_rate("25") - 25.0).abs() < 0.001);
        assert!((parse_frame_rate("0/0") - 24.0).abs() < 0.001); // fallback
    }

    #[test]
    fn test_scaled_dimensions() {
        // Under max width - no change (height made even)
        assert_eq!(scaled_dimensions(640, 480), (640, 480));
        assert_eq!(scaled_dimensions(640, 481), (640, 482)); // odd height rounded up

        // Over max width - scaled down
        let (w, h) = scaled_dimensions(1920, 1080);
        assert_eq!(w, 800);
        assert!(h % 2 == 0); // even height
        assert!((h as f64 - 450.0).abs() < 2.0); // roughly 1080 * (800/1920)
    }

    #[test]
    fn test_format_time() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(65.0), "1:05");
        assert_eq!(format_time(3661.0), "61:01");
    }

    #[test]
    fn test_video_extensions_coverage() {
        let expected = ["mp4", "avi", "mkv", "webm", "mov", "wmv", "flv", "m4v", "mpg", "mpeg", "ts"];
        for ext in &expected {
            assert!(
                is_video_file(Path::new(&format!("test.{}", ext))),
                "Expected {} to be recognized as video",
                ext
            );
        }
    }
}
