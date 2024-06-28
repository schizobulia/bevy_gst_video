use bevy::{
    prelude::*,
    render::{render_asset::RenderAssetUsages, render_resource::Extent3d},
};
use image::DynamicImage;
use std::{
    collections::VecDeque, sync::{Arc, Mutex}, thread, time::Duration
};

use crate::video::{self, VideoInfo};

pub struct RateId {
    pub rate: f64,
    pub id: Entity,
}

struct VideoTask {
    uri: String,
    frames: Arc<Mutex<VecDeque<VideoInfo>>>,
    rate: Arc<Mutex<Vec<RateId>>>,
    id: Entity
}

#[derive(Resource)]
pub struct VideoPlayerState {
    pub frames: Arc<Mutex<VecDeque<VideoInfo>>>,
    pub rate: Arc<Mutex<Vec<RateId>>>,
    pub tasks: Vec<VideoTask>,
}

pub enum VideoState {
    Playing,
    Paused,
    Stopped,
    Start,
    NONE,
}

pub struct FrameData {
    data: Vec<u8>,
    height: u32,
    width: u32,
}

#[derive(Component)]
pub struct VideoPlayer {
    pub state: VideoState,
    pub timer: Timer,
    pub id: Option<Entity>,
    pub data: VecDeque<FrameData>,
    pub width: f32,
    pub height: f32,
    pub uri: String,
}

pub struct VideoPlugin;

impl Plugin for VideoPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(VideoPlayerState {
            frames: Arc::new(Mutex::new(VecDeque::new())),
            rate: Arc::new(Mutex::new(Vec::new())),
            tasks: Vec::new(),
        });
    }
}

pub fn add_video_frame(mut query: Query<&mut VideoPlayer>, video_state: ResMut<VideoPlayerState>) {
    for mut video_player in query.iter_mut() {
        let mut frames = video_state.frames.lock().unwrap();
        if let Some(frame) = frames.pop_front() {
            if let Some(id) = video_player.id {
                if frame.id == id {
                    video_player.data.push_back(FrameData {
                        data: frame.data,
                        height: frame.height,
                        width: frame.width,
                    });
                    let res = video_state.rate.lock().unwrap();
                    let rate_item = res.iter().find(|rate| rate.id == id).unwrap();
                    if video_player.timer.duration().as_secs_f64() != rate_item.rate {
                        video_player
                            .timer
                            .set_duration(Duration::from_nanos(rate_item.rate as u64));
                    }
                }
            }
        }
    }
}

pub fn render_video_frame(
    mut query: Query<(&mut VideoPlayer, &mut Handle<Image>)>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
    mut video_state: ResMut<VideoPlayerState>,
) {
    let task_item = video_state.tasks.pop();
    if let Some(task) = task_item {
        thread::spawn(move || {
            video::create_pipeline(task.uri.as_str(), task.frames, task.id, task.rate);
        });
    }
    for (mut video_player, mut image_handle) in query.iter_mut() {
        match video_player.state {
            VideoState::Playing => {
                if video_player.timer.tick(time.delta()).just_finished() {
                    if let Some(data) = video_player.data.pop_front() {
                        let canvas = Image::from_dynamic(
                            DynamicImage::ImageRgba8(
                                image::RgbaImage::from_raw(data.width, data.height, data.data)
                                    .unwrap(),
                            ),
                            true,
                            RenderAssetUsages::default(),
                        );
                        let new_image_handle = images.add(canvas);
                        *image_handle = new_image_handle;
                    }
                }
            }
            VideoState::Start => {
                let frame_state_clone = Arc::clone(&video_state.frames);
                let rate_state_clone = Arc::clone(&video_state.rate);
                if let Some(id) = video_player.id {
                    video_state.tasks.push(VideoTask {
                        uri: video_player.uri.clone(),
                        frames: frame_state_clone,
                        rate: rate_state_clone,
                        id,
                    });
                }
            }
            _ => {}
        }
    }
}

pub fn insert_video_component(
    mut images: ResMut<Assets<Image>>,
    default_size: Vec2,
) -> SpriteBundle {
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
    SpriteBundle {
        texture: image_handle,
        sprite: Sprite {
            custom_size: Some(default_size),
            ..Default::default()
        },
        ..Default::default()
    }
}
