use bevy::{
    prelude::*,
    asset::RenderAssetUsages,
    render::render_resource::Extent3d,
};
use image::DynamicImage;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use crate::video::FfmpegPlayer;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoState {
    Init,
    Playing,
    Paused,
    Start,
    Ready,
    Loading,
    #[allow(dead_code)]
    Stop,
}

#[derive(Component, Clone)]
pub struct VideoPlayer {
    pub state: VideoState,
    pub timer: Arc<Mutex<Timer>>,
    pub id: Option<Entity>,
    pub width: f32,
    pub height: f32,
    pub uri: String,
    pub pipeline: Option<FfmpegPlayer>,
}

impl VideoPlayer {
    /// Get current playback position in seconds
    pub fn position(&self) -> f64 {
        self.pipeline
            .as_ref()
            .and_then(|p| p.current_position.lock().ok())
            .map(|p| *p)
            .unwrap_or(0.0)
    }

    /// Get total video duration in seconds
    pub fn duration(&self) -> f64 {
        self.pipeline
            .as_ref()
            .and_then(|p| p.duration.lock().ok())
            .map(|d| *d)
            .unwrap_or(0.0)
    }

    /// Get playback progress as a ratio (0.0 to 1.0)
    pub fn progress(&self) -> f32 {
        let duration = self.duration();
        if duration > 0.0 {
            (self.position() / duration) as f32
        } else {
            0.0
        }
    }
}

pub struct VideoPlugin;

impl Plugin for VideoPlugin {
    fn build(&self, _: &mut App) {}
}

fn handle_playing_state(
    video_player: &mut VideoPlayer,
    image_handle: &mut ImageNode,
    images: &mut Assets<Image>,
    time: &Res<Time>,
) {
    if let Ok(mut player_time) = video_player.timer.lock() {
        if player_time.tick(time.delta()).just_finished() {
            if let Some(ref_pipeline) = video_player.pipeline.as_ref() {
                if let Ok(mut frames) = ref_pipeline.frame.lock() {
                    if let Some(data) = frames.pop_front() {
                        // Update current position based on the frame being rendered
                        if let Ok(mut pos) = ref_pipeline.current_position.lock() {
                            *pos = data.position_secs;
                        }

                        if let Some(rbg_data) =
                            image::RgbaImage::from_raw(data.width, data.height, data.data)
                        {
                            let canvas = Image::from_dynamic(
                                DynamicImage::ImageRgba8(rbg_data),
                                true,
                                RenderAssetUsages::default(),
                            );
                            image_handle.image = images.add(canvas);
                            if let Ok(mut pts) = ref_pipeline.previous_pts.lock() {
                                // Handle first frame: initialize previous_pts
                                if *pts == 0 {
                                    *pts = data.pts;
                                    player_time.set_duration(Duration::from_millis(33)); // ~30fps default
                                } else if data.pts > *pts {
                                    let dt = (data.pts - *pts) / 1_000_000;
                                    // Clamp dt to reasonable range (1ms - 100ms)
                                    let dt = dt.max(1).min(100);
                                    player_time.set_duration(Duration::from_millis(dt));
                                    *pts = data.pts;
                                } else {
                                    *pts = data.pts;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn initialize_video_player(video_player: &mut VideoPlayer) {
    let pipeline = FfmpegPlayer::new(video_player.uri.as_str());
    let pipeline_clone = Arc::new(Mutex::new(pipeline.clone()));
    thread::spawn(move || {
        if let Ok(mut pipeline) = pipeline_clone.lock() {
            pipeline.start();
        }
    });
    video_player.pipeline = Some(pipeline);
}

pub fn render_video_frame(
    mut query: Query<(&mut VideoPlayer, &mut ImageNode)>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
) {
    for (mut video_player, mut image_handle) in query.iter_mut() {
        match video_player.state {
            VideoState::Playing => {
                handle_playing_state(&mut video_player, &mut image_handle, &mut images, &time)
            }
            VideoState::Init => {
                if video_player.id.is_some() {
                    // println!("[DEBUG] State: Init -> Ready, initializing video player");
                    video_player.state = VideoState::Ready;
                    initialize_video_player(&mut video_player);
                }
            }
            VideoState::Start => {
                // println!("[DEBUG] State: Start, checking if ready...");
                // Initialize pipeline if not already done (handles Init -> Start skip)
                if video_player.pipeline.is_none() {
                    // println!("[DEBUG] Pipeline was None, initializing...");
                    initialize_video_player(&mut video_player);
                }
                // Check if video is ready
                let is_ready = video_player
                    .pipeline
                    .as_ref()
                    .map(|p| p.is_ready.load(std::sync::atomic::Ordering::Relaxed))
                    .unwrap_or(false);

                if is_ready {
                    // println!("[DEBUG] Video is ready, starting playback");
                    video_player.state = VideoState::Playing;
                    if let Some(ref pipeline) = video_player.pipeline {
                        pipeline.play();
                    }
                } else {
                    // println!("[DEBUG] Video not ready yet, switching to Loading state");
                    video_player.state = VideoState::Loading;
                }
            }
            VideoState::Loading => {
                // Wait for video to be ready
                let is_ready = video_player
                    .pipeline
                    .as_ref()
                    .map(|p| p.is_ready.load(std::sync::atomic::Ordering::Relaxed))
                    .unwrap_or(false);

                if is_ready {
                    // println!("[DEBUG] Video is now ready, starting playback");
                    video_player.state = VideoState::Playing;
                    if let Some(ref pipeline) = video_player.pipeline {
                        pipeline.play();
                    }
                }
            }
            VideoState::Paused => {
                if let Some(ref pipeline) = video_player.pipeline {
                    pipeline.pause();
                }
            }
            VideoState::Stop => {
                if let Some(ref pipeline) = video_player.pipeline {
                    pipeline.destroy();
                }
            }
            _ => {}
        }
    }
}

pub fn insert_video_component(
    mut images: ResMut<Assets<Image>>,
    default_size: Vec2,
) -> impl Bundle {
    let mut canvas = Image::from_dynamic(
        DynamicImage::new_rgb8(500, 500),
        true,
        RenderAssetUsages::default(),
    );
    canvas.resize(Extent3d {
        width: default_size.x as u32,
        height: default_size.y as u32,
        ..default()
    });
    let image_handle = images.add(canvas);
    (
        ImageNode::new(image_handle),
        Node {
            width: Val::Px(default_size.x),
            height: Val::Px(default_size.y),
            ..Default::default()
        },
    )
}
