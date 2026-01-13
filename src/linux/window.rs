//! Linux X11 window enumeration using EWMH

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt, GetPropertyReply, Window,
};
use x11rb::rust_connection::RustConnection;

pub struct WindowInfo {
    pub id: u32,
    pub name: String,
    pub owner: String,
    pub bounds: (i32, i32, u32, u32), // x, y, width, height
}

/// Get or intern an atom
fn get_atom(conn: &RustConnection, name: &str) -> Result<Atom> {
    let reply = conn
        .intern_atom(false, name.as_bytes())
        .context("Failed to intern atom")?
        .reply()
        .context("Failed to get atom reply")?;
    Ok(reply.atom)
}

/// Get a property value as bytes
fn get_property_value(
    conn: &RustConnection,
    window: Window,
    property: Atom,
    prop_type: Atom,
) -> Result<Option<GetPropertyReply>> {
    let reply = conn
        .get_property(false, window, property, prop_type, 0, u32::MAX)
        .context("Failed to get property")?
        .reply()
        .context("Failed to get property reply")?;

    if reply.value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(reply))
    }
}

/// Get window name using _NET_WM_NAME or WM_NAME
fn get_window_name(conn: &RustConnection, window: Window) -> Result<String> {
    // Try _NET_WM_NAME first (UTF-8)
    let net_wm_name = get_atom(conn, "_NET_WM_NAME")?;
    let utf8_string = get_atom(conn, "UTF8_STRING")?;

    if let Ok(Some(reply)) = get_property_value(conn, window, net_wm_name, utf8_string) {
        if let Ok(name) = String::from_utf8(reply.value) {
            if !name.is_empty() {
                return Ok(name);
            }
        }
    }

    // Fall back to WM_NAME
    if let Ok(Some(reply)) = get_property_value(conn, window, AtomEnum::WM_NAME.into(), AtomEnum::STRING.into()) {
        if let Ok(name) = String::from_utf8(reply.value) {
            return Ok(name);
        }
    }

    Ok(String::new())
}

/// Get WM_CLASS (application name/class)
fn get_wm_class(conn: &RustConnection, window: Window) -> Result<String> {
    if let Ok(Some(reply)) = get_property_value(conn, window, AtomEnum::WM_CLASS.into(), AtomEnum::STRING.into()) {
        // WM_CLASS contains two null-terminated strings: instance name and class name
        // We want the class name (second one) as it's typically the application name
        let parts: Vec<&[u8]> = reply.value.split(|&b| b == 0).collect();
        if parts.len() >= 2 {
            if let Ok(class) = String::from_utf8(parts[1].to_vec()) {
                if !class.is_empty() {
                    return Ok(class);
                }
            }
        }
        // Fall back to instance name
        if !parts.is_empty() {
            if let Ok(instance) = String::from_utf8(parts[0].to_vec()) {
                return Ok(instance);
            }
        }
    }

    Ok(String::new())
}

/// Get window geometry including border width
fn get_window_geometry(
    conn: &RustConnection,
    window: Window,
    root: Window,
) -> Result<(i32, i32, u32, u32)> {
    // Get window geometry
    let geom = conn
        .get_geometry(window)
        .context("Failed to get window geometry")?
        .reply()
        .context("Failed to get geometry reply")?;

    // Translate coordinates to root window (absolute screen coordinates)
    let coords = conn
        .translate_coordinates(window, root, 0, 0)
        .context("Failed to translate coordinates")?
        .reply()
        .context("Failed to get coordinates reply")?;

    Ok((
        coords.dst_x as i32,
        coords.dst_y as i32,
        geom.width as u32,
        geom.height as u32,
    ))
}

pub fn list_windows() -> Result<Vec<WindowInfo>> {
    let (conn, screen_num) = RustConnection::connect(None)
        .context("Failed to connect to X11 display")?;

    let setup = conn.setup();
    let screen = &setup.roots[screen_num];
    let root = screen.root;

    // Get _NET_CLIENT_LIST atom (EWMH)
    let net_client_list = get_atom(&conn, "_NET_CLIENT_LIST")?;

    // Get the list of managed windows
    let reply = get_property_value(&conn, root, net_client_list, AtomEnum::WINDOW.into())?;

    let windows = match reply {
        Some(reply) => {
            // Convert bytes to window IDs (u32)
            reply.value32().map(|iter| iter.collect::<Vec<_>>()).unwrap_or_default()
        }
        None => Vec::new(),
    };

    let mut result = Vec::new();

    for window_id in windows {
        // Get window name
        let name = get_window_name(&conn, window_id).unwrap_or_default();

        // Get application name from WM_CLASS
        let owner = get_wm_class(&conn, window_id).unwrap_or_default();

        // Get window bounds
        let bounds = match get_window_geometry(&conn, window_id, root) {
            Ok(b) => b,
            Err(_) => continue, // Skip windows we can't get geometry for
        };

        // Filter out small windows (like toolbars, etc.)
        if bounds.2 > 100 && bounds.3 > 100 && !name.is_empty() {
            result.push(WindowInfo {
                id: window_id,
                name,
                owner,
                bounds,
            });
        }
    }

    Ok(result)
}
