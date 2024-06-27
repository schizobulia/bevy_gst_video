use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{
            Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
        },
    },
};
use image::DynamicImage;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread::{self},
    time::Duration,
};
use video::VideoInfo;
mod video;

#[derive(Resource)]
pub struct UiState {
    pub frames: Arc<Mutex<VecDeque<VideoInfo>>>,
    pub rate: Arc<Mutex<f64>>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            frames: Arc::new(Mutex::new(VecDeque::new())),
            rate: Arc::new(Mutex::new(0.0)),
        }
    }
}

fn main() {
    App::new()
        .init_resource::<UiState>()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, start_up)
        .add_systems(Update, update)
        .run();
}

#[derive(Component)]
struct VideoPlayer {
    state: u8,
    timer: Timer,
}

fn start_up(mut commands: Commands, ui_state: ResMut<UiState>, mut images: ResMut<Assets<Image>>) {
    commands.spawn((Camera2dBundle::default(), IsDefaultUiCamera));
    let video_player = VideoPlayer {
        state: 0,
        timer: Timer::from_seconds(0.1, TimerMode::Repeating),
    };

    let mut canvas = Image::from_dynamic(
        DynamicImage::new_rgb8(500, 500),
        true,
        RenderAssetUsages::default(),
    );
    canvas.resize(Extent3d {
        width: 500,
        height: 500,
        ..default()
    });
    let image_handle = images.add(canvas);
    commands
        .spawn(SpriteBundle {
            texture: image_handle,
            sprite: Sprite {
                custom_size: Some(Vec2::new(500.0, 500.0)),
                ..Default::default()
            },
            ..Default::default()
        })
        .insert(video_player);

    let ui_state_clone = Arc::clone(&ui_state.frames);
    let rate = Arc::clone(&ui_state.rate);
    thread::spawn(move || {
        video::create_pipeline(ui_state_clone, rate);
    });
}

fn update(
    mut query: Query<(&mut VideoPlayer, &mut Handle<Image>)>,
    ui_state: ResMut<UiState>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time>,
) {
    for (mut video_player, mut img_render) in query.iter_mut() {
        let rate = ui_state.rate.lock().unwrap().abs();
        if rate > 0.0 {
            if video_player.timer.tick(time.delta()).just_finished() && video_player.state == 0 {
                let mut data = ui_state.frames.lock().unwrap();
                if let Some(item) = data.pop_front() {
                    let canvas = Image {
                        data: item.data,
                        texture_descriptor: TextureDescriptor {
                            label: None,
                            size: Extent3d {
                                width: item.width,
                                height: item.height,
                                ..Default::default()
                            },
                            dimension: TextureDimension::D2,
                            format: TextureFormat::Bgra8UnormSrgb,
                            mip_level_count: 1,
                            sample_count: 1,
                            usage: TextureUsages::TEXTURE_BINDING
                                | TextureUsages::COPY_DST
                                | TextureUsages::RENDER_ATTACHMENT,
                            view_formats: &[],
                        },
                        ..default()
                    };
                    *img_render = images.add(canvas);
                }
                if video_player.timer.duration().as_secs_f64() != rate {
                    video_player
                        .timer
                        .set_duration(Duration::from_nanos(rate as u64));
                }
            }
        }
    }
}
