use crate::metadata::RecordingMetadata;
use crate::zoom::{calculate_zoom, ZoomConfig};
use anyhow::{Context, Result};
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

// Output canvas dimensions
const OUTPUT_WIDTH: u32 = 1920;
const OUTPUT_HEIGHT: u32 = 1080;

/// Background type for video processing
#[derive(Clone)]
enum Background {
    Color(Rgba<u8>),
    Image(Arc<RgbaImage>),
}

impl Background {
    /// Parse background from string: hex color (e.g., "#1a1a2e") or image path
    fn parse(input: Option<&str>) -> Result<Self> {
        match input {
            None => {
                // Default dark gray
                Ok(Background::Color(Rgba([26, 26, 46, 255])))
            }
            Some(s) => {
                // Check if it's a hex color
                let hex = s.trim_start_matches('#');
                if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
                    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
                    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
                    Ok(Background::Color(Rgba([r, g, b, 255])))
                } else {
                    // Try to load as image
                    let img = image::open(s)
                        .with_context(|| format!("Failed to load background image: {}", s))?;
                    // Resize to output dimensions
                    let resized = img.resize_to_fill(
                        OUTPUT_WIDTH,
                        OUTPUT_HEIGHT,
                        image::imageops::FilterType::Lanczos3,
                    );
                    Ok(Background::Image(Arc::new(resized.to_rgba8())))
                }
            }
        }
    }

    /// Create a canvas with this background
    fn create_canvas(&self) -> RgbaImage {
        match self {
            Background::Color(color) => {
                RgbaImage::from_pixel(OUTPUT_WIDTH, OUTPUT_HEIGHT, *color)
            }
            Background::Image(img) => {
                img.as_ref().clone()
            }
        }
    }
}

pub fn process_video(
    input: &Path,
    output: &Path,
    background: Option<&str>,
    trim_start: Option<f64>,
    trim_end: Option<f64>,
) -> Result<()> {
    // Load metadata
    let metadata = RecordingMetadata::load(input)
        .context("Failed to load recording metadata. Was this video recorded with glide?")?;

    // Parse background
    let bg = Background::parse(background)?;
    println!("Processing video: {}", input.display());
    println!(
        "  Source: {:?} ({}x{})",
        metadata.source_type, metadata.width, metadata.height
    );
    println!("  Output: {}x{}", OUTPUT_WIDTH, OUTPUT_HEIGHT);
    println!("  Cursor events: {}", metadata.cursor_events.len());

    // Get video info (fps and duration)
    let fps = get_video_fps(input)?;
    let original_duration = get_video_duration(input)?;
    println!("  FPS: {:.2}", fps);
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

    // Calculate timestamp offset for synchronization
    // If cursor tracking ran longer than video, cursor events are ahead
    // Also account for trim_start: cursor events need to be shifted by trim_start
    let base_time_offset = if metadata.cursor_tracking_duration > 0.0 {
        metadata.cursor_tracking_duration - original_duration
    } else {
        0.0 // Old recordings without this field
    };
    // Add trim_start to offset since we're starting from a later point in the video
    let time_offset = base_time_offset + trim_start_secs;

    if base_time_offset.abs() > 0.01 {
        println!(
            "  Time offset: {:.3}s (cursor tracking started before video)",
            base_time_offset
        );
    }

    // Process frames in parallel
    println!("\nProcessing frames with zoom effects (parallel)...");
    let zoom_config = ZoomConfig::default();
    process_frames_parallel(frames_dir, frame_count, fps, &metadata, &zoom_config, &bg, time_offset)?;

    // Re-encode
    println!("\nEncoding output video...");
    encode_video(frames_dir, output, fps)?;

    println!("\nDone! Output saved to: {}", output.display());

    Ok(())
}

fn extract_frames(
    input: &Path,
    output_dir: &Path,
    trim_start: f64,
    duration: f64,
) -> Result<usize> {
    // Use JPEG for faster extraction/encoding
    let output_pattern = output_dir.join("frame_%06d.jpg");

    // Pre-format strings to avoid lifetime issues
    let trim_start_str = format!("{:.3}", trim_start);
    let duration_str = format!("{:.3}", duration);

    let mut args = Vec::new();

    // Add seek before input for faster seeking (input seeking)
    if trim_start > 0.0 {
        args.extend(["-ss", trim_start_str.as_str()]);
    }

    args.extend(["-i", input.to_str().unwrap()]);

    // Add duration limit
    args.extend(["-t", duration_str.as_str()]);

    args.extend([
        "-vsync",
        "0",
        "-q:v",
        "2", // High quality JPEG
    ]);
    args.push(output_pattern.to_str().unwrap());

    let status = Command::new("ffmpeg")
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run ffmpeg for frame extraction")?;

    if !status.success() {
        anyhow::bail!("FFmpeg frame extraction failed");
    }

    // Count extracted frames
    let count = std::fs::read_dir(output_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "jpg"))
        .count();

    Ok(count)
}

fn get_video_fps(input: &Path) -> Result<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=r_frame_rate",
            "-of",
            "csv=p=0",
            input.to_str().unwrap(),
        ])
        .output()
        .context("Failed to run ffprobe")?;

    let fps_str = String::from_utf8_lossy(&output.stdout);
    let fps_str = fps_str.trim();

    if let Some((num, den)) = fps_str.split_once('/') {
        let num: f64 = num.parse().unwrap_or(60.0);
        let den: f64 = den.parse().unwrap_or(1.0);
        Ok(num / den)
    } else {
        Ok(fps_str.parse().unwrap_or(60.0))
    }
}

fn get_video_duration(input: &Path) -> Result<f64> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
            input.to_str().unwrap(),
        ])
        .output()
        .context("Failed to run ffprobe")?;

    let duration_str = String::from_utf8_lossy(&output.stdout);
    let duration_str = duration_str.trim();

    Ok(duration_str.parse().unwrap_or(0.0))
}

/// Layout info for placing content on canvas
struct ContentLayout {
    scale: f64,
    offset_x: u32,
    offset_y: u32,
    scaled_width: u32,
    scaled_height: u32,
}

// Corner radius for rounded corners
const CORNER_RADIUS: u32 = 12;
// Shadow settings
const SHADOW_OFFSET: i64 = 8;
const SHADOW_BLUR_RADIUS: u32 = 20;
const SHADOW_COLOR: Rgba<u8> = Rgba([0, 0, 0, 80]);

impl ContentLayout {
    fn calculate(content_width: u32, content_height: u32) -> Self {
        // Calculate scale to fit content with padding (leave ~100px on each side)
        let max_content_width = OUTPUT_WIDTH - 200;
        let max_content_height = OUTPUT_HEIGHT - 200;

        let scale_x = max_content_width as f64 / content_width as f64;
        let scale_y = max_content_height as f64 / content_height as f64;
        let scale = scale_x.min(scale_y).min(1.0); // Don't upscale

        let scaled_width = (content_width as f64 * scale) as u32;
        let scaled_height = (content_height as f64 * scale) as u32;

        // Center on canvas
        let offset_x = (OUTPUT_WIDTH - scaled_width) / 2;
        let offset_y = (OUTPUT_HEIGHT - scaled_height) / 2;

        Self {
            scale,
            offset_x,
            offset_y,
            scaled_width,
            scaled_height,
        }
    }
}

/// Apply rounded corners to an RGBA image
fn apply_rounded_corners(img: &mut RgbaImage, radius: u32) {
    let width = img.width();
    let height = img.height();
    let radius = radius.min(width / 2).min(height / 2);

    for y in 0..height {
        for x in 0..width {
            let alpha = corner_alpha(x, y, width, height, radius);
            if alpha < 255 {
                let pixel = img.get_pixel_mut(x, y);
                // Multiply existing alpha by corner alpha
                let new_alpha = (pixel[3] as u32 * alpha as u32 / 255) as u8;
                pixel[3] = new_alpha;
            }
        }
    }
}

/// Calculate alpha value for a pixel based on corner rounding
fn corner_alpha(x: u32, y: u32, width: u32, height: u32, radius: u32) -> u8 {
    let radius_f = radius as f64;

    // Check each corner
    let corners = [
        (radius, radius),                         // top-left
        (width - radius - 1, radius),             // top-right
        (radius, height - radius - 1),            // bottom-left
        (width - radius - 1, height - radius - 1), // bottom-right
    ];

    for (cx, cy) in corners {
        // Check if pixel is in the corner region
        let in_corner_x = (x <= radius && cx == radius) || (x >= width - radius - 1 && cx == width - radius - 1);
        let in_corner_y = (y <= radius && cy == radius) || (y >= height - radius - 1 && cy == height - radius - 1);

        if in_corner_x && in_corner_y {
            let dx = x as f64 - cx as f64;
            let dy = y as f64 - cy as f64;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist > radius_f {
                return 0; // Outside corner
            } else if dist > radius_f - 1.5 {
                // Anti-aliasing at edge
                let alpha = (radius_f - dist + 0.5).clamp(0.0, 1.0);
                return (alpha * 255.0) as u8;
            }
        }
    }

    255 // Fully opaque
}

/// Draw a shadow on the canvas
fn draw_shadow(canvas: &mut RgbaImage, x: i64, y: i64, width: u32, height: u32, radius: u32) {
    let shadow_x = x + SHADOW_OFFSET;
    let shadow_y = y + SHADOW_OFFSET;

    // Draw multiple layers for blur effect
    for blur_layer in 0..SHADOW_BLUR_RADIUS {
        let expand = blur_layer as i64;
        let layer_alpha = SHADOW_COLOR[3] as u32 * (SHADOW_BLUR_RADIUS - blur_layer) as u32
                          / (SHADOW_BLUR_RADIUS * SHADOW_BLUR_RADIUS) as u32;

        if layer_alpha == 0 {
            continue;
        }

        let sx = (shadow_x - expand).max(0) as u32;
        let sy = (shadow_y - expand).max(0) as u32;
        let sw = (width as i64 + expand * 2).min(canvas.width() as i64 - sx as i64) as u32;
        let sh = (height as i64 + expand * 2).min(canvas.height() as i64 - sy as i64) as u32;

        for py in sy..sy + sh {
            for px in sx..sx + sw {
                if px >= canvas.width() || py >= canvas.height() {
                    continue;
                }

                // Check if inside rounded rectangle
                let local_x = px as i64 - shadow_x + expand;
                let local_y = py as i64 - shadow_y + expand;
                let layer_width = width + 2 * expand as u32;
                let layer_height = height + 2 * expand as u32;

                if is_inside_rounded_rect(local_x, local_y, layer_width, layer_height, radius + expand as u32) {
                    let pixel = canvas.get_pixel_mut(px, py);
                    // Blend shadow with existing pixel
                    let alpha = layer_alpha as u8;
                    pixel[0] = blend_channel(pixel[0], SHADOW_COLOR[0], alpha);
                    pixel[1] = blend_channel(pixel[1], SHADOW_COLOR[1], alpha);
                    pixel[2] = blend_channel(pixel[2], SHADOW_COLOR[2], alpha);
                }
            }
        }
    }
}

fn is_inside_rounded_rect(x: i64, y: i64, width: u32, height: u32, radius: u32) -> bool {
    if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
        return false;
    }

    let x = x as u32;
    let y = y as u32;
    let radius_f = radius as f64;

    // Check corners
    let corners = [
        (radius, radius),
        (width - radius - 1, radius),
        (radius, height - radius - 1),
        (width - radius - 1, height - radius - 1),
    ];

    for (cx, cy) in corners {
        let in_corner_x = (x <= radius && cx == radius) || (x >= width - radius - 1 && cx == width - radius - 1);
        let in_corner_y = (y <= radius && cy == radius) || (y >= height - radius - 1 && cy == height - radius - 1);

        if in_corner_x && in_corner_y {
            let dx = x as f64 - cx as f64;
            let dy = y as f64 - cy as f64;
            if dx * dx + dy * dy > radius_f * radius_f {
                return false;
            }
        }
    }

    true
}

fn blend_channel(bg: u8, fg: u8, alpha: u8) -> u8 {
    let bg = bg as u32;
    let fg = fg as u32;
    let alpha = alpha as u32;
    ((bg * (255 - alpha) + fg * alpha) / 255) as u8
}

fn process_frames_parallel(
    frames_dir: &Path,
    frame_count: usize,
    fps: f64,
    metadata: &RecordingMetadata,
    zoom_config: &ZoomConfig,
    background: &Background,
    time_offset: f64,
) -> Result<()> {
    let pb = ProgressBar::new(frame_count as u64);
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

    // Process frames in parallel using rayon
    let results: Vec<Result<()>> = (1..=frame_count)
        .into_par_iter()
        .map(|frame_num| {
            let frame_path = frames_dir.join(format!("frame_{:06}.jpg", frame_num));
            let timestamp = (frame_num - 1) as f64 / fps;

            // Load content frame
            let content = image::open(&frame_path)
                .with_context(|| format!("Failed to open frame {}", frame_num))?;

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

            // Scale content to fit
            let scaled_content = content.resize_exact(
                layout.scaled_width,
                layout.scaled_height,
                image::imageops::FilterType::Triangle,
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

            // Translate cursor from screen coordinates to window-relative coordinates
            let (offset_x, offset_y) = metadata.window_offset;
            let window_cursor_x = cursor_x - offset_x as f64;
            let window_cursor_y = cursor_y - offset_y as f64;

            // Transform cursor coordinates to canvas space
            let canvas_cursor_x = layout.offset_x as f64 + window_cursor_x * layout.scale;
            let canvas_cursor_y = layout.offset_y as f64 + window_cursor_y * layout.scale;

            let final_img = if zoom > 1.01 {
                // Apply zoom transformation to canvas
                apply_zoom(&DynamicImage::ImageRgba8(canvas), zoom, canvas_cursor_x, canvas_cursor_y)
            } else {
                DynamicImage::ImageRgba8(canvas)
            };

            // Save processed frame
            final_img
                .save(&frame_path)
                .with_context(|| format!("Failed to save frame {}", frame_num))?;

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

fn apply_zoom(
    img: &DynamicImage,
    zoom: f64,
    cursor_x: f64,
    cursor_y: f64,
) -> DynamicImage {
    let (width, height) = img.dimensions();
    let width_f = width as f64;
    let height_f = height as f64;

    // Calculate the size of the visible area after zoom
    let view_width = width_f / zoom;
    let view_height = height_f / zoom;

    // Center the view on the cursor position, but clamp to image bounds
    let view_left = (cursor_x - view_width / 2.0)
        .max(0.0)
        .min(width_f - view_width);
    let view_top = (cursor_y - view_height / 2.0)
        .max(0.0)
        .min(height_f - view_height);

    // Crop and resize (use Triangle filter for speed, still decent quality)
    let cropped = img.crop_imm(
        view_left as u32,
        view_top as u32,
        view_width as u32,
        view_height as u32,
    );

    cropped.resize_exact(width, height, image::imageops::FilterType::Triangle)
}

fn encode_video(frames_dir: &Path, output: &Path, fps: f64) -> Result<()> {
    let input_pattern = frames_dir.join("frame_%06d.jpg");

    // Try hardware encoding first (VideoToolbox on macOS)
    let status = Command::new("ffmpeg")
        .args([
            "-framerate",
            &format!("{}", fps),
            "-i",
            input_pattern.to_str().unwrap(),
            "-c:v",
            "h264_videotoolbox", // macOS GPU encoding
            "-q:v",
            "65", // Quality (0-100, higher is better)
            "-pix_fmt",
            "yuv420p",
            "-y",
            output.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    // Fall back to CPU if hardware encoding fails
    if status.is_err() || !status.unwrap().success() {
        let status = Command::new("ffmpeg")
            .args([
                "-framerate",
                &format!("{}", fps),
                "-i",
                input_pattern.to_str().unwrap(),
                "-c:v",
                "libx264",
                "-preset",
                "fast",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-y",
                output.to_str().unwrap(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to run ffmpeg for encoding")?;

        if !status.success() {
            anyhow::bail!("FFmpeg encoding failed");
        }
    }

    Ok(())
}
