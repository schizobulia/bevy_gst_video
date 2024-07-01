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
    Playing,
    Paused,
    Start,
    Ready,
    Init,
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
pub fn render_video_frame(
    mut query: Query<(&mut VideoPlayer, &mut UiImage)>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
) {
    for (mut video_player, mut image_handle) in query.iter_mut() {
        match video_player.state {
            VideoState::Playing => {
                let mut player_timer = video_player.timer.lock().unwrap();
                if player_timer.tick(time.delta()).just_finished() {
                    let ref_pipeline = video_player.pipeline.as_ref().unwrap();
                    let mut frames = ref_pipeline.frame.lock().unwrap();
                    if let Some(data) = frames.pop_front() {
                        let canvas = Image::from_dynamic(
                            DynamicImage::ImageRgba8(
                                image::RgbaImage::from_raw(data.width, data.height, data.data)
                                    .unwrap(),
                            ),
                            true,
                            RenderAssetUsages::default(),
                        );
                        image_handle.texture = images.add(canvas);
                        let p_pts = *ref_pipeline.previous_pts.lock().unwrap();
                        let dt = (data.pts - p_pts) / 1_000_000;
                        player_timer.set_duration(Duration::from_millis(dt));
                        *ref_pipeline.previous_pts.lock().unwrap() = data.pts;
                    }
                }
            }
            VideoState::Init => {
                if let Some(_) = video_player.id {
                    video_player.state = VideoState::Ready;
                    let pipeline = GstPlayer::new(video_player.uri.as_str());
                    let pipeline_clone = Arc::new(Mutex::new(pipeline.clone()));
                    thread::spawn(move || {
                        pipeline_clone.lock().unwrap().start();
                    });
                    video_player.pipeline = Some(pipeline);
                }
            }
            VideoState::Start => {
                video_player.state = VideoState::Playing;
                video_player.pipeline.as_ref().unwrap().play();
            }
            VideoState::Paused => {
                video_player.pipeline.as_ref().unwrap().pause();
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
