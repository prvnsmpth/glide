use crate::macos::event_tap::{CursorEvent, EventType};

/// Zoom configuration
pub struct ZoomConfig {
    pub max_zoom: f64,  // Target zoom level
    pub ease_in: f64,   // Ease in duration (anticipatory - starts before click)
    pub hold: f64,      // Hold duration at max zoom; also determines panning behavior
    pub ease_out: f64,  // Ease out duration
    pub debounce: f64,  // Ignore clicks within this time of previous click
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self {
            max_zoom: 1.5,  // Gentler zoom
            ease_in: 0.6,   // Anticipatory zoom starts 0.6s before click
            hold: 4.0,      // Hold duration at max zoom
            ease_out: 0.8,  // Slow zoom out
            debounce: 0.5,  // Ignore clicks within 0.5s of previous
        }
    }
}

impl ZoomConfig {
    pub fn total_duration(&self) -> f64 {
        self.ease_in + self.hold + self.ease_out
    }
}

/// Calculate zoom level and cursor position for a given timestamp.
/// Uses anticipatory zoom (starts before click) and smart panning between nearby clicks.
pub fn calculate_zoom(
    timestamp: f64,
    cursor_events: &[CursorEvent],
    config: &ZoomConfig,
) -> (f64, f64, f64) {
    // Get all effective clicks (debounced)
    let effective_clicks = get_effective_clicks(cursor_events, config);

    // Find previous click (most recent before timestamp) and next click (first after timestamp)
    let prev_click = effective_clicks
        .iter()
        .filter(|c| c.timestamp <= timestamp)
        .last()
        .copied();

    let next_click = effective_clicks
        .iter()
        .find(|c| c.timestamp > timestamp)
        .copied();

    // Find current cursor position for idle state
    let default_pos = cursor_events
        .iter()
        .filter(|e| e.timestamp <= timestamp)
        .last()
        .map(|e| (e.x, e.y))
        .unwrap_or((0.0, 0.0));

    // Pan if next click's anticipatory zoom would start before current zoom-out completes.
    // This ensures smooth transitions with no discontinuity in zoom level.
    // pan_window = hold + ease_out + ease_in
    let pan_window = config.hold + config.ease_out + config.ease_in;

    // Case 1: Anticipatory zoom-in (next click coming soon)
    if let Some(next) = next_click {
        let time_to_next = next.timestamp - timestamp;
        if time_to_next > 0.0 && time_to_next <= config.ease_in {
            // We're in the anticipatory zoom-in phase
            let progress = 1.0 - (time_to_next / config.ease_in);
            let zoom = 1.0 + (config.max_zoom - 1.0) * ease_out_cubic(progress);

            // Check if we're also transitioning from a previous click (panning while zooming)
            if let Some(prev) = prev_click {
                let gap = next.timestamp - prev.timestamp;
                if gap <= pan_window {
                    // Pan from prev to next while staying zoomed
                    let x = lerp(prev.x, next.x, ease_in_out_cubic(progress));
                    let y = lerp(prev.y, next.y, ease_in_out_cubic(progress));
                    return (zoom.max(config.max_zoom), x, y);
                }
            }

            return (zoom, next.x, next.y);
        }
    }

    // Case 2: Currently at/after a click
    if let Some(prev) = prev_click {
        let elapsed = timestamp - prev.timestamp;

        // Check if we should pan to next click (staying zoomed)
        if let Some(next) = next_click {
            let gap = next.timestamp - prev.timestamp;

            if gap <= pan_window {
                // We're in pan mode - stay at max zoom and interpolate position
                let time_to_next = next.timestamp - timestamp;

                // During hold phase: stay at prev position
                if elapsed <= config.hold && time_to_next > config.ease_in {
                    return (config.max_zoom, prev.x, prev.y);
                }

                // During pan phase: interpolate from prev to next
                // Pan starts after hold ends OR when we're within ease_in of next click
                let pan_start_time = (prev.timestamp + config.hold).min(next.timestamp - config.ease_in);
                if timestamp >= pan_start_time {
                    let pan_duration = next.timestamp - pan_start_time;
                    let pan_elapsed = timestamp - pan_start_time;
                    let pan_progress = (pan_elapsed / pan_duration).clamp(0.0, 1.0);

                    let x = lerp(prev.x, next.x, ease_in_out_cubic(pan_progress));
                    let y = lerp(prev.y, next.y, ease_in_out_cubic(pan_progress));
                    return (config.max_zoom, x, y);
                }

                // Still in hold phase
                return (config.max_zoom, prev.x, prev.y);
            }
        }

        // No upcoming click within pan window - normal hold/zoom-out behavior
        if elapsed <= config.hold {
            // Hold phase
            return (config.max_zoom, prev.x, prev.y);
        } else if elapsed <= config.hold + config.ease_out {
            // Zoom out phase
            let progress = (elapsed - config.hold) / config.ease_out;
            let zoom = config.max_zoom - (config.max_zoom - 1.0) * ease_in_cubic(progress);
            return (zoom, prev.x, prev.y);
        }
    }

    // Case 3: Idle (no relevant clicks)
    (1.0, default_pos.0, default_pos.1)
}

/// Get all effective clicks (filtered by debounce)
fn get_effective_clicks<'a>(events: &'a [CursorEvent], config: &ZoomConfig) -> Vec<&'a CursorEvent> {
    let clicks: Vec<_> = events
        .iter()
        .filter(|e| matches!(e.event_type, EventType::LeftClick | EventType::RightClick))
        .collect();

    let mut effective: Vec<&CursorEvent> = Vec::new();

    for click in clicks {
        match effective.last() {
            None => effective.push(click),
            Some(prev) => {
                if click.timestamp - prev.timestamp > config.debounce {
                    effective.push(click);
                }
            }
        }
    }

    effective
}

/// Linear interpolation
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Ease-out cubic: starts fast, ends slow (responsive)
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// Ease-in cubic: starts slow, ends fast (smooth exit)
fn ease_in_cubic(t: f64) -> f64 {
    t * t * t
}

/// Ease-in-out cubic: slow start, fast middle, slow end (smooth panning)
fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
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

    #[test]
    fn test_anticipatory_zoom_single_click() {
        let config = ZoomConfig::default();
        // Click at t=1.0s, hold=4.0s, ease_out=0.8s
        let events = vec![make_click(100.0, 100.0, 1.0)];

        // Before anticipatory window: should be idle (zoom=1.0)
        let (zoom, _, _) = calculate_zoom(0.3, &events, &config);
        assert!(
            (zoom - 1.0).abs() < 0.01,
            "Should be idle before anticipatory window"
        );

        // During anticipatory zoom (0.4s before click)
        let (zoom, x, y) = calculate_zoom(0.6, &events, &config);
        assert!(zoom > 1.0 && zoom < config.max_zoom, "Should be zooming in");
        assert!((x - 100.0).abs() < 0.01, "Should target click position");
        assert!((y - 100.0).abs() < 0.01, "Should target click position");

        // At click moment: should be at max zoom
        let (zoom, _, _) = calculate_zoom(1.0, &events, &config);
        assert!(
            (zoom - config.max_zoom).abs() < 0.01,
            "Should be at max zoom at click moment"
        );

        // During hold
        let (zoom, _, _) = calculate_zoom(3.0, &events, &config);
        assert!(
            (zoom - config.max_zoom).abs() < 0.01,
            "Should hold at max zoom"
        );

        // During zoom out (hold ends at 1.0 + 4.0 = 5.0s)
        let (zoom, _, _) = calculate_zoom(5.5, &events, &config);
        assert!(
            zoom > 1.0 && zoom < config.max_zoom,
            "Should be zooming out"
        );

        // After zoom out complete (5.0 + 0.8 = 5.8s)
        let (zoom, _, _) = calculate_zoom(6.0, &events, &config);
        assert!((zoom - 1.0).abs() < 0.01, "Should be back to idle");
    }

    #[test]
    fn test_panning_between_close_clicks() {
        let config = ZoomConfig::default();
        // Pan window = hold + ease_out + ease_in = 4.0 + 0.8 + 0.6 = 5.4s
        // Two clicks 4.0s apart (within pan window)
        let events = vec![
            make_click(100.0, 100.0, 1.0),
            make_click(200.0, 200.0, 5.0),
        ];

        // At first click: max zoom at first position
        let (zoom, x, _) = calculate_zoom(1.0, &events, &config);
        assert!((zoom - config.max_zoom).abs() < 0.01);
        assert!((x - 100.0).abs() < 0.01);

        // During hold at first click
        let (zoom, x, _) = calculate_zoom(3.0, &events, &config);
        assert!(
            (zoom - config.max_zoom).abs() < 0.01,
            "Should stay at max zoom"
        );
        assert!(
            (x - 100.0).abs() < 0.01,
            "Should stay at first click position during hold"
        );

        // During pan phase (approaching second click)
        let (zoom, x, _) = calculate_zoom(4.7, &events, &config);
        assert!(
            (zoom - config.max_zoom).abs() < 0.01,
            "Should stay at max zoom during pan"
        );
        assert!(
            x > 100.0 && x < 200.0,
            "Should be interpolating x position"
        );

        // At second click: max zoom at second position
        let (zoom, x, y) = calculate_zoom(5.0, &events, &config);
        assert!((zoom - config.max_zoom).abs() < 0.01);
        assert!((x - 200.0).abs() < 0.01);
        assert!((y - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_zoom_out_between_far_clicks() {
        let config = ZoomConfig::default();
        // Pan window = hold + ease_out + ease_in = 4.0 + 0.8 + 0.6 = 5.4s
        // Two clicks 10s apart (outside pan window)
        let events = vec![
            make_click(100.0, 100.0, 1.0),
            make_click(200.0, 200.0, 11.0),
        ];

        // After first click's zoom out completes (1.0 + 4.0 hold + 0.8 ease_out = 5.8s)
        let (zoom, _, _) = calculate_zoom(6.0, &events, &config);
        assert!(
            (zoom - 1.0).abs() < 0.01,
            "Should zoom out to idle between far clicks"
        );

        // Before second click's anticipatory zoom
        let (zoom, _, _) = calculate_zoom(10.0, &events, &config);
        assert!((zoom - 1.0).abs() < 0.01, "Should be idle before second click");

        // During anticipatory zoom to second click
        let (zoom, x, _) = calculate_zoom(10.6, &events, &config);
        assert!(zoom > 1.0, "Should be zooming in to second click");
        assert!((x - 200.0).abs() < 0.01, "Should target second click position");
    }

    #[test]
    fn test_double_click_debounce() {
        let config = ZoomConfig::default();
        // Two clicks 0.1s apart (within debounce of 0.5s)
        let events = vec![
            make_click(100.0, 100.0, 1.0),
            make_click(150.0, 150.0, 1.1),
        ];

        let effective = get_effective_clicks(&events, &config);
        assert_eq!(effective.len(), 1, "Second click should be debounced");
        assert!(
            (effective[0].timestamp - 1.0).abs() < 0.01,
            "Should keep first click"
        );
    }

    #[test]
    fn test_three_rapid_clicks_pan_through() {
        let config = ZoomConfig::default();
        // Pan window = 5.4s, three clicks each 3s apart (within pan window)
        let events = vec![
            make_click(100.0, 100.0, 1.0),
            make_click(200.0, 200.0, 4.0),
            make_click(300.0, 300.0, 7.0),
        ];

        // Should stay zoomed throughout and pan between all three
        let (zoom, _, _) = calculate_zoom(2.0, &events, &config);
        assert!((zoom - config.max_zoom).abs() < 0.01, "Should stay zoomed");

        let (zoom, _, _) = calculate_zoom(5.0, &events, &config);
        assert!(
            (zoom - config.max_zoom).abs() < 0.01,
            "Should stay zoomed through second click"
        );

        // After third click, should eventually zoom out (7.0 + 4.0 hold + 0.8 ease_out = 11.8s)
        let (zoom, _, _) = calculate_zoom(12.0, &events, &config);
        assert!(
            (zoom - 1.0).abs() < 0.01,
            "Should zoom out after last click"
        );
    }
}
