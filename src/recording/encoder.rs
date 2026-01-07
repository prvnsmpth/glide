//! FFmpeg-based video encoding for raw video frames
//!
//! This module provides video encoding by piping raw BGRA frames to FFmpeg's stdin.

use anyhow::{Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, Stdio};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// FFmpeg video encoder that accepts raw BGRA frames via stdin
pub struct VideoEncoder {
    child: Child,
    stdin: std::process::ChildStdin,
    width: u32,
    height: u32,
    frame_count: u64,
}

impl VideoEncoder {
    /// Spawn a new FFmpeg encoder process
    ///
    /// # Arguments
    /// * `width` - Frame width in pixels
    /// * `height` - Frame height in pixels
    /// * `fps` - Frames per second (typically 60)
    /// * `output` - Output file path (.mp4)
    pub fn new(width: u32, height: u32, fps: u32, output: &Path) -> Result<Self> {
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            // Use wall clock for timestamps - frames get real-time timing
            "-use_wallclock_as_timestamps",
            "1",
            // Input format: raw video
            "-f",
            "rawvideo",
            // Pixel format: BGRA (what ScreenCaptureKit gives us)
            "-pix_fmt",
            "bgra",
            // Frame size
            "-s",
            &format!("{}x{}", width, height),
            // Expected frame rate (hint for timing)
            "-framerate",
            &fps.to_string(),
            // Read from stdin
            "-i",
            "pipe:0",
            // Output codec: H.264
            "-c:v",
            "libx264",
            // Preset: ultrafast for real-time encoding
            "-preset",
            "ultrafast",
            // Quality: good quality
            "-crf",
            "18",
            // Output pixel format
            "-pix_fmt",
            "yuv420p",
            // Overwrite output
            "-y",
        ])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

        // Put FFmpeg in its own process group so it doesn't receive SIGINT
        // when user presses Ctrl+C. We control FFmpeg by closing stdin.
        #[cfg(unix)]
        cmd.process_group(0);

        let mut child = cmd.spawn().context("Failed to start FFmpeg encoder")?;

        let stdin = child.stdin.take().context("Failed to get FFmpeg stdin")?;

        Ok(Self {
            child,
            stdin,
            width,
            height,
            frame_count: 0,
        })
    }

    /// Write a raw BGRA frame to the encoder
    ///
    /// The frame data must be exactly `width * height * 4` bytes.
    pub fn write_frame(&mut self, frame_data: &[u8]) -> Result<()> {
        let expected_size = (self.width * self.height * 4) as usize;
        if frame_data.len() != expected_size {
            anyhow::bail!(
                "Frame size mismatch: expected {} bytes, got {}",
                expected_size,
                frame_data.len()
            );
        }

        self.stdin
            .write_all(frame_data)
            .context("Failed to write frame to FFmpeg")?;

        self.frame_count += 1;
        Ok(())
    }

    /// Get the number of frames written
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Finish encoding and wait for FFmpeg to complete
    pub fn finish(mut self) -> Result<()> {
        // Close stdin to signal end of input
        drop(self.stdin);

        // Wait for FFmpeg to finish
        let status = self
            .child
            .wait()
            .context("Failed to wait for FFmpeg to finish")?;

        // Check if FFmpeg exited successfully or was killed by SIGINT (Ctrl+C)
        // When user presses Ctrl+C, FFmpeg receives signal 2 which is expected
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                if signal == 2 {
                    // SIGINT is expected when user presses Ctrl+C
                    return Ok(());
                }
            }
        }

        if !status.success() {
            // Try to read stderr for error info
            if let Some(ref mut stderr) = self.child.stderr {
                use std::io::Read;
                let mut error_output = String::new();
                let _ = stderr.read_to_string(&mut error_output);
                if !error_output.is_empty() {
                    // Get last few lines
                    let last_lines: Vec<&str> = error_output.lines().rev().take(5).collect();
                    let error_context = last_lines.into_iter().rev().collect::<Vec<_>>().join("\n");
                    anyhow::bail!("FFmpeg encoding failed:\n{}", error_context);
                }
            }
            anyhow::bail!("FFmpeg encoding failed with status: {}", status);
        }

        Ok(())
    }
}

/// Check if FFmpeg is available
pub fn check_ffmpeg() -> Result<()> {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("FFmpeg not found. Please install it with: brew install ffmpeg")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_ffmpeg() {
        // This test will pass if FFmpeg is installed
        let result = check_ffmpeg();
        assert!(result.is_ok(), "FFmpeg should be available");
    }
}
