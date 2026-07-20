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
use bevy::ui::FocusPolicy;
use genesis_render::{
    ColorsDirty, CurrentRenderMode, HexEntityCache, HexMeshIndex, RiversDirty, WorldDirty,
    WorldResource,
};

use crate::hex_inspect::{
    BlocksMapPick, HoveredHex, InspectorTab, InspectorVisible, clear_inspect_on_exit,
    despawn_hex_inspect_ui, handle_inspector_tabs, handle_map_hex_click, inspector_hotkeys,
    refresh_tab_colors, spawn_hex_inspect_ui, update_hex_inspector, update_hex_tooltip,
    update_hovered_hex, viewer_escape,
};
use crate::worldgen::{GenEvent, HistoryFrame, WorldGenConfig, generate_world_streaming};

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
    ContinentalFraction,
    WaterInventory,
    LandFraction,
    Mountains,
    Islands,
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

/// Marks the timeline buffered-region fill node (dim, behind the position).
#[derive(Component)]
pub struct TimelineBufferedFill;

/// Active world configuration being edited on the setup screen.
#[derive(Resource, Default)]
pub struct ActiveConfig(pub WorldGenConfig);

/// Channel receiver for the in-flight generation's [`GenEvent`] stream.
#[derive(Resource)]
pub struct GenerationTask(pub Mutex<Receiver<GenEvent>>);

/// Buffered history for timeline scrubbing. Grows while generation streams
/// (YouTube-style buffering); `complete` flips when the thread finishes.
#[derive(Resource)]
pub struct WorldTimeline {
    pub frames: Vec<HistoryFrame>,
    pub current: usize,
    pub playing: bool,
    pub play_timer: Timer,
    pub target_year: i64,
    pub complete: bool,
    /// Set when `current` changed before the display world existed; the next
    /// poll applies the frame once the inserted `WorldResource` is visible.
    pub needs_apply: bool,
}

/// Key-repeat state for hold-to-scrub.
#[derive(Resource)]
pub struct ScrubRepeat(pub Timer);

impl Default for ScrubRepeat {
    fn default() -> Self {
        Self(Timer::from_seconds(SCRUB_INITIAL_DELAY_S, TimerMode::Once))
    }
}

/// Delay before hold-to-scrub starts repeating (s).
pub const SCRUB_INITIAL_DELAY_S: f32 = 0.35;
/// Repeat interval while an arrow key is held (s).
pub const SCRUB_REPEAT_INTERVAL_S: f32 = 0.06;

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
            .init_resource::<ScrubRepeat>()
            .init_resource::<HoveredHex>()
            .init_resource::<InspectorTab>()
            .init_resource::<InspectorVisible>()
            .add_systems(OnEnter(AppScreen::MainMenu), spawn_main_menu)
            .add_systems(OnEnter(AppScreen::Setup), spawn_setup_screen)
            .add_systems(OnEnter(AppScreen::Generating), spawn_generating_screen)
            .add_systems(
                OnEnter(AppScreen::Viewing),
                (spawn_viewing_hud, spawn_hex_inspect_ui),
            )
            .add_systems(OnExit(AppScreen::MainMenu), despawn_screen)
            .add_systems(OnExit(AppScreen::Setup), despawn_screen)
            .add_systems(OnExit(AppScreen::Generating), despawn_screen)
            .add_systems(
                OnExit(AppScreen::Viewing),
                (
                    despawn_screen,
                    despawn_hex_inspect_ui,
                    clear_inspect_on_exit,
                    teardown_world,
                ),
            )
            .add_systems(
                Update,
                (
                    button_hover_feedback,
                    handle_actions,
                    refresh_param_values.run_if(in_state(AppScreen::Setup)),
                    poll_generation.run_if(resource_exists::<GenerationTask>),
                    (
                        update_hovered_hex,
                        handle_map_hex_click,
                        inspector_hotkeys,
                        viewer_escape,
                        handle_inspector_tabs,
                        refresh_tab_colors,
                        update_hex_tooltip,
                        update_hex_inspector,
                        timeline_keyboard,
                        timeline_playback,
                        refresh_hud,
                    )
                        .chain()
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
/// Inspector tab buttons manage their own colors.
type ChangedButtons<'w, 's, T> = Query<
    'w,
    's,
    (&'static Interaction, T),
    (
        Changed<Interaction>,
        With<Button>,
        Without<crate::hex_inspect::InspectorTabButton>,
    ),
>;

fn despawn_screen(mut commands: Commands, roots: Query<Entity, With<ScreenRoot>>) {
    for root in &roots {
        commands.entity(root).despawn();
    }
}

/// Removes the world, its chunk entities, and any in-flight generation when
/// leaving the viewer. Dropping the receiver makes the orphaned generation
/// thread's sends fail silently; it drains its tick loop and exits.
fn teardown_world(
    mut commands: Commands,
    mut cache: ResMut<HexEntityCache>,
    mut index: ResMut<HexMeshIndex>,
) {
    for entity in cache.entities.drain(..) {
        commands.entity(entity).despawn();
    }
    index.clear();
    commands.remove_resource::<WorldResource>();
    commands.remove_resource::<WorldTimeline>();
    commands.remove_resource::<GenerationTask>();
    // Selection outline is despawned when SelectedHex clears.
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

const SETUP_PARAMS: [(Param, &str); 10] = [
    (Param::Seed, "Seed"),
    (Param::SubdivisionLevel, "Detail (subdivision level)"),
    (Param::TargetYear, "Simulate to year"),
    (Param::LandFraction, "Land coverage %"),
    (Param::Mountains, "Mountains"),
    (Param::Islands, "Islands"),
    (Param::MajorPlates, "Major plates"),
    (Param::MinorPlates, "Minor plates"),
    (Param::ContinentalFraction, "Continental crust seed %"),
    (Param::WaterInventory, "Water inventory (GEL m)"),
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
        Param::ContinentalFraction => format!("{:.0}%", config.continental_fraction * 100.0),
        Param::WaterInventory => format!("{:.0}", config.water_inventory_gel_m),
        Param::LandFraction => format!("{:.0}%", config.land_fraction * 100.0),
        Param::Mountains => format!("{:.2}x", config.orogeny_intensity),
        Param::Islands => format!("{:.1}x", config.island_density),
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
        Param::ContinentalFraction => {
            // Steps of 2 percentage points; 22% is the Hadean-ish default,
            // ~29% present-day Earth.
            let steps = (config.continental_fraction * 50.0).round() + f32::from(direction);
            config.continental_fraction = (steps / 50.0).clamp(0.05, 0.60);
        }
        Param::WaterInventory => {
            let next = config.water_inventory_gel_m + f32::from(direction) * 250.0;
            config.water_inventory_gel_m = next.clamp(500.0, 6000.0);
        }
        Param::LandFraction => {
            // Steps of 2 percentage points; the solved land coverage target.
            let steps = (config.land_fraction * 50.0).round() + f32::from(direction);
            config.land_fraction = (steps / 50.0).clamp(0.05, 0.95);
        }
        Param::Mountains => {
            let next = config.orogeny_intensity + f32::from(direction) * 0.25;
            config.orogeny_intensity = next.clamp(0.0, 3.0);
        }
        Param::Islands => {
            let next = config.island_density + f32::from(direction) * 0.5;
            config.island_density = next.clamp(0.0, 3.0);
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
            parent.spawn(label("Generating world...", 30.0));
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
        // Progress throttling and frame striding happen inside the generator;
        // worst-case channel backlog is the frame memory budget (Doc 05 §A).
        generate_world_streaming(&config, |event| {
            let _ = tx.send(event);
        });
    });
    commands.insert_resource(GenerationTask(Mutex::new(rx)));
}

#[allow(clippy::too_many_arguments)]
fn poll_generation(
    mut commands: Commands,
    task: Option<Res<GenerationTask>>,
    screen: Res<State<AppScreen>>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut world_dirty: ResMut<WorldDirty>,
    mut colors_dirty: ResMut<ColorsDirty>,
    config: Res<ActiveConfig>,
    timeline: Option<ResMut<WorldTimeline>>,
    world_res: Option<ResMut<WorldResource>>,
    mut bar: Query<&mut Node, With<ProgressBarFill>>,
    mut progress_text: Query<&mut Text, With<ProgressText>>,
) {
    let Some(task) = task else {
        return;
    };
    let Ok(rx) = task.0.lock() else {
        return;
    };
    let mut timeline = timeline;
    let mut world_res = world_res;

    // Deferred apply: a frame landed before the freshly inserted WorldResource
    // was visible to this system (commands apply between frames).
    if let (Some(timeline), Some(world_res)) = (timeline.as_mut(), world_res.as_mut())
        && timeline.needs_apply
        && let Some(frame) = timeline.frames.get(timeline.current)
    {
        frame.apply(&mut world_res.0.data);
        colors_dirty.0 = true;
        timeline.needs_apply = false;
    }

    for event in rx.try_iter() {
        match event {
            GenEvent::Stage(stage) => {
                if let Ok(mut text) = progress_text.single_mut() {
                    text.0 = stage.to_string();
                }
            }
            GenEvent::Progress { year, target } => {
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
            GenEvent::InitialWorld(world) => {
                commands.insert_resource(WorldResource(*world));
                world_res = None; // stale handle; re-fetched next frame
                commands.insert_resource(WorldTimeline {
                    frames: Vec::new(),
                    current: 0,
                    // Play from year 0 as history streams in (YouTube-style).
                    playing: true,
                    play_timer: Timer::from_seconds(0.25, TimerMode::Repeating),
                    target_year: config.0.target_year.max(1),
                    complete: false,
                    needs_apply: false,
                });
                timeline = None;
                world_dirty.0 = true;
            }
            GenEvent::Frame(frame) => {
                let Some(timeline) = timeline.as_mut() else {
                    continue;
                };
                let first = timeline.frames.is_empty();
                timeline.frames.push(*frame);
                if first {
                    if let Some(world_res) = world_res.as_mut()
                        && let Some(frame) = timeline.frames.first()
                    {
                        frame.apply(&mut world_res.0.data);
                        colors_dirty.0 = true;
                    } else {
                        timeline.needs_apply = true;
                    }
                    // The world is visible from its first buffered year on;
                    // the rest of history streams in behind the viewer.
                    if *screen.get() == AppScreen::Generating {
                        next_screen.set(AppScreen::Viewing);
                    }
                }
            }
            GenEvent::Done { .. } => {
                if let Some(timeline) = timeline.as_mut() {
                    timeline.complete = true;
                }
                commands.remove_resource::<GenerationTask>();
            }
            GenEvent::Failed(err) => {
                if let Ok(mut text) = progress_text.single_mut() {
                    text.0 = format!("generation failed: {err} - Esc to return");
                }
                if let Some(timeline) = timeline.as_mut() {
                    timeline.complete = true;
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
            FocusPolicy::Pass,
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
                    BlocksMapPick,
                    FocusPolicy::Block,
                    Interaction::default(),
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
                                // Dim buffered region (grows as frames stream in)...
                                bar.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(0.0),
                                        top: Val::Px(0.0),
                                        width: Val::Percent(0.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.24, 0.34, 0.48)),
                                    TimelineBufferedFill,
                                ));
                                // ...under the bright playhead position.
                                bar.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(0.0),
                                        top: Val::Px(0.0),
                                        width: Val::Percent(0.0),
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
    rivers_dirty: &mut RiversDirty,
) {
    if let Some(frame) = timeline.frames.get(timeline.current) {
        frame.apply(&mut world_res.0.data);
        colors_dirty.0 = true;
        rivers_dirty.dirty = true;
    }
}

fn timeline_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut repeat: ResMut<ScrubRepeat>,
    timeline: Option<ResMut<WorldTimeline>>,
    world_res: Option<ResMut<WorldResource>>,
    mut colors_dirty: ResMut<ColorsDirty>,
    mut rivers_dirty: ResMut<RiversDirty>,
) {
    let (Some(mut timeline), Some(mut world_res)) = (timeline, world_res) else {
        return;
    };
    if keys.just_pressed(KeyCode::Space) {
        timeline.playing = !timeline.playing;
    }

    let held: i64 = if keys.pressed(KeyCode::ArrowLeft) {
        -1
    } else if keys.pressed(KeyCode::ArrowRight) {
        1
    } else {
        repeat.0 = Timer::from_seconds(SCRUB_INITIAL_DELAY_S, TimerMode::Once);
        return;
    };

    // Step immediately on press, then repeat while held after an initial delay.
    let step_now =
        if keys.just_pressed(KeyCode::ArrowLeft) || keys.just_pressed(KeyCode::ArrowRight) {
            repeat.0 = Timer::from_seconds(SCRUB_INITIAL_DELAY_S, TimerMode::Once);
            true
        } else {
            repeat.0.tick(time.delta());
            if repeat.0.is_finished() {
                repeat.0 = Timer::from_seconds(SCRUB_REPEAT_INTERVAL_S, TimerMode::Once);
                true
            } else {
                false
            }
        };

    if step_now {
        timeline.playing = false;
        step_timeline(&mut timeline, held);
        apply_current_frame(
            &timeline,
            &mut world_res,
            &mut colors_dirty,
            &mut rivers_dirty,
        );
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
    mut rivers_dirty: ResMut<RiversDirty>,
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
        // At the live edge: stall (stay playing) while frames still stream in,
        // like a video buffering; only stop at the true end of history.
        if timeline.complete {
            timeline.playing = false;
        }
        return;
    }
    timeline.current += 1;
    apply_current_frame(
        &timeline,
        &mut world_res,
        &mut colors_dirty,
        &mut rivers_dirty,
    );
}

#[allow(clippy::type_complexity)]
fn refresh_hud(
    timeline: Option<Res<WorldTimeline>>,
    mode: Res<CurrentRenderMode>,
    mut hud: Query<&mut Text, With<HudText>>,
    mut bars: ParamSet<(
        Query<&mut Node, With<TimelineBarFill>>,
        Query<&mut Node, With<TimelineBufferedFill>>,
    )>,
) {
    let Some(timeline) = timeline else {
        return;
    };
    let Some(frame) = timeline.frames.get(timeline.current) else {
        return;
    };
    let target = timeline.target_year.max(1) as f32;
    let buffered_year = timeline.frames.last().map(|f| f.year).unwrap_or(0);

    if let Ok(mut text) = hud.single_mut() {
        let generating = if timeline.complete {
            String::new()
        } else {
            format!(
                "Generating... {} / {} buffered  |  ",
                format_year(buffered_year),
                format_year(timeline.target_year)
            )
        };
        text.0 = format!(
            "{generating}Year {}  |  Mode: {} [M]  |  scrub/hold < >, Space plays, Esc for menu",
            format_year(frame.year),
            mode.0.label(),
        );
    }
    // Both widths are year-based so the playhead sits correctly inside the
    // buffered region even with uneven frame strides.
    if let Ok(mut node) = bars.p0().single_mut() {
        node.width = Val::Percent((frame.year as f32 / target * 100.0).clamp(0.0, 100.0));
    }
    if let Ok(mut node) = bars.p1().single_mut() {
        node.width = Val::Percent((buffered_year as f32 / target * 100.0).clamp(0.0, 100.0));
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
    rivers_dirty: Option<ResMut<RiversDirty>>,
) {
    let mut timeline = timeline;
    let mut world_res = world_res;
    let mut colors_dirty = colors_dirty;
    let mut rivers_dirty = rivers_dirty;

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
                if let (Some(timeline), Some(world_res), Some(colors_dirty), Some(rivers_dirty)) = (
                    timeline.as_mut(),
                    world_res.as_mut(),
                    colors_dirty.as_mut(),
                    rivers_dirty.as_mut(),
                ) {
                    timeline.playing = false;
                    step_timeline(timeline, step);
                    apply_current_frame(timeline, world_res, colors_dirty, rivers_dirty);
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
        AppScreen::Setup => next_screen.set(AppScreen::MainMenu),
        // Viewing Esc is handled in the viewing chain (`viewer_escape`).
        AppScreen::Viewing => {}
        AppScreen::Generating => {
            // Generation threads cannot be safely cancelled mid-tick; let the
            // run finish in the background and return to the menu.
            next_screen.set(AppScreen::MainMenu);
        }
    }
}
