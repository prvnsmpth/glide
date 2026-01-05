mod cli;
mod cursor;
mod cursor_smooth;
mod display;
mod metadata;
mod processor;
mod recorder;
mod window;
mod zoom;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands, ListTarget};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::List { target } => match target {
            ListTarget::Displays => {
                let displays = display::list_displays()?;
                if displays.is_empty() {
                    println!("No displays found.");
                } else {
                    println!("Available displays:");
                    for d in displays {
                        println!(
                            "  [{index}] {width}x{height}{main}",
                            index = d.index,
                            width = d.width,
                            height = d.height,
                            main = if d.is_main { " (main)" } else { "" }
                        );
                    }
                }
            }
            ListTarget::Windows => {
                let windows = window::list_windows()?;
                if windows.is_empty() {
                    println!("No windows found.");
                } else {
                    println!("Available windows:");
                    for w in windows {
                        println!(
                            "  [{id}] {owner} - {name} ({width}x{height})",
                            id = w.id,
                            owner = w.owner,
                            name = if w.name.is_empty() { "(untitled)" } else { &w.name },
                            width = w.bounds.2,
                            height = w.bounds.3,
                        );
                    }
                }
            }
        },
        Commands::Record {
            display,
            window,
            output,
            capture_system_cursor,
        } => {
            if let Some(display_index) = display {
                // Look up the display info
                let displays = display::list_displays()?;
                let display_info = displays
                    .into_iter()
                    .find(|d| d.index == display_index as usize)
                    .ok_or_else(|| anyhow::anyhow!("Display {} not found", display_index))?;
                recorder::record_display(&display_info, &output, capture_system_cursor)?;
            } else if let Some(window_id) = window {
                let windows = window::list_windows()?;
                let window_info = windows
                    .into_iter()
                    .find(|w| w.id == window_id)
                    .ok_or_else(|| anyhow::anyhow!("Window {} not found", window_id))?;
                recorder::record_window(&window_info, &output, capture_system_cursor)?;
            } else {
                anyhow::bail!("Must specify either --display or --window");
            }
        }
        Commands::Process {
            input,
            output,
            background,
            trim_start,
            trim_end,
            cursor_scale,
            cursor_timeout,
            no_cursor,
        } => {
            processor::process_video(
                &input,
                &output,
                background.as_deref(),
                trim_start,
                trim_end,
                cursor_scale,
                cursor_timeout,
                no_cursor,
            )?;
        }
    }

    Ok(())
}
