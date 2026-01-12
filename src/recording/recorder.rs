use crate::macos::{list_displays, CursorTracker, DisplayInfo, WindowInfo};
use crate::recording::capture::{self, CaptureConfig};
use crate::recording::encoder::{self, VideoEncoder};
use crate::recording::metadata::RecordingMetadata;
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub fn record_display(display: &DisplayInfo, output: &Path, capture_system_cursor: bool) -> Result<()> {
    // Check FFmpeg availability (still needed for encoding)
    encoder::check_ffmpeg()?;

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("Failed to set Ctrl+C handler")?;

    println!("Recording screen to {}", output.display());
    println!("Press Ctrl+C to stop recording...\n");

    // Find the display in ScreenCaptureKit
    let sc_display = capture::find_display(display.index)
        .context("Failed to find display in ScreenCaptureKit")?;

    // Get the display frame for dimensions
    let frame = sc_display.frame();
    let width = (frame.width * display.scale_factor) as u32;
    let height = (frame.height * display.scale_factor) as u32;

    // Configure capture
    let config = CaptureConfig {
        show_cursor: capture_system_cursor,
        width,
        height,
    };

    // Start ScreenCaptureKit capture
    let mut capture_session = capture::start_display_capture(&sc_display, &config)
        .context("Failed to start screen capture")?;

    // Start cursor tracking and record the start instant for offset calculation
    let mut cursor_tracker = CursorTracker::new();
    let cursor_start_instant = Instant::now();
    cursor_tracker.start()?;

    // Progress indicator
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} Recording... {elapsed_precise}")
            .unwrap(),
    );

    let start = Instant::now();

    // Wait for first frame to get actual dimensions
    let first_frame = loop {
        if !running.load(Ordering::SeqCst) {
            pb.finish_and_clear();
            let _ = cursor_tracker.stop();
            capture_session.stop()?;
            anyhow::bail!("Recording cancelled before first frame");
        }

        if let Some(frame) = capture_session.try_recv() {
            break frame;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };

    let actual_width = first_frame.width as u32;
    let actual_height = first_frame.height as u32;

    // Calculate offset: time from cursor tracking start to first video frame
    // This is the precise timing relationship between cursor events and video frames
    let cursor_to_video_offset = cursor_start_instant.elapsed().as_secs_f64();

    // Start FFmpeg encoder with actual dimensions
    let mut encoder = VideoEncoder::new(actual_width, actual_height, 60, output)
        .context("Failed to start video encoder")?;

    // Write the first frame
    encoder.write_frame(&first_frame.data)?;
    let mut frame_count: u64 = 1;

    // Main recording loop
    while running.load(Ordering::SeqCst) {
        pb.tick();

        // Try to receive a frame
        if let Some(frame) = capture_session.try_recv() {
            encoder.write_frame(&frame.data)?;
            frame_count += 1;
        } else {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    pb.finish_and_clear();

    // Stop cursor tracking and get events + duration
    let (cursor_events, cursor_duration) = cursor_tracker.stop();

    // Drain any remaining frames from the channel before stopping
    while let Some(frame) = capture_session.try_recv() {
        encoder.write_frame(&frame.data)?;
        frame_count += 1;
    }

    // Stop capture
    capture_session.stop()?;

    // Finish encoding
    encoder.finish().context("Failed to finish video encoding")?;

    let duration = start.elapsed();
    let expected_frames = (duration.as_secs_f64() * 60.0) as u64;
    eprintln!(
        "Debug: captured {} frames in {:.1}s (expected ~{} at 60fps)",
        frame_count,
        duration.as_secs_f64(),
        expected_frames
    );

    // Save metadata
    let mut metadata = RecordingMetadata::new_display(display.index, actual_width, actual_height, display.scale_factor);
    metadata.cursor_events = cursor_events;
    metadata.cursor_tracking_duration = cursor_duration;
    metadata.cursor_to_video_offset = cursor_to_video_offset;
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
    encoder::check_ffmpeg()?;

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

    // Find the window in ScreenCaptureKit
    let sc_window = capture::find_window(window.id)
        .context("Failed to find window in ScreenCaptureKit")?;

    // Get the display scale factor for dimensions
    let displays = list_displays()?;
    let display = displays.into_iter().find(|d| d.is_main).unwrap();

    // Get window frame for dimensions (ScreenCaptureKit handles this natively!)
    let frame = sc_window.frame();
    let width = (frame.width * display.scale_factor) as u32;
    let height = (frame.height * display.scale_factor) as u32;

    // Configure capture
    let config = CaptureConfig {
        show_cursor: capture_system_cursor,
        width,
        height,
    };

    // Start ScreenCaptureKit capture (native window capture - no cropping needed!)
    let mut capture_session = capture::start_window_capture(&sc_window, &config)
        .context("Failed to start window capture")?;

    // Start cursor tracking and record the start instant for offset calculation
    let mut cursor_tracker = CursorTracker::new();
    let cursor_start_instant = Instant::now();
    cursor_tracker.start()?;

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} Recording... {elapsed_precise}")
            .unwrap(),
    );

    let start = Instant::now();

    // Wait for first frame to get actual dimensions
    let first_frame = loop {
        if !running.load(Ordering::SeqCst) {
            pb.finish_and_clear();
            let _ = cursor_tracker.stop();
            capture_session.stop()?;
            anyhow::bail!("Recording cancelled before first frame");
        }

        if let Some(frame) = capture_session.try_recv() {
            break frame;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };

    let actual_width = first_frame.width as u32;
    let actual_height = first_frame.height as u32;

    // Calculate offset: time from cursor tracking start to first video frame
    // This is the precise timing relationship between cursor events and video frames
    let cursor_to_video_offset = cursor_start_instant.elapsed().as_secs_f64();

    // Start FFmpeg encoder with actual dimensions
    let mut encoder = VideoEncoder::new(actual_width, actual_height, 60, output)
        .context("Failed to start video encoder")?;

    // Write the first frame
    encoder.write_frame(&first_frame.data)?;
    let mut frame_count: u64 = 1;

    // Main recording loop
    while running.load(Ordering::SeqCst) {
        pb.tick();

        if let Some(frame) = capture_session.try_recv() {
            encoder.write_frame(&frame.data)?;
            frame_count += 1;
        } else {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    pb.finish_and_clear();

    let (cursor_events, cursor_duration) = cursor_tracker.stop();

    // Drain any remaining frames from the channel before stopping
    while let Some(frame) = capture_session.try_recv() {
        encoder.write_frame(&frame.data)?;
        frame_count += 1;
    }

    capture_session.stop()?;
    encoder.finish().context("Failed to finish video encoding")?;

    let expected_frames = (start.elapsed().as_secs_f64() * 60.0) as u64;
    eprintln!(
        "Debug: captured {} frames in {:.1}s (expected ~{} at 60fps)",
        frame_count,
        start.elapsed().as_secs_f64(),
        expected_frames
    );

    let mut metadata = RecordingMetadata::new_window(
        window.id,
        actual_width,
        actual_height,
        window.bounds.0,  // x offset
        window.bounds.1,  // y offset
        display.scale_factor,
    );
    metadata.cursor_events = cursor_events;
    metadata.cursor_tracking_duration = cursor_duration;
    metadata.cursor_to_video_offset = cursor_to_video_offset;
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
