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
struct PlayButton;

#[derive(Component)]
struct StopButton;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Video Player".to_string(),
                resolution: (800, 650).into(),
                ..default()
            }),
            ..default()
        }), plugin::VideoPlugin))
        .add_systems(Startup, start_up)
        .add_systems(Update, (update, plugin::render_video_frame, update_status_text, update_progress, button_hover_effect))
        .run();
}

fn start_up(mut commands: Commands, images: ResMut<Assets<Image>>, asset_server: Res<AssetServer>) {
    commands.spawn(Camera2d);
    let uri = "https://gstreamer.freedesktop.org/data/media/sintel_trailer-480p.webm";
    let video_player = VideoPlayer {
        uri: uri.to_string(),
        state: VideoState::Init,
        timer: Arc::new(Mutex::new(Timer::from_seconds(0.001, TimerMode::Repeating))),
        width: 720.0,
        height: 400.0,
        id: None,
        pipeline: None,
    };
    
    let font = asset_server.load("fonts/FiraMono-Medium.ttf");

    // Main container
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::FlexStart,
            padding: UiRect::all(Val::Px(20.0)),
            ..Default::default()
        })
        .with_children(|parent| {
            // Title
            parent.spawn((
                Text::new("Bevy Video Player"),
                TextFont {
                    font: font.clone(),
                    font_size: 24.0,
                    ..Default::default()
                },
                TextColor(Color::WHITE),
                Node {
                    margin: UiRect::bottom(Val::Px(15.0)),
                    ..Default::default()
                },
            ));

            // Video container with border
            parent
                .spawn((
                    Node {
                        width: Val::Px(724.0),
                        height: Val::Px(404.0),
                        border: UiRect::all(Val::Px(2.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..Default::default()
                    },
                    BorderColor::all(Color::srgb(0.3, 0.3, 0.3)),
                    BackgroundColor(Color::BLACK),
                ))
                .with_children(|video_container| {
                    video_container
                        .spawn(insert_video_component(
                            images,
                            Vec2::new(video_player.width, video_player.height),
                        ))
                        .insert(video_player);
                });

            // Controls container
            parent
                .spawn(Node {
                    width: Val::Px(720.0),
                    margin: UiRect::top(Val::Px(15.0)),
                    flex_direction: FlexDirection::Column,
                    ..Default::default()
                })
                .with_children(|controls| {
                    // Progress bar
                    controls
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(6.0),
                            margin: UiRect::bottom(Val::Px(10.0)),
                            ..Default::default()
                        })
                        .with_children(|progress_container| {
                            progress_container
                                .spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        height: Val::Percent(100.0),
                                        border: UiRect::all(Val::Px(1.0)),
                                        ..Default::default()
                                    },
                                    BackgroundColor(Color::srgb(0.15, 0.15, 0.15)),
                                    BorderColor::all(Color::srgb(0.3, 0.3, 0.3)),
                                    ProgressBar,
                                ))
                                .with_children(|bar| {
                                    bar.spawn((
                                        Node {
                                            width: Val::Percent(0.0),
                                            height: Val::Percent(100.0),
                                            ..Default::default()
                                        },
                                        BackgroundColor(Color::srgb(0.0, 0.6, 1.0)),
                                        ProgressFill,
                                    ));
                                });
                        });

                    // Time and buttons row
                    controls
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            ..Default::default()
                        })
                        .with_children(|row| {
                            // Time display
                            row.spawn((
                                Text::new("00:00 / 00:00"),
                                TextFont {
                                    font: font.clone(),
                                    font_size: 14.0,
                                    ..Default::default()
                                },
                                TextColor(Color::srgb(0.7, 0.7, 0.7)),
                                ProgressText,
                            ));

                            // Buttons container
                            row.spawn(Node {
                                column_gap: Val::Px(10.0),
                                ..Default::default()
                            })
                            .with_children(|buttons| {
                                // Play button
                                buttons
                                    .spawn((
                                        Node {
                                            padding: UiRect::axes(Val::Px(20.0), Val::Px(8.0)),
                                            border: UiRect::all(Val::Px(1.0)),
                                            ..Default::default()
                                        },
                                        BackgroundColor(Color::srgb(0.0, 0.5, 0.2)),
                                        BorderColor::all(Color::srgb(0.0, 0.7, 0.3)),
                                        Interaction::None,
                                        PlayButton,
                                    ))
                                    .with_children(|btn| {
                                        btn.spawn((
                                            Text::new("Play"),
                                            TextFont {
                                                font: font.clone(),
                                                font_size: 14.0,
                                                ..Default::default()
                                            },
                                            TextColor(Color::WHITE),
                                        ));
                                    });

                                // Stop button
                                buttons
                                    .spawn((
                                        Node {
                                            padding: UiRect::axes(Val::Px(20.0), Val::Px(8.0)),
                                            border: UiRect::all(Val::Px(1.0)),
                                            ..Default::default()
                                        },
                                        BackgroundColor(Color::srgb(0.5, 0.1, 0.1)),
                                        BorderColor::all(Color::srgb(0.7, 0.2, 0.2)),
                                        Interaction::None,
                                        StopButton,
                                    ))
                                    .with_children(|btn| {
                                        btn.spawn((
                                            Text::new("Stop"),
                                            TextFont {
                                                font: font.clone(),
                                                font_size: 14.0,
                                                ..Default::default()
                                            },
                                            TextColor(Color::WHITE),
                                        ));
                                    });
                            });

                            // Status text
                            row.spawn((
                                Text::new("Ready"),
                                TextFont {
                                    font: font.clone(),
                                    font_size: 14.0,
                                    ..Default::default()
                                },
                                TextColor(Color::srgb(0.6, 0.8, 1.0)),
                                StatusText,
                            ));
                        });
                });
        });
}

fn button_hover_effect(
    mut play_query: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<PlayButton>)>,
    mut stop_query: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<StopButton>, Without<PlayButton>)>,
) {
    for (interaction, mut bg) in play_query.iter_mut() {
        *bg = match interaction {
            Interaction::Hovered => BackgroundColor(Color::srgb(0.0, 0.6, 0.3)),
            Interaction::Pressed => BackgroundColor(Color::srgb(0.0, 0.4, 0.15)),
            Interaction::None => BackgroundColor(Color::srgb(0.0, 0.5, 0.2)),
        };
    }
    for (interaction, mut bg) in stop_query.iter_mut() {
        *bg = match interaction {
            Interaction::Hovered => BackgroundColor(Color::srgb(0.6, 0.15, 0.15)),
            Interaction::Pressed => BackgroundColor(Color::srgb(0.4, 0.08, 0.08)),
            Interaction::None => BackgroundColor(Color::srgb(0.5, 0.1, 0.1)),
        };
    }
}

fn update(
    play_query: Query<&Interaction, (Changed<Interaction>, With<PlayButton>)>,
    stop_query: Query<&Interaction, (Changed<Interaction>, With<StopButton>)>,
    mut query_video: Query<(&mut VideoPlayer, Entity)>,
) {
    for (mut video_player, id) in query_video.iter_mut() {
        if video_player.id.is_none() {
            video_player.id = Some(id);
            // println!("[DEBUG] VideoPlayer id set: {:?}", id);
        }

        for interaction in play_query.iter() {
            if *interaction == Interaction::Pressed && video_player.state != VideoState::Loading {
                // println!("[DEBUG] Play button pressed! Changing state to Start");
                video_player.state = VideoState::Start;
            }
        }

        for interaction in stop_query.iter() {
            if *interaction == Interaction::Pressed {
                // println!("[DEBUG] Stop button pressed! Changing state to Paused");
                video_player.state = VideoState::Paused;
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
                VideoState::Init => "Initializing",
                VideoState::Ready => "Ready",
                VideoState::Loading => "Loading...",
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
            let loading_progress = (time.elapsed_secs() * 3.0).sin() * 0.3 + 0.5;

            for (mut node, mut bg_color) in query_fill.iter_mut() {
                node.width = Val::Percent(loading_progress * 100.0);
                bg_color.0 = Color::srgb(0.3, 0.5, 1.0);
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

            for (mut node, mut bg_color) in query_fill.iter_mut() {
                node.width = Val::Percent(progress * 100.0);
                bg_color.0 = Color::srgb(0.0, 0.6, 1.0);
            }

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
