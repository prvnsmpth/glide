use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "glide")]
#[command(about = "CLI screen recorder for macOS with auto-zoom on clicks")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// List available displays or windows
    List {
        #[arg(value_enum)]
        target: ListTarget,
    },

    /// Record screen or window
    Record {
        /// Display ID to record
        #[arg(long, conflicts_with = "window")]
        display: Option<u32>,

        /// Window ID to record
        #[arg(long, conflicts_with = "display")]
        window: Option<u32>,

        /// Output file path
        #[arg(short, long)]
        output: PathBuf,

        /// Capture system cursor in video (default: false, custom cursor rendered during processing)
        #[arg(long)]
        capture_system_cursor: bool,
    },

    /// Process recorded video with effects
    Process {
        /// Input video file
        input: PathBuf,

        /// Output video file
        #[arg(short, long)]
        output: PathBuf,

        /// Background color (hex) or image path
        #[arg(long)]
        background: Option<String>,

        /// Trim N seconds from the start of the video
        #[arg(long, value_name = "SECONDS")]
        trim_start: Option<f64>,

        /// Trim N seconds from the end of the video
        #[arg(long, value_name = "SECONDS")]
        trim_end: Option<f64>,

        /// Cursor scale factor (default: 1.5)
        #[arg(long, default_value = "1.5")]
        cursor_scale: f64,

        /// Seconds of inactivity before cursor fades (default: 2.0)
        #[arg(long, default_value = "2.0")]
        cursor_timeout: f64,

        /// Disable custom cursor rendering
        #[arg(long)]
        no_cursor: bool,

        /// Disable motion blur during zoom/pan transitions
        #[arg(long)]
        no_motion_blur: bool,
    },
}

#[derive(Clone, ValueEnum)]
pub enum ListTarget {
    /// List available displays
    Displays,
    /// List available windows
    Windows,
}
