use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use plugin::{insert_video_component, VideoPlayer, VideoState};
mod plugin;
mod video;

#[derive(Component)]
struct StatusText;

#[derive(Component)]
struct ProgressBar;

#[derive(Component)]
struct ProgressFill;

#[derive(Component)]
struct ProgressText;

#[derive(Component)]
struct ButtonText;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, plugin::VideoPlugin))
        .add_systems(Startup, start_up)
        .add_systems(Update, (update, plugin::render_video_frame, update_status_text, update_progress))
        .run();
}

fn start_up(mut commands: Commands, images: ResMut<Assets<Image>>, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2d);
    let uri = "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm";
    let video_player = VideoPlayer {
        uri: uri.to_string(),
        state: VideoState::Init,
        timer: Arc::new(Mutex::new(Timer::from_seconds(0.001, TimerMode::Repeating))),
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

    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    commands
        .spawn(Node {
            top: Val::Px(550.0),
            left: Val::Px(200.0),
            ..Default::default()
        })
        .with_children(|parent| {
            parent
                .spawn((
                    Text::new("start    "),
                    TextFont {
                        font: font.clone(),
                        font_size: 12.0,
                        ..Default::default()
                    },
                    TextColor(Color::WHITE),
                    ButtonText,
                ))
                .insert(Interaction::Pressed);
            parent
                .spawn((
                    Text::new("   stop"),
                    TextFont {
                        font: font.clone(),
                        font_size: 12.0,
                        ..Default::default()
                    },
                    TextColor(Color::WHITE),
                    ButtonText,
                ))
                .insert(Interaction::Pressed);
        });

    // Status text
    commands
        .spawn((
            Text::new(""),
            TextFont {
                font: font.clone(),
                font_size: 14.0,
                ..Default::default()
            },
            TextColor(Color::srgb(1.0, 1.0, 0.0)),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(10.0),
                left: Val::Px(10.0),
                ..Default::default()
            },
            StatusText,
        ));

    // Progress bar container
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(50.0),
            left: Val::Px(150.0),
            width: Val::Px(500.0),
            height: Val::Px(20.0),
            flex_direction: FlexDirection::Column,
            ..Default::default()
        })
        .with_children(|parent| {
            // Progress bar background
            parent
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(8.0),
                        ..Default::default()
                    },
                    BackgroundColor(Color::srgb(0.3, 0.3, 0.3)),
                    ProgressBar,
                ))
                .with_children(|parent| {
                    // Progress bar fill
                    parent.spawn((
                        Node {
                            width: Val::Percent(0.0),
                            height: Val::Percent(100.0),
                            ..Default::default()
                        },
                        BackgroundColor(Color::srgb(0.2, 0.7, 1.0)),
                        ProgressFill,
                    ));
                });

            // Progress text (time display)
            parent.spawn((
                Text::new("00:00 / 00:00"),
                TextFont {
                    font: font.clone(),
                    font_size: 12.0,
                    ..Default::default()
                },
                TextColor(Color::WHITE),
                ProgressText,
            ));
        });
}

fn update(
    query: Query<(&Interaction, &Text), (Changed<Interaction>, With<ButtonText>)>,
    mut query_video: Query<(&mut VideoPlayer, Entity, &mut ImageNode)>,
) {
    for (mut video_player, id, _) in query_video.iter_mut() {
        if video_player.id.is_none() {
            video_player.id = Some(id);
            println!("[DEBUG] VideoPlayer id set: {:?}", id);
        }

        for (interaction, text) in query.iter() {
            let btn_text = text.0.trim();
            println!("[DEBUG] Interaction: {:?}, Button: {}, State: {:?}", interaction, btn_text, video_player.state);

            match interaction {
                Interaction::Pressed => {
                    if video_player.id.is_some() {
                        match btn_text {
                            "start" => {
                                // Prevent multiple clicks while loading
                                if video_player.state != VideoState::Loading {
                                    println!("[DEBUG] Start button pressed! Changing state to Start");
                                    video_player.state = VideoState::Start;
                                }
                            }
                            "stop" => {
                                println!("[DEBUG] Stop button pressed! Changing state to Paused");
                                video_player.state = VideoState::Paused;
                            }
                            _ => {}
                        }
                    } else {
                        println!("[DEBUG] Button pressed but video_player.id is None");
                    }
                }
                _ => {}
            }
        }
    }
}

fn update_status_text(
    query_video: Query<&VideoPlayer>,
    mut query_status: Query<&mut Text, With<StatusText>>,
) {
    for video_player in query_video.iter() {
        for mut text in query_status.iter_mut() {
            let status = match video_player.state {
                VideoState::Init => "Initializing...",
                VideoState::Ready => "Ready",
                VideoState::Loading => "Loading video...",
                VideoState::Playing => "Playing",
                VideoState::Paused => "Paused",
                VideoState::Start => "Starting...",
                VideoState::Stop => "Stopped",
            };
            text.0 = status.to_string();
        }
    }
}

fn update_progress(
    query_video: Query<&VideoPlayer>,
    mut query_fill: Query<(&mut Node, &mut BackgroundColor), With<ProgressFill>>,
    mut query_text: Query<&mut Text, With<ProgressText>>,
    time: Res<Time>,
) {
    for video_player in query_video.iter() {
        let is_loading = matches!(
            video_player.state,
            VideoState::Loading | VideoState::Start | VideoState::Init
        );

        if is_loading {
            // Show loading animation
            let loading_progress = (time.elapsed_secs() * 2.0).sin() * 0.5 + 0.5;

            for (mut node, mut bg_color) in query_fill.iter_mut() {
                node.width = Val::Percent(loading_progress * 100.0);
                // Pulsing color for loading
                bg_color.0 = Color::srgb(0.5, 0.5 + loading_progress * 0.3, 1.0);
            }

            for mut text in query_text.iter_mut() {
                let dots = match ((time.elapsed_secs() * 2.0) as u32) % 4 {
                    0 => "",
                    1 => ".",
                    2 => "..",
                    _ => "...",
                };
                text.0 = format!("Loading{}", dots);
            }
        } else {
            let progress = video_player.progress();
            let position = video_player.position();
            let duration = video_player.duration();

            // Update progress bar fill
            for (mut node, mut bg_color) in query_fill.iter_mut() {
                node.width = Val::Percent(progress * 100.0);
                // Normal playback color
                bg_color.0 = Color::srgb(0.2, 0.7, 1.0);
            }

            // Update time display
            for mut text in query_text.iter_mut() {
                let pos_min = (position / 60.0) as u32;
                let pos_sec = (position % 60.0) as u32;
                let dur_min = (duration / 60.0) as u32;
                let dur_sec = (duration % 60.0) as u32;
                text.0 = format!("{:02}:{:02} / {:02}:{:02}", pos_min, pos_sec, dur_min, dur_sec);
            }
        }
    }
}
