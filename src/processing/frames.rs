use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Extract frames from video to output directory
pub fn extract_frames(
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

/// Get video frame rate using ffprobe
pub fn get_video_fps(input: &Path) -> Result<f64> {
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

/// Get video duration using ffprobe
pub fn get_video_duration(input: &Path) -> Result<f64> {
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

/// Encode frames back to video
pub fn encode_video(frames_dir: &Path, output: &Path, fps: f64) -> Result<()> {
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
