use std::collections::VecDeque;

use bevy::prelude::*;
use plugin::{insert_video_component, VideoPlayer, VideoState};
mod plugin;
mod video;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, plugin::VideoPlugin))
        .add_systems(Startup, start_up)
        .add_systems(Update, (update, plugin::add_video_frame, plugin::render_video_frame))
        .run();
}

fn start_up(mut commands: Commands, images: ResMut<Assets<Image>>) {
    commands.spawn((Camera2dBundle::default(), IsDefaultUiCamera));
    let uri = "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm";
    let video_player = VideoPlayer {
        uri: uri.to_string(),
        state: VideoState::Start,
        timer: Timer::from_seconds(0.1, TimerMode::Repeating),
        data: VecDeque::new(),
        width: 500.0,
        height: 500.0,
        id: None,
    };
    let _ = commands.spawn(insert_video_component(
        images,
        Vec2::new(video_player.width, video_player.height),
    )).insert(video_player).id();
}

fn update(mut query: Query<(&mut VideoPlayer, Entity)>,) {
    for (mut video_player, id) in query.iter_mut() {
        if video_player.id.is_none() {
            video_player.id = Some(id);
        } else {
            video_player.state = VideoState::Playing;
        }
    }
}
