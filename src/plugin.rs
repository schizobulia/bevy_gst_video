use bevy::{
    prelude::*,
    render::{render_asset::RenderAssetUsages, render_resource::Extent3d},
};
use image::DynamicImage;
use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use crate::video::GstPlayer;

#[derive(Debug, Clone, Copy)]
pub enum VideoState {
    Init,
    Playing,
    Paused,
    Start,
    Ready,
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
    pub pipeline: Option<GstPlayer>,
}

pub struct VideoPlugin;

impl Plugin for VideoPlugin {
    fn build(&self, _: &mut App) {}
}

fn handle_playing_state(
    video_player: &mut VideoPlayer,
    image_handle: &mut UiImage,
    images: &mut Assets<Image>,
    time: &Res<Time>,
) {
    if let Ok(mut player_time) = video_player.timer.lock() {
        if player_time.tick(time.delta()).just_finished() {
            if let Some(ref_pipeline) = video_player.pipeline.as_ref() {
                if let Ok(mut frames) = ref_pipeline.frame.lock() {
                    if let Some(data) = frames.pop_front() {
                        if let Some(rbg_data) =
                            image::RgbaImage::from_raw(data.width, data.height, data.data)
                        {
                            let canvas = Image::from_dynamic(
                                DynamicImage::ImageRgba8(rbg_data),
                                true,
                                RenderAssetUsages::default(),
                            );
                            image_handle.texture = images.add(canvas);
                            if let Ok(mut pts) = ref_pipeline.previous_pts.lock() {
                                let dt = (data.pts - *pts) / 1_000_000;
                                player_time.set_duration(Duration::from_millis(dt));
                                *pts = data.pts;
                            }
                        }
                    }
                }
            }
        }
    }
}

fn initialize_video_player(video_player: &mut VideoPlayer) {
    let pipeline = GstPlayer::new(video_player.uri.as_str());
    let pipeline_clone = Arc::new(Mutex::new(pipeline.clone()));
    thread::spawn(move || {
        if let Ok(mut pipeline) = pipeline_clone.lock() {
            pipeline.start();
        }
    });
    video_player.pipeline = Some(pipeline);
}

pub fn render_video_frame(
    mut query: Query<(&mut VideoPlayer, &mut UiImage)>,
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
                    video_player.state = VideoState::Ready;
                    initialize_video_player(&mut video_player);
                }
            }
            VideoState::Start => {
                video_player.state = VideoState::Playing;
                if let Some(video_player) = video_player.pipeline.as_ref() {
                    video_player.play();
                }
            }
            VideoState::Paused => {
                if let Some(video_player) = video_player.pipeline.as_ref() {
                    video_player.pause();
                }
            }
            VideoState::Stop => {
                if let Some(video_player) = video_player.pipeline.as_ref() {
                    video_player.destroy();
                }
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
