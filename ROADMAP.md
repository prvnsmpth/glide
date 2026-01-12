# Glide Roadmap - Screen Studio Features

This document lists Screen Studio features that are not yet implemented in Glide, organized by priority.

## Phase 1: Quick Wins (Low Complexity, High Impact)

| Feature | Description | Status |
|---------|-------------|--------|
| Click Highlighting | Visual ripple/circle effect on clicks | Done |
| 4K Output | Configurable output resolution (3840x2160) | Planned |
| GIF Export | Optimized animated GIF format via FFmpeg | Planned |
| Aspect Ratio Presets | Vertical (9:16), Square (1:1) for social media | Planned |

## Phase 2: Core Enhancements

| Feature | Description | Status |
|---------|-------------|--------|
| Audio Recording | Capture system audio + microphone | Planned |
| Keyboard Shortcut Display | Show pressed keys during recording | Planned |
| Device Frame Mockups | Add iPhone/Mac frames around recordings | Planned |
| Speed Control | Variable playback speed (slow motion, speed up) | Planned |

## Phase 3: Advanced Features

| Feature | Description | Status |
|---------|-------------|--------|
| Region Recording | Record a selected portion of screen | Planned |
| Webcam Overlay | Picture-in-picture camera feed | Planned |
| Manual Zoom Regions | User-defined zoom points (not just auto) | Planned |
| Blur/Mask Sensitive Data | Privacy blur for passwords, API keys | Planned |

## Phase 4: Pro Features

| Feature | Description | Status |
|---------|-------------|--------|
| Auto Subtitles | Speech-to-text transcription (Whisper) | Planned |
| Audio Enhancement | Background noise reduction | Planned |
| Volume Normalization | Consistent audio levels | Planned |
| Timeline Editing | Cut, split, reorder video segments | Planned |
| Scene Transitions | Fade, slide between segments | Planned |
| iPhone/iPad Recording | Record connected iOS devices via USB | Planned |

## Implementation Notes

### 4K Output
- Change `OUTPUT_WIDTH` and `OUTPUT_HEIGHT` constants to be configurable
- Add `--resolution` CLI flag with presets (1080p, 1440p, 4K)

### GIF Export
- Use FFmpeg's `palettegen` and `paletteuse` filters for optimal quality
- Add `--format gif` option or detect from output extension

### Aspect Ratio Presets
- Add `--aspect-ratio` flag: `16:9` (default), `9:16`, `1:1`, `4:3`
- Recalculate output dimensions and content layout

### Audio Recording
- Use FFmpeg's AVFoundation input for audio capture
- Add `--audio` flag with options: `none`, `mic`, `system`, `both`
- Mux audio track with video during encoding

### Keyboard Shortcut Display
- Use CGEventTap to capture keyboard events (like cursor events)
- Render key labels as overlay during processing
- Add `--show-keys` flag
