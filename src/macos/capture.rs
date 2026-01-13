//! ScreenCaptureKit-based screen and window capture
//!
//! This module provides screen capture using Apple's ScreenCaptureKit framework,
//! which properly supports cursor visibility control.

use anyhow::{Context, Result};
use screencapturekit::cm::CMTime;
use screencapturekit::cv::CVPixelBufferLockFlags;
use screencapturekit::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;

/// A captured video frame with raw BGRA pixel data
pub struct CapturedFrame {
    /// Raw BGRA pixel data
    pub data: Vec<u8>,
    /// Frame width in pixels
    pub width: usize,
    /// Frame height in pixels
    pub height: usize,
    /// Presentation timestamp in seconds
    pub timestamp: f64,
}

/// Capture configuration
pub struct CaptureConfig {
    /// Whether to show the system cursor in the capture
    pub show_cursor: bool,
    /// Target width (0 = native resolution)
    pub width: u32,
    /// Target height (0 = native resolution)
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

/// Frame handler that sends captured frames through a channel
struct FrameHandler {
    sender: SyncSender<CapturedFrame>,
    running: Arc<AtomicBool>,
}

impl SCStreamOutputTrait for FrameHandler {
    fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
        if !self.running.load(Ordering::Relaxed) {
            return;
        }

        if of_type != SCStreamOutputType::Screen {
            return;
        }

        // Get the pixel buffer from the sample
        let Some(pixel_buffer) = sample.image_buffer() else {
            return;
        };

        // Lock the pixel buffer for reading
        let Ok(guard) = pixel_buffer.lock(CVPixelBufferLockFlags::READ_ONLY) else {
            return;
        };

        let width = guard.width();
        let height = guard.height();
        let bytes_per_row = guard.bytes_per_row();
        let pixels = guard.as_slice();

        // Get timestamp
        let pts = sample.presentation_timestamp();
        let timestamp = if pts.timescale > 0 {
            pts.value as f64 / pts.timescale as f64
        } else {
            0.0
        };

        // Copy pixel data, stripping any row padding
        // CVPixelBuffer may have bytes_per_row > width * 4 for memory alignment
        let expected_bytes_per_row = width * 4; // BGRA = 4 bytes per pixel
        let data = if bytes_per_row == expected_bytes_per_row {
            // No padding, copy directly
            pixels.to_vec()
        } else {
            // Has padding, copy row by row
            let mut data = Vec::with_capacity(width * height * 4);
            for y in 0..height {
                let row_start = y * bytes_per_row;
                let row_end = row_start + expected_bytes_per_row;
                if row_end <= pixels.len() {
                    data.extend_from_slice(&pixels[row_start..row_end]);
                }
            }
            data
        };

        let frame = CapturedFrame {
            data,
            width,
            height,
            timestamp,
        };

        // Send frame (ignore if receiver is closed)
        let _ = self.sender.try_send(frame);
    }
}

/// Active screen capture session
pub struct CaptureSession {
    stream: SCStream,
    receiver: Receiver<CapturedFrame>,
    running: Arc<AtomicBool>,
    pub width: u32,
    pub height: u32,
}

impl CaptureSession {
    /// Receive the next captured frame (blocks until available)
    pub fn recv(&self) -> Option<CapturedFrame> {
        self.receiver.recv().ok()
    }

    /// Try to receive a frame without blocking
    pub fn try_recv(&self) -> Option<CapturedFrame> {
        self.receiver.try_recv().ok()
    }

    /// Check if the capture is still running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Stop the capture session
    pub fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        self.stream
            .stop_capture()
            .map_err(|e| anyhow::anyhow!("Failed to stop capture: {:?}", e))
    }
}

/// Find a display by index from ScreenCaptureKit
pub fn find_display(display_index: usize) -> Result<SCDisplay> {
    let content = SCShareableContent::get()
        .context("Failed to get shareable content from ScreenCaptureKit")?;

    let displays = content.displays();
    displays
        .into_iter()
        .nth(display_index)
        .ok_or_else(|| anyhow::anyhow!("Display {} not found", display_index))
}

/// Find a window by ID from ScreenCaptureKit
pub fn find_window(window_id: u32) -> Result<SCWindow> {
    let content = SCShareableContent::get()
        .context("Failed to get shareable content from ScreenCaptureKit")?;

    let windows = content.windows();
    windows
        .into_iter()
        .find(|w| w.window_id() == window_id)
        .ok_or_else(|| anyhow::anyhow!("Window {} not found", window_id))
}

/// Start capturing a display
pub fn start_display_capture(
    display: &SCDisplay,
    config: &CaptureConfig,
) -> Result<CaptureSession> {
    // Create content filter for the display
    let filter = SCContentFilter::create()
        .with_display(display)
        .with_excluding_windows(&[])
        .build();

    start_capture_with_filter(filter, config)
}

/// Start capturing a specific window
pub fn start_window_capture(window: &SCWindow, config: &CaptureConfig) -> Result<CaptureSession> {
    // Create content filter for the window
    let filter = SCContentFilter::create().with_window(window).build();

    start_capture_with_filter(filter, config)
}

/// Internal function to start capture with a given filter
fn start_capture_with_filter(
    filter: SCContentFilter,
    config: &CaptureConfig,
) -> Result<CaptureSession> {
    // Frame interval for 60 FPS
    let frame_interval = CMTime::new(1, 60);

    // Determine dimensions
    // If config specifies 0, we'll use native resolution
    // The actual dimensions will be determined from the first frame
    let (width, height) = if config.width > 0 && config.height > 0 {
        (config.width, config.height)
    } else {
        // Use a reasonable default that will be adjusted
        (1920, 1080)
    };

    // Configure the stream
    let stream_config = SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height)
        .with_pixel_format(PixelFormat::BGRA)
        .with_minimum_frame_interval(&frame_interval)
        .with_shows_cursor(config.show_cursor);

    // Create the stream
    let mut stream = SCStream::new(&filter, &stream_config);

    // Set up the channel for frames (buffer a few frames)
    let (sender, receiver) = mpsc::sync_channel(3);
    let running = Arc::new(AtomicBool::new(true));

    // Add the frame handler
    let handler = FrameHandler {
        sender,
        running: running.clone(),
    };
    stream.add_output_handler(handler, SCStreamOutputType::Screen);

    // Start capture
    stream
        .start_capture()
        .map_err(|e| anyhow::anyhow!("Failed to start capture: {:?}", e))?;

    Ok(CaptureSession {
        stream,
        receiver,
        running,
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
