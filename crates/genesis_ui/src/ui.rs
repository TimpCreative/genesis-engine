//! Interactive application shell: main menu, world setup, generation progress,
//! and the timeline viewer (Doc 02 Phase 3 UI chrome).
//!
//! Screens are Bevy states; each screen spawns a UI tree tagged with
//! [`ScreenRoot`] on enter and despawns it on exit. Generation runs on a
//! background thread so the UI stays responsive; progress and the finished
//! world arrive over a channel.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};

use bevy::prelude::*;
use genesis_render::{ColorsDirty, CurrentRenderMode, HexEntityCache, WorldDirty, WorldResource};

use crate::worldgen::{HistoryFrame, WorldGenConfig, generate_world_with_history};

/// Top-level application screen.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppScreen {
    #[default]
    MainMenu,
    Setup,
    Generating,
    Viewing,
}

/// Root entity of the active screen's UI tree (despawned on screen exit).
#[derive(Component)]
pub struct ScreenRoot(pub AppScreen);

/// Clickable menu actions.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    NewWorld,
    Quit,
    Back,
    Generate,
    Adjust(Param, i8),
    TimelineStep(i64),
    PlayPause,
}

/// User-adjustable world parameters shown on the setup screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Param {
    Seed,
    SubdivisionLevel,
    TargetYear,
    MajorPlates,
    MinorPlates,
}

/// Marks the text node displaying a parameter's current value.
#[derive(Component)]
pub struct ParamValueText(pub Param);

/// Marks the generation progress bar fill node.
#[derive(Component)]
pub struct ProgressBarFill;

/// Marks the generation progress text.
#[derive(Component)]
pub struct ProgressText;

/// Marks the viewer HUD status line.
#[derive(Component)]
pub struct HudText;

/// Marks the timeline position bar fill node.
#[derive(Component)]
pub struct TimelineBarFill;

/// Active world configuration being edited on the setup screen.
#[derive(Resource, Default)]
pub struct ActiveConfig(pub WorldGenConfig);

/// Messages from the generation thread.
pub enum GenMsg {
    Progress { year: i64, target: i64 },
    Done(Box<(genesis_core::World, Vec<HistoryFrame>)>),
    Failed(String),
}

/// Channel receiver for the in-flight generation.
#[derive(Resource)]
pub struct GenerationTask(pub Mutex<Receiver<GenMsg>>);

/// Buffered history for timeline scrubbing.
#[derive(Resource)]
pub struct WorldTimeline {
    pub frames: Vec<HistoryFrame>,
    pub current: usize,
    pub playing: bool,
    pub play_timer: Timer,
}

/// Target-year presets cycled by the setup screen.
pub const TARGET_YEAR_PRESETS: [i64; 7] = [
    1_000_000,
    10_000_000,
    100_000_000,
    500_000_000,
    1_000_000_000,
    2_000_000_000,
    4_500_000_000,
];

pub struct GenesisUiPlugin;

impl Plugin for GenesisUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppScreen>()
            .init_resource::<ActiveConfig>()
            .add_systems(OnEnter(AppScreen::MainMenu), spawn_main_menu)
            .add_systems(OnEnter(AppScreen::Setup), spawn_setup_screen)
            .add_systems(OnEnter(AppScreen::Generating), spawn_generating_screen)
            .add_systems(OnEnter(AppScreen::Viewing), spawn_viewing_hud)
            .add_systems(OnExit(AppScreen::MainMenu), despawn_screen)
            .add_systems(OnExit(AppScreen::Setup), despawn_screen)
            .add_systems(OnExit(AppScreen::Generating), despawn_screen)
            .add_systems(OnExit(AppScreen::Viewing), (despawn_screen, teardown_world))
            .add_systems(
                Update,
                (
                    button_hover_feedback,
                    handle_actions,
                    refresh_param_values.run_if(in_state(AppScreen::Setup)),
                    poll_generation.run_if(in_state(AppScreen::Generating)),
                    (timeline_keyboard, timeline_playback, refresh_hud)
                        .run_if(in_state(AppScreen::Viewing)),
                    escape_navigation,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// Shared UI helpers
// ---------------------------------------------------------------------------

const PANEL_BG: Color = Color::srgba(0.08, 0.09, 0.12, 0.92);
const BUTTON_BG: Color = Color::srgb(0.17, 0.19, 0.24);
const BUTTON_BG_HOVER: Color = Color::srgb(0.25, 0.29, 0.38);
const ACCENT: Color = Color::srgb(0.35, 0.65, 0.95);

/// Query alias: buttons whose interaction state changed this frame.
type ChangedButtons<'w, 's, T> =
    Query<'w, 's, (&'static Interaction, T), (Changed<Interaction>, With<Button>)>;

fn despawn_screen(mut commands: Commands, roots: Query<Entity, With<ScreenRoot>>) {
    for root in &roots {
        commands.entity(root).despawn();
    }
}

/// Removes the world and its hex entities when leaving the viewer.
fn teardown_world(mut commands: Commands, mut cache: ResMut<HexEntityCache>) {
    for entity in cache.entities.drain(..) {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<WorldResource>();
    commands.remove_resource::<WorldTimeline>();
}

fn full_screen_root(screen: AppScreen) -> (ScreenRoot, Node, BackgroundColor) {
    (
        ScreenRoot(screen),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            row_gap: Val::Px(14.0),
            ..default()
        },
        BackgroundColor(PANEL_BG),
    )
}

fn button(action: UiAction) -> (Button, UiAction, Node, BackgroundColor) {
    (
        Button,
        action,
        Node {
            padding: UiRect::axes(Val::Px(18.0), Val::Px(8.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(BUTTON_BG),
    )
}

fn label(text: &str, size: f32) -> (Text, TextFont, TextColor) {
    (
        Text::new(text),
        TextFont {
            font_size: size,
            ..default()
        },
        TextColor(Color::WHITE),
    )
}

fn button_hover_feedback(mut query: ChangedButtons<&'static mut BackgroundColor>) {
    for (interaction, mut bg) in &mut query {
        bg.0 = match interaction {
            Interaction::Hovered | Interaction::Pressed => BUTTON_BG_HOVER,
            Interaction::None => BUTTON_BG,
        };
    }
}

// ---------------------------------------------------------------------------
// Main menu
// ---------------------------------------------------------------------------

fn spawn_main_menu(mut commands: Commands) {
    commands
        .spawn(full_screen_root(AppScreen::MainMenu))
        .with_children(|parent| {
            parent.spawn(label("GENESIS ENGINE", 52.0));
            parent.spawn(label("deterministic worldbuilding simulator", 16.0));
            parent.spawn(Node {
                height: Val::Px(24.0),
                ..default()
            });
            parent.spawn(button(UiAction::NewWorld)).with_children(|b| {
                b.spawn(label("New World", 24.0));
            });
            parent.spawn(button(UiAction::Quit)).with_children(|b| {
                b.spawn(label("Quit", 24.0));
            });
        });
}

// ---------------------------------------------------------------------------
// Setup screen
// ---------------------------------------------------------------------------

const SETUP_PARAMS: [(Param, &str); 5] = [
    (Param::Seed, "Seed"),
    (Param::SubdivisionLevel, "Detail (subdivision level)"),
    (Param::TargetYear, "Simulate to year"),
    (Param::MajorPlates, "Major plates"),
    (Param::MinorPlates, "Minor plates"),
];

fn spawn_setup_screen(mut commands: Commands) {
    commands
        .spawn(full_screen_root(AppScreen::Setup))
        .with_children(|parent| {
            parent.spawn(label("New World", 36.0));
            for (param, name) in SETUP_PARAMS {
                parent
                    .spawn(Node {
                        column_gap: Val::Px(10.0),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            label(name, 18.0).0,
                            label(name, 18.0).1,
                            label(name, 18.0).2,
                            Node {
                                width: Val::Px(280.0),
                                ..default()
                            },
                        ));
                        row.spawn(button(UiAction::Adjust(param, -1)))
                            .with_children(|b| {
                                b.spawn(label("-", 18.0));
                            });
                        row.spawn((
                            label("", 18.0).0,
                            label("", 18.0).1,
                            TextColor(ACCENT),
                            ParamValueText(param),
                            Node {
                                width: Val::Px(140.0),
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                        ));
                        row.spawn(button(UiAction::Adjust(param, 1)))
                            .with_children(|b| {
                                b.spawn(label("+", 18.0));
                            });
                    });
            }
            parent.spawn(Node {
                height: Val::Px(16.0),
                ..default()
            });
            parent
                .spawn(Node {
                    column_gap: Val::Px(12.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(button(UiAction::Generate)).with_children(|b| {
                        b.spawn(label("Generate", 24.0));
                    });
                    row.spawn(button(UiAction::Back)).with_children(|b| {
                        b.spawn(label("Back", 24.0));
                    });
                });
        });
}

fn format_param(config: &WorldGenConfig, param: Param) -> String {
    match param {
        Param::Seed => config.seed.to_string(),
        Param::SubdivisionLevel => format!("{}", config.subdivision_level),
        Param::TargetYear => format_year(config.target_year),
        Param::MajorPlates => config.major_plates.to_string(),
        Param::MinorPlates => config.minor_plates.to_string(),
    }
}

pub fn format_year(year: i64) -> String {
    if year >= 1_000_000_000 {
        format!("{:.2}B", year as f64 / 1e9)
    } else if year >= 1_000_000 {
        format!("{:.0}M", year as f64 / 1e6)
    } else {
        format!("{year}")
    }
}

fn adjust_param(config: &mut WorldGenConfig, param: Param, direction: i8) {
    let up = direction > 0;
    match param {
        Param::Seed => {
            config.seed = if up {
                config.seed.wrapping_add(1)
            } else {
                config.seed.wrapping_sub(1)
            };
        }
        Param::SubdivisionLevel => {
            let level = config.subdivision_level as i16 + direction as i16;
            config.subdivision_level = level.clamp(5, 8) as u8;
        }
        Param::TargetYear => {
            let idx = TARGET_YEAR_PRESETS
                .iter()
                .position(|&y| y >= config.target_year)
                .unwrap_or(0);
            let next = (idx as i16 + direction as i16)
                .clamp(0, TARGET_YEAR_PRESETS.len() as i16 - 1) as usize;
            config.target_year = TARGET_YEAR_PRESETS[next];
        }
        Param::MajorPlates => {
            let v = config.major_plates as i16 + direction as i16;
            config.major_plates = v.clamp(6, 9) as u8;
        }
        Param::MinorPlates => {
            let v = config.minor_plates as i16 + direction as i16;
            config.minor_plates = v.clamp(6, 10) as u8;
        }
    }
}

fn refresh_param_values(
    config: Res<ActiveConfig>,
    mut labels: Query<(&ParamValueText, &mut Text)>,
) {
    if !config.is_changed() {
        return;
    }
    for (param_text, mut text) in &mut labels {
        text.0 = format_param(&config.0, param_text.0);
    }
}

// ---------------------------------------------------------------------------
// Generating screen
// ---------------------------------------------------------------------------

fn spawn_generating_screen(mut commands: Commands) {
    commands
        .spawn(full_screen_root(AppScreen::Generating))
        .with_children(|parent| {
            parent.spawn(label("Generating world…", 30.0));
            parent.spawn((
                label("simulating year 0", 18.0).0,
                label("", 18.0).1,
                TextColor(ACCENT),
                ProgressText,
            ));
            parent
                .spawn((
                    Node {
                        width: Val::Px(480.0),
                        height: Val::Px(14.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.15, 0.16, 0.20)),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        Node {
                            width: Val::Percent(0.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(ACCENT),
                        ProgressBarFill,
                    ));
                });
        });
}

pub fn start_generation(commands: &mut Commands, config: WorldGenConfig) {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let tx_progress = tx.clone();
        let mut last_report = -1_i64;
        let result = generate_world_with_history(&config, |year, target| {
            // Throttle channel traffic: report at most once per 1/200 of the run.
            if year - last_report >= (target / 200).max(1) {
                last_report = year;
                let _ = tx_progress.send(GenMsg::Progress { year, target });
            }
        });
        let _ = match result {
            Ok(done) => tx.send(GenMsg::Done(Box::new(done))),
            Err(e) => tx.send(GenMsg::Failed(e)),
        };
    });
    commands.insert_resource(GenerationTask(Mutex::new(rx)));
}

#[allow(clippy::too_many_arguments)]
fn poll_generation(
    mut commands: Commands,
    task: Option<Res<GenerationTask>>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut world_dirty: ResMut<WorldDirty>,
    mut bar: Query<&mut Node, With<ProgressBarFill>>,
    mut progress_text: Query<&mut Text, With<ProgressText>>,
) {
    let Some(task) = task else {
        return;
    };
    let Ok(rx) = task.0.lock() else {
        return;
    };

    for msg in rx.try_iter() {
        match msg {
            GenMsg::Progress { year, target } => {
                let fraction = (year as f64 / target.max(1) as f64).clamp(0.0, 1.0);
                if let Ok(mut node) = bar.single_mut() {
                    node.width = Val::Percent((fraction * 100.0) as f32);
                }
                if let Ok(mut text) = progress_text.single_mut() {
                    text.0 = format!(
                        "simulating year {} / {}",
                        format_year(year),
                        format_year(target)
                    );
                }
            }
            GenMsg::Done(done) => {
                let (world, frames) = *done;
                let last = frames.len().saturating_sub(1);
                commands.insert_resource(WorldResource(world));
                commands.insert_resource(WorldTimeline {
                    frames,
                    current: last,
                    playing: false,
                    play_timer: Timer::from_seconds(0.25, TimerMode::Repeating),
                });
                world_dirty.0 = true;
                commands.remove_resource::<GenerationTask>();
                next_screen.set(AppScreen::Viewing);
            }
            GenMsg::Failed(err) => {
                if let Ok(mut text) = progress_text.single_mut() {
                    text.0 = format!("generation failed: {err} — Esc to return");
                }
                commands.remove_resource::<GenerationTask>();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Viewing screen (HUD + timeline)
// ---------------------------------------------------------------------------

fn spawn_viewing_hud(mut commands: Commands) {
    commands
        .spawn((
            ScreenRoot(AppScreen::Viewing),
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            },
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(10.0)),
                        row_gap: Val::Px(6.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.85)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        label("", 16.0).0,
                        label("", 16.0).1,
                        TextColor(Color::WHITE),
                        HudText,
                    ));
                    panel
                        .spawn(Node {
                            column_gap: Val::Px(8.0),
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn(button(UiAction::TimelineStep(-1)))
                                .with_children(|b| {
                                    b.spawn(label("<", 16.0));
                                });
                            row.spawn(button(UiAction::PlayPause)).with_children(|b| {
                                b.spawn(label("Play", 16.0));
                            });
                            row.spawn(button(UiAction::TimelineStep(1)))
                                .with_children(|b| {
                                    b.spawn(label(">", 16.0));
                                });
                            row.spawn((
                                Node {
                                    flex_grow: 1.0,
                                    height: Val::Px(10.0),
                                    ..default()
                                },
                                BackgroundColor(Color::srgb(0.15, 0.16, 0.20)),
                            ))
                            .with_children(|bar| {
                                bar.spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(ACCENT),
                                    TimelineBarFill,
                                ));
                            });
                            row.spawn(button(UiAction::Back)).with_children(|b| {
                                b.spawn(label("Menu", 16.0));
                            });
                        });
                });
        });
}

/// Applies the timeline's current frame to the rendered world.
fn apply_current_frame(
    timeline: &WorldTimeline,
    world_res: &mut WorldResource,
    colors_dirty: &mut ColorsDirty,
) {
    if let Some(frame) = timeline.frames.get(timeline.current) {
        frame.apply(&mut world_res.0.data);
        colors_dirty.0 = true;
    }
}

fn timeline_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    timeline: Option<ResMut<WorldTimeline>>,
    world_res: Option<ResMut<WorldResource>>,
    mut colors_dirty: ResMut<ColorsDirty>,
) {
    let (Some(mut timeline), Some(mut world_res)) = (timeline, world_res) else {
        return;
    };
    let step: i64 = if keys.just_pressed(KeyCode::ArrowLeft) {
        -1
    } else if keys.just_pressed(KeyCode::ArrowRight) {
        1
    } else if keys.just_pressed(KeyCode::Space) {
        timeline.playing = !timeline.playing;
        0
    } else {
        return;
    };
    if step != 0 {
        timeline.playing = false;
        step_timeline(&mut timeline, step);
        apply_current_frame(&timeline, &mut world_res, &mut colors_dirty);
    }
}

fn step_timeline(timeline: &mut WorldTimeline, step: i64) {
    let last = timeline.frames.len().saturating_sub(1) as i64;
    let next = (timeline.current as i64 + step).clamp(0, last);
    timeline.current = next as usize;
}

fn timeline_playback(
    time: Res<Time>,
    timeline: Option<ResMut<WorldTimeline>>,
    world_res: Option<ResMut<WorldResource>>,
    mut colors_dirty: ResMut<ColorsDirty>,
) {
    let (Some(mut timeline), Some(mut world_res)) = (timeline, world_res) else {
        return;
    };
    if !timeline.playing {
        return;
    }
    timeline.play_timer.tick(time.delta());
    if !timeline.play_timer.just_finished() {
        return;
    }
    if timeline.current + 1 >= timeline.frames.len() {
        timeline.playing = false;
        return;
    }
    timeline.current += 1;
    apply_current_frame(&timeline, &mut world_res, &mut colors_dirty);
}

fn refresh_hud(
    timeline: Option<Res<WorldTimeline>>,
    mode: Res<CurrentRenderMode>,
    mut hud: Query<&mut Text, With<HudText>>,
    mut bar: Query<&mut Node, With<TimelineBarFill>>,
) {
    let Some(timeline) = timeline else {
        return;
    };
    let Some(frame) = timeline.frames.get(timeline.current) else {
        return;
    };
    if let Ok(mut text) = hud.single_mut() {
        text.0 = format!(
            "Year {}  |  Mode: {} [M]  |  scrub with < > or arrow keys, Space plays, Esc for menu",
            format_year(frame.year),
            mode.0.label(),
        );
    }
    if let Ok(mut node) = bar.single_mut() {
        let last = timeline.frames.len().saturating_sub(1).max(1);
        node.width = Val::Percent(timeline.current as f32 / last as f32 * 100.0);
    }
}

// ---------------------------------------------------------------------------
// Actions and navigation
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_actions(
    mut commands: Commands,
    interactions: ChangedButtons<&'static UiAction>,
    mut config: ResMut<ActiveConfig>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    screen: Res<State<AppScreen>>,
    mut exit: MessageWriter<AppExit>,
    timeline: Option<ResMut<WorldTimeline>>,
    world_res: Option<ResMut<WorldResource>>,
    colors_dirty: Option<ResMut<ColorsDirty>>,
) {
    let mut timeline = timeline;
    let mut world_res = world_res;
    let mut colors_dirty = colors_dirty;

    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match *action {
            UiAction::NewWorld => next_screen.set(AppScreen::Setup),
            UiAction::Quit => {
                exit.write(AppExit::Success);
            }
            UiAction::Back => match screen.get() {
                AppScreen::Viewing => next_screen.set(AppScreen::MainMenu),
                _ => next_screen.set(AppScreen::MainMenu),
            },
            UiAction::Generate => {
                start_generation(&mut commands, config.0.clone());
                next_screen.set(AppScreen::Generating);
            }
            UiAction::Adjust(param, direction) => {
                adjust_param(&mut config.0, param, direction);
            }
            UiAction::TimelineStep(step) => {
                if let (Some(timeline), Some(world_res), Some(colors_dirty)) =
                    (timeline.as_mut(), world_res.as_mut(), colors_dirty.as_mut())
                {
                    timeline.playing = false;
                    step_timeline(timeline, step);
                    apply_current_frame(timeline, world_res, colors_dirty);
                }
            }
            UiAction::PlayPause => {
                if let Some(timeline) = timeline.as_mut() {
                    timeline.playing = !timeline.playing;
                }
            }
        }
    }
}

fn escape_navigation(
    keys: Res<ButtonInput<KeyCode>>,
    screen: Res<State<AppScreen>>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    match screen.get() {
        AppScreen::MainMenu => {
            exit.write(AppExit::Success);
        }
        AppScreen::Setup | AppScreen::Viewing => next_screen.set(AppScreen::MainMenu),
        AppScreen::Generating => {
            // Generation threads cannot be safely cancelled mid-tick; let the
            // run finish in the background and return to the menu.
            next_screen.set(AppScreen::MainMenu);
        }
    }
}
