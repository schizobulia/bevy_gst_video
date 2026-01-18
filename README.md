# Bevy Video Player

A video player built with [Bevy](https://bevyengine.org/) game engine, using FFmpeg for video decoding and cpal for audio playback.

![Bevy](https://img.shields.io/badge/Bevy-0.18-232326?logo=bevy)
![Rust](https://img.shields.io/badge/Rust-1.75+-orange?logo=rust)
![License](https://img.shields.io/badge/License-MIT-blue)

## Features

- **Video Playback** - Supports multiple video formats (WebM, MP4, etc.)
- **Audio Sync** - Lock-free ring buffer for low-latency audio playback
- **UI Controls** - Play/Stop buttons, progress bar, status display
- **Network Streaming** - Play videos directly from URLs
- **High Performance** - Multi-threaded decoding with prebuffering

## Dependencies

| Dependency | Version | Purpose |
|------------|---------|---------|
| bevy | 0.18 | Game engine & UI framework |
| ffmpeg-next | 7.0 | Video/audio decoding |
| cpal | 0.15 | Cross-platform audio output |
| ringbuf | 0.4 | Lock-free ring buffer |
| image | 0.25 | Image processing |

## Prerequisites

### macOS
```bash
brew install ffmpeg
```

### Linux (Ubuntu/Debian)
```bash
sudo apt install libavcodec-dev libavformat-dev libavutil-dev libswscale-dev libswresample-dev
```

### Windows
Install FFmpeg and add it to your PATH.

## Quick Start

```bash
git clone https://github.com/yourusername/bevy_gst_video.git
cd bevy_gst_video
cargo run
```

## Usage

### Basic Example

```rust
use bevy::prelude::*;
use std::sync::{Arc, Mutex};

mod plugin;
mod video;

use plugin::{VideoPlayer, VideoState, VideoPlugin, insert_video_component};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, VideoPlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, plugin::render_video_frame)
        .run();
}

fn setup(mut commands: Commands, images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d);
    
    let video_player = VideoPlayer {
        uri: "https://example.com/video.mp4".to_string(),
        state: VideoState::Init,
        timer: Arc::new(Mutex::new(Timer::from_seconds(0.001, TimerMode::Repeating))),
        width: 720.0,
        height: 400.0,
        id: None,
        pipeline: None,
    };
    
    commands
        .spawn(insert_video_component(images, Vec2::new(720.0, 400.0)))
        .insert(video_player);
}
```

### Playback Control

```rust
// Start playback
video_player.state = VideoState::Start;

// Pause playback
video_player.state = VideoState::Paused;

// Get current position and duration
let position = video_player.position(); // in seconds
let duration = video_player.duration(); // in seconds
let progress = video_player.progress(); // 0.0 to 1.0
```

## Project Structure

```
bevy_gst_video/
├── Cargo.toml          # Project configuration
├── README.md           # Documentation
├── assets/
│   └── fonts/          # Font assets
└── src/
    ├── main.rs         # Entry point & UI
    ├── plugin.rs       # Bevy video plugin
    └── video.rs        # FFmpeg decoding & audio
```

## Architecture

### Video Pipeline

1. **Initialization** - Open video stream, get video/audio decoders
2. **Prebuffering** - Buffer 30 video frames and 0.5s of audio
3. **Decode Loop** - Continuous decoding in a separate thread
4. **Rendering** - Convert decoded frames to RGBA and update Bevy Image

### Audio Processing

- FFmpeg SwResampler converts audio to system sample rate
- Lock-free SPSC ring buffer for audio data transfer
- Supports F32 Planar to F32 Packed format conversion

## License

MIT License. See [LICENSE](LICENSE) for details.

## Acknowledgements

- [Bevy Engine](https://bevyengine.org/)
- [FFmpeg](https://ffmpeg.org/)
- [cpal](https://github.com/RustAudio/cpal)