use bevy::{
    prelude::*,
    render::{render_asset::RenderAssetUsages, render_resource::Extent3d},
};
use image::DynamicImage;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use crate::video::{GstPlayer, VideoInfo};

pub struct RateId {
    pub rate: f64,
    pub id: Entity,
}

#[derive(Resource)]
pub struct VideoPlayerState {
    pub frames: Arc<Mutex<VecDeque<VideoInfo>>>,
    pub rate: Arc<Mutex<Vec<RateId>>>,
}

#[derive(Debug, Clone, Copy)]
pub enum VideoState {
    Playing,
    Paused,
    Start,
    Ready,
    Init,
}

#[derive(Debug, Clone)]
pub struct FrameData {
    data: Vec<u8>,
    height: u32,
    width: u32,
}

#[derive(Component, Clone)]
pub struct VideoPlayer {
    pub state: VideoState,
    pub timer: Timer,
    pub id: Option<Entity>,
    pub data: VecDeque<FrameData>,
    pub width: f32,
    pub height: f32,
    pub uri: String,
    pub pipeline: Option<GstPlayer>,
}

pub struct VideoPlugin;

impl Plugin for VideoPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(VideoPlayerState {
            frames: Arc::new(Mutex::new(VecDeque::new())),
            rate: Arc::new(Mutex::new(Vec::new())),
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
    mut query: Query<(&mut VideoPlayer, &mut UiImage)>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
    video_state: ResMut<VideoPlayerState>,
) {
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
                        image_handle.texture = images.add(canvas);
                    }
                }
            }
            VideoState::Init => {
                let frame_state_clone = Arc::clone(&video_state.frames);
                let rate_state_clone = Arc::clone(&video_state.rate);
                if let Some(id) = video_player.id {
                    video_player.state = VideoState::Ready;
                    let pipeline = GstPlayer::new(video_player.uri.as_str());
                    let pipeline_clone = Arc::new(Mutex::new(pipeline.clone()));
                    thread::spawn(move || {
                        pipeline_clone.lock().unwrap().start(
                            frame_state_clone,
                            id,
                            rate_state_clone,
                        );
                    });
                    video_player.pipeline = Some(pipeline);
                }
            }
            VideoState::Start => {
                video_player.state = VideoState::Playing;
                video_player.pipeline.clone().unwrap().play();
            }
            VideoState::Paused => {
                video_player.pipeline.clone().unwrap().pause();
            }
            _ => {}
        }
    }
}

pub fn insert_video_component(
    mut images: ResMut<Assets<Image>>,
    default_size: Vec2,
) -> ImageBundle {
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
    ImageBundle {
        image: UiImage {
            texture: image_handle,
            ..Default::default()
        },
        style: Style {
            width: Val::Px(default_size.x),
            height: Val::Px(default_size.y),
            ..Default::default()
        },
        ..Default::default()
    }
}
