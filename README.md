# Glide

A CLI screen recorder for macOS and Linux with automatic zoom effects on clicks, smooth cursor tracking, and professional-looking output.

## Features

- **Display & Window Recording** - Record full displays or specific windows at 60fps
- **Auto-Zoom on Clicks** - Automatically zooms in when you click, with smooth anticipatory animations
- **Smart Panning** - Camera smoothly pans between click locations while staying zoomed
- **Custom Backgrounds** - Add solid colors or images behind your recordings
- **Rounded Corners & Shadows** - Professional styling with anti-aliased corners and drop shadows
- **Motion Blur** - Cinematic blur during zoom and pan transitions
- **Click Highlights** - Animated expanding rings on clicks for visual emphasis
- **Custom Cursor** - Enlarged cursor with shadow, configurable size and fade timeout
- **Trimming** - Remove unwanted start/end sections during processing
- **Hardware Acceleration** - GPU encoding via VideoToolbox (macOS), NVENC, or VAAPI (Linux)

## Requirements

### macOS
- macOS 12.3 or later
- FFmpeg (install via `brew install ffmpeg`)
- Rust toolchain (for building from source)

**Permissions required:**
- **Screen Recording** - To capture display/window content
- **Accessibility** - To track cursor position and clicks

Grant these in **System Preferences > Privacy & Security**.

### Linux
- X11 display server
- FFmpeg (install via your package manager)
- Rust toolchain (for building from source)

**GPU encoding support:**
- NVIDIA GPUs: NVENC (requires nvidia drivers)
- AMD/Intel GPUs: VAAPI
- Falls back to software encoding (libx264) if unavailable

## Installation

```bash
# Clone the repository
git clone https://github.com/your-username/glide.git
cd glide

# Build in release mode
cargo build --release

# The binary will be at ./target/release/glide
```

## Usage

### List Available Displays & Windows

```bash
# List displays with indices
glide list displays

# List windows with IDs
glide list windows
```

### Record

```bash
# Record a display (use index from 'list displays')
glide record --display 0 -o recording.mp4

# Record a specific window (use ID from 'list windows')
glide record --window 1234 -o recording.mp4
```

Press `Ctrl+C` to stop recording.

### Process

Apply zoom effects and styling to your recording:

```bash
# Basic processing with default dark background
glide process recording.mp4 -o final.mp4

# With custom solid color background
glide process recording.mp4 -o final.mp4 --background "#1a1a2e"

# With image background
glide process recording.mp4 -o final.mp4 --background wallpaper.png

# Trim the video (remove first 2s and last 1s)
glide process recording.mp4 -o final.mp4 --trim-start 2.0 --trim-end 1.0
```

## How It Works

Glide uses a two-pass system:

### 1. Recording Phase
- Captures screen/window content at 60fps using FFmpeg (AVFoundation on macOS, x11grab on Linux)
- Simultaneously tracks cursor position and click events (CGEventTap on macOS, X11 polling on Linux)
- Saves cursor metadata to a JSON file alongside the video

### 2. Processing Phase
- Extracts video frames to a temporary directory
- Processes frames in parallel using rayon
- Applies zoom effects based on recorded click events:
  - **Anticipatory zoom** starts 0.6s before each click
  - **Hold** at 1.8x zoom for 4 seconds
  - **Smooth ease-out** over 0.8 seconds
- Adds rounded corners, drop shadows, and custom backgrounds
- Re-encodes to MP4 with hardware acceleration

## Command Reference

### `glide list`

| Option | Description |
|--------|-------------|
| `displays` | List available displays with indices and dimensions |
| `windows` | List available windows with IDs and bounds |

### `glide record`

| Option | Description |
|--------|-------------|
| `--display <N>` | Record display by index |
| `--window <ID>` | Record window by ID |
| `-o, --output <PATH>` | Output file path (required) |
| `--capture-system-cursor` | Capture system cursor in video (default: off) |

### `glide process`

| Option | Description |
|--------|-------------|
| `<input>` | Input video file |
| `-o, --output <PATH>` | Output file path (required) |
| `--background <VALUE>` | Hex color (`#RRGGBB`) or image path |
| `--trim-start <SECS>` | Seconds to trim from start |
| `--trim-end <SECS>` | Seconds to trim from end |
| `--cursor-scale <N>` | Cursor size multiplier (default: 2.0) |
| `--cursor-timeout <SECS>` | Seconds before cursor fades (default: 2.0) |
| `--no-cursor` | Disable custom cursor rendering |
| `--no-motion-blur` | Disable motion blur during zoom/pan |
| `--no-click-highlight` | Disable click highlight effect (expanding rings) |

## Examples

### Record a Presentation

```bash
# Find your presentation window
glide list windows
# Output: [12345] Keynote - My Presentation (1920x1080)

# Record it
glide record --window 12345 -o presentation.mp4

# Process with a gradient background
glide process presentation.mp4 -o final.mp4 --background gradient.png
```

### Record a Tutorial

```bash
# Record your main display
glide record --display 0 -o tutorial-raw.mp4

# Process with trimming and custom background
glide process tutorial-raw.mp4 -o tutorial.mp4 \
  --background "#0d1117" \
  --trim-start 3.0 \
  --trim-end 2.0
```

## Technical Details

- **Output Resolution**: 1920x1080
- **Frame Rate**: 60fps
- **Codec**: H.264 (VideoToolbox on macOS, NVENC/VAAPI on Linux, libx264 fallback)
- **Zoom Level**: 1.8x on clicks
- **Corner Radius**: 12px with anti-aliasing
- **Window Shadow**: 8px offset, 20px blur
- **Cursor**: 2x scale with drop shadow, Lanczos3 interpolation
- **Motion Blur**: Radial blur on zoom, directional blur on pan
- **Click Highlights**: Animated expanding rings with fade effect
- **Scaling**: Lanczos3 filter for sharp results at all zoom levels

## License

DO WHATEVER YOU WANT PUBLIC LICENSE - See [LICENSE](LICENSE) for details.
