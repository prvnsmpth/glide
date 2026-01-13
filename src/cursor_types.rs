//! Shared cursor event types used across platforms

use serde::{Deserialize, Serialize};

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
