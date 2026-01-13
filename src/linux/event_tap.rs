//! Linux X11 cursor tracking using RECORD extension or polling fallback

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ButtonPressEvent, ConnectionExt, MotionNotifyEvent, Window};
use x11rb::protocol::record::{self, ConnectionExt as RecordExt, Range8, Range16, ExtRange, CS, Context};
use x11rb::rust_connection::RustConnection;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    Move,
    LeftClick,
    RightClick,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorEvent {
    pub x: f64,
    pub y: f64,
    pub timestamp: f64,
    pub event_type: EventType,
}

pub struct CursorTracker {
    events: Arc<Mutex<Vec<CursorEvent>>>,
    start_time: Instant,
    stop_tx: Option<Sender<()>>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl CursorTracker {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            start_time: Instant::now(),
            stop_tx: None,
            thread_handle: None,
        }
    }

    pub fn start(&mut self) -> Result<()> {
        self.start_time = Instant::now();

        let events = Arc::clone(&self.events);
        let start_time = self.start_time;
        let (stop_tx, stop_rx) = mpsc::channel();
        self.stop_tx = Some(stop_tx);

        let handle = thread::spawn(move || {
            // Try RECORD extension first, fall back to polling
            if let Err(e) = run_record_tracking(events.clone(), start_time, &stop_rx) {
                eprintln!("RECORD extension failed ({}), falling back to polling", e);
                run_polling_tracking(events, start_time, stop_rx);
            }
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) -> (Vec<CursorEvent>, f64) {
        let duration = self.start_time.elapsed().as_secs_f64();

        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }

        let events = self.events.lock().unwrap();
        (events.clone(), duration)
    }
}

/// Try to use RECORD extension for efficient event tracking
fn run_record_tracking(
    events: Arc<Mutex<Vec<CursorEvent>>>,
    start_time: Instant,
    stop_rx: &Receiver<()>,
) -> Result<()> {
    // We need two connections for RECORD: one for control, one for data
    let (ctrl_conn, _) = RustConnection::connect(None)?;
    let (data_conn, screen_num) = RustConnection::connect(None)?;

    // Query RECORD extension
    let _record_query = ctrl_conn.record_query_version(1, 13)?.reply()?;

    // Create record context to capture pointer events
    let ctx = ctrl_conn.generate_id()?;

    // Set up range to capture core pointer events
    let range = ExtRange {
        major: Range8 { first: 0, last: 0 },
        minor: Range16 { first: 0, last: 0 },
    };

    let client_spec = CS::ALL_CLIENTS;

    // Create the record context
    ctrl_conn.record_create_context(
        ctx,
        0,
        &[client_spec],
        &[record::Range {
            core_requests: Range8 { first: 0, last: 0 },
            core_replies: Range8 { first: 0, last: 0 },
            ext_requests: range,
            ext_replies: range,
            delivered_events: Range8 { first: 0, last: 0 },
            device_events: Range8 { first: 6, last: 6 }, // MotionNotify = 6
            errors: Range8 { first: 0, last: 0 },
            client_started: false,
            client_died: false,
        }, record::Range {
            core_requests: Range8 { first: 0, last: 0 },
            core_replies: Range8 { first: 0, last: 0 },
            ext_requests: range,
            ext_replies: range,
            delivered_events: Range8 { first: 0, last: 0 },
            device_events: Range8 { first: 4, last: 5 }, // ButtonPress = 4, ButtonRelease = 5
            errors: Range8 { first: 0, last: 0 },
            client_started: false,
            client_died: false,
        }],
    )?.check()?;

    // Enable the context (this blocks until disabled)
    // We need to run this in a separate thread because it blocks
    let events_clone = Arc::clone(&events);
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = Arc::clone(&running);

    let record_thread = thread::spawn(move || {
        // Use data_conn to receive events
        if let Ok(()) = data_conn.record_enable_context(ctx).map(|_| ()) {
            // Process events until stopped
            while running_clone.load(Ordering::Relaxed) {
                // Note: This simplified implementation doesn't fully parse RECORD data
                // In practice, you'd need to properly parse the intercepted data
                thread::sleep(Duration::from_millis(10));
            }
        }
    });

    // Wait for stop signal
    while stop_rx.try_recv().is_err() {
        thread::sleep(Duration::from_millis(50));
    }

    running.store(false, Ordering::SeqCst);

    // Disable and free the context
    let _ = ctrl_conn.record_disable_context(ctx);
    let _ = ctrl_conn.record_free_context(ctx);

    let _ = record_thread.join();

    // If we got here but have no events, the RECORD approach didn't work well
    // Return an error to trigger fallback
    let event_count = events_clone.lock().unwrap().len();
    if event_count == 0 {
        anyhow::bail!("RECORD extension captured no events");
    }

    Ok(())
}

/// Fallback: poll cursor position using XQueryPointer
fn run_polling_tracking(
    events: Arc<Mutex<Vec<CursorEvent>>>,
    start_time: Instant,
    stop_rx: Receiver<()>,
) {
    let Ok((conn, screen_num)) = RustConnection::connect(None) else {
        eprintln!("Failed to connect to X11 display for cursor tracking");
        return;
    };

    let setup = conn.setup();
    let screen = &setup.roots[screen_num];
    let root = screen.root;

    let mut last_x: i16 = 0;
    let mut last_y: i16 = 0;
    let mut last_buttons: u16 = 0;

    // Poll at ~120Hz
    let poll_interval = Duration::from_micros(8333);

    loop {
        // Check for stop signal
        if stop_rx.try_recv().is_ok() {
            break;
        }

        // Query pointer position and button state
        let Ok(reply) = conn.query_pointer(root).and_then(|cookie| cookie.reply()) else {
            thread::sleep(poll_interval);
            continue;
        };

        let x = reply.root_x;
        let y = reply.root_y;
        let buttons = reply.mask.bits();

        let timestamp = start_time.elapsed().as_secs_f64();

        // Check for button state changes (clicks)
        let button1_now = (buttons & 0x100) != 0; // Button 1 (left)
        let button3_now = (buttons & 0x400) != 0; // Button 3 (right)
        let button1_was = (last_buttons & 0x100) != 0;
        let button3_was = (last_buttons & 0x400) != 0;

        if let Ok(mut events) = events.lock() {
            // Left click (button pressed)
            if button1_now && !button1_was {
                events.push(CursorEvent {
                    x: x as f64,
                    y: y as f64,
                    timestamp,
                    event_type: EventType::LeftClick,
                });
            }

            // Right click (button pressed)
            if button3_now && !button3_was {
                events.push(CursorEvent {
                    x: x as f64,
                    y: y as f64,
                    timestamp,
                    event_type: EventType::RightClick,
                });
            }

            // Movement (only record if position changed significantly)
            if (x != last_x || y != last_y) && (x - last_x).abs() + (y - last_y).abs() > 2 {
                events.push(CursorEvent {
                    x: x as f64,
                    y: y as f64,
                    timestamp,
                    event_type: EventType::Move,
                });
            }
        }

        last_x = x;
        last_y = y;
        last_buttons = buttons;

        thread::sleep(poll_interval);
    }
}
