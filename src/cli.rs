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
    },
}

#[derive(Clone, ValueEnum)]
pub enum ListTarget {
    /// List available displays
    Displays,
    /// List available windows
    Windows,
}
