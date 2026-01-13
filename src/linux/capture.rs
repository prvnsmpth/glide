//! Linux X11 screen capture using FFmpeg x11grab

use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread;

use crate::linux::display::DisplayInfo;
use crate::linux::window::WindowInfo;

/// A captured video frame with raw BGRA pixel data
pub struct CapturedFrame {
    pub data: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub timestamp: f64,
}

/// Capture configuration
pub struct CaptureConfig {
    pub show_cursor: bool,
    pub width: u32,
    pub height: u32,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            show_cursor: false,
            width: 0,
            height: 0,
        }
    }
}

/// Wrapper type to mimic ScreenCaptureKit's display handle
pub struct X11Display {
    pub index: usize,
    pub display_string: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl X11Display {
    pub fn frame(&self) -> DisplayFrame {
        DisplayFrame {
            width: self.width as f64,
            height: self.height as f64,
        }
    }
}

pub struct DisplayFrame {
    pub width: f64,
    pub height: f64,
}

/// Wrapper type to mimic ScreenCaptureKit's window handle
pub struct X11Window {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub display_string: String,
}

impl X11Window {
    pub fn window_id(&self) -> u32 {
        self.id
    }

    pub fn frame(&self) -> DisplayFrame {
        DisplayFrame {
            width: self.width as f64,
            height: self.height as f64,
        }
    }
}

/// Find a display by index
pub fn find_display(display_index: usize) -> Result<X11Display> {
    let displays = crate::linux::list_displays()?;
    let display = displays
        .into_iter()
        .find(|d| d.index == display_index)
        .ok_or_else(|| anyhow::anyhow!("Display {} not found", display_index))?;

    Ok(X11Display {
        index: display.index,
        display_string: display.display_string,
        x: display.x,
        y: display.y,
        width: display.width,
        height: display.height,
    })
}

/// Find a window by ID
pub fn find_window(window_id: u32) -> Result<X11Window> {
    let windows = crate::linux::list_windows()?;
    let window = windows
        .into_iter()
        .find(|w| w.id == window_id)
        .ok_or_else(|| anyhow::anyhow!("Window {} not found", window_id))?;

    let display_string = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());

    Ok(X11Window {
        id: window.id,
        x: window.bounds.0,
        y: window.bounds.1,
        width: window.bounds.2,
        height: window.bounds.3,
        display_string,
    })
}

/// Active screen capture session
pub struct CaptureSession {
    ffmpeg_process: Child,
    receiver: Receiver<CapturedFrame>,
    running: Arc<AtomicBool>,
    reader_thread: Option<thread::JoinHandle<()>>,
    pub width: u32,
    pub height: u32,
}

impl CaptureSession {
    pub fn recv(&self) -> Option<CapturedFrame> {
        self.receiver.recv().ok()
    }

    pub fn try_recv(&self) -> Option<CapturedFrame> {
        self.receiver.try_recv().ok()
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);

        // Send SIGINT to FFmpeg for graceful shutdown
        #[cfg(unix)]
        {
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;

            if let Some(pid) = self.ffmpeg_process.id() {
                let _ = kill(Pid::from_raw(pid as i32), Signal::SIGINT);
            }
        }

        // Wait a bit for graceful shutdown
        thread::sleep(std::time::Duration::from_millis(100));

        // Force kill if still running
        let _ = self.ffmpeg_process.kill();
        let _ = self.ffmpeg_process.wait();

        // Wait for reader thread
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }

        Ok(())
    }
}

/// Start capturing a display
pub fn start_display_capture(display: &X11Display, config: &CaptureConfig) -> Result<CaptureSession> {
    let width = if config.width > 0 { config.width } else { display.width };
    let height = if config.height > 0 { config.height } else { display.height };

    // Build FFmpeg command for x11grab
    // Format: ffmpeg -f x11grab -framerate 60 -video_size WxH -i :0+X,Y -pix_fmt bgra -f rawvideo -
    let display_input = format!(
        "{}+{},{}",
        display.display_string, display.x, display.y
    );

    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-f", "x11grab",
        "-framerate", "60",
        "-video_size", &format!("{}x{}", width, height),
    ]);

    // Add cursor visibility option
    if config.show_cursor {
        cmd.args(["-draw_mouse", "1"]);
    } else {
        cmd.args(["-draw_mouse", "0"]);
    }

    cmd.args([
        "-i", &display_input,
        "-pix_fmt", "bgra",
        "-f", "rawvideo",
        "-",
    ]);

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());

    start_capture_process(cmd, width, height)
}

/// Start capturing a specific window
pub fn start_window_capture(window: &X11Window, config: &CaptureConfig) -> Result<CaptureSession> {
    let width = if config.width > 0 { config.width } else { window.width };
    let height = if config.height > 0 { config.height } else { window.height };

    // For window capture, we can use the -window_id option if available,
    // or fall back to capturing the window's region
    let display_input = format!(
        "{}+{},{}",
        window.display_string, window.x, window.y
    );

    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-f", "x11grab",
        "-framerate", "60",
        "-video_size", &format!("{}x{}", width, height),
    ]);

    // Add cursor visibility option
    if config.show_cursor {
        cmd.args(["-draw_mouse", "1"]);
    } else {
        cmd.args(["-draw_mouse", "0"]);
    }

    cmd.args([
        "-i", &display_input,
        "-pix_fmt", "bgra",
        "-f", "rawvideo",
        "-",
    ]);

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());

    start_capture_process(cmd, width, height)
}

/// Start the FFmpeg capture process
fn start_capture_process(mut cmd: Command, width: u32, height: u32) -> Result<CaptureSession> {
    let mut ffmpeg_process = cmd.spawn().context("Failed to start FFmpeg for capture")?;

    let stdout = ffmpeg_process
        .stdout
        .take()
        .context("Failed to get FFmpeg stdout")?;

    let (sender, receiver) = mpsc::sync_channel(3);
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    let frame_size = (width * height * 4) as usize; // BGRA = 4 bytes per pixel
    let w = width as usize;
    let h = height as usize;

    let reader_thread = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut frame_buffer = vec![0u8; frame_size];
        let mut frame_count: u64 = 0;

        while running_clone.load(Ordering::Relaxed) {
            // Read exactly one frame
            match reader.read_exact(&mut frame_buffer) {
                Ok(()) => {
                    let timestamp = frame_count as f64 / 60.0;
                    frame_count += 1;

                    let frame = CapturedFrame {
                        data: frame_buffer.clone(),
                        width: w,
                        height: h,
                        timestamp,
                    };

                    // Send frame (stop if receiver is closed)
                    if sender.try_send(frame).is_err() {
                        // Channel full or closed, wait a bit
                        thread::sleep(std::time::Duration::from_millis(1));
                        continue;
                    }
                }
                Err(_) => {
                    // EOF or error, stop reading
                    break;
                }
            }
        }
    });

    Ok(CaptureSession {
        ffmpeg_process,
        receiver,
        running,
        reader_thread: Some(reader_thread),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_config_default() {
        let config = CaptureConfig::default();
        assert!(!config.show_cursor);
        assert_eq!(config.width, 0);
        assert_eq!(config.height, 0);
    }
}
