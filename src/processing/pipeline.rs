use crate::processing::click_highlight::{
    draw_click_highlights, get_active_ripples, ClickHighlightConfig,
};
use crate::processing::cursor::{draw_cursor, get_smoothed_cursor, CursorConfig};
use crate::processing::effects::{
    apply_rounded_corners, apply_zoom, draw_shadow, Background, ContentLayout, CORNER_RADIUS,
    OUTPUT_HEIGHT, OUTPUT_WIDTH,
};
use crate::processing::frames::{encode_video, extract_frames, get_video_duration};
use crate::processing::motion_blur::{apply_motion_blur, calculate_motion_state, MotionBlurConfig};
use crate::processing::zoom::{calculate_zoom, ZoomConfig};
use crate::recording::metadata::RecordingMetadata;
use anyhow::{Context, Result};
use image::DynamicImage;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

pub fn process_video(
    input: &Path,
    output: &Path,
    background: Option<&str>,
    trim_start: Option<f64>,
    trim_end: Option<f64>,
    cursor_scale: f64,
    cursor_timeout: f64,
    no_cursor: bool,
    no_motion_blur: bool,
    no_click_highlight: bool,
) -> Result<()> {
    // Load metadata
    let metadata = RecordingMetadata::load(input)
        .context("Failed to load recording metadata. Was this video recorded with glide?")?;

    // Parse background
    let bg = Background::parse(background)?;

    // Create cursor config
    let cursor_config = if no_cursor {
        None
    } else {
        Some(CursorConfig::new(cursor_scale, cursor_timeout))
    };

    // Create motion blur config
    let motion_blur_config = MotionBlurConfig {
        enabled: !no_motion_blur,
        ..Default::default()
    };

    // Create click highlight config
    let click_highlight_config = ClickHighlightConfig {
        enabled: !no_click_highlight,
        ..Default::default()
    };

    println!("Processing video: {}", input.display());
    println!(
        "  Source: {:?} ({}x{})",
        metadata.source_type, metadata.width, metadata.height
    );
    println!("  Output: {}x{}", OUTPUT_WIDTH, OUTPUT_HEIGHT);
    println!("  Cursor events: {}", metadata.cursor_events.len());
    if let Some(ref config) = cursor_config {
        println!(
            "  Cursor: scale={:.1}x, timeout={:.1}s",
            config.cursor_scale, config.inactivity_timeout
        );
    } else {
        println!("  Cursor: disabled");
    }
    println!(
        "  Motion blur: {}",
        if motion_blur_config.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Click highlight: {}",
        if click_highlight_config.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );

    // Get video duration
    let original_duration = get_video_duration(input)?;
    println!("  Original duration: {:.2}s", original_duration);

    // Calculate trim parameters
    let trim_start_secs = trim_start.unwrap_or(0.0).max(0.0);
    let trim_end_secs = trim_end.unwrap_or(0.0).max(0.0);
    let trimmed_duration = (original_duration - trim_start_secs - trim_end_secs).max(0.0);

    if trimmed_duration <= 0.0 {
        anyhow::bail!(
            "Trim values ({:.2}s + {:.2}s = {:.2}s) exceed video duration ({:.2}s)",
            trim_start_secs,
            trim_end_secs,
            trim_start_secs + trim_end_secs,
            original_duration
        );
    }

    if trim_start_secs > 0.0 || trim_end_secs > 0.0 {
        println!(
            "  Trimming: {:.2}s from start, {:.2}s from end",
            trim_start_secs, trim_end_secs
        );
        println!("  Trimmed duration: {:.2}s", trimmed_duration);
    }

    // Create temp directory for frames
    let temp_dir = TempDir::new().context("Failed to create temp directory")?;
    let frames_dir = temp_dir.path();

    // Extract frames (use JPEG for speed)
    println!("\nExtracting frames...");
    let frame_count = extract_frames(input, frames_dir, trim_start_secs, trimmed_duration)?;
    println!("  Extracted {} frames", frame_count);

    // Calculate source FPS from extracted frames
    let source_fps = if trimmed_duration > 0.0 {
        frame_count as f64 / trimmed_duration
    } else {
        30.0 // fallback
    };
    println!("  Source FPS: {:.2}", source_fps);

    // Target 60fps for smooth animations
    let target_fps = 60.0;
    let output_frame_count = (trimmed_duration * target_fps).ceil() as usize;
    println!("  Output: {} frames at {:.0}fps", output_frame_count, target_fps);

    // Calculate timestamp offset for synchronization
    // Use the precise offset recorded at capture time if available,
    // otherwise fall back to the approximate calculation for old recordings
    let base_time_offset = if metadata.cursor_to_video_offset > 0.0 {
        // Precise offset: time from cursor tracking start to first video frame
        metadata.cursor_to_video_offset
    } else if metadata.cursor_tracking_duration > 0.0 {
        // Fallback for old recordings: approximate from durations
        metadata.cursor_tracking_duration - original_duration
    } else {
        0.0 // Very old recordings without timing fields
    };
    // Add trim_start to offset since we're starting from a later point in the video
    let time_offset = base_time_offset + trim_start_secs;

    if base_time_offset.abs() > 0.01 {
        println!(
            "  Time offset: {:.3}s (cursor tracking started before video)",
            base_time_offset
        );
    }

    // Process frames in parallel - generate 60fps output with smooth zoom/cursor
    println!("\nProcessing frames with zoom effects (parallel)...");
    let zoom_config = ZoomConfig::default();
    process_frames_parallel(
        frames_dir,
        frame_count,
        output_frame_count,
        source_fps,
        target_fps,
        &metadata,
        &zoom_config,
        &bg,
        time_offset,
        cursor_config.as_ref(),
        &motion_blur_config,
        &click_highlight_config,
    )?;

    // Encode the generated 60fps frames
    println!("\nEncoding output video...");
    encode_video(frames_dir, output, target_fps, target_fps)?;

    println!("\nDone! Output saved to: {}", output.display());

    Ok(())
}

fn process_frames_parallel(
    frames_dir: &Path,
    source_frame_count: usize,
    output_frame_count: usize,
    source_fps: f64,
    target_fps: f64,
    metadata: &RecordingMetadata,
    zoom_config: &ZoomConfig,
    background: &Background,
    time_offset: f64,
    cursor_config: Option<&CursorConfig>,
    motion_blur_config: &MotionBlurConfig,
    click_highlight_config: &ClickHighlightConfig,
) -> Result<()> {
    let pb = ProgressBar::new(output_frame_count as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let processed = AtomicUsize::new(0);
    let frames_dir = frames_dir.to_path_buf();

    // Calculate content layout once (all frames have same dimensions)
    let layout = ContentLayout::calculate(metadata.width, metadata.height);
    let background = background.clone();

    // Pre-load all source frames for faster access
    println!("  Loading source frames...");
    let source_frames: Vec<_> = (1..=source_frame_count)
        .map(|i| {
            let path = frames_dir.join(format!("frame_{:06}.png", i));
            image::open(&path).expect("Failed to load source frame")
        })
        .collect();

    // Generate output frames at target fps with smooth zoom/cursor interpolation
    let results: Vec<Result<()>> = (1..=output_frame_count)
        .into_par_iter()
        .map(|output_frame_num| {
            // Calculate timestamp for this output frame
            let timestamp = (output_frame_num - 1) as f64 / target_fps;

            // Find the corresponding source frame (nearest neighbor)
            let source_idx = ((timestamp * source_fps).floor() as usize).min(source_frame_count - 1);
            let content = &source_frames[source_idx];

            // Output frame path (new numbering for 60fps output)
            let output_path = frames_dir.join(format!("out_{:06}.png", output_frame_num));

            // Create canvas with background
            let mut canvas = background.create_canvas();

            // Draw shadow first (before content)
            draw_shadow(
                &mut canvas,
                layout.offset_x as i64,
                layout.offset_y as i64,
                layout.scaled_width,
                layout.scaled_height,
                CORNER_RADIUS,
            );

            // Scale content to fit (use Lanczos3 for sharp, high-quality results)
            let scaled_content = content.resize_exact(
                layout.scaled_width,
                layout.scaled_height,
                image::imageops::FilterType::Lanczos3,
            );

            // Apply rounded corners to content
            let mut rounded_content = scaled_content.to_rgba8();
            apply_rounded_corners(&mut rounded_content, CORNER_RADIUS);

            // Overlay content on canvas
            image::imageops::overlay(
                &mut canvas,
                &rounded_content,
                layout.offset_x as i64,
                layout.offset_y as i64,
            );

            // Calculate zoom for this frame
            // Add time_offset to align cursor timestamps with video timestamps
            let adjusted_timestamp = timestamp + time_offset;
            let (zoom, cursor_x, cursor_y) =
                calculate_zoom(adjusted_timestamp, &metadata.cursor_events, zoom_config);

            // Get scale factor for coordinate conversion (screen points -> pixels)
            // CGEventTap returns screen points, but video is captured at pixel resolution
            let scale_factor = metadata.scale_factor.max(1.0);

            // Scale cursor coordinates from screen points to pixels
            let cursor_x_scaled = cursor_x * scale_factor;
            let cursor_y_scaled = cursor_y * scale_factor;

            // Translate cursor from screen coordinates to window-relative coordinates
            // Window offset is also in screen points, so scale it too
            let (offset_x, offset_y) = metadata.window_offset;
            let offset_x_scaled = offset_x as f64 * scale_factor;
            let offset_y_scaled = offset_y as f64 * scale_factor;
            let window_cursor_x = cursor_x_scaled - offset_x_scaled;
            let window_cursor_y = cursor_y_scaled - offset_y_scaled;

            // Transform cursor coordinates to canvas space
            let canvas_cursor_x = layout.offset_x as f64 + window_cursor_x * layout.scale;
            let canvas_cursor_y = layout.offset_y as f64 + window_cursor_y * layout.scale;

            // Draw cursor if enabled
            if let Some(cursor_cfg) = cursor_config {
                let cursor_state =
                    get_smoothed_cursor(adjusted_timestamp, &metadata.cursor_events, cursor_cfg);

                if cursor_state.opacity > 0.01 {
                    // Transform smoothed cursor coordinates to canvas space
                    // Apply scale_factor to convert from screen points to pixels
                    let smoothed_canvas_x =
                        layout.offset_x as f64 + (cursor_state.x * scale_factor - offset_x_scaled) * layout.scale;
                    let smoothed_canvas_y =
                        layout.offset_y as f64 + (cursor_state.y * scale_factor - offset_y_scaled) * layout.scale;

                    draw_cursor(
                        &mut canvas,
                        smoothed_canvas_x,
                        smoothed_canvas_y,
                        cursor_cfg.cursor_scale * layout.scale,
                        cursor_state.opacity,
                    );
                }
            }

            // Draw click highlights if enabled
            if click_highlight_config.enabled {
                let ripples = get_active_ripples(
                    adjusted_timestamp,
                    &metadata.cursor_events,
                    click_highlight_config,
                );

                // Transform ripples to canvas space
                let canvas_ripples: Vec<_> = ripples
                    .iter()
                    .map(|r| {
                        // Transform from screen points to canvas space
                        let ripple_canvas_x = layout.offset_x as f64
                            + (r.x * scale_factor - offset_x_scaled) * layout.scale;
                        let ripple_canvas_y = layout.offset_y as f64
                            + (r.y * scale_factor - offset_y_scaled) * layout.scale;
                        crate::processing::click_highlight::ActiveRipple {
                            x: ripple_canvas_x,
                            y: ripple_canvas_y,
                            progress: r.progress,
                        }
                    })
                    .collect();

                // Use fixed sizes in canvas space (don't scale with content)
                // This ensures the highlight is always visible regardless of content scale
                draw_click_highlights(&mut canvas, &canvas_ripples, click_highlight_config);
            }

            let zoomed_img = if zoom > 1.01 {
                // Apply zoom transformation to canvas
                apply_zoom(
                    &DynamicImage::ImageRgba8(canvas),
                    zoom,
                    canvas_cursor_x,
                    canvas_cursor_y,
                )
            } else {
                DynamicImage::ImageRgba8(canvas)
            };

            // Apply motion blur during zoom/pan transitions
            let final_img = if motion_blur_config.enabled {
                let motion_state = calculate_motion_state(
                    adjusted_timestamp,
                    &metadata.cursor_events,
                    zoom_config,
                    &layout,
                    metadata.window_offset,
                    scale_factor,
                );
                let blurred = apply_motion_blur(&zoomed_img.to_rgba8(), &motion_state, motion_blur_config);
                DynamicImage::ImageRgba8(blurred)
            } else {
                zoomed_img
            };

            // Save processed frame
            final_img
                .save(&output_path)
                .with_context(|| format!("Failed to save frame {}", output_frame_num))?;

            let count = processed.fetch_add(1, Ordering::Relaxed);
            if count % 10 == 0 {
                pb.set_position(count as u64);
            }

            Ok(())
        })
        .collect();

    pb.finish_with_message("Processing complete");

    // Check for any errors
    for result in results {
        result?;
    }

    Ok(())
}
