pub mod capture;
pub mod encoder;
pub mod metadata;
pub mod recorder;

// Re-export commonly used types
pub use recorder::{record_display, record_window};
