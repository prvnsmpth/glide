//! Linux X11 support module
//!
//! Provides display enumeration, window enumeration, cursor tracking,
//! and screen capture for Linux X11 environments.

pub mod capture;
pub mod display;
pub mod event_tap;
pub mod window;

// Re-export commonly used types
pub use capture::{
    find_display, find_window, start_display_capture, start_window_capture, CaptureConfig,
    CaptureSession, CapturedFrame,
};
pub use display::{list_displays, DisplayInfo};
pub use event_tap::CursorTracker;
pub use window::{list_windows, WindowInfo};
