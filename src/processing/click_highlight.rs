use crate::cursor_types::{CursorEvent, EventType};
use crate::processing::effects::blend_channel;
use image::{Rgba, RgbaImage};

/// Configuration for click highlighting effect
pub struct ClickHighlightConfig {
    pub enabled: bool,
    pub duration: f64,   // How long the ripple animation lasts
    pub max_radius: f64, // Maximum radius of the expanding ring
    pub ring_width: f64, // Width of the ring stroke
    pub color: Rgba<u8>, // Color of the ring (with alpha)
}

impl Default for ClickHighlightConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            duration: 0.4,                     // 400ms animation
            max_radius: 50.0,                  // 50px max radius
            ring_width: 3.0,                   // 3px ring width
            color: Rgba([255, 255, 255, 255]), // White (shadow provides contrast)
        }
    }
}

/// Represents an active ripple effect
pub struct ActiveRipple {
    pub x: f64,
    pub y: f64,
    pub progress: f64, // 0.0 to 1.0
}

/// Find all active ripples at a given timestamp
pub fn get_active_ripples(
    timestamp: f64,
    cursor_events: &[CursorEvent],
    config: &ClickHighlightConfig,
) -> Vec<ActiveRipple> {
    cursor_events
        .iter()
        .filter(|e| matches!(e.event_type, EventType::LeftClick | EventType::RightClick))
        .filter_map(|click| {
            let elapsed = timestamp - click.timestamp;
            // Only include clicks that are within the animation window
            if elapsed >= 0.0 && elapsed < config.duration {
                let progress = elapsed / config.duration;
                Some(ActiveRipple {
                    x: click.x,
                    y: click.y,
                    progress,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Ease-out cubic: starts fast, ends slow
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// Draw click highlights on the canvas
pub fn draw_click_highlights(
    canvas: &mut RgbaImage,
    ripples: &[ActiveRipple],
    config: &ClickHighlightConfig,
) {
    if !config.enabled {
        return;
    }

    for ripple in ripples {
        draw_ring(canvas, ripple.x, ripple.y, ripple.progress, config);
    }
}

/// Draw a single expanding ring with shadow for visibility
fn draw_ring(
    canvas: &mut RgbaImage,
    center_x: f64,
    center_y: f64,
    progress: f64,
    config: &ClickHighlightConfig,
) {
    let eased_progress = ease_out_cubic(progress);

    // Calculate current radius (expands from 0 to max_radius)
    let radius = config.max_radius * eased_progress;

    // Calculate opacity (fades from 1.0 to 0.0)
    let opacity = 1.0 - eased_progress;

    if radius < 1.0 || opacity < 0.01 {
        return;
    }

    // Draw shadow/outline first (slightly larger, dark)
    let shadow_width = config.ring_width + 3.0;
    let shadow_inner = (radius - shadow_width / 2.0).max(0.0);
    let shadow_outer = radius + shadow_width / 2.0;
    let shadow_color = Rgba([0, 0, 0, 150]); // Dark semi-transparent shadow
    draw_ring_pixels(
        canvas,
        center_x,
        center_y,
        shadow_inner,
        shadow_outer,
        opacity * 0.6,
        &shadow_color,
    );

    // Draw main ring on top
    let inner_radius = (radius - config.ring_width / 2.0).max(0.0);
    let outer_radius = radius + config.ring_width / 2.0;
    draw_ring_pixels(
        canvas,
        center_x,
        center_y,
        inner_radius,
        outer_radius,
        opacity,
        &config.color,
    );
}

/// Draw ring pixels with given radii and color
fn draw_ring_pixels(
    canvas: &mut RgbaImage,
    center_x: f64,
    center_y: f64,
    inner_radius: f64,
    outer_radius: f64,
    opacity: f64,
    color: &Rgba<u8>,
) {
    if outer_radius < 1.0 {
        return;
    }

    // Calculate bounding box for the ring
    let min_x = ((center_x - outer_radius - 1.0).max(0.0)) as u32;
    let min_y = ((center_y - outer_radius - 1.0).max(0.0)) as u32;
    let max_x = ((center_x + outer_radius + 1.0).min(canvas.width() as f64 - 1.0)) as u32;
    let max_y = ((center_y + outer_radius + 1.0).min(canvas.height() as f64 - 1.0)) as u32;

    // Draw the ring pixel by pixel
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let dx = px as f64 - center_x;
            let dy = py as f64 - center_y;
            let dist = (dx * dx + dy * dy).sqrt();

            // Check if pixel is within the ring
            if dist >= inner_radius && dist <= outer_radius {
                // Calculate anti-aliased alpha based on distance to ring edges
                let edge_alpha = if dist < inner_radius + 1.0 {
                    // Inner edge anti-aliasing
                    dist - inner_radius
                } else if dist > outer_radius - 1.0 {
                    // Outer edge anti-aliasing
                    outer_radius - dist
                } else {
                    1.0
                };

                let final_alpha = (edge_alpha * opacity * color[3] as f64 / 255.0 * 255.0) as u8;

                if final_alpha > 0 {
                    let pixel = canvas.get_pixel_mut(px, py);
                    pixel[0] = blend_channel(pixel[0], color[0], final_alpha);
                    pixel[1] = blend_channel(pixel[1], color[1], final_alpha);
                    pixel[2] = blend_channel(pixel[2], color[2], final_alpha);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_click(x: f64, y: f64, timestamp: f64) -> CursorEvent {
        CursorEvent {
            x,
            y,
            timestamp,
            event_type: EventType::LeftClick,
        }
    }

    fn make_move(x: f64, y: f64, timestamp: f64) -> CursorEvent {
        CursorEvent {
            x,
            y,
            timestamp,
            event_type: EventType::Move,
        }
    }

    #[test]
    fn test_no_ripples_before_click() {
        let config = ClickHighlightConfig::default();
        let events = vec![make_click(100.0, 100.0, 1.0)];

        let ripples = get_active_ripples(0.5, &events, &config);
        assert!(ripples.is_empty(), "Should have no ripples before click");
    }

    #[test]
    fn test_ripple_during_animation() {
        let config = ClickHighlightConfig::default();
        let events = vec![make_click(100.0, 100.0, 1.0)];

        // At midpoint of animation
        let ripples = get_active_ripples(1.2, &events, &config);
        assert_eq!(ripples.len(), 1, "Should have one ripple");
        assert!((ripples[0].x - 100.0).abs() < 0.01);
        assert!((ripples[0].y - 100.0).abs() < 0.01);
        assert!(ripples[0].progress > 0.0 && ripples[0].progress < 1.0);
    }

    #[test]
    fn test_no_ripple_after_duration() {
        let config = ClickHighlightConfig::default();
        let events = vec![make_click(100.0, 100.0, 1.0)];

        let ripples = get_active_ripples(1.5, &events, &config);
        assert!(ripples.is_empty(), "Should have no ripples after duration");
    }

    #[test]
    fn test_only_clicks_create_ripples() {
        let config = ClickHighlightConfig::default();
        let events = vec![
            make_move(50.0, 50.0, 0.9),
            make_click(100.0, 100.0, 1.0),
            make_move(150.0, 150.0, 1.1),
        ];

        let ripples = get_active_ripples(1.2, &events, &config);
        assert_eq!(ripples.len(), 1, "Only clicks should create ripples");
        assert!((ripples[0].x - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_multiple_overlapping_ripples() {
        let config = ClickHighlightConfig::default();
        let events = vec![make_click(100.0, 100.0, 1.0), make_click(200.0, 200.0, 1.2)];

        // At 1.3s: first click at 0.3s progress, second at 0.1s progress
        let ripples = get_active_ripples(1.3, &events, &config);
        assert_eq!(ripples.len(), 2, "Should have two overlapping ripples");
    }

    #[test]
    fn test_draw_ring_modifies_canvas() {
        let config = ClickHighlightConfig::default();
        let mut canvas = RgbaImage::from_pixel(200, 200, Rgba([0, 0, 0, 255]));

        let ripples = vec![ActiveRipple {
            x: 100.0,
            y: 100.0,
            progress: 0.5,
        }];

        draw_click_highlights(&mut canvas, &ripples, &config);

        // Check that some pixels were modified (ring was drawn)
        let mut found_white = false;
        for y in 0..200 {
            for x in 0..200 {
                let pixel = canvas.get_pixel(x, y);
                if pixel[0] > 0 || pixel[1] > 0 || pixel[2] > 0 {
                    found_white = true;
                    break;
                }
            }
        }
        assert!(found_white, "Ring should have been drawn on canvas");
    }
}
