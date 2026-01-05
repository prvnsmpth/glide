use crate::cursor::{CursorEvent, EventType};

/// Zoom configuration
pub struct ZoomConfig {
    pub max_zoom: f64,      // Target zoom level (1.8x)
    pub ease_in: f64,       // Ease in duration (0.3s)
    pub hold: f64,          // Hold duration (2.5s)
    pub ease_out: f64,      // Ease out duration (0.3s)
    pub debounce: f64,      // Ignore clicks within this time of previous click
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self {
            max_zoom: 1.5,       // Gentler zoom
            ease_in: 0.6,        // Slower, gentler zoom in
            hold: 2.0,           // Hold duration
            ease_out: 0.8,       // Slow zoom out
            debounce: 0.5,       // Ignore clicks within 0.5s of previous
        }
    }
}

impl ZoomConfig {
    pub fn total_duration(&self) -> f64 {
        self.ease_in + self.hold + self.ease_out
    }
}

/// Calculate zoom level and cursor position for a given timestamp
pub fn calculate_zoom(
    timestamp: f64,
    cursor_events: &[CursorEvent],
    config: &ZoomConfig,
) -> (f64, f64, f64) {
    // Get all clicks before this timestamp
    let clicks: Vec<_> = cursor_events
        .iter()
        .filter(|e| e.timestamp <= timestamp)
        .filter(|e| matches!(e.event_type, EventType::LeftClick | EventType::RightClick))
        .collect();

    // Find effective clicks (current and previous) for smooth transitions
    let (prev_click, current_click) = find_effective_clicks(&clicks, config);

    // Find current cursor position (most recent move/click before timestamp)
    let current_pos = cursor_events
        .iter()
        .filter(|e| e.timestamp <= timestamp)
        .last();

    let default_pos = current_pos
        .map(|e| (e.x, e.y))
        .unwrap_or((0.0, 0.0));

    match current_click {
        Some(click) => {
            let elapsed = timestamp - click.timestamp;

            // Check if we're transitioning from a previous zoom
            if let Some(prev) = prev_click {
                let prev_elapsed = timestamp - prev.timestamp;
                let prev_zoom = calculate_zoom_level(prev_elapsed, config);

                // If previous zoom is still active (not back to 1.0), blend from it
                if prev_zoom > 1.01 && elapsed < config.ease_in {
                    let blend_t = elapsed / config.ease_in;
                    let blend = ease_out_cubic(blend_t);

                    // Blend zoom level
                    let target_zoom = config.max_zoom;
                    let zoom = prev_zoom + (target_zoom - prev_zoom) * blend;

                    // Blend position
                    let cursor_x = prev.x + (click.x - prev.x) * blend;
                    let cursor_y = prev.y + (click.y - prev.y) * blend;

                    return (zoom, cursor_x, cursor_y);
                }
            }

            // Normal zoom calculation
            let zoom = calculate_zoom_level(elapsed, config);
            (zoom, click.x, click.y)
        }
        None => (1.0, default_pos.0, default_pos.1),
    }
}

/// Find the current and previous effective clicks for smooth transitions
fn find_effective_clicks<'a>(clicks: &[&'a CursorEvent], config: &ZoomConfig) -> (Option<&'a CursorEvent>, Option<&'a CursorEvent>) {
    if clicks.is_empty() {
        return (None, None);
    }

    // Walk through clicks and find effective ones
    // A click is "effective" if it's not within debounce period of a previous effective click
    let mut effective_clicks: Vec<&CursorEvent> = Vec::new();

    for click in clicks {
        match effective_clicks.last() {
            None => {
                effective_clicks.push(click);
            }
            Some(prev) => {
                let time_since_prev = click.timestamp - prev.timestamp;
                // Only accept this click if enough time has passed since the previous effective click
                if time_since_prev > config.debounce {
                    effective_clicks.push(click);
                }
                // Otherwise, ignore this click (it's part of a double-click or rapid clicking)
            }
        }
    }

    let len = effective_clicks.len();
    if len == 0 {
        (None, None)
    } else if len == 1 {
        (None, Some(effective_clicks[0]))
    } else {
        (Some(effective_clicks[len - 2]), Some(effective_clicks[len - 1]))
    }
}

/// Calculate zoom level based on elapsed time since click
fn calculate_zoom_level(elapsed: f64, config: &ZoomConfig) -> f64 {
    let total_duration = config.total_duration();

    if elapsed < 0.0 || elapsed > total_duration {
        return 1.0;
    }

    if elapsed < config.ease_in {
        // Zoom in: use ease-out (starts fast, ends slow) for responsive feel
        let t = elapsed / config.ease_in;
        let eased = ease_out_cubic(t);
        1.0 + (config.max_zoom - 1.0) * eased
    } else if elapsed < config.ease_in + config.hold {
        // Hold at max zoom
        config.max_zoom
    } else {
        // Zoom out: use ease-in (starts slow, ends fast) for smooth exit
        let t = (elapsed - config.ease_in - config.hold) / config.ease_out;
        let eased = ease_in_cubic(t);
        config.max_zoom - (config.max_zoom - 1.0) * eased
    }
}

/// Ease-out cubic: starts fast, ends slow (responsive)
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// Ease-in cubic: starts slow, ends fast (smooth exit)
fn ease_in_cubic(t: f64) -> f64 {
    t * t * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zoom_timing() {
        let config = ZoomConfig::default();

        assert!((calculate_zoom_level(-0.1, &config) - 1.0).abs() < 0.01);
        assert!((calculate_zoom_level(0.0, &config) - 1.0).abs() < 0.01);

        let mid_ease_in = calculate_zoom_level(0.3, &config);
        assert!(mid_ease_in > 1.0 && mid_ease_in < config.max_zoom);

        // At ease_in duration, should be at max zoom
        assert!((calculate_zoom_level(config.ease_in + 0.1, &config) - config.max_zoom).abs() < 0.01);
        // During hold
        assert!((calculate_zoom_level(config.ease_in + 1.0, &config) - config.max_zoom).abs() < 0.01);
        // After total duration
        assert!((calculate_zoom_level(config.total_duration() + 1.0, &config) - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_double_click_debounce() {
        let config = ZoomConfig::default();

        // Simulate double-click: two clicks 0.1s apart
        let click1 = CursorEvent {
            x: 100.0,
            y: 100.0,
            timestamp: 1.0,
            event_type: EventType::LeftClick,
        };
        let click2 = CursorEvent {
            x: 100.0,
            y: 100.0,
            timestamp: 1.1, // 0.1s after first click
            event_type: EventType::LeftClick,
        };

        let clicks = vec![&click1, &click2];
        let (_, current) = find_effective_clicks(&clicks, &config);

        // Should use first click, ignore second (debounced)
        assert_eq!(current.unwrap().timestamp, 1.0);
    }

    #[test]
    fn test_smooth_transition_between_clicks() {
        let config = ZoomConfig::default();

        // Two clicks far enough apart to both be effective
        let click1 = CursorEvent {
            x: 100.0,
            y: 100.0,
            timestamp: 1.0,
            event_type: EventType::LeftClick,
        };
        let click2 = CursorEvent {
            x: 200.0,
            y: 200.0,
            timestamp: 2.0, // 1s after first click (after debounce)
            event_type: EventType::LeftClick,
        };

        let clicks = vec![&click1, &click2];
        let (prev, current) = find_effective_clicks(&clicks, &config);

        // Should have both clicks as effective
        assert_eq!(prev.unwrap().timestamp, 1.0);
        assert_eq!(current.unwrap().timestamp, 2.0);
    }
}
