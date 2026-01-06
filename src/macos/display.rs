use anyhow::{Context, Result};
use core_graphics::display::CGDisplay;
use std::process::{Command, Stdio};

// FFI declarations for display mode pixel dimensions
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGDisplayCopyDisplayMode(display: u32) -> *mut std::ffi::c_void;
    fn CGDisplayModeGetPixelWidth(mode: *mut std::ffi::c_void) -> usize;
    fn CGDisplayModeGetPixelHeight(mode: *mut std::ffi::c_void) -> usize;
    fn CGDisplayModeRelease(mode: *mut std::ffi::c_void);
}

pub struct DisplayInfo {
    pub index: usize,        // 0-based index for user
    pub avf_index: usize,    // AVFoundation device index for FFmpeg
    pub width: u32,          // Width in points (logical pixels)
    pub height: u32,         // Height in points (logical pixels)
    pub x: i32,
    pub y: i32,
    pub is_main: bool,
    pub scale_factor: f64,   // Retina scale factor (2.0 on Retina, 1.0 otherwise)
}

/// Get the native pixel dimensions of a display (accounts for Retina scaling)
fn get_native_pixel_dimensions(display_id: u32) -> Option<(usize, usize)> {
    unsafe {
        let mode = CGDisplayCopyDisplayMode(display_id);
        if mode.is_null() {
            return None;
        }
        let width = CGDisplayModeGetPixelWidth(mode);
        let height = CGDisplayModeGetPixelHeight(mode);
        CGDisplayModeRelease(mode);
        Some((width, height))
    }
}

pub fn list_displays() -> Result<Vec<DisplayInfo>> {
    // Get display info from Core Graphics
    let cg_displays = CGDisplay::active_displays()
        .map_err(|e| anyhow::anyhow!("Failed to get displays: {:?}", e))?;

    // Get AVFoundation device indices
    let avf_screen_indices = get_avfoundation_screen_indices()?;

    let mut displays = Vec::new();

    for (index, cg_id) in cg_displays.iter().enumerate() {
        let display = CGDisplay::new(*cg_id);
        let bounds = display.bounds();

        // Calculate Retina scale factor using native pixel dimensions
        let scale_factor = if let Some((native_width, _)) = get_native_pixel_dimensions(*cg_id) {
            let points_wide = bounds.size.width;
            if points_wide > 0.0 {
                native_width as f64 / points_wide
            } else {
                1.0
            }
        } else {
            1.0
        };

        // Map to AVFoundation index if available
        let avf_index = avf_screen_indices.get(index).copied().unwrap_or(index);

        displays.push(DisplayInfo {
            index,
            avf_index,
            width: bounds.size.width as u32,
            height: bounds.size.height as u32,
            x: bounds.origin.x as i32,
            y: bounds.origin.y as i32,
            is_main: display.is_main(),
            scale_factor,
        });
    }

    Ok(displays)
}

/// Parse FFmpeg's AVFoundation device list to find screen capture indices
fn get_avfoundation_screen_indices() -> Result<Vec<usize>> {
    let output = Command::new("ffmpeg")
        .args(["-f", "avfoundation", "-list_devices", "true", "-i", ""])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("Failed to run ffmpeg")?;

    // FFmpeg outputs device list to stderr
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut screen_indices = Vec::new();

    for line in stderr.lines() {
        // Look for lines like "[AVFoundation indev @ ...] [3] Capture screen 0"
        if line.contains("Capture screen") {
            if let Some(idx_str) = extract_device_index(line) {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    screen_indices.push(idx);
                }
            }
        }
    }

    Ok(screen_indices)
}

/// Extract device index from FFmpeg output line like "[3] Capture screen 0"
fn extract_device_index(line: &str) -> Option<&str> {
    let start = line.find('[')? + 1;
    let rest = &line[start..];
    let end = rest.find(']')?;

    // Skip if this is the "AVFoundation indev @" bracket
    let idx_str = &rest[..end];
    if idx_str.starts_with("AVFoundation") {
        // Find the next bracket pair
        let rest2 = &rest[end + 1..];
        let start2 = rest2.find('[')? + 1;
        let rest3 = &rest2[start2..];
        let end2 = rest3.find(']')?;
        Some(&rest3[..end2])
    } else {
        Some(idx_str)
    }
}
