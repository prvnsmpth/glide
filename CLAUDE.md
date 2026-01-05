# Glide - Technical Specification

## Overview
CLI screen recorder for macOS with auto-zoom on clicks, smooth cursor movement, and motion blur effects.

**Architecture**: Two-pass system (record → process)

## Core Workflow
```bash
glide list --displays                    # List available displays
glide list --windows                     # List available windows
glide record --display 0 -o demo.mp4    # Record full display
glide record --window 1234 -o demo.mp4  # Record specific window
glide process demo.mp4 -o final.mp4     # Apply effects
glide process demo.mp4 -o final.mp4 --background "#1a1a2e"  # With solid color
glide process demo.mp4 -o final.mp4 --background gradient.png  # With image
```

## Technology Stack

**Language**: Rust

**Key Dependencies**:
- `clap` - CLI argument parsing
- `indicatif` - Terminal progress bars
- `serde/serde_json` - JSON serialization
- `rayon` - Parallel frame processing
- `cocoa`, `core-graphics`, `core-foundation` - macOS bindings

**External**: FFmpeg (required for screen capture and video encoding)

## System Architecture

### Recording Phase
1. **Screen/Window Capture**: Use FFmpeg with AVFoundation input to capture display or specific window at 60fps
2. **Cursor Tracking**: Use Core Graphics EventTap to monitor mouse position and clicks
3. **Metadata Storage**: Save cursor events (position, timestamp, type) to JSON file
4. **Stop Handling**: Gracefully terminate on Ctrl+C signal

### Processing Phase
1. **Frame Extraction**: Extract video frames to temporary directory
2. **Parallel Processing**: Process each frame based on cursor metadata:
   - Calculate zoom level from nearby click events
   - Apply zoom transformation (scale + crop to cursor position)
   - Draw enlarged cursor overlay
   - Apply motion blur (optional)
3. **Re-encoding**: Encode processed frames back to MP4
4. **Cleanup**: Remove temporary frame files

## Key Components

### 1. Screen Recorder
- Spawn FFmpeg subprocess with AVFoundation input
- Capture specified display or window at 60fps, 1080p
- Use CGWindowListCopyWindowInfo for window enumeration
- Monitor process and handle termination

### 2. Cursor Tracker
- Create CGEventTap for mouse events (moves, left/right clicks)
- Store events with coordinates and timestamps
- Serialize to JSON on stop

### 3. Video Processor
- Extract frames from input video
- Load cursor event metadata
- Process frames in parallel
- Re-encode with H.264

### 4. Zoom Controller
- Calculate zoom level based on time since last click
- Zoom timing: ease in (0.3s) → hold (2.5s) → ease out (0.3s)
- Use cubic ease-in-out interpolation for smooth animation
- Target zoom: 1.8x

### 5. Effects System
- **Zoom**: Scale and crop frame to cursor position
- **Cursor Enhancement**: Overlay enlarged cursor (2x size)
- **Motion Blur**: Apply frame blending or FFmpeg blur filter
- **Cursor Smoothing**: Moving average filter on cursor positions
- **Custom Background**: Solid color or image behind window content

### 6. Background System
- Render custom background behind recorded content
- Support solid colors (hex format: "#rrggbb")
- Support image files (PNG, JPG) - stretched or tiled
- Useful for window recordings to replace transparent/desktop background

## Data Structures

### Cursor Event
```
{
  x: f64              # X coordinate
  y: f64              # Y coordinate
  timestamp: f64      # Unix timestamp
  event_type: enum    # Move | LeftClick | RightClick
}
```

### Recording Metadata
```
{
  source_type: enum   # Display | Window
  source_id: u32      # Display or window ID
  window_bounds: Option<Rect>  # For window recordings
  cursor_events: Vec<CursorEvent>
}
```

## Technical Requirements

### Permissions
- Screen Recording permission (for Terminal or compiled binary)
- Accessibility permission (for cursor event tracking)

### Dependencies
- macOS 12.3+ (for modern AVFoundation APIs)
- FFmpeg installed via Homebrew
- Rust toolchain

### Performance Considerations
- Parallel frame processing using rayon thread pool
- FFmpeg hardware acceleration (VideoToolbox)
- Efficient buffer reuse
- Batch file I/O operations

## Critical Implementation Details

### macOS API Usage
- **CGEventTap**: System-wide event monitoring (requires unsafe Rust FFI)
- **AVFoundation**: Screen capture via FFmpeg subprocess
- **Core Graphics**: Display enumeration and cursor coordinates
- **CGWindowListCopyWindowInfo**: Window enumeration and metadata
- **ScreenCaptureKit** (via FFmpeg): Window-specific capture on macOS 12.3+

### Zoom Algorithm
- Find most recent click before current frame timestamp
- Calculate elapsed time since click
- Apply zoom based on timing curve:
  - t < 0.3s: Interpolate 1.0 → 1.8x (ease-in-out)
  - 0.3s ≤ t ≤ 2.5s: Hold at 1.8x
  - 2.5s < t ≤ 2.8s: Interpolate 1.8x → 1.0 (ease-in-out)
  - t > 2.8s: No zoom (1.0x)

### Frame Transformation
- Scale frame by zoom factor
- Translate so cursor position becomes center
- Crop to original resolution
- Overlay cursor graphic at 2x size

### Error Handling
- Check FFmpeg availability on startup
- Validate permissions before recording
- Handle Ctrl+C gracefully
- Clean up temp files on error

## Limitations (MVP Scope)

**Excluded**:
- GUI interface
- Real-time preview
- Audio recording
- Webcam overlay
- Advanced editing
- Multiple export formats
- Keyboard shortcut display

**Included**:
- CLI interface
- Display recording
- Window-specific recording
- Custom backgrounds (solid color or image)
- Auto-zoom on clicks
- Smooth cursor movement
- Motion blur
- Cursor enhancement
- MP4 export
- Progress indicators
