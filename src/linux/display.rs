//! Linux X11 display enumeration using RandR extension

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::randr::{self, ConnectionExt as RandrExt};
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::rust_connection::RustConnection;

pub struct DisplayInfo {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub is_main: bool,
    pub scale_factor: f64,
    /// X11 display string (e.g., ":0")
    pub display_string: String,
}

pub fn list_displays() -> Result<Vec<DisplayInfo>> {
    let (conn, screen_num) = RustConnection::connect(None)
        .context("Failed to connect to X11 display")?;

    let setup = conn.setup();
    let screen = &setup.roots[screen_num];
    let root = screen.root;

    // Query RandR extension
    let resources = conn
        .randr_get_screen_resources(root)
        .context("Failed to query RandR screen resources")?
        .reply()
        .context("Failed to get RandR screen resources reply")?;

    let mut displays = Vec::new();
    let mut index = 0;

    // Iterate through CRTCs to find active monitors
    for crtc in &resources.crtcs {
        let crtc_info = conn
            .randr_get_crtc_info(*crtc, resources.config_timestamp)
            .context("Failed to query CRTC info")?
            .reply()
            .context("Failed to get CRTC info reply")?;

        // Skip disabled CRTCs (no outputs connected or zero size)
        if crtc_info.outputs.is_empty() || crtc_info.width == 0 || crtc_info.height == 0 {
            continue;
        }

        // Check if any output is connected
        let has_connected_output = crtc_info.outputs.iter().any(|output| {
            if let Ok(output_info) = conn
                .randr_get_output_info(*output, resources.config_timestamp)
                .and_then(|cookie| cookie.reply())
            {
                output_info.connection == randr::Connection::CONNECTED
            } else {
                false
            }
        });

        if !has_connected_output {
            continue;
        }

        // Get the display string from environment or default
        let display_string = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());

        // First display is considered main (in X11, we can also check for primary output)
        let is_main = index == 0;

        displays.push(DisplayInfo {
            index,
            width: crtc_info.width as u32,
            height: crtc_info.height as u32,
            x: crtc_info.x as i32,
            y: crtc_info.y as i32,
            is_main,
            scale_factor: 1.0, // X11 typically doesn't have HiDPI scaling at the display level
            display_string,
        });

        index += 1;
    }

    // If no CRTCs found, fall back to screen dimensions
    if displays.is_empty() {
        let display_string = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string());
        displays.push(DisplayInfo {
            index: 0,
            width: screen.width_in_pixels as u32,
            height: screen.height_in_pixels as u32,
            x: 0,
            y: 0,
            is_main: true,
            scale_factor: 1.0,
            display_string,
        });
    }

    Ok(displays)
}
