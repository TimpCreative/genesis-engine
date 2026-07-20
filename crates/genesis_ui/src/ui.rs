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
use genesis_core::data::BiomeId;
use genesis_render::{
    ActiveBiologyView, ColorsDirty, CurrentRenderMode, HexEntityCache, HexMeshIndex, RenderMode,
    RiversDirty, SelectedHex, WorldDirty, WorldResource, biome_color, heatmap_color,
    precipitation_to_color, regime_to_color, soil_class_color, temperature_to_color,
};

use crate::biology_view::StubBiologyView;
use crate::hex_inspect::{
    BlocksMapPick, HoveredHex, InspectorTab, InspectorVisible, PendingMenuQuit,
    clear_inspect_on_exit, despawn_hex_inspect_ui, handle_inspector_tabs, handle_map_hex_click,
    inspector_hotkeys, refresh_tab_colors, spawn_hex_inspect_ui, update_hex_inspector,
    update_hex_tooltip, update_hovered_hex,
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
    SelectTab(SetupTab),
    RandomizeSeed,
    ConfirmQuit,
    CancelQuit,
    SetRenderMode(RenderMode),
    JumpToYear(i64),
    ToggleBestiary,
    ToggleTree,
}

/// Which full-screen overlay is open over the map (Prep-09 §7–§8).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenOverlay {
    #[default]
    None,
    Bestiary,
    Tree,
}

/// Root of the Bestiary overlay; its content is rebuilt on open.
#[derive(Component)]
pub struct BestiaryOverlay;
#[derive(Component)]
pub struct BestiaryContent;
/// Root of the Tree-of-Life overlay; content rebuilt on open.
#[derive(Component)]
pub struct TreeOverlay;
#[derive(Component)]
pub struct TreeContent;
/// True while an overlay's content matches the current world/hex/year.
#[derive(Resource, Default)]
pub struct OverlayBuilt(pub bool);

/// Marks a top-bar layer-selector tab for its render mode (active highlight).
#[derive(Component)]
pub struct ModeTab(pub RenderMode);

/// Marks the top-bar year + geological-era readout text.
#[derive(Component)]
pub struct TopBarStatusText;

/// Geological eon for a simulation year (year 0 = formation), with a band color
/// for the top bar and the timeline strip (Prep-09 §5.2). Reused by Prep9-3.
pub fn geological_era(year: i64) -> (&'static str, Color) {
    if year < 500_000_000 {
        ("Hadean", Color::srgb(0.42, 0.20, 0.20))
    } else if year < 2_000_000_000 {
        ("Archean", Color::srgb(0.45, 0.35, 0.22))
    } else if year < 4_000_000_000 {
        ("Proterozoic", Color::srgb(0.22, 0.42, 0.38))
    } else {
        ("Phanerozoic", Color::srgb(0.24, 0.40, 0.55))
    }
}

/// Setup-screen parameter groups, so the world recipe stays organized as knobs
/// grow. Ordered left-to-right in the tab bar.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SetupTab {
    #[default]
    World,
    Terrain,
    Climate,
}

impl SetupTab {
    pub const ALL: [SetupTab; 3] = [SetupTab::World, SetupTab::Terrain, SetupTab::Climate];

    pub fn label(self) -> &'static str {
        match self {
            SetupTab::World => "World",
            SetupTab::Terrain => "Terrain",
            SetupTab::Climate => "Climate",
        }
    }
}

/// Active setup tab; drives which parameter rows are visible.
#[derive(Resource, Default)]
pub struct ActiveSetupTab(pub SetupTab);

/// Tags a setup-screen parameter row with the tab it belongs to.
#[derive(Component)]
pub struct TabRow(pub SetupTab);

/// Number of pre-spawned legend rows (max entries any render mode uses — Soil
/// has the most: 9 classes + water).
const LEGEND_ROWS: usize = 10;

/// True until the user's next keystroke replaces the seed, so typing on a
/// freshly-shown or just-randomized seed starts a new one instead of appending
/// to it.
#[derive(Resource)]
pub struct SeedInputFresh(pub bool);

impl Default for SeedInputFresh {
    fn default() -> Self {
        Self(true)
    }
}

/// Whether the viewing-screen legend is shown (toggle with [L]).
#[derive(Resource)]
pub struct LegendVisible(pub bool);

impl Default for LegendVisible {
    fn default() -> Self {
        Self(true)
    }
}

/// Root of the "return to menu?" confirm overlay (toggled by `PendingMenuQuit`).
#[derive(Component)]
pub struct QuitConfirmOverlay;

/// Viewing-screen legend markers — rows are pre-spawned and updated per mode.
#[derive(Component)]
pub struct LegendPanel;
#[derive(Component)]
pub struct LegendTitle;
#[derive(Component)]
pub struct LegendRow(pub usize);
#[derive(Component)]
pub struct LegendSwatch(pub usize);
#[derive(Component)]
pub struct LegendLabel(pub usize);

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

/// Container for the geological era bands (behind the timeline fills).
#[derive(Component)]
pub struct EraBandStrip;
/// Container for event pips (on top of the timeline fills).
#[derive(Component)]
pub struct PipStrip;
/// True once the era bands + pips have been built for the current world.
#[derive(Resource, Default)]
pub struct TimelineMarksBuilt(pub bool);

/// Pip color by life-event category (Prep-09 §5.1).
fn pip_color(category: genesis_core::LifeEventCategory) -> Color {
    use genesis_core::LifeEventCategory as C;
    match category {
        C::Origin => Color::srgb(0.45, 0.85, 0.55),
        C::Innovation => Color::srgb(0.45, 0.70, 0.95),
        C::Extinction => Color::srgb(0.95, 0.45, 0.40),
        C::Milestone => Color::srgb(0.95, 0.82, 0.35),
    }
}

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
            .init_resource::<ActiveSetupTab>()
            .init_resource::<SeedInputFresh>()
            .init_resource::<LegendVisible>()
            .init_resource::<PendingMenuQuit>()
            .init_resource::<TimelineMarksBuilt>()
            .init_resource::<OpenOverlay>()
            .init_resource::<OverlayBuilt>()
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
                    (
                        refresh_param_values,
                        update_seed_display,
                        update_tab_visibility,
                        seed_text_input,
                        seed_clipboard,
                    )
                        .run_if(in_state(AppScreen::Setup)),
                    poll_generation.run_if(resource_exists::<GenerationTask>),
                    (
                        update_hovered_hex,
                        handle_map_hex_click,
                        inspector_hotkeys,
                        escape_ladder,
                        handle_inspector_tabs,
                        refresh_tab_colors,
                        update_hex_tooltip,
                        update_hex_inspector,
                        timeline_keyboard,
                        timeline_playback,
                        refresh_hud,
                        refresh_legend,
                        toggle_legend,
                        update_quit_overlay,
                        refresh_mode_tabs,
                        build_timeline_marks,
                        overlay_hotkeys,
                        update_overlays,
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

const SETUP_PARAMS: [(Param, &str, SetupTab); 10] = [
    (Param::Seed, "Seed", SetupTab::World),
    (
        Param::SubdivisionLevel,
        "Detail (subdivision level)",
        SetupTab::World,
    ),
    (Param::TargetYear, "Simulate to year", SetupTab::World),
    (Param::LandFraction, "Land coverage %", SetupTab::Terrain),
    (Param::Mountains, "Mountains", SetupTab::Terrain),
    (Param::Islands, "Islands", SetupTab::Terrain),
    (Param::MajorPlates, "Major plates", SetupTab::Terrain),
    (Param::MinorPlates, "Minor plates", SetupTab::Terrain),
    (
        Param::ContinentalFraction,
        "Continental crust seed %",
        SetupTab::Terrain,
    ),
    (
        Param::WaterInventory,
        "Total water (m deep if spread flat)",
        SetupTab::Climate,
    ),
];

fn spawn_setup_screen(
    mut commands: Commands,
    active_tab: Res<ActiveSetupTab>,
    mut seed_fresh: ResMut<SeedInputFresh>,
) {
    // Next keystroke starts a fresh seed rather than appending to the shown one.
    seed_fresh.0 = true;
    let current_tab = active_tab.0;
    commands
        .spawn(full_screen_root(AppScreen::Setup))
        .with_children(|parent| {
            parent.spawn(label("New World", 36.0));

            // Tab bar — one button per group; rows below toggle visibility.
            parent
                .spawn(Node {
                    column_gap: Val::Px(8.0),
                    margin: UiRect::vertical(Val::Px(12.0)),
                    ..default()
                })
                .with_children(|bar| {
                    for tab in SetupTab::ALL {
                        bar.spawn(button(UiAction::SelectTab(tab)))
                            .with_children(|b| {
                                b.spawn(label(tab.label(), 18.0));
                            });
                    }
                });

            for (param, name, tab) in SETUP_PARAMS {
                // Display::None (not Visibility::Hidden) so inactive rows take no
                // layout space — otherwise the hidden rows leave large gaps.
                let display = if tab == current_tab {
                    Display::Flex
                } else {
                    Display::None
                };
                parent
                    .spawn((
                        Node {
                            display,
                            column_gap: Val::Px(10.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        TabRow(tab),
                    ))
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
                        if param == Param::Seed {
                            // Typed hex value + Random button (no +/- counter).
                            row.spawn((
                                label("", 18.0).0,
                                label("", 18.0).1,
                                TextColor(ACCENT),
                                ParamValueText(param),
                                Node {
                                    width: Val::Px(180.0),
                                    justify_content: JustifyContent::Center,
                                    ..default()
                                },
                            ));
                            row.spawn(button(UiAction::RandomizeSeed))
                                .with_children(|b| {
                                    b.spawn(label("Random", 16.0));
                                });
                        } else {
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
                        }
                    });
            }
            {
                let hint =
                    "Seed: type 0-9 a-f  ·  Backspace  ·  Cmd/Ctrl+C/V copy-paste  ·  Random";
                parent.spawn((
                    label(hint, 14.0).0,
                    label(hint, 14.0).1,
                    TextColor(Color::srgb(0.6, 0.6, 0.65)),
                ));
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
        Param::Seed => {
            if config.seed_text.is_empty() {
                "(type or Random)".to_string()
            } else {
                config.seed_text.clone()
            }
        }
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
    match param {
        // Seed is a typed hex field with a Random button, not a +/- counter
        // (see `seed_text_input` / `UiAction::RandomizeSeed`).
        Param::Seed => {}
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
        // The Seed field is owned by `update_seed_display` (blinking cursor).
        if param_text.0 == Param::Seed {
            continue;
        }
        text.0 = format_param(&config.0, param_text.0);
    }
}

/// Renders the seed value with a blinking text cursor so it reads as an editable
/// field (and is clearly receiving input).
fn update_seed_display(
    time: Res<Time>,
    config: Res<ActiveConfig>,
    mut labels: Query<(&ParamValueText, &mut Text)>,
) {
    let cursor = if time.elapsed_secs().fract() < 0.5 {
        "|"
    } else {
        " "
    };
    let shown = format!("{}{}", config.0.seed_text, cursor);
    for (param_text, mut text) in &mut labels {
        if param_text.0 == Param::Seed {
            text.0 = shown.clone();
        }
    }
}

/// Shows only the rows belonging to the active setup tab (via `Display`, so
/// inactive rows collapse instead of leaving gaps).
fn update_tab_visibility(active: Res<ActiveSetupTab>, mut rows: Query<(&TabRow, &mut Node)>) {
    if !active.is_changed() {
        return;
    }
    for (row, mut node) in &mut rows {
        node.display = if row.0 == active.0 {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// Hex keys accepted in the seed field, mapped to their character.
const SEED_HEX_KEYS: [(KeyCode, char); 16] = [
    (KeyCode::Digit0, '0'),
    (KeyCode::Digit1, '1'),
    (KeyCode::Digit2, '2'),
    (KeyCode::Digit3, '3'),
    (KeyCode::Digit4, '4'),
    (KeyCode::Digit5, '5'),
    (KeyCode::Digit6, '6'),
    (KeyCode::Digit7, '7'),
    (KeyCode::Digit8, '8'),
    (KeyCode::Digit9, '9'),
    (KeyCode::KeyA, 'a'),
    (KeyCode::KeyB, 'b'),
    (KeyCode::KeyC, 'c'),
    (KeyCode::KeyD, 'd'),
    (KeyCode::KeyE, 'e'),
    (KeyCode::KeyF, 'f'),
];

/// Maximum seed-string length (plenty of entropy; keeps the field tidy).
const SEED_MAX_LEN: usize = 16;

/// Types hex characters into the seed field on the setup screen (validated
/// charset only; Backspace deletes). Only mutates the config when a relevant key
/// fired, so change detection stays quiet otherwise.
fn seed_text_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut config: ResMut<ActiveConfig>,
    mut fresh: ResMut<SeedInputFresh>,
) {
    // Cmd/Ctrl combos (copy/paste) are handled by `seed_clipboard`; don't also
    // type their letter (e.g. Cmd+C would otherwise insert a hex 'c').
    if seed_modifier_held(&keys) {
        return;
    }
    let backspace = keys.just_pressed(KeyCode::Backspace);
    let typed = SEED_HEX_KEYS
        .iter()
        .find(|(code, _)| keys.just_pressed(*code))
        .map(|(_, ch)| *ch);
    if !backspace && typed.is_none() {
        return;
    }
    let seed = &mut config.0.seed_text;
    if let Some(ch) = typed {
        // First keystroke after the screen loaded or Random was clicked starts a
        // brand-new seed instead of appending to the shown one.
        if fresh.0 {
            seed.clear();
            fresh.0 = false;
        }
        if seed.len() < SEED_MAX_LEN {
            seed.push(ch);
        }
    } else if backspace {
        fresh.0 = false;
        seed.pop();
    }
}

/// Whether a copy/paste modifier (Cmd on macOS, Ctrl elsewhere) is held.
fn seed_modifier_held(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::SuperLeft)
        || keys.pressed(KeyCode::SuperRight)
        || keys.pressed(KeyCode::ControlLeft)
        || keys.pressed(KeyCode::ControlRight)
}

/// Copy (Cmd/Ctrl+C) the seed to the clipboard and paste (Cmd/Ctrl+V) a seed
/// from it (hex-filtered), so worlds can be shared by seed.
fn seed_clipboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut config: ResMut<ActiveConfig>,
    mut fresh: ResMut<SeedInputFresh>,
) {
    if !seed_modifier_held(&keys) {
        return;
    }
    if keys.just_pressed(KeyCode::KeyC) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(config.0.seed_text.clone());
        }
    } else if keys.just_pressed(KeyCode::KeyV) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            if let Ok(text) = clipboard.get_text() {
                let hex: String = text
                    .chars()
                    .filter(char::is_ascii_hexdigit)
                    .map(|c| c.to_ascii_lowercase())
                    .take(SEED_MAX_LEN)
                    .collect();
                if !hex.is_empty() {
                    config.0.seed_text = hex;
                    fresh.0 = false;
                }
            }
        }
    }
}

/// A fresh random hex seed string (time-seeded xorshift — variety, not crypto).
fn random_seed_string() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    format!("{x:016x}")
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
                // Prep-09 seam: register the stub biology view for this world.
                // Doc 09 swaps this line for the real `genesis_biology` adapter.
                let seed = world.data.parameters.core.seed.value;
                commands.insert_resource(ActiveBiologyView(Box::new(StubBiologyView::new(seed))));
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

fn spawn_viewing_hud(
    mut commands: Commands,
    mut pending_quit: ResMut<PendingMenuQuit>,
    mut marks_built: ResMut<TimelineMarksBuilt>,
    mut open_overlay: ResMut<OpenOverlay>,
) {
    pending_quit.0 = false;
    marks_built.0 = false; // rebuild era bands + pips for this world
    *open_overlay = OpenOverlay::None;
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
            // Top bar: layer selector (left) + year/era readout (right).
            parent
                .spawn((
                    BlocksMapPick,
                    FocusPolicy::Block,
                    Interaction::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(0.0),
                        left: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(4.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.9)),
                ))
                .with_children(|bar| {
                    for mode in RenderMode::ALL {
                        bar.spawn((button(UiAction::SetRenderMode(mode)), ModeTab(mode)))
                            .with_children(|b| {
                                b.spawn(label(mode.label(), 15.0));
                            });
                    }
                    // Spacer pushes the readout + overlay buttons to the right.
                    bar.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    bar.spawn((
                        label("", 15.0).0,
                        label("", 15.0).1,
                        TextColor(Color::srgb(0.85, 0.85, 0.9)),
                        TopBarStatusText,
                        Node {
                            margin: UiRect::right(Val::Px(10.0)),
                            ..default()
                        },
                    ));
                    bar.spawn(button(UiAction::ToggleTree)).with_children(|b| {
                        b.spawn(label("Tree of Life", 14.0));
                    });
                    bar.spawn(button(UiAction::ToggleBestiary))
                        .with_children(|b| {
                            b.spawn(label("Bestiary", 14.0));
                        });
                });

            // Full-screen Bestiary + Tree overlays (hidden; filled on open).
            for (is_bestiary, title) in [(true, "Bestiary"), (false, "Tree of Life")] {
                let mut overlay = parent.spawn((
                    BlocksMapPick,
                    FocusPolicy::Block,
                    Interaction::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(20.0)),
                        row_gap: Val::Px(12.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.04, 0.05, 0.07, 0.97)),
                ));
                if is_bestiary {
                    overlay.insert(BestiaryOverlay);
                } else {
                    overlay.insert(TreeOverlay);
                }
                overlay.with_children(|o| {
                    o.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|h| {
                        h.spawn(label(title, 28.0));
                        let close = if is_bestiary {
                            UiAction::ToggleBestiary
                        } else {
                            UiAction::ToggleTree
                        };
                        h.spawn(button(close)).with_children(|b| {
                            b.spawn(label("Close [Esc]", 16.0));
                        });
                    });
                    let mut content = o.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        align_content: AlignContent::FlexStart,
                        column_gap: Val::Px(10.0),
                        row_gap: Val::Px(10.0),
                        flex_grow: 1.0,
                        overflow: Overflow::clip(),
                        ..default()
                    });
                    if is_bestiary {
                        content.insert(BestiaryContent);
                    } else {
                        content.insert(TreeContent);
                    }
                });
            }

            // "Return to menu?" confirm overlay — hidden until Esc; blocks the
            // map so an accidental Esc can't discard the world.
            parent
                .spawn((
                    QuitConfirmOverlay,
                    BlocksMapPick,
                    FocusPolicy::Block,
                    Interaction::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        row_gap: Val::Px(16.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                ))
                .with_children(|modal| {
                    modal.spawn(label("Return to main menu?", 28.0));
                    modal.spawn((
                        label("This world will be discarded.", 16.0).0,
                        label("This world will be discarded.", 16.0).1,
                        TextColor(Color::srgb(0.82, 0.82, 0.88)),
                    ));
                    modal
                        .spawn(Node {
                            column_gap: Val::Px(12.0),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn(button(UiAction::ConfirmQuit)).with_children(|b| {
                                b.spawn(label("Return to menu", 20.0));
                            });
                            row.spawn(button(UiAction::CancelQuit)).with_children(|b| {
                                b.spawn(label("Keep exploring", 20.0));
                            });
                        });
                });

            // Color legend for the active render mode (top-right overlay, [L] toggles).
            parent
                .spawn((
                    LegendPanel,
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(12.0),
                        right: Val::Px(12.0),
                        // Fixed width so the panel is the same size in every mode
                        // (Soil has the most/longest labels).
                        width: Val::Px(230.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(10.0)),
                        row_gap: Val::Px(4.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.85)),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        label("", 15.0).0,
                        label("", 15.0).1,
                        TextColor(Color::WHITE),
                        LegendTitle,
                    ));
                    for i in 0..LEGEND_ROWS {
                        panel
                            .spawn((
                                Node {
                                    column_gap: Val::Px(8.0),
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                LegendRow(i),
                                Visibility::Hidden,
                            ))
                            .with_children(|row| {
                                row.spawn((
                                    Node {
                                        width: Val::Px(18.0),
                                        height: Val::Px(18.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::WHITE),
                                    LegendSwatch(i),
                                ));
                                row.spawn((
                                    label("", 14.0).0,
                                    label("", 14.0).1,
                                    TextColor(Color::srgb(0.85, 0.85, 0.9)),
                                    LegendLabel(i),
                                ));
                            });
                    }
                });
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
                                // Geological era bands (behind everything).
                                bar.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(0.0),
                                        top: Val::Px(0.0),
                                        width: Val::Percent(100.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    EraBandStrip,
                                ));
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
                                // Event pips (on top; filled by build_timeline_marks).
                                bar.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Px(0.0),
                                        top: Val::Px(0.0),
                                        width: Val::Percent(100.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    PipStrip,
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
            "{generating}Year {}  |  Mode: {} [M]  |  [L] legend  |  scrub/hold < >, Space plays, Esc for menu",
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

/// (swatch color, label) entries for the legend of a render mode. Colors come
/// from the same ramps the map uses, so the key matches what's on screen.
fn legend_entries(mode: RenderMode) -> Vec<(Color, &'static str)> {
    use genesis_core::data::ClimateRegimePlaceholder as Rg;
    let ice = Color::srgb(0.95, 0.97, 1.0);
    match mode {
        RenderMode::Elevation => vec![
            (Color::srgb(0.05, 0.12, 0.35), "Deep ocean"),
            (Color::srgb(0.20, 0.45, 0.70), "Shelf / shallow sea"),
            (Color::srgb(0.47, 0.63, 0.35), "Lowland"),
            (Color::srgb(0.55, 0.50, 0.30), "Highland"),
            (Color::srgb(0.90, 0.90, 0.92), "Mountain peaks"),
        ],
        RenderMode::Temperature => vec![
            (ice, "Ice / permafrost"),
            (temperature_to_color(-30.0), "Frozen"),
            (temperature_to_color(0.0), "Cold"),
            (temperature_to_color(15.0), "Mild"),
            (temperature_to_color(30.0), "Hot"),
            (temperature_to_color(45.0), "Very hot"),
        ],
        RenderMode::Precipitation => vec![
            (precipitation_to_color(50.0), "Arid"),
            (precipitation_to_color(400.0), "Semi-arid"),
            (precipitation_to_color(900.0), "Temperate"),
            (precipitation_to_color(1600.0), "Wet"),
            (precipitation_to_color(2300.0), "Very wet"),
        ],
        RenderMode::ClimateRegime => vec![
            (Color::srgb(0.08, 0.45, 0.60), "Ocean"),
            (regime_to_color(Rg::Tropical), "Tropical"),
            (regime_to_color(Rg::HotDesert), "Hot desert"),
            (regime_to_color(Rg::Mediterranean), "Mediterranean"),
            (regime_to_color(Rg::Temperate), "Temperate"),
            (regime_to_color(Rg::Boreal), "Boreal"),
            (regime_to_color(Rg::Tundra), "Tundra"),
            (ice, "Ice / polar"),
        ],
        RenderMode::Soil => {
            use genesis_core::data::SoilClass as S;
            // Representative fertility so the swatches match the map's tint,
            // including the barren (purple-grey) and saline (pink) classes.
            let f = 0.3;
            vec![
                (Color::srgb(0.08, 0.28, 0.55), "Water"),
                (soil_class_color(S::None, f), "Barren / no soil"),
                (soil_class_color(S::Sandy, f), "Sandy"),
                (soil_class_color(S::Loamy, f), "Loamy"),
                (soil_class_color(S::Alluvial, f), "Alluvial (floodplain)"),
                (soil_class_color(S::Loess, f), "Loess"),
                (soil_class_color(S::Volcanic, f), "Volcanic"),
                (soil_class_color(S::Calcareous, f), "Calcareous"),
                (soil_class_color(S::Peaty, f), "Peaty"),
                (soil_class_color(S::Saline, f), "Saline (salt)"),
            ]
        }
        RenderMode::Biome => vec![
            (biome_color(BiomeId::NONE), "Ocean"),
            (biome_color(BiomeId(0)), "Tropical rainforest"),
            (biome_color(BiomeId(1)), "Tropical savanna"),
            (biome_color(BiomeId(2)), "Hot desert"),
            (biome_color(BiomeId(4)), "Temperate forest"),
            (biome_color(BiomeId(5)), "Grassland"),
            (biome_color(BiomeId(6)), "Boreal forest"),
            (biome_color(BiomeId(7)), "Tundra"),
            (biome_color(BiomeId(9)), "Wetland"),
            (biome_color(BiomeId(10)), "Alpine"),
        ],
        RenderMode::Biomass => vec![
            (heatmap_color(0.05), "Barren"),
            (heatmap_color(0.30), "Sparse"),
            (heatmap_color(0.55), "Moderate"),
            (heatmap_color(0.80), "Rich"),
            (heatmap_color(1.0), "Lush"),
        ],
        RenderMode::Diversity => vec![
            (heatmap_color(0.05), "Depauperate"),
            (heatmap_color(0.35), "Low"),
            (heatmap_color(0.60), "Moderate"),
            (heatmap_color(0.85), "High"),
            (heatmap_color(1.0), "Hyperdiverse"),
        ],
        RenderMode::Society => vec![(Color::srgb(0.30, 0.30, 0.34), "Not simulated (Doc 10)")],
    }
}

/// Repaints the legend when the render mode changes.
#[allow(clippy::type_complexity)]
fn refresh_legend(
    mode: Res<CurrentRenderMode>,
    mut title: Query<&mut Text, (With<LegendTitle>, Without<LegendLabel>)>,
    mut rows: Query<(&LegendRow, &mut Visibility)>,
    mut swatches: Query<(&LegendSwatch, &mut BackgroundColor)>,
    mut labels: Query<(&LegendLabel, &mut Text), Without<LegendTitle>>,
) {
    // Cheap enough to refresh every frame (8 rows), which also covers the first
    // frame after entering the viewer without special-casing initialization.
    let entries = legend_entries(mode.0);
    if let Ok(mut text) = title.single_mut() {
        text.0 = format!("{} key   [L] hide", mode.0.label());
    }
    for (row, mut vis) in &mut rows {
        *vis = if row.0 < entries.len() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    for (swatch, mut bg) in &mut swatches {
        if let Some((color, _)) = entries.get(swatch.0) {
            bg.0 = *color;
        }
    }
    for (lbl, mut text) in &mut labels {
        if let Some((_, name)) = entries.get(lbl.0) {
            text.0 = (*name).to_string();
        }
    }
}

/// Shows/hides the "return to menu?" confirm overlay from `PendingMenuQuit`.
fn update_quit_overlay(
    pending: Res<PendingMenuQuit>,
    mut overlay: Query<&mut Node, With<QuitConfirmOverlay>>,
) {
    if !pending.is_changed() {
        return;
    }
    if let Ok(mut node) = overlay.single_mut() {
        node.display = if pending.0 {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// Highlights the active layer-selector tab and refreshes the top-bar year/era.
fn refresh_mode_tabs(
    mode: Res<CurrentRenderMode>,
    timeline: Option<Res<WorldTimeline>>,
    mut tabs: Query<(&ModeTab, &mut BackgroundColor)>,
    mut status: Query<&mut Text, With<TopBarStatusText>>,
) {
    let active = Color::srgb(0.20, 0.32, 0.48);
    for (tab, mut bg) in &mut tabs {
        bg.0 = if tab.0 == mode.0 { active } else { BUTTON_BG };
    }
    if let Some(tl) = timeline {
        if let (Ok(mut text), Some(frame)) = (status.single_mut(), tl.frames.get(tl.current)) {
            let (era, _) = geological_era(frame.year);
            text.0 = format!("{}  ·  {}", format_year(frame.year), era);
        }
    }
}

/// Builds the era bands + event pips once the timeline is ready (Prep-09 §5).
#[allow(clippy::type_complexity)]
fn build_timeline_marks(
    mut commands: Commands,
    mut built: ResMut<TimelineMarksBuilt>,
    timeline: Option<Res<WorldTimeline>>,
    biology: Option<Res<ActiveBiologyView>>,
    era_strip: Query<Entity, With<EraBandStrip>>,
    pip_strip: Query<Entity, With<PipStrip>>,
) {
    if built.0 {
        return;
    }
    let Some(tl) = timeline else {
        return;
    };
    let target = tl.target_year.max(1);
    let (Ok(era_e), Ok(pip_e)) = (era_strip.single(), pip_strip.single()) else {
        return;
    };

    // Geological era bands, clamped to the run's target year.
    let bounds = [0i64, 500_000_000, 2_000_000_000, 4_000_000_000, i64::MAX];
    commands.entity(era_e).with_children(|s| {
        for w in bounds.windows(2) {
            let (start, end) = (w[0], w[1].min(target));
            if start >= target {
                continue;
            }
            let (_, color) = geological_era(start);
            let left = (start as f32 / target as f32 * 100.0).clamp(0.0, 100.0);
            let width = ((end - start) as f32 / target as f32 * 100.0).clamp(0.0, 100.0);
            s.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(left),
                    top: Val::Px(0.0),
                    width: Val::Percent(width),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(color.with_alpha(0.5)),
            ));
        }
    });

    // Life-event pips (stub now; physical event pips join when the event stream
    // is wired, Prep-09 §5.1). Each is a click-to-jump marker.
    if let Some(bio) = biology.as_ref() {
        let events = bio
            .0
            .life_events(genesis_core::time::WorldYear(0), genesis_core::time::WorldYear(target));
        commands.entity(pip_e).with_children(|s| {
            for ev in events {
                let left = (ev.year as f32 / target as f32 * 100.0).clamp(0.0, 100.0);
                s.spawn((
                    Button,
                    UiAction::JumpToYear(ev.year),
                    Interaction::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(left),
                        top: Val::Px(-4.0),
                        width: Val::Px(10.0),
                        height: Val::Px(10.0),
                        ..default()
                    },
                    BackgroundColor(pip_color(ev.category)),
                ));
            }
        });
    }
    built.0 = true;
}

/// The Viewing-screen Esc ladder (Prep-09 §3): a full-screen overlay closes
/// first, then a hex selection clears, then the "return to menu?" confirm opens
/// (a second Esc dismisses it) — so nothing is ever discarded by a single Esc.
fn escape_ladder(
    keys: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<OpenOverlay>,
    mut selected: ResMut<SelectedHex>,
    mut pending: ResMut<PendingMenuQuit>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if *open != OpenOverlay::None {
        *open = OpenOverlay::None;
    } else if selected.0.is_some() {
        selected.0 = None;
    } else {
        pending.0 = !pending.0;
    }
}

/// [B]/[T] toggle the Bestiary / Tree overlays.
fn overlay_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<OpenOverlay>,
    mut built: ResMut<OverlayBuilt>,
) {
    if keys.just_pressed(KeyCode::KeyB) {
        *open = if *open == OpenOverlay::Bestiary {
            OpenOverlay::None
        } else {
            OpenOverlay::Bestiary
        };
        built.0 = false;
    }
    if keys.just_pressed(KeyCode::KeyT) {
        *open = if *open == OpenOverlay::Tree {
            OpenOverlay::None
        } else {
            OpenOverlay::Tree
        };
        built.0 = false;
    }
}

/// Shows/hides the Bestiary + Tree overlays and (re)builds their content from
/// the active `BiologyView` when opened (Prep-09 §7–§8).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_overlays(
    open: Res<OpenOverlay>,
    selected: Res<SelectedHex>,
    mut built: ResMut<OverlayBuilt>,
    mut commands: Commands,
    world_res: Option<Res<WorldResource>>,
    biology: Option<Res<ActiveBiologyView>>,
    timeline: Option<Res<WorldTimeline>>,
    children: Query<&Children>,
    mut bestiary: Query<&mut Node, (With<BestiaryOverlay>, Without<TreeOverlay>)>,
    mut tree: Query<&mut Node, (With<TreeOverlay>, Without<BestiaryOverlay>)>,
    bestiary_content: Query<Entity, With<BestiaryContent>>,
    tree_content: Query<Entity, With<TreeContent>>,
) {
    if open.is_changed() {
        if let Ok(mut n) = bestiary.single_mut() {
            n.display = if *open == OpenOverlay::Bestiary {
                Display::Flex
            } else {
                Display::None
            };
        }
        if let Ok(mut n) = tree.single_mut() {
            n.display = if *open == OpenOverlay::Tree {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
    if selected.is_changed() && *open == OpenOverlay::Bestiary {
        built.0 = false;
    }
    if built.0 || *open == OpenOverlay::None {
        return;
    }
    let Some(wr) = world_res else {
        return;
    };
    let Some(bio) = biology else {
        return;
    };
    let data = &wr.0.data;

    let clear = |commands: &mut Commands, entity: Entity| {
        if let Ok(kids) = children.get(entity) {
            for k in kids.iter() {
                commands.entity(k).despawn();
            }
        }
    };

    match *open {
        OpenOverlay::Bestiary => {
            let Ok(content) = bestiary_content.single() else {
                return;
            };
            clear(&mut commands, content);
            let assemblage = selected
                .0
                .map(|h| bio.0.assemblage(data, h))
                .unwrap_or_default();
            commands.entity(content).with_children(|c| {
                if assemblage.species.is_empty() {
                    c.spawn(label(
                        "Select a land hex on the map, then open the Bestiary.",
                        18.0,
                    ));
                    return;
                }
                for sp in &assemblage.species {
                    c.spawn((
                        Node {
                            width: Val::Px(240.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(10.0)),
                            row_gap: Val::Px(4.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.10, 0.12, 0.16, 0.95)),
                    ))
                    .with_children(|card| {
                        card.spawn(label(&sp.name, 18.0));
                        card.spawn((
                            label(&format!("{} · {}", sp.guild, sp.family), 13.0).0,
                            label("", 13.0).1,
                            TextColor(Color::srgb(0.70, 0.80, 0.95)),
                        ));
                        card.spawn((
                            label(&format!("[{}]", sp.trait_chips.join(", ")), 12.0).0,
                            label("", 12.0).1,
                            TextColor(Color::srgb(0.65, 0.65, 0.72)),
                        ));
                        card.spawn((
                            label(&sp.description, 12.0).0,
                            label("", 12.0).1,
                            TextColor(Color::srgb(0.80, 0.80, 0.84)),
                        ));
                    });
                }
            });
            built.0 = true;
        }
        OpenOverlay::Tree => {
            let Ok(content) = tree_content.single() else {
                return;
            };
            clear(&mut commands, content);
            build_tree_content(&mut commands, content, bio.0.as_ref(), timeline.as_deref());
            built.0 = true;
        }
        OpenOverlay::None => {}
    }
}

/// Builds the Tree-of-Life overlay content: an indented, time-aware branch list
/// from `tree_snapshot(current_year)` — extinct branches greyed (Prep-09 §7).
fn build_tree_content(
    commands: &mut Commands,
    content: Entity,
    view: &dyn genesis_core::BiologyView,
    timeline: Option<&WorldTimeline>,
) {
    let year = timeline
        .and_then(|t| t.frames.get(t.current))
        .map(|f| f.year)
        .unwrap_or(4_500_000_000);
    let tree = view.tree_snapshot(genesis_core::time::WorldYear(year));
    let depth = |rank: &str| match rank {
        "root" => 0.0,
        "kingdom" => 1.0,
        _ => 2.0,
    };
    commands.entity(content).with_children(|c| {
        c.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|col| {
            col.spawn(label(
                &format!(
                    "As of {}  ·  {} living branches",
                    format_year(year),
                    tree.nodes.iter().filter(|n| n.extinction_year.is_none()).count()
                ),
                15.0,
            ));
            for node in &tree.nodes {
                let extinct = node.extinction_year.is_some();
                let color = if extinct {
                    Color::srgb(0.48, 0.48, 0.52)
                } else {
                    Color::srgb(0.85, 0.92, 0.86)
                };
                let suffix = if extinct { "  (extinct)" } else { "" };
                col.spawn((
                    label(
                        &format!(
                            "{} · {} · {}{}",
                            node.name, node.rank, node.defining_trait, suffix
                        ),
                        14.0,
                    )
                    .0,
                    label("", 14.0).1,
                    TextColor(color),
                    Node {
                        margin: UiRect::left(Val::Px(depth(&node.rank) * 22.0)),
                        ..default()
                    },
                ));
            }
        });
    });
}

/// [L] toggles the legend; the panel's `Display` follows `LegendVisible`.
fn toggle_legend(
    keys: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<LegendVisible>,
    mut panel: Query<&mut Node, With<LegendPanel>>,
) {
    if keys.just_pressed(KeyCode::KeyL) {
        visible.0 = !visible.0;
    }
    if visible.is_changed() {
        if let Ok(mut node) = panel.single_mut() {
            node.display = if visible.0 {
                Display::Flex
            } else {
                Display::None
            };
        }
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
    mut active_tab: ResMut<ActiveSetupTab>,
    mut seed_fresh: ResMut<SeedInputFresh>,
    mut pending_quit: ResMut<PendingMenuQuit>,
    mut render_mode: ResMut<CurrentRenderMode>,
    mut open_overlay: ResMut<OpenOverlay>,
    mut overlay_built: ResMut<OverlayBuilt>,
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
            UiAction::SelectTab(tab) => {
                active_tab.0 = tab;
            }
            UiAction::RandomizeSeed => {
                config.0.seed_text = random_seed_string();
                // Next keystroke starts fresh rather than appending to the roll.
                seed_fresh.0 = true;
            }
            UiAction::ConfirmQuit => {
                pending_quit.0 = false;
                next_screen.set(AppScreen::MainMenu);
            }
            UiAction::CancelQuit => {
                pending_quit.0 = false;
            }
            UiAction::SetRenderMode(mode) => {
                render_mode.0 = mode;
                if let Some(cd) = colors_dirty.as_mut() {
                    cd.0 = true;
                }
            }
            UiAction::ToggleBestiary => {
                *open_overlay = if *open_overlay == OpenOverlay::Bestiary {
                    OpenOverlay::None
                } else {
                    OpenOverlay::Bestiary
                };
                overlay_built.0 = false;
            }
            UiAction::ToggleTree => {
                *open_overlay = if *open_overlay == OpenOverlay::Tree {
                    OpenOverlay::None
                } else {
                    OpenOverlay::Tree
                };
                overlay_built.0 = false;
            }
            UiAction::JumpToYear(year) => {
                if let (Some(tl), Some(wr), Some(cd), Some(rd)) = (
                    timeline.as_mut(),
                    world_res.as_mut(),
                    colors_dirty.as_mut(),
                    rivers_dirty.as_mut(),
                ) {
                    if let Some((idx, _)) = tl
                        .frames
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, f)| (f.year - year).abs())
                    {
                        tl.playing = false;
                        tl.current = idx;
                        apply_current_frame(tl, wr, cd, rd);
                    }
                }
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
