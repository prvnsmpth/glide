use crate::cursor::CursorEvent;

/// Configuration for cursor rendering and smoothing
pub struct CursorConfig {
    /// Time window for smoothing (seconds)
    pub smooth_window: f64,
    /// Seconds of inactivity before cursor starts fading
    pub inactivity_timeout: f64,
    /// Duration of fade animation (seconds)
    pub fade_duration: f64,
    /// Cursor scale factor
    pub cursor_scale: f64,
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            smooth_window: 0.15,      // 150ms smoothing window (more noticeable)
            inactivity_timeout: 2.0,  // Fade after 2s inactivity
            fade_duration: 0.3,       // 300ms fade animation
            cursor_scale: 1.5,        // 1.5x cursor size
        }
    }
}

impl CursorConfig {
    pub fn new(cursor_scale: f64, inactivity_timeout: f64) -> Self {
        Self {
            cursor_scale,
            inactivity_timeout,
            ..Default::default()
        }
    }
}

/// Current state of the cursor for rendering
pub struct CursorState {
    pub x: f64,
    pub y: f64,
    pub opacity: f64,
}

/// Get the smoothed cursor position and opacity for a given timestamp
pub fn get_smoothed_cursor(
    timestamp: f64,
    cursor_events: &[CursorEvent],
    config: &CursorConfig,
) -> CursorState {
    // Find smoothed position
    let (x, y) = get_smoothed_position(timestamp, cursor_events, config.smooth_window);

    // Calculate opacity based on activity
    let opacity = calculate_activity_opacity(timestamp, cursor_events, config);

    CursorState { x, y, opacity }
}

/// Get smoothed cursor position using Gaussian-weighted moving average
fn get_smoothed_position(
    timestamp: f64,
    cursor_events: &[CursorEvent],
    smooth_window: f64,
) -> (f64, f64) {
    // Use a larger window for gathering events, smooth_window controls the falloff
    let window_start = timestamp - smooth_window * 2.0;
    let window_end = timestamp + smooth_window * 0.5; // Less look-ahead to reduce lag

    let events_in_window: Vec<_> = cursor_events
        .iter()
        .filter(|e| e.timestamp >= window_start && e.timestamp <= window_end)
        .collect();

    if events_in_window.is_empty() {
        // Fall back to most recent event before timestamp
        return cursor_events
            .iter()
            .filter(|e| e.timestamp <= timestamp)
            .last()
            .map(|e| (e.x, e.y))
            .unwrap_or((0.0, 0.0));
    }

    if events_in_window.len() == 1 {
        return (events_in_window[0].x, events_in_window[0].y);
    }

    // Gaussian-weighted moving average for smoother results
    // Sigma controls the smoothing amount
    let sigma = smooth_window;
    let mut total_weight = 0.0;
    let mut weighted_x = 0.0;
    let mut weighted_y = 0.0;

    for event in &events_in_window {
        let time_diff = event.timestamp - timestamp;
        // Gaussian weight: e^(-(t^2)/(2*sigma^2))
        // Bias towards past events slightly (less lag)
        let adjusted_diff = if time_diff > 0.0 { time_diff * 2.0 } else { time_diff };
        let weight = (-adjusted_diff * adjusted_diff / (2.0 * sigma * sigma)).exp();

        weighted_x += event.x * weight;
        weighted_y += event.y * weight;
        total_weight += weight;
    }

    if total_weight > 0.0 {
        (weighted_x / total_weight, weighted_y / total_weight)
    } else {
        (events_in_window[0].x, events_in_window[0].y)
    }
}

/// Calculate cursor opacity based on activity state
fn calculate_activity_opacity(
    timestamp: f64,
    cursor_events: &[CursorEvent],
    config: &CursorConfig,
) -> f64 {
    // Find last activity (any event - move or click)
    let last_activity = cursor_events
        .iter()
        .filter(|e| e.timestamp <= timestamp)
        .last();

    let last_activity_time = match last_activity {
        Some(event) => event.timestamp,
        None => return 0.0, // No events yet, cursor hidden
    };

    let idle_time = timestamp - last_activity_time;

    if idle_time < config.inactivity_timeout {
        // Fully visible
        1.0
    } else if idle_time < config.inactivity_timeout + config.fade_duration {
        // Fading out
        let fade_progress = (idle_time - config.inactivity_timeout) / config.fade_duration;
        1.0 - ease_out_cubic(fade_progress)
    } else {
        // Fully hidden
        0.0
    }
}

/// Ease-out cubic: starts fast, ends slow
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::EventType;

    fn make_move(x: f64, y: f64, timestamp: f64) -> CursorEvent {
        CursorEvent {
            x,
            y,
            timestamp,
            event_type: EventType::Move,
        }
    }

    #[test]
    fn test_smoothed_position_single_event() {
        let events = vec![make_move(100.0, 200.0, 1.0)];
        let config = CursorConfig::default();

        let state = get_smoothed_cursor(1.0, &events, &config);
        assert!((state.x - 100.0).abs() < 0.01);
        assert!((state.y - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_smoothed_position_multiple_events() {
        let events = vec![
            make_move(100.0, 100.0, 0.98),
            make_move(110.0, 110.0, 1.0),
            make_move(120.0, 120.0, 1.02),
        ];
        let config = CursorConfig::default();

        let state = get_smoothed_cursor(1.0, &events, &config);
        // Should be weighted average, closer to the middle event
        assert!(state.x > 105.0 && state.x < 115.0);
        assert!(state.y > 105.0 && state.y < 115.0);
    }

    #[test]
    fn test_opacity_active() {
        let events = vec![make_move(100.0, 100.0, 1.0)];
        let config = CursorConfig::default();

        // Immediately after event
        let state = get_smoothed_cursor(1.0, &events, &config);
        assert!((state.opacity - 1.0).abs() < 0.01);

        // Still within timeout
        let state = get_smoothed_cursor(2.5, &events, &config);
        assert!((state.opacity - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_opacity_fading() {
        let events = vec![make_move(100.0, 100.0, 1.0)];
        let config = CursorConfig::default();

        // During fade (2.0s timeout + some fade time)
        let state = get_smoothed_cursor(3.15, &events, &config);
        assert!(state.opacity > 0.0 && state.opacity < 1.0, "Should be fading");
    }

    #[test]
    fn test_opacity_hidden() {
        let events = vec![make_move(100.0, 100.0, 1.0)];
        let config = CursorConfig::default();

        // After fade complete (2.0s timeout + 0.3s fade)
        let state = get_smoothed_cursor(3.5, &events, &config);
        assert!(state.opacity < 0.01, "Should be hidden");
    }

    #[test]
    fn test_no_events() {
        let events: Vec<CursorEvent> = vec![];
        let config = CursorConfig::default();

        let state = get_smoothed_cursor(1.0, &events, &config);
        assert!(state.opacity < 0.01, "Should be hidden with no events");
    }
}
