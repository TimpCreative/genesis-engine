//! Interactive application shell: main menu, world setup, generation progress,
//! and the timeline viewer (Doc 02 Phase 3 UI chrome).
//!
//! Screens are Bevy states; each screen spawns a UI tree tagged with
//! [`ScreenRoot`] on enter and despawns it on exit. Generation runs on a
//! background thread so the UI stays responsive; progress and the finished
//! world arrive over a channel.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition, ScrollPosition};
use bevy::window::PrimaryWindow;
use genesis_core::data::BiomeId;
use genesis_render::{
    ActiveBiologyView, ColorsDirty, CurrentProjection, CurrentRenderMode, HexEntityCache,
    HexMeshIndex, PointerCapturedByUi, RenderMode, RiversDirty, SelectedClade, SelectedHex,
    WorldDirty, WorldResource, biome_color, heatmap_color, precipitation_to_color, regime_to_color,
    soil_class_color, temperature_to_color,
};

use crate::hex_inspect::{
    BlocksMapPick, HoveredHex, InspectorTab, InspectorVisible, PendingMenuQuit,
    clear_inspect_on_exit, despawn_hex_inspect_ui, handle_inspector_tabs, handle_map_hex_click,
    inspector_hotkeys, refresh_tab_colors, spawn_hex_inspect_ui, update_hex_inspector,
    update_hex_tooltip, update_hovered_hex,
};
use crate::worldgen::{GenEvent, HISTORY_STRIDE_YEARS, HistoryFrame, SimCommand, WorldGenConfig, run_live_simulation};

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
    ToggleClassification,
    ToggleProjection,
    CycleStepSpan,
}

/// Marks the top-bar projection button's label so it can show the active
/// projection ("Flat map" / "Globe").
#[derive(Component)]
pub struct ProjectionTabLabel;

/// Which full-screen overlay is open over the map (Prep-09 §7–§8).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
pub enum OpenOverlay {
    #[default]
    None,
    Bestiary,
    Tree,
    /// The scientific-classification browser (Kingdom → … → Species by rank).
    Classification,
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
/// Root of the scientific-classification overlay; content rebuilt on open.
#[derive(Component)]
pub struct ClassificationOverlay;
#[derive(Component)]
pub struct ClassificationContent;
/// True while an overlay's content matches the current world/hex/year.
#[derive(Resource, Default)]
pub struct OverlayBuilt(pub bool);

/// The species whose detail panel is open (its `species_id`), or `None` when the
/// panel is closed. Set by clicking a Bestiary card (Doc 09 §9 drill-down).
#[derive(Resource, Default)]
pub struct SelectedSpecies(pub Option<u64>);
/// True while the species detail panel matches `SelectedSpecies`.
#[derive(Resource, Default)]
pub struct SpeciesDetailBuilt(pub bool);
/// A clickable Bestiary species card, carrying its `species_id` for the drill-down.
#[derive(Component)]
pub struct SpeciesCard(pub u64);
/// Root of the species detail modal (absolute; hidden until a card is clicked).
#[derive(Component)]
pub struct SpeciesDetailPanel;
/// Content container of the detail panel, rebuilt when the selection changes.
#[derive(Component)]
pub struct SpeciesDetailContent;
/// The detail panel's "Close" button.
#[derive(Component)]
pub struct SpeciesDetailClose;
/// The detail panel's "Back" button (retraces navigation through clicked species).
#[derive(Component)]
pub struct SpeciesDetailBack;
/// The stack of previously-viewed species, for the detail panel's Back button.
#[derive(Resource, Default)]
pub struct SpeciesHistory(pub Vec<u64>);
/// A generic hover tooltip payload — put on tree rows and trait chips; the
/// `hover_tooltip` system shows the floating panel for whichever is hovered.
#[derive(Component)]
pub struct HoverTip(pub String);

/// Sort order for the global Bestiary list (no hex selected).
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default)]
pub enum BestiarySort {
    /// Most notable / prominent species first (the adapter's default order).
    #[default]
    Notable,
    /// Alphabetical by name.
    Alpha,
    /// Reverse alphabetical.
    ReverseAlpha,
}
impl BestiarySort {
    fn label(self) -> &'static str {
        match self {
            BestiarySort::Notable => "Notable first",
            BestiarySort::Alpha => "A–Z",
            BestiarySort::ReverseAlpha => "Z–A",
        }
    }
    fn next(self) -> Self {
        match self {
            BestiarySort::Notable => BestiarySort::Alpha,
            BestiarySort::Alpha => BestiarySort::ReverseAlpha,
            BestiarySort::ReverseAlpha => BestiarySort::Notable,
        }
    }
}
/// The Bestiary's sort-cycle button + its label.
#[derive(Component)]
pub struct BestiarySortButton;
#[derive(Component)]
pub struct BestiarySortLabel;
/// "Show all species" button on the hex-local Bestiary — clears the selected hex
/// so the list returns to the global catalog.
#[derive(Component)]
pub struct BestiaryShowAllButton;

/// A clickable Tree-of-Life row (its lineage id, species id, and whether it has
/// children — a leaf opens detail, an internal node toggles expand).
#[derive(Component)]
pub struct TreeRowButton {
    pub lineage_id: u64,
    pub species_id: u64,
    pub has_children: bool,
}
/// Which tree lineage ids are expanded (their children shown). Empty = only the
/// default top levels are open.
#[derive(Resource, Default)]
pub struct TreeExpanded(pub std::collections::BTreeSet<u64>);
/// Whether the tree has been initialized with its default-expanded roots.
#[derive(Resource, Default)]
pub struct TreeExpandInit(pub bool);
/// Root of the floating tree tooltip; `TreeTooltipText` is its label.
#[derive(Component)]
pub struct TreeTooltip;
#[derive(Component)]
pub struct TreeTooltipText;

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

/// Bright playhead line at the current year (over the era bands).
#[derive(Component)]
pub struct TimelinePlayhead;

/// Dim scrim over the buffered-but-not-yet-played span (faded era color).
#[derive(Component)]
pub struct TimelineFadedScrim;

/// Dark scrim over the not-yet-buffered future (era color barely shows).
#[derive(Component)]
pub struct TimelineFutureScrim;

/// Container for the geological era bands (behind the timeline fills).
#[derive(Component)]
pub struct EraBandStrip;
/// Container for event pips (on top of the timeline fills).
#[derive(Component)]
pub struct PipStrip;
/// True once the era bands + pips have been built for the current world.
#[derive(Resource, Default)]
pub struct TimelineMarksBuilt(pub bool);

/// The real biology chronicle (true years) for the timeline pips; empty until
/// generation streams it. Replaces the adapter's fabricated pips.
#[derive(Resource, Default)]
pub struct RealLifeEvents(pub Vec<genesis_core::biology_view::LifeEventPip>);

/// Marks a timeline life-event pip so real events can replace fabricated ones.
#[derive(Component)]
pub struct LifePip;

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

/// Sender for the persistent live-simulation worker ([`worldgen::run_live_simulation`]).
/// The UI pushes [`SimCommand`]s (advance to year N) and the worker streams back
/// real captured frames on the [`GenerationTask`] channel. Dropped on teardown,
/// which ends the worker.
#[derive(Resource)]
pub struct SimControl {
    sender: Mutex<Sender<SimCommand>>,
    /// The furthest year the worker has been asked to reach — so repeated
    /// forward presses queue additional spans without waiting for each frame to
    /// stream back.
    pub requested_year: i64,
}

impl SimControl {
    /// Sends a command to the worker; a closed channel (worker gone) is ignored.
    pub fn send(&self, command: SimCommand) {
        if let Ok(tx) = self.sender.lock() {
            let _ = tx.send(command);
        }
    }
}

/// Buffered history of **real** simulated states. Forward stepping past the
/// buffered edge commands the live worker for one more real frame; backward and
/// within-buffer stepping read this buffer. Nothing is interpolated.
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
    /// True while `current` tracks the live edge: a newly streamed frame from
    /// the worker auto-advances the view onto it (so a forward press at the edge
    /// shows the new real state as soon as it arrives).
    pub following_edge: bool,
}

impl WorldTimeline {
    /// The furthest simulated year in the buffer (the live edge).
    pub fn edge_year(&self) -> i64 {
        self.frames.last().map(|f| f.year).unwrap_or(0)
    }
    /// True when `current` sits on the last buffered frame.
    pub fn at_edge(&self) -> bool {
        self.current + 1 >= self.frames.len()
    }
}

/// Key-repeat state for hold-to-scrub.
#[derive(Resource)]
pub struct ScrubRepeat(pub Timer);

impl Default for ScrubRepeat {
    fn default() -> Self {
        Self(Timer::from_seconds(SCRUB_INITIAL_DELAY_S, TimerMode::Once))
    }
}

/// Key-repeat state for hold-to-delete on the seed field's Backspace.
#[derive(Resource)]
pub struct SeedBackspaceRepeat(pub Timer);

impl Default for SeedBackspaceRepeat {
    fn default() -> Self {
        Self(Timer::from_seconds(SCRUB_INITIAL_DELAY_S, TimerMode::Once))
    }
}

/// Selectable forward-step spans (years), cycled by the bottom-bar button.
/// Every value is an exact multiple of the 500k Geological tick, so stepping
/// the live worker by one of these lands on real computed states — 500 ky is
/// one true tectonic tick, the finest the model computes; 10 My matches the
/// historical overview stride. There is deliberately no sub-500k option: the
/// tectonic model has no finer state (plates do not move measurably in less),
/// so it would be fiction.
pub const STEP_SPANS_YEARS: [i64; 4] = [500_000, 1_000_000, 2_000_000, 10_000_000];

/// Bottom-bar forward-step span: index into [`STEP_SPANS_YEARS`].
#[derive(Resource, Default)]
pub struct StepSpan(pub usize);

impl StepSpan {
    pub fn years(&self) -> i64 {
        STEP_SPANS_YEARS[self.0 % STEP_SPANS_YEARS.len()]
    }
    pub fn cycle(&mut self) {
        self.0 = (self.0 + 1) % STEP_SPANS_YEARS.len();
    }
    /// Human label, e.g. "Step: 500 ky" / "Step: 10 My".
    pub fn label(&self) -> String {
        let y = self.years();
        if y >= 1_000_000 {
            format!("Step: {} My", y / 1_000_000)
        } else {
            format!("Step: {} ky", y / 1_000)
        }
    }
}

/// Marks the step-size button's label so it can show the active span.
#[derive(Component)]
pub struct StepSizeLabel;

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
            .init_resource::<RealLifeEvents>()
            .init_resource::<OpenOverlay>()
            .init_resource::<OverlayBuilt>()
            .init_resource::<SelectedSpecies>()
            .init_resource::<SpeciesDetailBuilt>()
            .init_resource::<BestiarySort>()
            .init_resource::<TreeExpanded>()
            .init_resource::<TreeExpandInit>()
            .init_resource::<SpeciesHistory>()
            .init_resource::<ScrubRepeat>()
            .init_resource::<StepSpan>()
            .init_resource::<SeedBackspaceRepeat>()
            .init_resource::<HoveredHex>()
            .init_resource::<InspectorTab>()
            .init_resource::<InspectorVisible>()
            .add_systems(Startup, install_ui_font)
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
                    // Split into two internally-chained groups to stay under
                    // Bevy's tuple-arity limit; the outer `.chain()` preserves the
                    // overall order across both groups.
                    (
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
                            pause_time_while_inspecting,
                            timeline_playback,
                        )
                            .chain(),
                        (
                            refresh_hud,
                            refresh_legend,
                            toggle_legend,
                            update_quit_overlay,
                            refresh_mode_tabs,
                            refresh_projection_tab,
                            handle_projection_toggle,
                            handle_timeline_step_buttons,
                            build_timeline_marks,
                            refresh_life_pips,
                            overlay_hotkeys,
                            update_overlays,
                            handle_species_card_clicks,
                            species_detail_panel,
                            scroll_overlays,
                            refresh_ui_scroll_capture,
                            (handle_bestiary_sort, handle_bestiary_show_all),
                            handle_tree_clicks,
                            hover_tooltip,
                            clear_clade_when_overlays_closed,
                        )
                            .chain(),
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
/// Width of the right-docked biology sidebar (Bestiary / Tree / Classification).
const SIDEBAR_WIDTH: f32 = 380.0;
/// Bestiary species-card background, and its hover/selected tint.
const SPECIES_CARD_BG: Color = Color::srgba(0.10, 0.12, 0.16, 0.95);
const SPECIES_CARD_HOVER: Color = Color::srgba(0.16, 0.20, 0.27, 0.98);

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
        // Bestiary cards and tree rows style themselves.
        Without<SpeciesCard>,
        Without<TreeRowButton>,
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
    // Dropping the command sender closes the worker's channel, so its blocking
    // `recv` returns and the persistent simulation thread exits cleanly.
    commands.remove_resource::<SimControl>();
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

/// Replaces Bevy's built-in `FiraMono-subset` (basic Latin only — box-drawing,
/// arrows, and geometric glyphs render as tofu `□`) with a bundled DejaVu Sans
/// Mono. It covers the tree connectors (`│ ├ └ ─`), triangles/arrows
/// (`▸ ▾ ‹ → ↻ ✕`), and punctuation (`· – — °`) the biology UI leans on, and is
/// still monospaced so the family-tree connectors keep their alignment.
///
/// Bevy loads its default font at `AssetId::default()` (see `bevy_text`), and
/// every `TextFont` in this app resolves to that handle — so overwriting that one
/// asset re-fonts the entire UI without touching any spawn site. DejaVu Sans Mono
/// is freely redistributable (Bitstream Vera / public-domain license, bundled at
/// `assets/fonts/LICENSE_DEJAVU.txt`).
fn install_ui_font(mut fonts: ResMut<Assets<Font>>) {
    const DEJAVU_MONO: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/fonts/DejaVuSansMono.ttf"
    ));
    match Font::try_from_bytes(DEJAVU_MONO.to_vec()) {
        Ok(font) => {
            let _ = fonts.insert(&Handle::<Font>::default(), font);
        }
        Err(e) => warn!("bundled UI font failed to load; keeping Bevy default: {e}"),
    }
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
    mut config: ResMut<ActiveConfig>,
) {
    // "New World" always opens on a fresh random seed (a different world every
    // time); the user can still type their own or roll Random. Other params are
    // preserved across entries.
    config.0.seed_text = random_seed_string();
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
                    "Seed: type any letters/numbers  ·  Backspace  ·  Cmd/Ctrl+C/V copy-paste  ·  Random";
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
    // Resolves down to ~1 My so era/status labels are no longer floored at the
    // old 10 My display quantum. The primary stepping readout uses
    // [`format_year_stepped`] for finer resolution matched to the step span.
    if year >= 1_000_000_000 {
        format!("{:.3}B", year as f64 / 1e9)
    } else if year >= 1_000_000 {
        format!("{:.1}M", year as f64 / 1e6)
    } else if year >= 1_000 {
        format!("{:.0}k", year as f64 / 1e3)
    } else {
        format!("{year}")
    }
}

/// Year label whose precision resolves `resolution_years` — the current step
/// span — so a fine step is always visible instead of flooring at the coarse
/// unit (the "0.00 B, 10 My floor" bug). Below ~10 ky resolution it switches to
/// an exact, thousands-grouped year for civ-scale stepping.
pub fn format_year_stepped(year: i64, resolution_years: i64) -> String {
    let res = resolution_years.max(1);
    if res < 10_000 {
        return format!("{} yr", group_thousands(year));
    }
    let a = year.unsigned_abs();
    if a >= 1_000_000_000 {
        format!("{:.*} By", decimals_for(1_000_000_000, res), year as f64 / 1e9)
    } else if a >= 1_000_000 {
        format!("{:.*} My", decimals_for(1_000_000, res), year as f64 / 1e6)
    } else if a >= 1_000 {
        format!("{:.*} ky", decimals_for(1_000, res), year as f64 / 1e3)
    } else {
        format!("{year} yr")
    }
}

/// Decimal places of `unit_years` needed so one display step (a ULP) is no
/// coarser than `res` — i.e. the smallest `d` with `unit / 10^d <= res`.
fn decimals_for(unit_years: i64, res: i64) -> usize {
    let mut d = 0usize;
    let mut ulp = unit_years as f64;
    while ulp > res as f64 && d < 9 {
        ulp /= 10.0;
        d += 1;
    }
    d
}

/// Formats an integer year with thousands separators (e.g. `4,500,500,000`).
fn group_thousands(year: i64) -> String {
    let neg = year < 0;
    let digits = year.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if neg { format!("-{out}") } else { out }
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

/// Maps a key to the character it types in the seed field. Alphanumeric
/// (`0-9 a-z`) so any word or hex string is a valid seed — every distinct string
/// hashes to a distinct world via `WorldSeed::from_string`.
fn keycode_to_seed_char(code: KeyCode) -> Option<char> {
    use KeyCode::*;
    Some(match code {
        Digit0 | Numpad0 => '0',
        Digit1 | Numpad1 => '1',
        Digit2 | Numpad2 => '2',
        Digit3 | Numpad3 => '3',
        Digit4 | Numpad4 => '4',
        Digit5 | Numpad5 => '5',
        Digit6 | Numpad6 => '6',
        Digit7 | Numpad7 => '7',
        Digit8 | Numpad8 => '8',
        Digit9 | Numpad9 => '9',
        KeyA => 'a',
        KeyB => 'b',
        KeyC => 'c',
        KeyD => 'd',
        KeyE => 'e',
        KeyF => 'f',
        KeyG => 'g',
        KeyH => 'h',
        KeyI => 'i',
        KeyJ => 'j',
        KeyK => 'k',
        KeyL => 'l',
        KeyM => 'm',
        KeyN => 'n',
        KeyO => 'o',
        KeyP => 'p',
        KeyQ => 'q',
        KeyR => 'r',
        KeyS => 's',
        KeyT => 't',
        KeyU => 'u',
        KeyV => 'v',
        KeyW => 'w',
        KeyX => 'x',
        KeyY => 'y',
        KeyZ => 'z',
        _ => return None,
    })
}

/// Maximum seed-string length (long enough for a memorable word or full hex).
const SEED_MAX_LEN: usize = 24;

/// Types hex characters into the seed field on the setup screen (validated
/// charset only; Backspace deletes). Only mutates the config when a relevant key
/// fired, so change detection stays quiet otherwise.
fn seed_text_input(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut config: ResMut<ActiveConfig>,
    mut fresh: ResMut<SeedInputFresh>,
    mut bksp_repeat: ResMut<SeedBackspaceRepeat>,
) {
    // Cmd/Ctrl combos (copy/paste) are handled by `seed_clipboard`; don't also
    // type their letter (e.g. Cmd+C would otherwise insert a hex 'c').
    if seed_modifier_held(&keys) {
        return;
    }
    // Delete on the initial press, then repeat while Backspace is held after an
    // initial delay, so a held key clears the field instead of one char at a time.
    let backspace = if keys.just_pressed(KeyCode::Backspace) {
        bksp_repeat.0 = Timer::from_seconds(SCRUB_INITIAL_DELAY_S, TimerMode::Once);
        true
    } else if keys.pressed(KeyCode::Backspace) {
        bksp_repeat.0.tick(time.delta());
        if bksp_repeat.0.is_finished() {
            bksp_repeat.0 = Timer::from_seconds(SCRUB_REPEAT_INTERVAL_S, TimerMode::Once);
            true
        } else {
            false
        }
    } else {
        false
    };
    let typed = keys
        .get_just_pressed()
        .find_map(|code| keycode_to_seed_char(*code));
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
                let pasted: String = text
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .map(|c| c.to_ascii_lowercase())
                    .take(SEED_MAX_LEN)
                    .collect();
                if !pasted.is_empty() {
                    config.0.seed_text = pasted;
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
    // Two channels: events stream back from the worker; commands (advance to
    // year N) flow to it. The worker runs the initial generation, then stays
    // resident to hand-crank real ticks forward on demand (live stepping).
    let (event_tx, event_rx) = channel();
    let (command_tx, command_rx) = channel::<SimCommand>();
    let requested_year = config.target_year.max(1);
    std::thread::spawn(move || {
        run_live_simulation(&config, command_rx, |event| {
            let _ = event_tx.send(event);
        });
    });
    commands.insert_resource(GenerationTask(Mutex::new(event_rx)));
    commands.insert_resource(SimControl {
        sender: Mutex::new(command_tx),
        requested_year,
    });
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
                // Doc 09: the real `genesis_biology` adapter reads the simulated
                // biology fields (biome/richness/biomass carried in frames) and
                // generates species/tree lazily. Replaces the Prep-09 stub.
                let seed = world.data.parameters.core.seed.value;
                commands.insert_resource(ActiveBiologyView(Box::new(
                    genesis_biology::GeneratedBiologyView::new(seed),
                )));
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
                    following_edge: false,
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
                let landed = timeline.frames.len() - 1;
                // Show the new real state now if it's the first frame or the
                // viewer is tracking the live edge (a forward press / live play
                // just commanded this frame). Otherwise it buffers silently and
                // the playback timer / scrub drives `current`.
                if first || timeline.following_edge {
                    timeline.current = landed;
                    if let Some(world_res) = world_res.as_mut()
                        && let Some(frame) = timeline.frames.get(timeline.current)
                    {
                        frame.apply(&mut world_res.0.data);
                        colors_dirty.0 = true;
                    } else {
                        timeline.needs_apply = true;
                    }
                }
                // The world is visible from its first buffered year on; the rest
                // of history streams in behind the viewer.
                if first && *screen.get() == AppScreen::Generating {
                    next_screen.set(AppScreen::Viewing);
                }
            }
            GenEvent::LifeEvents(pips) => {
                // Real chronicle with true years — replaces the fabricated pips.
                commands.insert_resource(RealLifeEvents(pips));
            }
            GenEvent::BiologyLedger(ledger) => {
                // The recorded tree of life is ready: upgrade the view to read it,
                // so the Tree of Life and Bestiary show the real simulated
                // phylogeny (the map already renders real biology from frames).
                let seed = config.0.to_parameters().core.seed.value;
                commands.insert_resource(ActiveBiologyView(Box::new(
                    genesis_biology::GeneratedBiologyView::with_ledger(seed, *ledger),
                )));
            }
            GenEvent::Done { .. } => {
                // The initial run reached the target. The worker stays alive for
                // live stepping, so we keep the event channel open (do NOT remove
                // GenerationTask) and just mark the initial buffer complete.
                if let Some(timeline) = timeline.as_mut() {
                    timeline.complete = true;
                }
            }
            GenEvent::Failed(err) => {
                if let Ok(mut text) = progress_text.single_mut() {
                    text.0 = format!("generation failed: {err} - Esc to return");
                }
                if let Some(timeline) = timeline.as_mut() {
                    timeline.complete = true;
                }
                commands.remove_resource::<GenerationTask>();
                commands.remove_resource::<SimControl>();
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
                    bar.spawn(button(UiAction::ToggleProjection))
                        .with_children(|b| {
                            b.spawn((
                                label("Flat map", 14.0).0,
                                label("Flat map", 14.0).1,
                                ProjectionTabLabel,
                            ));
                        });
                    bar.spawn(button(UiAction::ToggleTree)).with_children(|b| {
                        b.spawn(label("Tree of Life", 14.0));
                    });
                    bar.spawn(button(UiAction::ToggleClassification))
                        .with_children(|b| {
                            b.spawn(label("Classification", 14.0));
                        });
                    bar.spawn(button(UiAction::ToggleBestiary))
                        .with_children(|b| {
                            b.spawn(label("Bestiary", 14.0));
                        });
                });

            // Bestiary / Tree / Classification — each a right-docked SIDEBAR
            // (like the hex inspector), so the map stays visible and usable.
            for kind in 0u8..3 {
                let (title, close) = match kind {
                    0 => ("Bestiary", UiAction::ToggleBestiary),
                    1 => ("Tree of Life", UiAction::ToggleTree),
                    _ => ("Classification", UiAction::ToggleClassification),
                };
                let mut overlay = parent.spawn((
                    BlocksMapPick,
                    FocusPolicy::Block,
                    Interaction::default(),
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(0.0),
                        top: Val::Px(crate::hex_inspect::TOP_BAR_CLEARANCE),
                        bottom: Val::Px(crate::hex_inspect::BOTTOM_BAR_CLEARANCE),
                        width: Val::Px(SIDEBAR_WIDTH),
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(12.0)),
                        row_gap: Val::Px(10.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.98)),
                    ZIndex(15),
                ));
                match kind {
                    0 => {
                        overlay.insert(BestiaryOverlay);
                    }
                    1 => {
                        overlay.insert(TreeOverlay);
                    }
                    _ => {
                        overlay.insert(ClassificationOverlay);
                    }
                }
                overlay.with_children(|o| {
                    o.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|h| {
                        h.spawn(label(title, 20.0));
                        h.spawn(button(close)).with_children(|b| {
                            b.spawn(label("✕", 15.0));
                        });
                    });
                    let mut content = o.spawn((
                        // Scrollable. `RelativeCursorPosition` detects hover
                        // geometrically (works even over a child button); a single
                        // column now that it's a narrow rail.
                        RelativeCursorPosition::default(),
                        ScrollPosition::default(),
                        Node {
                            flex_direction: FlexDirection::Column,
                            flex_wrap: FlexWrap::NoWrap,
                            align_content: AlignContent::FlexStart,
                            row_gap: Val::Px(8.0),
                            flex_grow: 1.0,
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                    ));
                    match kind {
                        0 => {
                            content.insert(BestiaryContent);
                        }
                        1 => {
                            content.insert(TreeContent);
                        }
                        _ => {
                            content.insert(ClassificationContent);
                        }
                    }
                });
            }

            // Species detail modal — opens over the Bestiary when a card is
            // clicked (full genome + Linnaean classification ladder, Doc 09 §9).
            parent
                .spawn((
                    SpeciesDetailPanel,
                    BlocksMapPick,
                    FocusPolicy::Block,
                    Interaction::default(),
                    // Docked in the same right rail as the lists, above them.
                    Node {
                        position_type: PositionType::Absolute,
                        right: Val::Px(0.0),
                        top: Val::Px(crate::hex_inspect::TOP_BAR_CLEARANCE),
                        bottom: Val::Px(crate::hex_inspect::BOTTOM_BAR_CLEARANCE),
                        width: Val::Px(SIDEBAR_WIDTH),
                        display: Display::None,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.06, 0.07, 0.10, 1.0)),
                    ZIndex(16),
                ))
                .with_children(|panel| {
                    panel
                        .spawn((
                            FocusPolicy::Block,
                            Interaction::default(),
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(14.0)),
                                row_gap: Val::Px(8.0),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|card| {
                            card.spawn((
                                SpeciesDetailContent,
                                RelativeCursorPosition::default(),
                                ScrollPosition::default(),
                                Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(6.0),
                                    flex_grow: 1.0,
                                    overflow: Overflow::scroll_y(),
                                    ..default()
                                },
                            ));
                            card.spawn(Node {
                                flex_direction: FlexDirection::Row,
                                justify_content: JustifyContent::SpaceBetween,
                                margin: UiRect::top(Val::Px(10.0)),
                                ..default()
                            })
                            .with_children(|footer| {
                                footer
                                    .spawn((
                                        SpeciesDetailBack,
                                        Button,
                                        Node {
                                            padding: UiRect::axes(Val::Px(16.0), Val::Px(7.0)),
                                            display: Display::None, // shown when history non-empty
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(BUTTON_BG),
                                    ))
                                    .with_children(|b| {
                                        b.spawn(label("‹ Back", 14.0));
                                    });
                                footer
                                    .spawn((
                                        SpeciesDetailClose,
                                        Button,
                                        Node {
                                            padding: UiRect::axes(Val::Px(16.0), Val::Px(7.0)),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            ..default()
                                        },
                                        BackgroundColor(BUTTON_BG),
                                    ))
                                    .with_children(|b| {
                                        b.spawn(label("Close", 14.0));
                                    });
                            });
                        });
                });

            // Floating Tree-of-Life hover tooltip (rank + defining trait).
            parent
                .spawn((
                    TreeTooltip,
                    Node {
                        position_type: PositionType::Absolute,
                        display: Display::None,
                        max_width: Val::Px(320.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.06, 0.09, 0.97)),
                    GlobalZIndex(50),
                    Visibility::Hidden,
                ))
                .with_children(|t| {
                    t.spawn((
                        TreeTooltipText,
                        label("", 12.5).0,
                        label("", 12.5).1,
                        TextColor(Color::srgb(0.88, 0.9, 0.85)),
                    ));
                });

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
                            row.spawn(button(UiAction::CycleStepSpan))
                                .with_children(|b| {
                                    let (text, font, color) = label("Step: 500 ky", 14.0);
                                    b.spawn((text, font, color, StepSizeLabel));
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
                                // Scrims over the era bands: buffered-but-unplayed
                                // faded, un-buffered future dark; the played span
                                // shows full era color underneath.
                                bar.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Percent(0.0),
                                        top: Val::Px(0.0),
                                        width: Val::Percent(0.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                                    TimelineFadedScrim,
                                ));
                                bar.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Percent(0.0),
                                        top: Val::Px(0.0),
                                        width: Val::Percent(100.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.72)),
                                    TimelineFutureScrim,
                                ));
                                // Bright playhead line at the current year.
                                bar.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: Val::Percent(0.0),
                                        top: Val::Px(-2.0),
                                        width: Val::Px(2.0),
                                        height: Val::Percent(140.0),
                                        ..default()
                                    },
                                    BackgroundColor(Color::WHITE),
                                    TimelinePlayhead,
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

/// Applies the timeline's current (real, buffered) frame to the rendered world.
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

/// Steps the view one buffered frame in `dir` (±1); at the live edge, a forward
/// step instead commands the worker to simulate one more real span (returns
/// `true` so the caller knows a frame will stream in rather than being applied
/// now). Backward and within-buffer steps move `current` and return `false`.
fn step_view(timeline: &mut WorldTimeline, dir: i64, span_years: i64, control: Option<&SimControl>)
-> bool {
    // Forward at the live edge extends real history: command the worker to
    // simulate one more `span`. The new real frame streams back and the view
    // follows onto it (`following_edge`).
    if dir > 0 && timeline.at_edge() {
        if let Some(control) = control {
            let next = timeline.edge_year() + span_years;
            let target = next.max(control.requested_year);
            control.send(SimCommand::AdvanceTo(target));
            timeline.following_edge = true;
            timeline.playing = false;
            return true;
        }
        return false;
    }

    // Within the buffer, move by ~`span` worth of real years, snapping to the
    // nearest captured frame in that direction. A span larger than the capture
    // stride skips multiple frames proportionally. A span finer than the gap
    // snaps to the closest frame — which may be the current one (no movement).
    // The live-edge path above handles true fine-stepping.
    if timeline.frames.is_empty() {
        return false;
    }
    let last = timeline.frames.len() - 1;
    let cur_year = timeline.frames[timeline.current].year;
    let target_year = cur_year + dir * span_years;
    let idx = nearest_frame_index(&timeline.frames, target_year);
    // When the span is finer than the capture stride, the nearest frame may be
    // the current one. Only force a minimum of one frame when the span is at
    // least the capture gap, so "Step: 500 ky" doesn't silently jump 10 My.
    if dir > 0 && idx == timeline.current && span_years >= HISTORY_STRIDE_YEARS {
        timeline.current = (timeline.current + 1).min(last);
    } else if dir < 0 && idx == timeline.current && span_years >= HISTORY_STRIDE_YEARS {
        timeline.current = timeline.current.saturating_sub(1);
    } else {
        timeline.current = idx;
    }
    // Following the edge only resumes if a forward step actually reached it.
    timeline.following_edge = dir > 0 && timeline.at_edge();
    false
}

/// Index of the buffered frame whose year is closest to `target_year`
/// (ascending frames; ties to the earlier index). Deterministic.
fn nearest_frame_index(frames: &[HistoryFrame], target_year: i64) -> usize {
    let mut best = 0usize;
    let mut best_dist = i64::MAX;
    for (i, frame) in frames.iter().enumerate() {
        let dist = (frame.year - target_year).abs();
        if dist < best_dist {
            best_dist = dist;
            best = i;
        }
    }
    best
}

#[allow(clippy::too_many_arguments)]
fn timeline_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut repeat: ResMut<ScrubRepeat>,
    span: Res<StepSpan>,
    control: Option<Res<SimControl>>,
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
        let commanded = step_view(&mut timeline, held, span.years(), control.as_deref());
        if !commanded {
            apply_current_frame(
                &timeline,
                &mut world_res,
                &mut colors_dirty,
                &mut rivers_dirty,
            );
        }
    }
}

fn timeline_playback(
    time: Res<Time>,
    span: Res<StepSpan>,
    control: Option<Res<SimControl>>,
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
        // At the live edge. During the initial buffered run, stall until more
        // frames stream in (video-buffering) and stop at the true end. Once the
        // initial run is complete, keep *playing live* by commanding the worker
        // to simulate the next real span — real-time playback, no interpolation.
        if timeline.complete {
            if let Some(control) = control.as_deref() {
                let next = timeline.edge_year() + span.years();
                let target = next.max(control.requested_year);
                control.send(SimCommand::AdvanceTo(target));
                timeline.following_edge = true;
            } else {
                timeline.playing = false;
            }
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
    span: Res<StepSpan>,
    mut hud: Query<&mut Text, With<HudText>>,
    mut bars: ParamSet<(
        Query<&mut Node, With<TimelinePlayhead>>,
        Query<&mut Node, With<TimelineFadedScrim>>,
        Query<&mut Node, With<TimelineFutureScrim>>,
    )>,
) {
    let Some(timeline) = timeline else {
        return;
    };
    let Some(frame) = timeline.frames.get(timeline.current) else {
        return;
    };
    let buffered_year = timeline.frames.last().map(|f| f.year).unwrap_or(0);
    // The timeline bar spans up to the target; live stepping can push the real
    // year past it (the playhead just pins at the end).
    let target = timeline.target_year.max(buffered_year).max(1) as f32;
    let display_year = frame.year;

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
            "{generating}Year {}  |  Mode: {} [M]  |  [L] legend  |  < > step (Step cycles size), Space plays, Esc for menu",
            format_year_stepped(display_year, span.years()),
            mode.0.label(),
        );
    }
    // Year-based positions so everything lines up with the era bands even with
    // uneven frame strides. Played span (0..playhead) shows full era color;
    // buffered-ahead (playhead..buffered) is faded; future (buffered..100) dark.
    let playhead = (display_year as f32 / target * 100.0).clamp(0.0, 100.0);
    let buffered = (buffered_year as f32 / target * 100.0).clamp(0.0, 100.0);
    if let Ok(mut node) = bars.p0().single_mut() {
        node.left = Val::Percent(playhead);
    }
    if let Ok(mut node) = bars.p1().single_mut() {
        node.left = Val::Percent(playhead);
        node.width = Val::Percent((buffered - playhead).max(0.0));
    }
    if let Ok(mut node) = bars.p2().single_mut() {
        node.left = Val::Percent(buffered);
        node.width = Val::Percent((100.0 - buffered).max(0.0));
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

/// Highlights the active layer-selector tab and refreshes the top-bar year/era —
/// including the **biological era** named for the dominant clade ("Age of the …").
fn refresh_mode_tabs(
    mode: Res<CurrentRenderMode>,
    timeline: Option<Res<WorldTimeline>>,
    biology: Option<Res<ActiveBiologyView>>,
    mut tabs: Query<(&ModeTab, &mut BackgroundColor)>,
    mut status: Query<&mut Text, With<TopBarStatusText>>,
) {
    let active = Color::srgb(0.20, 0.32, 0.48);
    for (tab, mut bg) in &mut tabs {
        bg.0 = if tab.0 == mode.0 { active } else { BUTTON_BG };
    }
    // Only recompute on a frame change (dominant-clade is an O(n) ledger scan).
    let Some(tl) = timeline else { return };
    if !tl.is_changed() {
        return;
    }
    let Some(frame) = tl.frames.get(tl.current) else {
        return;
    };
    let (era, _) = geological_era(frame.year);
    let age = biology
        .and_then(|b| b.0.dominant_clade(genesis_core::WorldYear(frame.year)))
        .map(|a| format!("  ·  {a}"))
        .unwrap_or_default();
    if let Ok(mut text) = status.single_mut() {
        text.0 = format!("{}  ·  {}{}", format_year(frame.year), era, age);
    }
}

/// Keeps the top-bar projection button's label in sync with the active
/// projection (also toggled by the `P` hotkey).
fn refresh_projection_tab(
    projection: Res<CurrentProjection>,
    mut label: Query<&mut Text, With<ProjectionTabLabel>>,
) {
    if !projection.is_changed() {
        return;
    }
    if let Ok(mut text) = label.single_mut() {
        text.0 = projection.0.label().to_string();
    }
}

/// Bottom-bar time stepping: `<` / `>` step one real frame at the selected span
/// (forward at the live edge commands the worker for one more real state), and
/// the step-size button cycles the span (500 ky · 1 My · 2 My · 10 My). Its own
/// system so `handle_actions` stays under Bevy's 16-param limit.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn handle_timeline_step_buttons(
    interactions: ChangedButtons<&'static UiAction>,
    mut span: ResMut<StepSpan>,
    control: Option<Res<SimControl>>,
    timeline: Option<ResMut<WorldTimeline>>,
    world_res: Option<ResMut<WorldResource>>,
    mut colors_dirty: ResMut<ColorsDirty>,
    mut rivers_dirty: ResMut<RiversDirty>,
    mut step_label: Query<&mut Text, With<StepSizeLabel>>,
) {
    let mut timeline = timeline;
    let mut world_res = world_res;
    for (interaction, action) in &interactions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match action {
            UiAction::CycleStepSpan => {
                span.cycle();
                if let Ok(mut text) = step_label.single_mut() {
                    text.0 = span.label();
                }
            }
            UiAction::TimelineStep(step) => {
                if let (Some(timeline), Some(world_res)) = (timeline.as_mut(), world_res.as_mut()) {
                    timeline.playing = false;
                    let commanded =
                        step_view(timeline, *step, span.years(), control.as_deref());
                    if !commanded {
                        apply_current_frame(
                            timeline,
                            world_res,
                            &mut colors_dirty,
                            &mut rivers_dirty,
                        );
                    }
                }
            }
            _ => {}
        }
    }
}

/// Handles the top-bar projection button. Same effect as the `P` hotkey: cycle
/// the projection and rebuild the mesh topology (the visible hex set differs);
/// refresh the river overlay. Its own system so `handle_actions` stays under
/// Bevy's 16-param limit.
fn handle_projection_toggle(
    interactions: ChangedButtons<&'static UiAction>,
    mut projection: ResMut<CurrentProjection>,
    mut world_dirty: ResMut<WorldDirty>,
    mut rivers_dirty: ResMut<RiversDirty>,
) {
    for (interaction, action) in &interactions {
        if *interaction == Interaction::Pressed && matches!(action, UiAction::ToggleProjection) {
            projection.0 = projection.0.cycle_next();
            world_dirty.0 = true;
            rivers_dirty.dirty = true;
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
        let events = bio.0.life_events(
            genesis_core::time::WorldYear(0),
            genesis_core::time::WorldYear(target),
        );
        commands.entity(pip_e).with_children(|s| {
            for ev in events {
                let left = (ev.year as f32 / target as f32 * 100.0).clamp(0.0, 100.0);
                s.spawn((
                    Button,
                    UiAction::JumpToYear(ev.year),
                    Interaction::default(),
                    LifePip,
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

/// Replaces the fabricated life-event pips with the real chronicle (true years)
/// once generation streams it (Doc 09 §13; limitations 2 + 15).
fn refresh_life_pips(
    mut commands: Commands,
    real: Res<RealLifeEvents>,
    timeline: Option<Res<WorldTimeline>>,
    pip_strip: Query<Entity, With<PipStrip>>,
    existing: Query<Entity, With<LifePip>>,
) {
    if !real.is_changed() || real.0.is_empty() {
        return;
    }
    let Some(tl) = timeline else {
        return;
    };
    let target = tl.target_year.max(1);
    let Ok(pip_e) = pip_strip.single() else {
        return;
    };
    for e in &existing {
        commands.entity(e).despawn();
    }
    commands.entity(pip_e).with_children(|s| {
        for ev in &real.0 {
            let left = (ev.year as f32 / target as f32 * 100.0).clamp(0.0, 100.0);
            s.spawn((
                Button,
                UiAction::JumpToYear(ev.year),
                Interaction::default(),
                LifePip,
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
    if keys.just_pressed(KeyCode::KeyC) {
        *open = if *open == OpenOverlay::Classification {
            OpenOverlay::None
        } else {
            OpenOverlay::Classification
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
    bestiary_sort: Res<BestiarySort>,
    tree_expanded: Res<TreeExpanded>,
    mut built: ResMut<OverlayBuilt>,
    mut commands: Commands,
    world_res: Option<Res<WorldResource>>,
    biology: Option<Res<ActiveBiologyView>>,
    timeline: Option<Res<WorldTimeline>>,
    children: Query<&Children>,
    #[allow(clippy::type_complexity)] mut bestiary: Query<
        &mut Node,
        (
            With<BestiaryOverlay>,
            Without<TreeOverlay>,
            Without<ClassificationOverlay>,
        ),
    >,
    #[allow(clippy::type_complexity)] mut tree: Query<
        &mut Node,
        (
            With<TreeOverlay>,
            Without<BestiaryOverlay>,
            Without<ClassificationOverlay>,
        ),
    >,
    #[allow(clippy::type_complexity)] mut classification: Query<
        &mut Node,
        (
            With<ClassificationOverlay>,
            Without<BestiaryOverlay>,
            Without<TreeOverlay>,
        ),
    >,
    bestiary_content: Query<Entity, With<BestiaryContent>>,
    tree_content: Query<Entity, With<TreeContent>>,
    classification_content: Query<Entity, With<ClassificationContent>>,
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
        if let Ok(mut n) = classification.single_mut() {
            n.display = if *open == OpenOverlay::Classification {
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

    let view_year = timeline
        .as_deref()
        .and_then(|t| t.frames.get(t.current))
        .map(|f| f.year)
        .unwrap_or(4_500_000_000);
    match *open {
        OpenOverlay::Bestiary => {
            let Ok(content) = bestiary_content.single() else {
                return;
            };
            clear(&mut commands, content);
            // A hex → its local assemblage; no hex → the whole living catalog,
            // sorted, so the Bestiary works without picking a hex first.
            let global = selected.0.is_none();
            let species = match selected.0 {
                Some(h) => bio.0.assemblage(data, h).species,
                None => {
                    let mut c = bio.0.species_catalog(genesis_core::WorldYear(view_year));
                    match *bestiary_sort {
                        BestiarySort::Notable => {}
                        BestiarySort::Alpha => c.sort_by(|a, b| a.name.cmp(&b.name)),
                        BestiarySort::ReverseAlpha => c.sort_by(|a, b| b.name.cmp(&a.name)),
                    }
                    c
                }
            };
            let sort_label = bestiary_sort.label();
            commands.entity(content).with_children(|c| {
                // Global: a full-width sort cycle + count header.
                if global {
                    c.spawn(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    })
                    .with_children(|h| {
                        h.spawn((
                            BestiarySortButton,
                            Button,
                            Node {
                                padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_BG),
                        ))
                        .with_children(|b| {
                            b.spawn(label(&format!("Sort: {sort_label}  ↻"), 14.0));
                        });
                        h.spawn((
                            label(&format!("{} living species", species.len()), 14.0).0,
                            label("", 14.0).1,
                            TextColor(Color::srgb(0.70, 0.75, 0.82)),
                        ));
                    });
                } else {
                    // Hex-local: make it obvious *why* the list is short (you
                    // clicked a hex), and give a one-click path back to the full
                    // catalog — otherwise the silent switch reads as a glitch.
                    c.spawn(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    })
                    .with_children(|h| {
                        h.spawn((
                            label("◆ Life at this hex", 14.0).0,
                            label("", 14.0).1,
                            TextColor(ACCENT),
                        ));
                        h.spawn((
                            BestiaryShowAllButton,
                            Button,
                            Node {
                                padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_BG),
                        ))
                        .with_children(|b| {
                            b.spawn(label("Show all species", 13.0));
                        });
                    });
                }
                if species.is_empty() {
                    let msg = if global {
                        "No species yet — advance time until life radiates."
                    } else {
                        "This hex has no life yet — try a warmer, wetter hex or advance time."
                    };
                    c.spawn(label(msg, 18.0));
                    return;
                }
                for sp in &species {
                    c.spawn((
                        Button,
                        SpeciesCard(sp.species_id),
                        Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(Val::Px(10.0)),
                            row_gap: Val::Px(4.0),
                            ..default()
                        },
                        BackgroundColor(SPECIES_CARD_BG),
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
                        card.spawn((
                            label("▸ details", 11.0).0,
                            label("", 11.0).1,
                            TextColor(ACCENT),
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
            build_tree_content(
                &mut commands,
                content,
                bio.0.as_ref(),
                timeline.as_deref(),
                &tree_expanded.0,
                false, // family tree: branch lines, no ranks
            );
            built.0 = true;
        }
        OpenOverlay::Classification => {
            let Ok(content) = classification_content.single() else {
                return;
            };
            clear(&mut commands, content);
            build_tree_content(
                &mut commands,
                content,
                bio.0.as_ref(),
                timeline.as_deref(),
                &tree_expanded.0,
                true, // classification: rank labels, indented
            );
            built.0 = true;
        }
        OpenOverlay::None => {}
    }
}

/// Bestiary card interactions: recolor on hover, and open the detail panel for
/// the clicked species (Doc 09 §9 drill-down).
#[allow(clippy::type_complexity)]
fn handle_species_card_clicks(
    mut cards: Query<
        (&Interaction, &SpeciesCard, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    mut selected: ResMut<SelectedSpecies>,
    mut built: ResMut<SpeciesDetailBuilt>,
    mut history: ResMut<SpeciesHistory>,
    mut clade: ResMut<SelectedClade>,
    mut colors_dirty: ResMut<ColorsDirty>,
) {
    for (interaction, card, mut bg) in &mut cards {
        match interaction {
            Interaction::Pressed => {
                // Record the trail so Back can retrace it.
                if let Some(old) = selected.0
                    && old != card.0
                {
                    history.0.push(old);
                }
                selected.0 = Some(card.0);
                built.0 = false;
                bg.0 = SPECIES_CARD_HOVER;
                // Paint this species' distribution on the map.
                clade.0 = Some(genesis_core::LineageSelector::Species(card.0));
                colors_dirty.0 = true;
            }
            Interaction::Hovered => bg.0 = SPECIES_CARD_HOVER,
            Interaction::None => bg.0 = SPECIES_CARD_BG,
        }
    }
}

/// Shows/hides and (re)builds the species detail modal from `species_detail`:
/// the full genome plus the separate Linnaean classification ladder (Doc 09
/// §9.2). Closes on the Close button or when the Bestiary closes.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn species_detail_panel(
    mut selected: ResMut<SelectedSpecies>,
    mut built: ResMut<SpeciesDetailBuilt>,
    open: Res<OpenOverlay>,
    timeline: Option<Res<WorldTimeline>>,
    mut history: ResMut<SpeciesHistory>,
    close: Query<&Interaction, (Changed<Interaction>, With<SpeciesDetailClose>)>,
    back: Query<&Interaction, (Changed<Interaction>, With<SpeciesDetailBack>)>,
    mut back_btn: Query<&mut Node, (With<SpeciesDetailBack>, Without<SpeciesDetailPanel>)>,
    mut panel: Query<&mut Node, With<SpeciesDetailPanel>>,
    content_q: Query<Entity, With<SpeciesDetailContent>>,
    children: Query<&Children>,
    mut commands: Commands,
    biology: Option<Res<ActiveBiologyView>>,
    mut clade: ResMut<SelectedClade>,
    mut colors_dirty: ResMut<ColorsDirty>,
) {
    // Back: retrace to the previously-viewed species — and re-paint its range on
    // the map (without this, the map kept showing the species you navigated away
    // from).
    if back.iter().any(|i| *i == Interaction::Pressed)
        && let Some(prev) = history.0.pop()
    {
        selected.0 = Some(prev);
        built.0 = false;
        clade.0 = Some(genesis_core::LineageSelector::Species(prev));
        colors_dirty.0 = true;
    }
    // Close on the button, or whenever no overlay is open. (The detail opens from
    // both the Bestiary and the Tree of Life.)
    let closed = close.iter().any(|i| *i == Interaction::Pressed);
    if (closed || *open == OpenOverlay::None) && selected.0.is_some() {
        selected.0 = None;
        history.0.clear(); // fresh trail next time
        // Returning to the list (or closing the sidebar) stops painting a range,
        // so the map reflects where you actually are.
        if clade.0.is_some() {
            clade.0 = None;
            colors_dirty.0 = true;
        }
    }

    let visible = *open != OpenOverlay::None && selected.0.is_some();
    if (selected.is_changed() || open.is_changed())
        && let Ok(mut n) = panel.single_mut()
    {
        n.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
    }

    if built.0 || !visible {
        return;
    }
    // Show the Back button only when there's a trail to retrace.
    if let Ok(mut n) = back_btn.single_mut() {
        n.display = if history.0.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
    }
    let (Some(bio), Ok(content)) = (biology, content_q.single()) else {
        return;
    };
    // Clear the previous card's content.
    if let Ok(kids) = children.get(content) {
        for k in kids.iter() {
            commands.entity(k).despawn();
        }
    }
    let detail = selected.0.and_then(|id| bio.0.species_detail(id));
    let year = timeline
        .as_deref()
        .and_then(|t| t.frames.get(t.current))
        .map(|f| f.year)
        .unwrap_or(4_500_000_000);
    let web = selected
        .0
        .map(|id| bio.0.food_web(id, genesis_core::WorldYear(year)))
        .unwrap_or_default();
    let relatives = selected
        .0
        .map(|id| bio.0.relatives(id, genesis_core::WorldYear(year)))
        .unwrap_or_default();
    commands.entity(content).with_children(|c| {
        let Some(d) = detail else {
            c.spawn(label("This species has no recorded detail.", 15.0));
            return;
        };
        c.spawn(label(&d.name, 24.0));
        c.spawn((
            label(&format!("{} · family {}", d.guild, d.family), 14.0).0,
            label("", 14.0).1,
            TextColor(Color::srgb(0.70, 0.80, 0.95)),
        ));
        c.spawn((
            label(&d.description, 13.0).0,
            label("", 13.0).1,
            TextColor(Color::srgb(0.82, 0.82, 0.86)),
        ));

        // Traits (full genome) — each a hoverable chip with a plain-English
        // definition on hover.
        c.spawn((
            label("Traits  (hover for meaning)", 13.0).0,
            label("", 13.0).1,
            TextColor(ACCENT),
        ))
        .insert(Node {
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        });
        c.spawn(Node {
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(6.0),
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|row| {
            for (name, desc) in &d.trait_details {
                let tip = if desc.is_empty() {
                    name.clone()
                } else {
                    format!("{name} — {desc}")
                };
                row.spawn((
                    Interaction::default(),
                    HoverTip(tip),
                    Node {
                        padding: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.16, 0.18, 0.22, 0.9)),
                ))
                .with_children(|chip| {
                    chip.spawn((
                        label(name, 12.0).0,
                        label("", 12.0).1,
                        TextColor(Color::srgb(0.78, 0.80, 0.86)),
                    ));
                });
            }
        });

        // Classification ladder — the separate Linnaean view.
        c.spawn((
            label("Classification", 13.0).0,
            label("", 13.0).1,
            TextColor(ACCENT),
        ))
        .insert(Node {
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        });
        for (depth, (rank, clade)) in d.classification.iter().enumerate() {
            c.spawn((
                label(&format!("{rank}: {clade}"), 12.5).0,
                label("", 12.5).1,
                TextColor(Color::srgb(0.86, 0.86, 0.90)),
            ))
            .insert(Node {
                margin: UiRect::left(Val::Px((depth as f32) * 12.0)),
                ..default()
            });
        }

        // Food web — what it eats, what eats it, who it competes with. Each
        // neighbor is a clickable chip that navigates to that species' detail.
        let sections: [(&str, &Vec<genesis_core::SpeciesPeek>); 3] = [
            ("Eats", &web.prey),
            ("Eaten by", &web.predators),
            ("Competes with", &web.competitors),
        ];
        if sections.iter().any(|(_, v)| !v.is_empty()) {
            c.spawn((
                label("Food web", 13.0).0,
                label("", 13.0).1,
                TextColor(ACCENT),
            ))
            .insert(Node {
                margin: UiRect::top(Val::Px(10.0)),
                ..default()
            });
            for (title, list) in sections {
                if list.is_empty() {
                    continue;
                }
                c.spawn((
                    label(title, 12.0).0,
                    label("", 12.0).1,
                    TextColor(Color::srgb(0.70, 0.75, 0.82)),
                ))
                .insert(Node {
                    margin: UiRect::top(Val::Px(4.0)),
                    ..default()
                });
                c.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|row| {
                    for sp in list {
                        row.spawn((
                            Button,
                            SpeciesCard(sp.species_id),
                            Node {
                                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(SPECIES_CARD_BG),
                        ))
                        .with_children(|chip| {
                            chip.spawn((
                                label(&format!("{} · {}", sp.name, sp.guild), 11.5).0,
                                label("", 11.5).1,
                                TextColor(Color::srgb(0.85, 0.9, 0.85)),
                            ));
                        });
                    }
                });
            }
        }

        // Relatives — evolutionarily-closest species (clickable to navigate).
        if !relatives.is_empty() {
            c.spawn((
                label("Relatives", 13.0).0,
                label("", 13.0).1,
                TextColor(ACCENT),
            ))
            .insert(Node {
                margin: UiRect::top(Val::Px(10.0)),
                ..default()
            });
            c.spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(6.0),
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|row| {
                for sp in &relatives {
                    row.spawn((
                        Button,
                        SpeciesCard(sp.species_id),
                        HoverTip(sp.description.clone()),
                        Node {
                            padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(SPECIES_CARD_BG),
                    ))
                    .with_children(|chip| {
                        chip.spawn((
                            label(&sp.name, 11.5).0,
                            label("", 11.5).1,
                            TextColor(Color::srgb(0.85, 0.9, 0.85)),
                        ));
                    });
                }
            });
        }
    });
    built.0 = true;
}

/// Mouse-wheel scrolling for whichever overlay/detail scroll container the cursor
/// is over (Bevy 0.18: `ScrollPosition` is a `Vec2`, vertical offset in `.0.y`).
/// The map-zoom wheel handler is gated off while an overlay is open (see
/// `refresh_ui_scroll_capture`), so the wheel affects only one thing at a time.
fn scroll_overlays(
    wheel: Res<AccumulatedMouseScroll>,
    mut containers: Query<(&RelativeCursorPosition, &mut ScrollPosition)>,
) {
    if wheel.delta.y == 0.0 {
        return;
    }
    let dy = match wheel.unit {
        MouseScrollUnit::Line => wheel.delta.y * 24.0,
        MouseScrollUnit::Pixel => wheel.delta.y,
    };
    for (cursor, mut sp) in &mut containers {
        // Geometric hover (the normalized cursor is within the node's rect), so
        // scrolling works even with the cursor over a child card/row button.
        let over = cursor
            .normalized
            .is_some_and(|n| (0.0..=1.0).contains(&n.x) && (0.0..=1.0).contains(&n.y));
        if over {
            sp.0.y = (sp.0.y - dy).max(0.0);
        }
    }
}

/// Tells the renderer whether the pointer is over a scroll container (the biology
/// sidebar / detail rail), so the wheel scrolls the rail there but still pans/zooms
/// the map everywhere else — the map stays usable beside the docked sidebar.
fn refresh_ui_scroll_capture(
    containers: Query<&RelativeCursorPosition, With<ScrollPosition>>,
    mut capture: ResMut<PointerCapturedByUi>,
) {
    let over = containers.iter().any(|c| {
        c.normalized
            .is_some_and(|n| (0.0..=1.0).contains(&n.x) && (0.0..=1.0).contains(&n.y))
    });
    if capture.0 != over {
        capture.0 = over;
    }
}

/// Stops painting a clade on the map once the biology sidebar is closed.
fn clear_clade_when_overlays_closed(
    open: Res<OpenOverlay>,
    mut clade: ResMut<SelectedClade>,
    mut colors_dirty: ResMut<ColorsDirty>,
) {
    if *open == OpenOverlay::None && clade.0.is_some() {
        clade.0 = None;
        colors_dirty.0 = true;
    }
}

/// Cycles the global Bestiary's sort order (Notable → A–Z → Z–A) and rebuilds.
fn handle_bestiary_sort(
    buttons: Query<&Interaction, (Changed<Interaction>, With<BestiarySortButton>)>,
    mut sort: ResMut<BestiarySort>,
    mut built: ResMut<OverlayBuilt>,
) {
    if buttons.iter().any(|i| *i == Interaction::Pressed) {
        *sort = sort.next();
        built.0 = false;
    }
}

/// "Show all species" on the hex-local Bestiary: clears the selected hex so the
/// list rebuilds as the global catalog (`update_overlays` rebuilds when it sees
/// `SelectedHex` change).
fn handle_bestiary_show_all(
    buttons: Query<&Interaction, (Changed<Interaction>, With<BestiaryShowAllButton>)>,
    mut selected_hex: ResMut<SelectedHex>,
) {
    if buttons.iter().any(|i| *i == Interaction::Pressed) && selected_hex.0.is_some() {
        selected_hex.0 = None;
    }
}

/// Tree-row interactions: hover highlight, expand/collapse a branch, or open a
/// leaf's species detail (the collapsible + clickable family tree).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn handle_tree_clicks(
    mut rows: Query<
        (&Interaction, &TreeRowButton, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    mut expanded: ResMut<TreeExpanded>,
    mut built: ResMut<OverlayBuilt>,
    mut selected_species: ResMut<SelectedSpecies>,
    mut detail_built: ResMut<SpeciesDetailBuilt>,
    mut history: ResMut<SpeciesHistory>,
    mut clade: ResMut<SelectedClade>,
    mut colors_dirty: ResMut<ColorsDirty>,
) {
    for (interaction, row, mut bg) in &mut rows {
        match interaction {
            Interaction::Pressed => {
                if row.has_children {
                    if !expanded.0.remove(&row.lineage_id) {
                        expanded.0.insert(row.lineage_id);
                    }
                    built.0 = false; // re-lay-out the tree
                    // Paint the whole clade's distribution on the map.
                    clade.0 = Some(genesis_core::LineageSelector::Clade(row.lineage_id));
                    colors_dirty.0 = true;
                } else {
                    if let Some(old) = selected_species.0
                        && old != row.species_id
                    {
                        history.0.push(old);
                    }
                    selected_species.0 = Some(row.species_id);
                    detail_built.0 = false;
                    clade.0 = Some(genesis_core::LineageSelector::Species(row.species_id));
                    colors_dirty.0 = true;
                }
            }
            Interaction::Hovered => bg.0 = Color::srgba(1.0, 1.0, 1.0, 0.08),
            Interaction::None => bg.0 = Color::NONE,
        }
    }
}

/// Floating tooltip for any hovered `HoverTip` node — a tree row's Linnaean rank,
/// or a trait chip's plain-English definition.
#[allow(clippy::type_complexity)]
fn hover_tooltip(
    tips: Query<(&Interaction, &HoverTip)>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut panel: Query<(&mut Node, &mut Visibility), With<TreeTooltip>>,
    mut text: Query<&mut Text, With<TreeTooltipText>>,
) {
    let Ok((mut node, mut vis)) = panel.single_mut() else {
        return;
    };
    let Ok(mut label_text) = text.single_mut() else {
        return;
    };
    let hovered = tips
        .iter()
        .find(|(i, _)| **i == Interaction::Hovered)
        .map(|(_, m)| m.0.clone());
    match hovered {
        Some(tip) => {
            *label_text = Text::new(tip);
            if let Ok(win) = window.single()
                && let Some(cur) = win.cursor_position()
            {
                node.left = Val::Px((cur.x + 16.0).min((win.width() - 340.0).max(0.0)));
                node.top = Val::Px((cur.y + 16.0).min((win.height() - 110.0).max(0.0)));
            }
            node.display = Display::Flex;
            *vis = Visibility::Visible;
        }
        None => {
            node.display = Display::None;
            *vis = Visibility::Hidden;
        }
    }
}

/// Viewing the Bestiary, Tree of Life, or a hex's details **pauses time** — you
/// study a moment, you don't watch these while history streams past. Time resumes
/// when the overlay closes / the hex is deselected (Doc 09 UX: real-time species
/// generation is only affordable while paused).
fn pause_time_while_inspecting(
    open: Res<OpenOverlay>,
    selected: Res<SelectedHex>,
    timeline: Option<ResMut<WorldTimeline>>,
) {
    let inspecting = *open != OpenOverlay::None || selected.0.is_some();
    if inspecting
        && let Some(mut tl) = timeline
        && tl.playing
    {
        tl.playing = false;
    }
}

/// Builds the Tree-of-Life overlay: a **collapsible, clickable family tree** from
/// `tree_snapshot(current_year)`. LUCA is open by default; other nodes with
/// children start collapsed and expand on click (▸/▾). A leaf click opens its
/// species detail; hovering any row shows its Linnaean rank (Prep-09 §7).
fn build_tree_content(
    commands: &mut Commands,
    content: Entity,
    view: &dyn genesis_core::BiologyView,
    timeline: Option<&WorldTimeline>,
    expanded: &std::collections::BTreeSet<u64>,
    show_ranks: bool,
) {
    let year = timeline
        .and_then(|t| t.frames.get(t.current))
        .map(|f| f.year)
        .unwrap_or(4_500_000_000);
    let tree = view.tree_snapshot(genesis_core::time::WorldYear(year));

    let mut children: std::collections::BTreeMap<Option<u64>, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (idx, n) in tree.nodes.iter().enumerate() {
        children.entry(n.parent).or_default().push(idx);
    }
    let has_children = |id: u64| children.get(&Some(id)).is_some_and(|v| !v.is_empty());
    // Open = the root (depth 0), or explicitly expanded. Everything else collapsed,
    // so the initial view is LUCA + its direct children, not a 400-row wall.
    let is_open = |id: u64, depth: usize| depth == 0 || expanded.contains(&id);

    // DFS from LUCA, descending only into open nodes, carrying the branch-line
    // "guides" (for each ancestor level, whether it continues below) so we can draw
    // real ├─ └─ │ connectors — a literal family tree, not just indentation.
    // Row = (node index, depth, prefix chars, is-last-child).
    let mut order: Vec<(usize, usize, Vec<char>, bool)> = Vec::new();
    // stack item = (idx, depth, is_last, guides)
    let mut stack: Vec<(usize, usize, bool, Vec<bool>)> = Vec::new();
    if let Some(roots) = children.get(&None) {
        let last = roots.last().copied();
        for &r in roots.iter().rev() {
            stack.push((r, 0, Some(r) == last, Vec::new()));
        }
    }
    while let Some((idx, depth, is_last, guides)) = stack.pop() {
        // The row's prefix: a vertical bar for each continuing ancestor, then this
        // node's elbow (└ last child, ├ otherwise). The root (depth 0) has none.
        let mut prefix: Vec<char> = guides.iter().map(|&c| if c { '│' } else { ' ' }).collect();
        if depth > 0 {
            prefix.push(if is_last { '└' } else { '├' });
        }
        let id = tree.nodes[idx].id;
        order.push((idx, depth, prefix, is_last));
        if is_open(id, depth)
            && let Some(kids) = children.get(&Some(id))
        {
            // Children inherit this node's guides plus its own continuation (unless
            // this is the root, whose children start at column 0).
            let child_guides = if depth == 0 {
                Vec::new()
            } else {
                let mut g = guides.clone();
                g.push(!is_last);
                g
            };
            let last_k = kids.last().copied();
            for &k in kids.iter().rev() {
                stack.push((k, depth + 1, Some(k) == last_k, child_guides.clone()));
            }
        }
    }

    let living = tree
        .nodes
        .iter()
        .filter(|n| n.extinction_year.is_none())
        .count();
    const MAX_ROWS: usize = 1200;
    let guide_color = Color::srgb(0.40, 0.45, 0.42);
    commands.entity(content).with_children(|c| {
        c.spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(1.0),
            ..default()
        })
        .with_children(|col| {
            let header = if show_ranks {
                format!(
                    "Classification as of {}  ·  {} lineages by rank  ·  ▸ expand",
                    format_year(year),
                    tree.nodes.len()
                )
            } else {
                format!(
                    "As of {}  ·  {living} living of {} lineages, back to LUCA  ·  ▸ expand · click a leaf",
                    format_year(year),
                    tree.nodes.len()
                )
            };
            col.spawn(label(&header, 14.0));
            for (row, (idx, _d, prefix, _is_last)) in order.iter().enumerate() {
                if row >= MAX_ROWS {
                    col.spawn((
                        label(&format!("… and {} more lineages", order.len() - MAX_ROWS), 13.0).0,
                        label("", 13.0).1,
                        TextColor(Color::srgb(0.55, 0.6, 0.65)),
                    ));
                    break;
                }
                let node = &tree.nodes[*idx];
                let extinct = node.extinction_year.is_some();
                let color = if extinct {
                    Color::srgb(0.5, 0.5, 0.55)
                } else {
                    Color::srgb(0.86, 0.93, 0.87)
                };
                let hc = has_children(node.id);
                let arrow = if hc {
                    if is_open(node.id, *_d) { "▾ " } else { "▸ " }
                } else {
                    ""
                };
                let dagger = if extinct { "  †" } else { "" };
                let extinct_note = node
                    .extinction_year
                    .map(|e| format!(", extinct {}", format_year(e)))
                    .unwrap_or_default();
                let tip = format!(
                    "{} — {}\ndistinguished by {}\noriginated {}{}",
                    node.name, node.rank, node.defining_trait, format_year(node.origin_year), extinct_note,
                );
                let prefix = prefix.clone();
                let depth = *_d;
                let rank = node.rank.clone();
                col.spawn((
                    Button,
                    TreeRowButton {
                        lineage_id: node.id,
                        species_id: node.species_id,
                        has_children: hc,
                    },
                    HoverTip(tip),
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        // Classification indents by depth; the family tree uses
                        // fixed-width branch-line cells instead.
                        margin: if show_ranks {
                            UiRect::left(Val::Px((depth.min(10) as f32) * 16.0))
                        } else {
                            UiRect::ZERO
                        },
                        padding: UiRect::axes(Val::Px(2.0), Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                ))
                .with_children(|r| {
                    if show_ranks {
                        // Rank-forward: "Order — Ventopus".
                        let rank_label = capitalize_first(&rank);
                        r.spawn((
                            label(&format!("{arrow}{rank_label}", ), 12.0).0,
                            label("", 12.0).1,
                            TextColor(Color::srgb(0.55, 0.70, 0.95)),
                        ));
                        r.spawn((
                            label(&format!("  {}{dagger}", node.name), 13.0).0,
                            label("", 13.0).1,
                            TextColor(color),
                        ));
                    } else {
                        // Fixed-width branch-line cells (aligned regardless of font).
                        for ch in &prefix {
                            r.spawn((
                                label(&ch.to_string(), 13.0).0,
                                label("", 13.0).1,
                                TextColor(guide_color),
                                Node {
                                    width: Val::Px(13.0),
                                    ..default()
                                },
                            ));
                        }
                        r.spawn((
                            label(&format!(" {arrow}{}{dagger}", node.name), 13.0).0,
                            label("", 13.0).1,
                            TextColor(color),
                        ));
                    }
                });
            }
        });
    });
}

/// Capitalizes the first letter (for rank labels: "order" → "Order").
fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
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
            UiAction::ToggleClassification => {
                *open_overlay = if *open_overlay == OpenOverlay::Classification {
                    OpenOverlay::None
                } else {
                    OpenOverlay::Classification
                };
                overlay_built.0 = false;
            }
            // Handled by `handle_projection_toggle` (a dedicated system keeps
            // `handle_actions` under Bevy's 16-param limit).
            UiAction::ToggleProjection => {}
            // Handled by `handle_timeline_step_buttons` (same reason).
            UiAction::CycleStepSpan => {}
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
                        tl.following_edge = tl.at_edge();
                        apply_current_frame(tl, wr, cd, rd);
                    }
                }
            }
            // Handled by `handle_timeline_step_buttons` (dedicated system:
            // needs the StepSpan resource; keeps `handle_actions` under
            // Bevy's param limit).
            UiAction::TimelineStep(_) => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    fn timeline_with(frames: usize) -> WorldTimeline {
        let mut params = genesis_core::parameters::WorldParameters::default();
        params.core.grid.subdivision_level = 5;
        let world = genesis_core::create_world(params).expect("world");
        let mut frame = HistoryFrame::capture(&world.data);
        let base = frame.clone();
        let mut buffer = Vec::new();
        for i in 0..frames {
            frame = base.clone();
            frame.year = i as i64 * 10_000_000;
            buffer.push(frame);
        }
        WorldTimeline {
            frames: buffer,
            current: 0,
            playing: false,
            play_timer: Timer::from_seconds(0.25, TimerMode::Repeating),
            target_year: 1,
            complete: true,
            needs_apply: false,
            following_edge: false,
        }
    }

    #[test]
    fn stepping_within_buffer_moves_one_frame_no_command() {
        // Without a SimControl (offline), forward/backward walk the buffered
        // frames. A span ≥ the capture stride (10 My) moves one frame; a finer
        // span snaps to the nearest frame (staying put if already closest).
        let mut tl = timeline_with(3);
        // Span = 500k < 10 My stride: stays on frame 0 (no finer data exists).
        assert!(!step_view(&mut tl, 1, 500_000, None));
        assert_eq!(tl.current, 0);
        // Span = 10 My: moves one frame.
        assert!(!step_view(&mut tl, 1, 10_000_000, None));
        assert_eq!(tl.current, 1);
        assert!(!step_view(&mut tl, 1, 10_000_000, None));
        assert_eq!(tl.current, 2);
        // At the edge with no worker: forward is a no-op (no command possible).
        assert!(!step_view(&mut tl, 1, 10_000_000, None));
        assert_eq!(tl.current, 2, "clamps at the live edge when offline");
        // Backward walks the buffer and leaves the edge.
        assert!(!step_view(&mut tl, -1, 10_000_000, None));
        assert_eq!(tl.current, 1);
        assert!(!tl.following_edge);
        // Down to the start and clamp.
        assert!(!step_view(&mut tl, -1, 10_000_000, None));
        assert!(!step_view(&mut tl, -1, 10_000_000, None));
        assert_eq!(tl.current, 0);
    }

    #[test]
    fn step_span_cycles_real_multiples_of_the_tectonic_tick() {
        let mut span = StepSpan::default();
        assert_eq!(span.years(), 500_000, "default is one real 500k tick");
        assert_eq!(span.label(), "Step: 500 ky");
        span.cycle();
        assert_eq!(span.years(), 1_000_000);
        span.cycle();
        assert_eq!(span.years(), 2_000_000);
        span.cycle();
        assert_eq!((span.years(), span.label().as_str()), (10_000_000, "Step: 10 My"));
        span.cycle();
        assert_eq!(span.years(), 500_000, "wraps back to the finest tick");
        // Every span is an exact multiple of the 500k tectonic tick, so the
        // worker always lands on real computed states.
        for s in STEP_SPANS_YEARS {
            assert_eq!(s % 500_000, 0, "span {s} is not a whole tectonic tick");
        }
    }

    #[test]
    fn span_scales_within_buffer_jump_size() {
        // A finely-captured buffer (500k spacing): the step span controls how
        // many real frames a press crosses, so the selected size actually
        // matters within the buffer (not always one frame).
        let mut tl = timeline_with(21); // years 0, 0.5M, 1M, ... 10M
        for f in tl.frames.iter_mut().enumerate() {
            f.1.year = f.0 as i64 * 500_000;
        }
        // 2 My span jumps four 500k frames.
        step_view(&mut tl, 1, 2_000_000, None);
        assert_eq!(tl.frames[tl.current].year, 2_000_000);
        // 500k span jumps one frame.
        step_view(&mut tl, 1, 500_000, None);
        assert_eq!(tl.frames[tl.current].year, 2_500_000);
        // 10 My span clamps at the live edge.
        step_view(&mut tl, 1, 10_000_000, None);
        assert_eq!(tl.frames[tl.current].year, 10_000_000, "clamps at the edge");
        // Backward by 1 My crosses two frames.
        step_view(&mut tl, -1, 1_000_000, None);
        assert_eq!(tl.frames[tl.current].year, 9_000_000);
    }

    #[test]
    fn stepped_year_label_resolves_the_step() {
        // The bug: billions-with-2-decimals floored at 10 My, so 500k/1M steps
        // showed no change. The stepped formatter must render distinct labels
        // for consecutive fine steps at billion scale.
        let y = 4_500_000_000;
        let a = format_year_stepped(y, 500_000);
        let b = format_year_stepped(y + 500_000, 500_000);
        assert_ne!(a, b, "500k step must change the label: {a} == {b}");
        // Coarse span stays readable (few decimals).
        assert_eq!(format_year_stepped(4_500_000_000, 10_000_000), "4.50 By");
        // My scale resolves fine steps too.
        assert_ne!(
            format_year_stepped(5_000_000, 500_000),
            format_year_stepped(5_500_000, 500_000)
        );
        // Civ-scale resolution shows exact grouped years.
        assert_eq!(format_year_stepped(4_500_500_123, 1), "4,500,500,123 yr");
        assert_eq!(group_thousands(1_000_000), "1,000,000");
    }
}
