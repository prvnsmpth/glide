pub mod display;
pub mod event_tap;
pub mod window;

// Re-export commonly used types
pub use display::{list_displays, DisplayInfo};
pub use event_tap::CursorTracker;
pub use window::{list_windows, WindowInfo};
