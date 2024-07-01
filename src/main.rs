use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use plugin::{insert_video_component, VideoPlayer, VideoState};
mod plugin;
mod video;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, plugin::VideoPlugin))
        .add_systems(Startup, start_up)
        .add_systems(Update, (update, plugin::render_video_frame))
        .run();
}

fn start_up(mut commands: Commands, images: ResMut<Assets<Image>>, asset_server: Res<AssetServer>) {
    commands.spawn((Camera2dBundle::default(), IsDefaultUiCamera));
    let uri = "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm";
    let video_player = VideoPlayer {
        uri: uri.to_string(),
        state: VideoState::Init,
        timer: Arc::new(Mutex::new(Timer::from_seconds(1.0, TimerMode::Repeating))),
        width: 500.0,
        height: 500.0,
        id: None,
        pipeline: None,
    };
    commands
        .spawn(insert_video_component(
            images,
            Vec2::new(video_player.width, video_player.height),
        ))
        .insert(video_player);

    commands
        .spawn(NodeBundle {
            style: Style {
                top: Val::Px(550.0),
                left: Val::Px(200.0),
                ..Default::default()
            },
            ..Default::default()
        })
        .with_children(|parent| {
            parent
                .spawn(TextBundle::from_section(
                    "start    ",
                    TextStyle {
                        font: asset_server.load("fonts/FiraMono-Medium.ttf"),
                        font_size: 12.0,
                        color: Color::WHITE,
                        ..Default::default()
                    },
                ))
                .insert(Interaction::Pressed);
            parent
                .spawn(TextBundle::from_section(
                    "   stop",
                    TextStyle {
                        font: asset_server.load("fonts/FiraMono-Medium.ttf"),
                        font_size: 12.0,
                        color: Color::WHITE,
                        ..Default::default()
                    },
                ))
                .insert(Interaction::Pressed);
        });
}

fn update(
    mut query: Query<(&Interaction, &mut Text)>,
    mut query_video: Query<(&mut VideoPlayer, Entity, &mut UiImage)>,
) {
    for (mut video_player, id, _) in query_video.iter_mut() {
        for (interaction, text) in query.iter_mut() {
            match interaction {
                Interaction::Pressed => {
                    if video_player.id.is_some() {
                        match text.sections[0].value.trim() {
                            "start" => {
                                video_player.state = VideoState::Start;
                            }
                            "stop" => {
                                video_player.state = VideoState::Paused;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        if video_player.id.is_none() {
            video_player.id = Some(id);
        }
    }
}
