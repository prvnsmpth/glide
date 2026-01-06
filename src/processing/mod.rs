pub mod cursor;
pub mod effects;
pub mod frames;
pub mod pipeline;
pub mod zoom;

// Re-export the main entry point
pub use pipeline::process_video;
