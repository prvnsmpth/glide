# Linux X11 Support for Glide

## Overview
Add Linux X11 support to Glide, enabling the same functionality as macOS: display/window enumeration, cursor tracking, and screen capture at 60fps.

**Target:** Ubuntu 24.04 LTS, X11, i3 window manager
**Approach:** Parallel `src/linux/` module with conditional compilation

---

## Architecture Changes

### Current Structure
```
src/
  macos/
    display.rs      # Core Graphics display enumeration
    window.rs       # CGWindowListCopyWindowInfo
    event_tap.rs    # CGEventTap cursor tracking
  recording/
    capture.rs      # ScreenCaptureKit (macOS-specific!)
    recorder.rs     # Orchestration
    encoder.rs      # FFmpeg encoding (platform-agnostic)
    metadata.rs     # JSON serialization (platform-agnostic)
  processing/       # All platform-agnostic
```

### Target Structure
```
src/
  macos/
    display.rs
    window.rs
    event_tap.rs
    capture.rs      # MOVED from recording/capture.rs
  linux/
    display.rs      # RandR display enumeration
    window.rs       # EWMH window enumeration
    event_tap.rs    # XRecord/polling cursor tracking
    capture.rs      # FFmpeg x11grab capture
  recording/
    recorder.rs     # Orchestration (with cfg imports)
    encoder.rs      # Unchanged
    metadata.rs     # Unchanged
  processing/       # Unchanged
```

---

## Implementation Steps

### Phase 1: Reorganize macOS Code

1. **Move `src/recording/capture.rs` to `src/macos/capture.rs`**
2. **Update `src/macos/mod.rs`** to export capture module
3. **Update `src/recording/recorder.rs`** to import from `crate::macos::capture`
4. **Update `src/recording/mod.rs`** to remove capture export

### Phase 2: Update Cargo.toml

Add target-specific dependencies:

```toml
# macOS-specific
[target.'cfg(target_os = "macos")'.dependencies]
core-graphics = "0.24"
core-foundation = "0.10"
screencapturekit = "1.5"

# Linux-specific
[target.'cfg(target_os = "linux")'.dependencies]
x11rb = { version = "0.13", features = ["record", "randr", "xfixes"] }
nix = { version = "0.29", features = ["signal"] }
```

### Phase 3: Create Linux Module

#### `src/linux/mod.rs`
```rust
pub mod display;
pub mod window;
pub mod event_tap;
pub mod capture;

pub use display::{list_displays, DisplayInfo};
pub use window::{list_windows, WindowInfo};
pub use event_tap::{CursorTracker, CursorEvent, EventType};
pub use capture::{start_display_capture, start_window_capture, CaptureSession, CaptureConfig, CapturedFrame};
```

#### `src/linux/display.rs`
- Use RandR extension via `x11rb`
- Query CRTCs for active monitors
- Return `DisplayInfo` with index, dimensions, position, scale_factor (1.0)

#### `src/linux/window.rs`
- Query `_NET_CLIENT_LIST` atom for managed windows
- Get `_NET_WM_NAME` or `WM_NAME` for window title
- Get `WM_CLASS` for owner/application name
- Use `XTranslateCoordinates` for absolute bounds

#### `src/linux/event_tap.rs`
- **Primary:** X11 RECORD extension for passive event monitoring
- **Fallback:** XQueryPointer polling at ~120Hz
- Same API as macOS: `CursorTracker::new()`, `start()`, `stop() -> (Vec<CursorEvent>, f64)`

#### `src/linux/capture.rs`
- Use FFmpeg with `x11grab` input device
- Display capture: `-f x11grab -i :0+X,Y -video_size WxH`
- Window capture: `-window_id 0xXXXX` option
- Output raw BGRA frames to stdout, same as macOS capture interface

### Phase 4: Add Conditional Compilation

#### `src/main.rs`
```rust
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "macos")]
use macos::{list_displays, list_windows};
#[cfg(target_os = "linux")]
use linux::{list_displays, list_windows};
```

#### `src/recording/recorder.rs`
```rust
#[cfg(target_os = "macos")]
use crate::macos::{capture, CursorTracker, DisplayInfo, WindowInfo};
#[cfg(target_os = "linux")]
use crate::linux::{capture, CursorTracker, DisplayInfo, WindowInfo};
```

### Phase 5: Update build.rs

```rust
fn main() {
    #[cfg(target_os = "macos")]
    {
        // Existing Swift runtime rpath configuration
    }

    #[cfg(target_os = "linux")]
    {
        // Link XCB libraries if needed
    }
}
```

---

## Key X11 APIs

| Component | X11 API | Crate |
|-----------|---------|-------|
| Display enumeration | RandR `GetScreenResources`, `GetCrtcInfo` | x11rb |
| Window enumeration | `_NET_CLIENT_LIST`, `GetProperty` | x11rb |
| Cursor tracking | RECORD extension or `QueryPointer` | x11rb |
| Screen capture | FFmpeg x11grab | subprocess |

---

## Files to Create

| File | Purpose |
|------|---------|
| `src/linux/mod.rs` | Module exports |
| `src/linux/display.rs` | RandR display enumeration |
| `src/linux/window.rs` | EWMH window enumeration |
| `src/linux/event_tap.rs` | X11 cursor tracking |
| `src/linux/capture.rs` | FFmpeg x11grab capture |

## Files to Modify

| File | Changes |
|------|---------|
| `Cargo.toml` | Add conditional dependencies |
| `src/main.rs` | Add cfg platform imports |
| `src/macos/mod.rs` | Export capture module |
| `src/recording/mod.rs` | Remove capture export |
| `src/recording/recorder.rs` | Use cfg imports for platform modules |
| `build.rs` | Add Linux build config |

## Files to Move

| From | To |
|------|-----|
| `src/recording/capture.rs` | `src/macos/capture.rs` |

---

## Linux Build Dependencies

Users need to install:
```bash
sudo apt-get install -y libxcb1-dev libxcb-record0-dev libxcb-randr0-dev ffmpeg
```

---

## Gotchas

1. **RECORD extension** - May not be available on all X servers; implement XQueryPointer fallback
2. **Window capture** - FFmpeg's `-window_id` may not work with all WMs; fallback is full-screen + crop
3. **HiDPI** - X11 scale_factor is typically 1.0; coordinates are already in pixels
4. **i3 compatibility** - i3 supports EWMH well, `_NET_CLIENT_LIST` should work

---

## Verification

1. Build on Linux: `cargo build --release`
2. Test display listing: `glide list --displays`
3. Test window listing: `glide list --windows`
4. Test display recording: `glide record --display 0 -o test.mp4` (Ctrl+C to stop)
5. Test window recording: `glide record --window <id> -o test.mp4`
6. Test processing: `glide process test.mp4 -o final.mp4`
7. Verify cursor tracking works (zoom on clicks in processed video)
