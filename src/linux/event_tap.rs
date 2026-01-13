//! Linux X11 cursor tracking using polling

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::rust_connection::RustConnection;

use crate::cursor_types::{CursorEvent, EventType};

pub struct CursorTracker {
    events: Arc<Mutex<Vec<CursorEvent>>>,
    start_time: Instant,
    stop_flag: Arc<AtomicBool>,
    thread_handle: Option<thread::JoinHandle<()>>,
}

impl CursorTracker {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            start_time: Instant::now(),
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread_handle: None,
        }
    }

    pub fn start(&mut self) -> Result<()> {
        self.start_time = Instant::now();
        self.stop_flag.store(false, Ordering::SeqCst);

        let events = Arc::clone(&self.events);
        let start_time = self.start_time;
        let stop_flag = Arc::clone(&self.stop_flag);

        let handle = thread::spawn(move || {
            run_polling_tracking(events, start_time, stop_flag);
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) -> (Vec<CursorEvent>, f64) {
        let duration = self.start_time.elapsed().as_secs_f64();

        // Signal the thread to stop
        self.stop_flag.store(true, Ordering::SeqCst);

        // Wait for thread with timeout
        if let Some(handle) = self.thread_handle.take() {
            // Give it 500ms to stop gracefully
            let start = Instant::now();
            while !handle.is_finished() && start.elapsed() < Duration::from_millis(500) {
                thread::sleep(Duration::from_millis(10));
            }
            if handle.is_finished() {
                let _ = handle.join();
            }
            // If not finished, just abandon the thread
        }

        let events = self.events.lock().unwrap();
        (events.clone(), duration)
    }
}

/// Poll cursor position using XQueryPointer
fn run_polling_tracking(
    events: Arc<Mutex<Vec<CursorEvent>>>,
    start_time: Instant,
    stop_flag: Arc<AtomicBool>,
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
        if stop_flag.load(Ordering::Relaxed) {
            break;
        }

        // Query pointer position and button state
        let Some(reply) = conn
            .query_pointer(root)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
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
