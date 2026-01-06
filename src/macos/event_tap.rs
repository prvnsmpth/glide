use anyhow::Result;
use core_foundation::runloop::{kCFRunLoopCommonModes, kCFRunLoopDefaultMode, CFRunLoop};
use core_graphics::event::{
    CGEvent, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
            start_time: Instant::now(), // Will be reset in start()
            stop_tx: None,
            thread_handle: None,
        }
    }

    pub fn start(&mut self) -> Result<()> {
        // Reset start time NOW (after FFmpeg has been spawned)
        self.start_time = Instant::now();

        let events = Arc::clone(&self.events);
        let start_time = self.start_time;
        let (stop_tx, stop_rx) = mpsc::channel();
        self.stop_tx = Some(stop_tx);

        let handle = thread::spawn(move || {
            run_event_tap(events, start_time, stop_rx);
        });

        self.thread_handle = Some(handle);
        Ok(())
    }

    /// Stop tracking and return (events, tracking_duration)
    pub fn stop(&mut self) -> (Vec<CursorEvent>, f64) {
        // Calculate duration before stopping
        let duration = self.start_time.elapsed().as_secs_f64();

        // Signal the event tap thread to stop
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }

        // Return collected events and duration
        let events = self.events.lock().unwrap();
        (events.clone(), duration)
    }
}

fn run_event_tap(events: Arc<Mutex<Vec<CursorEvent>>>, start_time: Instant, stop_rx: Receiver<()>) {
    // Event types to monitor
    let event_types = vec![
        CGEventType::MouseMoved,
        CGEventType::LeftMouseDown,
        CGEventType::RightMouseDown,
        CGEventType::LeftMouseDragged,
        CGEventType::RightMouseDragged,
    ];

    let events_clone = Arc::clone(&events);

    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        event_types,
        move |_proxy, event_type, event: &CGEvent| {
            let location = event.location();
            let timestamp = start_time.elapsed().as_secs_f64();

            let cursor_event_type = match event_type {
                CGEventType::MouseMoved
                | CGEventType::LeftMouseDragged
                | CGEventType::RightMouseDragged => EventType::Move,
                CGEventType::LeftMouseDown => EventType::LeftClick,
                CGEventType::RightMouseDown => EventType::RightClick,
                _ => return None,
            };

            let cursor_event = CursorEvent {
                x: location.x,
                y: location.y,
                timestamp,
                event_type: cursor_event_type,
            };

            if let Ok(mut events) = events_clone.lock() {
                events.push(cursor_event);
            }

            None // Don't modify the event
        },
    );

    let tap = match tap {
        Ok(t) => t,
        Err(()) => {
            eprintln!("Failed to create event tap. Make sure Accessibility permissions are granted.");
            return;
        }
    };

    // Add to run loop
    let source = tap
        .mach_port
        .create_runloop_source(0)
        .expect("Failed to create run loop source");

    let run_loop = CFRunLoop::get_current();
    run_loop.add_source(&source, unsafe { kCFRunLoopCommonModes });

    tap.enable();

    // Run until stop signal
    loop {
        // Process events for a short time (use DefaultMode, not CommonModes)
        CFRunLoop::run_in_mode(
            unsafe { kCFRunLoopDefaultMode },
            Duration::from_millis(100),
            false,
        );

        // Check for stop signal
        if stop_rx.try_recv().is_ok() {
            break;
        }
    }
}
