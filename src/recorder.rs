use crate::cursor::CursorTracker;
use crate::display::{list_displays, DisplayInfo};
use crate::metadata::RecordingMetadata;
use crate::window::WindowInfo;
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub fn record_display(display: &DisplayInfo, output: &Path, capture_system_cursor: bool) -> Result<()> {
    // Check FFmpeg availability
    check_ffmpeg()?;

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("Failed to set Ctrl+C handler")?;

    println!("Recording screen to {}", output.display());
    println!("Press Ctrl+C to stop recording...\n");

    // Spawn FFmpeg process
    let mut child = spawn_ffmpeg(display.avf_index, output, capture_system_cursor)?;

    // Start cursor tracking (offset will be calculated during processing)
    let mut cursor_tracker = CursorTracker::new();
    cursor_tracker.start()?;

    // Progress indicator
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} Recording... {elapsed_precise}")
            .unwrap(),
    );

    let start = Instant::now();

    // Wait for Ctrl+C or process exit
    while running.load(Ordering::SeqCst) {
        pb.tick();
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Check if FFmpeg exited on its own
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    pb.finish_and_clear();
                    let _ = cursor_tracker.stop();
                    anyhow::bail!("FFmpeg exited with error: {}", status);
                }
                break;
            }
            Ok(None) => {} // Still running
            Err(e) => {
                pb.finish_and_clear();
                let _ = cursor_tracker.stop();
                anyhow::bail!("Error checking FFmpeg status: {}", e);
            }
        }
    }

    pb.finish_and_clear();

    // Stop cursor tracking and get events + duration
    let (cursor_events, cursor_duration) = cursor_tracker.stop();

    // Send 'q' to FFmpeg to stop gracefully
    stop_ffmpeg(&mut child)?;

    // Save metadata
    let mut metadata = RecordingMetadata::new_display(display.index, display.width, display.height);
    metadata.cursor_events = cursor_events;
    metadata.cursor_tracking_duration = cursor_duration;
    metadata.save(output)?;

    let duration = start.elapsed();
    println!(
        "\nRecording complete! Duration: {:.1}s",
        duration.as_secs_f64()
    );
    println!("Saved to: {}", output.display());
    println!(
        "Metadata: {} ({} cursor events)",
        output.with_extension("json").display(),
        metadata.cursor_events.len()
    );

    Ok(())
}

pub fn record_window(window: &WindowInfo, output: &Path, capture_system_cursor: bool) -> Result<()> {
    check_ffmpeg()?;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("Failed to set Ctrl+C handler")?;

    println!(
        "Recording window: {} - {} ({}x{})",
        window.owner, window.name, window.bounds.2, window.bounds.3
    );
    println!("Press Ctrl+C to stop recording...\n");

    // Get the display's AVF index and scale factor (use main display for now)
    let displays = list_displays()?;
    let display = displays.into_iter().find(|d| d.is_main).unwrap();

    // Spawn FFmpeg process
    let mut child = spawn_ffmpeg_window(window, display.avf_index, display.scale_factor, output, capture_system_cursor)?;

    // Start cursor tracking (offset will be calculated during processing)
    let mut cursor_tracker = CursorTracker::new();
    cursor_tracker.start()?;

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} Recording... {elapsed_precise}")
            .unwrap(),
    );

    let start = Instant::now();

    while running.load(Ordering::SeqCst) {
        pb.tick();
        std::thread::sleep(std::time::Duration::from_millis(100));

        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    pb.finish_and_clear();
                    let _ = cursor_tracker.stop();
                    anyhow::bail!("FFmpeg exited with error: {}", status);
                }
                break;
            }
            Ok(None) => {}
            Err(e) => {
                pb.finish_and_clear();
                let _ = cursor_tracker.stop();
                anyhow::bail!("Error checking FFmpeg status: {}", e);
            }
        }
    }

    pb.finish_and_clear();

    let (cursor_events, cursor_duration) = cursor_tracker.stop();
    stop_ffmpeg(&mut child)?;

    let mut metadata = RecordingMetadata::new_window(
        window.id,
        window.bounds.2,
        window.bounds.3,
        window.bounds.0,  // x offset
        window.bounds.1,  // y offset
    );
    metadata.cursor_events = cursor_events;
    metadata.cursor_tracking_duration = cursor_duration;
    metadata.save(output)?;

    let duration = start.elapsed();
    println!(
        "\nRecording complete! Duration: {:.1}s",
        duration.as_secs_f64()
    );
    println!("Saved to: {}", output.display());
    println!(
        "Metadata: {} ({} cursor events)",
        output.with_extension("json").display(),
        metadata.cursor_events.len()
    );

    Ok(())
}

fn check_ffmpeg() -> Result<()> {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("FFmpeg not found. Please install it with: brew install ffmpeg")?;
    Ok(())
}

fn spawn_ffmpeg(avf_index: usize, output: &Path, capture_cursor: bool) -> Result<Child> {
    // AVFoundation device index (video:audio, "none" for no audio)
    let input_device = format!("{}:none", avf_index);
    let capture_cursor_val = if capture_cursor { "1" } else { "0" };

    let child = Command::new("ffmpeg")
        .args([
            "-f",
            "avfoundation",
            "-framerate",
            "60",
            "-capture_cursor",
            capture_cursor_val,
            "-i",
            &input_device,
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-y", // Overwrite output
        ])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start FFmpeg")?;

    Ok(child)
}

fn spawn_ffmpeg_window(window: &WindowInfo, avf_index: usize, scale_factor: f64, output: &Path, capture_cursor: bool) -> Result<Child> {
    // FFmpeg AVFoundation doesn't support direct window capture
    // So we capture the display and crop to window bounds
    // Scale coordinates for Retina displays (window bounds are in points, capture is in pixels)
    let input_device = format!("{}:none", avf_index);
    let capture_cursor_val = if capture_cursor { "1" } else { "0" };
    let (x, y, w, h) = window.bounds;
    let scaled_x = (x as f64 * scale_factor) as i32;
    let scaled_y = (y as f64 * scale_factor) as i32;
    let scaled_w = (w as f64 * scale_factor) as u32;
    let scaled_h = (h as f64 * scale_factor) as u32;
    let crop_filter = format!("crop={}:{}:{}:{}", scaled_w, scaled_h, scaled_x, scaled_y);

    let child = Command::new("ffmpeg")
        .args([
            "-f",
            "avfoundation",
            "-framerate",
            "60",
            "-capture_cursor",
            capture_cursor_val,
            "-i",
            &input_device,
            "-vf",
            &crop_filter,
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start FFmpeg for window capture")?;

    Ok(child)
}

fn stop_ffmpeg(child: &mut Child) -> Result<()> {
    // Send 'q' to FFmpeg stdin to stop gracefully
    if let Some(ref mut stdin) = child.stdin {
        use std::io::Write;
        let _ = stdin.write_all(b"q");
    }

    // Wait for FFmpeg to finish
    let status = child.wait().context("Failed to wait for FFmpeg")?;

    if !status.success() {
        // Read stderr for error info
        if let Some(ref mut stderr) = child.stderr {
            let reader = BufReader::new(stderr);
            let last_lines: Vec<_> = reader.lines().filter_map(|l| l.ok()).collect();
            let error_context = last_lines
                .iter()
                .rev()
                .take(5)
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            if !error_context.is_empty() {
                eprintln!("FFmpeg output:\n{}", error_context);
            }
        }
    }

    Ok(())
}
