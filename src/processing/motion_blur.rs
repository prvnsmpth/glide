//! Motion blur effects for zoom and pan transitions
//!
//! Applies radial blur during zoom-in/zoom-out and directional blur during panning.

use crate::cursor_types::CursorEvent;
use crate::processing::effects::ContentLayout;
use crate::processing::zoom::{calculate_zoom, ZoomConfig};
use image::{Rgba, RgbaImage};

/// Motion state at a specific timestamp
#[derive(Debug, Clone, Default)]
pub struct MotionState {
    /// Current zoom level
    pub zoom: f64,
    /// Zoom velocity (d(zoom)/dt) - positive = zooming in, negative = zooming out
    pub zoom_velocity: f64,
    /// Current cursor position (canvas coordinates)
    pub cursor_x: f64,
    pub cursor_y: f64,
    /// Pan velocity in pixels per second (canvas coordinates)
    pub pan_velocity_x: f64,
    pub pan_velocity_y: f64,
    /// Motion phase for context
    pub phase: MotionPhase,
}

/// What phase of motion we're in
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum MotionPhase {
    #[default]
    Idle,
    ZoomIn,
    Hold,
    ZoomOut,
    Pan,
}

/// Configuration for motion blur
#[derive(Debug, Clone)]
pub struct MotionBlurConfig {
    /// Enable/disable motion blur
    pub enabled: bool,
    /// Maximum blur strength for zoom (pixels at edges)
    pub zoom_blur_strength: f64,
    /// Number of samples for radial blur (more = better quality, slower)
    pub zoom_blur_samples: u32,
    /// Maximum blur strength for pan (pixels)
    pub pan_blur_strength: f64,
    /// Number of samples for directional blur
    pub pan_blur_samples: u32,
    /// Minimum velocity threshold to apply blur
    pub velocity_threshold: f64,
}

impl Default for MotionBlurConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            zoom_blur_strength: 90.0,
            zoom_blur_samples: 16,
            pan_blur_strength: 60.0,
            pan_blur_samples: 12,
            velocity_threshold: 0.05,
        }
    }
}

/// Calculate motion state at a given timestamp using finite differences
pub fn calculate_motion_state(
    timestamp: f64,
    cursor_events: &[CursorEvent],
    zoom_config: &ZoomConfig,
    layout: &ContentLayout,
    window_offset: (i32, i32),
    scale_factor: f64,
) -> MotionState {
    // Small time delta for numerical differentiation (~8ms, half a frame at 60fps)
    const DT: f64 = 1.0 / 120.0;

    // Get zoom state at t-dt, t, and t+dt
    let (zoom_prev, cx_prev, cy_prev) =
        calculate_zoom((timestamp - DT).max(0.0), cursor_events, zoom_config);
    let (zoom_curr, cx_curr, cy_curr) = calculate_zoom(timestamp, cursor_events, zoom_config);
    let (zoom_next, cx_next, cy_next) = calculate_zoom(timestamp + DT, cursor_events, zoom_config);

    // Central difference for velocity (more accurate than forward/backward)
    let zoom_velocity = (zoom_next - zoom_prev) / (2.0 * DT);

    // Transform cursor to canvas coordinates
    let to_canvas = |cx: f64, cy: f64| -> (f64, f64) {
        let offset_x = window_offset.0 as f64 * scale_factor;
        let offset_y = window_offset.1 as f64 * scale_factor;
        let window_x = cx * scale_factor - offset_x;
        let window_y = cy * scale_factor - offset_y;
        (
            layout.offset_x as f64 + window_x * layout.scale,
            layout.offset_y as f64 + window_y * layout.scale,
        )
    };

    let (canvas_prev_x, canvas_prev_y) = to_canvas(cx_prev, cy_prev);
    let (canvas_curr_x, canvas_curr_y) = to_canvas(cx_curr, cy_curr);
    let (canvas_next_x, canvas_next_y) = to_canvas(cx_next, cy_next);

    // Central difference for pan velocity
    let pan_velocity_x = (canvas_next_x - canvas_prev_x) / (2.0 * DT);
    let pan_velocity_y = (canvas_next_y - canvas_prev_y) / (2.0 * DT);

    // Determine motion phase
    let phase = determine_motion_phase(zoom_curr, zoom_velocity, pan_velocity_x, pan_velocity_y);

    MotionState {
        zoom: zoom_curr,
        zoom_velocity,
        cursor_x: canvas_curr_x,
        cursor_y: canvas_curr_y,
        pan_velocity_x,
        pan_velocity_y,
        phase,
    }
}

fn determine_motion_phase(zoom: f64, zoom_velocity: f64, pan_vx: f64, pan_vy: f64) -> MotionPhase {
    const ZOOM_THRESHOLD: f64 = 0.05; // Lower threshold
    const PAN_THRESHOLD: f64 = 50.0; // pixels/second

    if zoom < 1.01 {
        return MotionPhase::Idle;
    }

    if zoom_velocity > ZOOM_THRESHOLD {
        return MotionPhase::ZoomIn;
    }

    if zoom_velocity < -ZOOM_THRESHOLD {
        return MotionPhase::ZoomOut;
    }

    let pan_speed = (pan_vx * pan_vx + pan_vy * pan_vy).sqrt();
    if pan_speed > PAN_THRESHOLD {
        return MotionPhase::Pan;
    }

    MotionPhase::Hold
}

/// Apply motion blur based on current motion state
pub fn apply_motion_blur(
    img: &RgbaImage,
    motion: &MotionState,
    config: &MotionBlurConfig,
) -> RgbaImage {
    if !config.enabled {
        return img.clone();
    }

    match motion.phase {
        MotionPhase::Idle | MotionPhase::Hold => img.clone(),
        MotionPhase::ZoomIn | MotionPhase::ZoomOut => apply_radial_blur(
            img,
            motion.cursor_x,
            motion.cursor_y,
            motion.zoom_velocity,
            config,
        ),
        MotionPhase::Pan => {
            apply_directional_blur(img, motion.pan_velocity_x, motion.pan_velocity_y, config)
        }
    }
}

/// Apply radial (zoom) blur to an image
///
/// The blur radiates from/toward the center point.
/// - Positive velocity: blur outward (zoom in - content rushes toward viewer)
/// - Negative velocity: blur inward (zoom out - content recedes)
fn apply_radial_blur(
    img: &RgbaImage,
    center_x: f64,
    center_y: f64,
    zoom_velocity: f64,
    config: &MotionBlurConfig,
) -> RgbaImage {
    if zoom_velocity.abs() < config.velocity_threshold {
        return img.clone();
    }

    let width = img.width();
    let height = img.height();
    let mut output = RgbaImage::new(width, height);

    // Normalize velocity to 0..1 range
    // Max expected zoom velocity is ~(max_zoom - 1) / ease_in_duration
    // With max_zoom=1.8 and ease_in=0.6s: ~1.33 zoom/sec
    let max_velocity = 2.0;
    let normalized_velocity = (zoom_velocity.abs() / max_velocity).clamp(0.0, 1.0);

    // Blur strength scales with velocity (linear for more visible effect)
    let blur_amount = config.zoom_blur_strength * normalized_velocity;

    // Direction: positive velocity = outward blur (zoom in)
    let direction = if zoom_velocity > 0.0 { 1.0 } else { -1.0 };

    let samples = config.zoom_blur_samples;
    let max_dist = (width.max(height) as f64) * 0.5;

    for y in 0..height {
        for x in 0..width {
            // Vector from center to this pixel
            let dx = x as f64 - center_x;
            let dy = y as f64 - center_y;
            let dist = (dx * dx + dy * dy).sqrt().max(1.0);

            // Blur amount increases with distance from center
            let dist_factor = (dist / max_dist).min(1.0);
            let pixel_blur = blur_amount * dist_factor;

            if pixel_blur < 0.5 {
                // No visible blur, just copy pixel
                output.put_pixel(x, y, *img.get_pixel(x, y));
                continue;
            }

            // Direction vector (normalized)
            let dir_x = dx / dist;
            let dir_y = dy / dist;

            // Accumulate samples along the radial direction
            let mut r_sum = 0.0f64;
            let mut g_sum = 0.0f64;
            let mut b_sum = 0.0f64;
            let mut a_sum = 0.0f64;
            let mut weight_sum = 0.0f64;

            for i in 0..samples {
                // Sample positions along radial line - ASYMMETRIC for motion blur effect
                // For zoom-in (direction=1), sample from outward (0 to 1) - content coming from edges
                // For zoom-out (direction=-1), sample from inward (-1 to 0) - content going to edges
                let t = i as f64 / (samples - 1) as f64; // 0 to 1
                let offset = t * pixel_blur * direction;

                let sample_x = (x as f64 + dir_x * offset).clamp(0.0, (width - 1) as f64);
                let sample_y = (y as f64 + dir_y * offset).clamp(0.0, (height - 1) as f64);

                // Bilinear interpolation for smooth sampling
                let pixel = bilinear_sample(img, sample_x, sample_y);

                // Linear falloff weight (closer samples weighted more)
                let weight = 1.0 - t * 0.7;

                r_sum += pixel[0] as f64 * weight;
                g_sum += pixel[1] as f64 * weight;
                b_sum += pixel[2] as f64 * weight;
                a_sum += pixel[3] as f64 * weight;
                weight_sum += weight;
            }

            output.put_pixel(
                x,
                y,
                Rgba([
                    (r_sum / weight_sum) as u8,
                    (g_sum / weight_sum) as u8,
                    (b_sum / weight_sum) as u8,
                    (a_sum / weight_sum) as u8,
                ]),
            );
        }
    }

    output
}

/// Apply directional (motion) blur in the direction of panning
fn apply_directional_blur(
    img: &RgbaImage,
    velocity_x: f64,
    velocity_y: f64,
    config: &MotionBlurConfig,
) -> RgbaImage {
    let speed = (velocity_x * velocity_x + velocity_y * velocity_y).sqrt();

    // Higher threshold for pan since velocities are in pixels/sec
    if speed < config.velocity_threshold * 500.0 {
        return img.clone();
    }

    let width = img.width();
    let height = img.height();
    let mut output = RgbaImage::new(width, height);

    // Normalize velocity to get direction
    let dir_x = velocity_x / speed;
    let dir_y = velocity_y / speed;

    // Blur strength proportional to speed (linear)
    // Typical pan speed: 500-2000 pixels/second
    let max_speed = 1500.0;
    let normalized_speed = (speed / max_speed).clamp(0.0, 1.0);
    let blur_amount = config.pan_blur_strength * normalized_speed;

    if blur_amount < 0.5 {
        return img.clone();
    }

    let samples = config.pan_blur_samples;

    for y in 0..height {
        for x in 0..width {
            let mut r_sum = 0.0f64;
            let mut g_sum = 0.0f64;
            let mut b_sum = 0.0f64;
            let mut a_sum = 0.0f64;
            let mut weight_sum = 0.0f64;

            for i in 0..samples {
                // Asymmetric sampling - motion blur trails BEHIND movement
                // Sample from current position back along velocity vector
                let t = i as f64 / (samples - 1) as f64; // 0 to 1
                let offset = -t * blur_amount; // Negative = behind movement direction

                let sample_x = (x as f64 + dir_x * offset).clamp(0.0, (width - 1) as f64);
                let sample_y = (y as f64 + dir_y * offset).clamp(0.0, (height - 1) as f64);

                let pixel = bilinear_sample(img, sample_x, sample_y);
                let weight = 1.0 - t * 0.7;

                r_sum += pixel[0] as f64 * weight;
                g_sum += pixel[1] as f64 * weight;
                b_sum += pixel[2] as f64 * weight;
                a_sum += pixel[3] as f64 * weight;
                weight_sum += weight;
            }

            output.put_pixel(
                x,
                y,
                Rgba([
                    (r_sum / weight_sum) as u8,
                    (g_sum / weight_sum) as u8,
                    (b_sum / weight_sum) as u8,
                    (a_sum / weight_sum) as u8,
                ]),
            );
        }
    }

    output
}

/// Bilinear interpolation for smooth sub-pixel sampling
fn bilinear_sample(img: &RgbaImage, x: f64, y: f64) -> Rgba<u8> {
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(img.width() - 1);
    let y1 = (y0 + 1).min(img.height() - 1);

    let fx = x - x0 as f64;
    let fy = y - y0 as f64;

    let p00 = img.get_pixel(x0, y0);
    let p10 = img.get_pixel(x1, y0);
    let p01 = img.get_pixel(x0, y1);
    let p11 = img.get_pixel(x1, y1);

    let lerp = |a: u8, b: u8, t: f64| -> u8 { (a as f64 * (1.0 - t) + b as f64 * t) as u8 };

    let lerp_pixel = |p1: &Rgba<u8>, p2: &Rgba<u8>, t: f64| -> Rgba<u8> {
        Rgba([
            lerp(p1[0], p2[0], t),
            lerp(p1[1], p2[1], t),
            lerp(p1[2], p2[2], t),
            lerp(p1[3], p2[3], t),
        ])
    };

    let top = lerp_pixel(p00, p10, fx);
    let bottom = lerp_pixel(p01, p11, fx);
    lerp_pixel(&top, &bottom, fy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_image(width: u32, height: u32) -> RgbaImage {
        let mut img = RgbaImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let r = (x * 255 / width) as u8;
                let g = (y * 255 / height) as u8;
                img.put_pixel(x, y, Rgba([r, g, 128, 255]));
            }
        }
        img
    }

    #[test]
    fn test_motion_phase_idle() {
        let phase = determine_motion_phase(1.0, 0.0, 0.0, 0.0);
        assert_eq!(phase, MotionPhase::Idle);
    }

    #[test]
    fn test_motion_phase_zoom_in() {
        let phase = determine_motion_phase(1.5, 0.5, 0.0, 0.0);
        assert_eq!(phase, MotionPhase::ZoomIn);
    }

    #[test]
    fn test_motion_phase_zoom_out() {
        let phase = determine_motion_phase(1.5, -0.5, 0.0, 0.0);
        assert_eq!(phase, MotionPhase::ZoomOut);
    }

    #[test]
    fn test_motion_phase_pan() {
        let phase = determine_motion_phase(1.8, 0.0, 200.0, 0.0);
        assert_eq!(phase, MotionPhase::Pan);
    }

    #[test]
    fn test_motion_phase_hold() {
        let phase = determine_motion_phase(1.8, 0.0, 0.0, 0.0);
        assert_eq!(phase, MotionPhase::Hold);
    }

    #[test]
    fn test_radial_blur_no_velocity() {
        let img = create_test_image(100, 100);
        let config = MotionBlurConfig::default();
        let result = apply_radial_blur(&img, 50.0, 50.0, 0.0, &config);
        // Should be unchanged
        assert_eq!(img.get_pixel(50, 50), result.get_pixel(50, 50));
    }

    #[test]
    fn test_radial_blur_with_velocity() {
        let img = create_test_image(100, 100);
        let config = MotionBlurConfig::default();
        let result = apply_radial_blur(&img, 50.0, 50.0, 1.0, &config);
        // Should be blurred (different from original at edges)
        // Center pixel should be similar since blur radiates outward
        let orig_center = img.get_pixel(50, 50);
        let blurred_center = result.get_pixel(50, 50);
        // Center should be close to original
        assert!((orig_center[0] as i32 - blurred_center[0] as i32).abs() < 20);
    }

    #[test]
    fn test_bilinear_sample_integer() {
        let img = create_test_image(100, 100);
        let sampled = bilinear_sample(&img, 50.0, 50.0);
        let direct = *img.get_pixel(50, 50);
        assert_eq!(sampled, direct);
    }
}
