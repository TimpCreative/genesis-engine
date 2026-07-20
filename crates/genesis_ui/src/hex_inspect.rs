//! Hex hover tooltip and click-to-inspect panel for the Viewing screen.

use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use bevy::window::PrimaryWindow;
use genesis_core::HexId;
use genesis_core::biology_view::BiologyView;
use genesis_core::data::{
    BedrockType, HydroFlags, WATER_NONE, WaterBodyId, WorldData, river_class,
};
use genesis_render::{
    ActiveBiologyView, CameraDragState, CameraState, SelectedHex, WorldResource, cursor_hex,
    screen_to_hex,
};

/// Hex under the cursor (tooltip only).
#[derive(Resource, Default, Clone, Copy)]
pub struct HoveredHex(pub Option<HexId>);

/// Active inspector tab.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum InspectorTab {
    #[default]
    Terrain,
    Climate,
    Water,
    Life,
    Society,
    Debug,
}

impl InspectorTab {
    pub const ALL: [InspectorTab; 6] = [
        InspectorTab::Terrain,
        InspectorTab::Climate,
        InspectorTab::Water,
        InspectorTab::Life,
        InspectorTab::Society,
        InspectorTab::Debug,
    ];

    pub fn label(self) -> &'static str {
        match self {
            InspectorTab::Terrain => "Terrain",
            InspectorTab::Climate => "Climate",
            InspectorTab::Water => "Water",
            InspectorTab::Life => "Life",
            InspectorTab::Society => "Society",
            InspectorTab::Debug => "Debug",
        }
    }
}

/// When false, the dock is hidden even if a hex is selected (`I` toggle).
#[derive(Resource)]
pub struct InspectorVisible(pub bool);

impl Default for InspectorVisible {
    fn default() -> Self {
        Self(true)
    }
}

/// Marks UI that should block map pick/click (HUD chrome, inspector).
#[derive(Component)]
pub struct BlocksMapPick;

#[derive(Component)]
pub struct HexTooltipRoot;

#[derive(Component)]
pub struct HexTooltipText;

#[derive(Component)]
pub struct HexInspectorRoot;

#[derive(Component)]
pub struct HexInspectorHeader;

#[derive(Component)]
pub struct HexInspectorBody;

#[derive(Component)]
pub struct InspectorTabButton(InspectorTab);

const TOOLTIP_BG: Color = Color::srgba(0.06, 0.07, 0.10, 0.92);
const INSPECTOR_BG: Color = Color::srgba(0.07, 0.08, 0.11, 0.94);
const TAB_IDLE: Color = Color::srgb(0.16, 0.18, 0.22);
const TAB_ACTIVE: Color = Color::srgb(0.28, 0.42, 0.62);
const INSPECTOR_WIDTH: f32 = 320.0;

type TabButtonInteraction<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static InspectorTabButton,
        &'static mut BackgroundColor,
    ),
    (Changed<Interaction>, With<Button>),
>;

type InspectUiRoots<'w, 's> =
    Query<'w, 's, Entity, Or<(With<HexTooltipRoot>, With<HexInspectorRoot>)>>;

type TooltipRootQuery<'w, 's> = Query<
    'w,
    's,
    (&'static mut Node, &'static mut Visibility),
    (With<HexTooltipRoot>, Without<HexTooltipText>),
>;

type TooltipTextQuery<'w, 's> =
    Query<'w, 's, &'static mut Text, (With<HexTooltipText>, Without<HexTooltipRoot>)>;

/// Spawn tooltip + inspector chrome (called from Viewing enter).
pub fn spawn_hex_inspect_ui(mut commands: Commands) {
    commands
        .spawn((
            HexTooltipRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                max_width: Val::Px(280.0),
                padding: UiRect::all(Val::Px(8.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(TOOLTIP_BG),
            ZIndex(20),
            FocusPolicy::Pass,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new(""),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                HexTooltipText,
            ));
        });

    commands
        .spawn((
            HexInspectorRoot,
            BlocksMapPick,
            FocusPolicy::Block,
            Interaction::default(),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(56.0),
                width: Val::Px(INSPECTOR_WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(8.0),
                display: Display::None,
                ..default()
            },
            BackgroundColor(INSPECTOR_BG),
            ZIndex(15),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new(""),
                TextFont {
                    font_size: 15.0,
                    ..default()
                },
                TextColor(Color::WHITE),
                HexInspectorHeader,
            ));
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(4.0),
                    row_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|tabs| {
                    for tab in InspectorTab::ALL {
                        tabs.spawn((
                            Button,
                            InspectorTabButton(tab),
                            BlocksMapPick,
                            Node {
                                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                                ..default()
                            },
                            BackgroundColor(TAB_IDLE),
                        ))
                        .with_children(|b| {
                            b.spawn((
                                Text::new(tab.label()),
                                TextFont {
                                    font_size: 12.0,
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                            ));
                        });
                    }
                });
            panel.spawn((
                Text::new(""),
                TextFont {
                    font_size: 13.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.9, 0.92)),
                HexInspectorBody,
            ));
        });
}

pub fn update_hovered_hex(
    window_query: Query<&Window, With<PrimaryWindow>>,
    camera: Res<CameraState>,
    world_res: Option<Res<WorldResource>>,
    blocked: Query<&Interaction, With<BlocksMapPick>>,
    mut hovered: ResMut<HoveredHex>,
) {
    let over_ui = blocked
        .iter()
        .any(|i| matches!(*i, Interaction::Hovered | Interaction::Pressed));
    if over_ui {
        if hovered.0.is_some() {
            hovered.0 = None;
        }
        return;
    }
    let Some(world_res) = world_res else {
        hovered.0 = None;
        return;
    };
    let next = cursor_hex(&window_query, &camera, &world_res.0.data.grid);
    if hovered.0 != next {
        hovered.0 = next;
    }
}

pub fn handle_map_hex_click(
    drag: Res<CameraDragState>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    camera: Res<CameraState>,
    world_res: Option<Res<WorldResource>>,
    blocked: Query<&Interaction, With<BlocksMapPick>>,
    mut selected: ResMut<SelectedHex>,
    mut visible: ResMut<InspectorVisible>,
) {
    if !drag.just_clicked_map {
        return;
    }
    let over_ui = blocked
        .iter()
        .any(|i| matches!(*i, Interaction::Hovered | Interaction::Pressed));
    if over_ui {
        return;
    }
    let Some(world_res) = world_res else {
        return;
    };
    let Ok(window) = window_query.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Some(hex) = screen_to_hex(window, &camera, cursor, &world_res.0.data.grid) else {
        return;
    };
    if selected.0 == Some(hex) {
        selected.0 = None;
    } else {
        selected.0 = Some(hex);
        visible.0 = true;
    }
}

pub fn inspector_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<SelectedHex>,
    mut visible: ResMut<InspectorVisible>,
) {
    if keys.just_pressed(KeyCode::KeyI) && selected.0.is_some() {
        visible.0 = !visible.0;
    }
}

/// Esc clears selection first; only a second Esc returns to the menu.
/// Set when the user asks to leave the viewer; drives the confirm dialog so an
/// accidental Esc doesn't discard a generated world. The Esc ladder itself lives
/// in `ui::escape_ladder` (it also needs the overlay state).
#[derive(Resource, Default)]
pub struct PendingMenuQuit(pub bool);

pub fn handle_inspector_tabs(
    mut interactions: TabButtonInteraction<'_, '_>,
    mut tab: ResMut<InspectorTab>,
) {
    for (interaction, button, mut bg) in &mut interactions {
        if *interaction == Interaction::Pressed {
            *tab = button.0;
        }
        bg.0 = if button.0 == *tab {
            TAB_ACTIVE
        } else {
            TAB_IDLE
        };
    }
}

pub fn refresh_tab_colors(
    tab: Res<InspectorTab>,
    mut buttons: Query<(&InspectorTabButton, &mut BackgroundColor)>,
) {
    if !tab.is_changed() {
        return;
    }
    for (button, mut bg) in &mut buttons {
        bg.0 = if button.0 == *tab {
            TAB_ACTIVE
        } else {
            TAB_IDLE
        };
    }
}

pub fn update_hex_tooltip(
    hovered: Res<HoveredHex>,
    selected: Res<SelectedHex>,
    world_res: Option<Res<WorldResource>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut root: TooltipRootQuery<'_, '_>,
    mut text: TooltipTextQuery<'_, '_>,
) {
    let Ok((mut node, mut visibility)) = root.single_mut() else {
        return;
    };
    let Ok(mut label) = text.single_mut() else {
        return;
    };

    // Hide tooltip when inspector covers the same hex (less clutter).
    let show = match (hovered.0, world_res.as_ref()) {
        (Some(hex), Some(world)) if selected.0 != Some(hex) => Some((hex, world)),
        _ => None,
    };

    if let Some((hex, world)) = show {
        *label = Text::new(format_hover_text(&world.0.data, hex));
        if let Ok(window) = window_query.single()
            && let Some(cursor) = window.cursor_position()
        {
            let x = (cursor.x + 14.0).min(window.width() - 290.0).max(0.0);
            let y = (cursor.y + 14.0).min(window.height() - 120.0).max(0.0);
            node.left = Val::Px(x);
            node.top = Val::Px(y);
        }
        node.display = Display::Flex;
        *visibility = Visibility::Visible;
    } else {
        node.display = Display::None;
        *visibility = Visibility::Hidden;
        *label = Text::new("");
    }
}

pub fn update_hex_inspector(
    selected: Res<SelectedHex>,
    visible: Res<InspectorVisible>,
    tab: Res<InspectorTab>,
    world_res: Option<Res<WorldResource>>,
    biology: Option<Res<ActiveBiologyView>>,
    mut root: Query<&mut Node, With<HexInspectorRoot>>,
    mut header: Query<&mut Text, (With<HexInspectorHeader>, Without<HexInspectorBody>)>,
    mut body: Query<&mut Text, (With<HexInspectorBody>, Without<HexInspectorHeader>)>,
) {
    let Ok(mut node) = root.single_mut() else {
        return;
    };
    let show = selected.0.is_some() && visible.0 && world_res.is_some();
    node.display = if show { Display::Flex } else { Display::None };
    if !show {
        return;
    }
    let hex = selected.0.expect("checked");
    let data = &world_res.expect("checked").0.data;
    if let Ok(mut h) = header.single_mut() {
        *h = Text::new(format_inspector_header(data, hex));
    }
    if let Ok(mut b) = body.single_mut() {
        let bio = biology.as_ref().map(|v| v.0.as_ref());
        *b = Text::new(format_inspector_tab(data, hex, *tab, bio));
    }
}

pub fn clear_inspect_on_exit(mut hovered: ResMut<HoveredHex>, mut selected: ResMut<SelectedHex>) {
    hovered.0 = None;
    selected.0 = None;
}

pub(crate) fn despawn_hex_inspect_ui(mut commands: Commands, roots: InspectUiRoots<'_, '_>) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}

// ---------------------------------------------------------------------------
// Formatters (unit-tested)
// ---------------------------------------------------------------------------

pub fn format_hover_text(data: &WorldData, hex: HexId) -> String {
    let i = hex.0 as usize;
    let (lat, lon) = data.grid.center_lat_lon(hex);
    let elev = data.elevation_mean[i];
    let sea = data.sea_level_m;
    let temp = data.temperature_mean[i];
    let precip = data.precipitation[i];
    let regime = format!("{:?}", data.climate_regime[i]);
    let surface = surface_summary(data, i);
    // Lead with height above sea — the only number that answers "is this land,
    // and how high?". `sea_level_m` sits ~1 km below the absolute datum in deep
    // time, so the raw `Elev` reads negative on ordinary dry land; show it as
    // secondary context, never first.
    format!(
        "Hex {}\n{:.1}°N  {:.1}°E\n{:+.0} m above sea  (Elev {:.0} m abs, sea {:.0} m)\n{}\nTemp {:.1} °C\nPrecip {:.0} mm/yr\nRegime {}",
        hex.0,
        lat.to_degrees(),
        lon.to_degrees(),
        elev - sea,
        elev,
        sea,
        surface,
        temp,
        precip,
        regime
    )
}

pub fn format_inspector_header(data: &WorldData, hex: HexId) -> String {
    let (lat, lon) = data.grid.center_lat_lon(hex);
    format!(
        "Hex {} — {:.2}°, {:.2}°",
        hex.0,
        lat.to_degrees(),
        lon.to_degrees()
    )
}

pub fn format_inspector_tab(
    data: &WorldData,
    hex: HexId,
    tab: InspectorTab,
    biology: Option<&dyn BiologyView>,
) -> String {
    let i = hex.0 as usize;
    match tab {
        InspectorTab::Terrain => format_terrain(data, i),
        InspectorTab::Climate => format_climate(data, i),
        InspectorTab::Water => format_water(data, i),
        InspectorTab::Life => format_life(data, hex, biology),
        InspectorTab::Society => "Society\nNot simulated yet.".to_string(),
        InspectorTab::Debug => format_debug(data, hex, i),
    }
}

fn surface_summary(data: &WorldData, i: usize) -> String {
    let elev = data.elevation_mean[i];
    let water = data.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
    let ice = data.ice_mask.get(i).copied().unwrap_or(false)
        || data.ice_load_m.get(i).copied().unwrap_or(0.0) > 0.0
        || data
            .hydro_flags
            .get(i)
            .is_some_and(|f| f.contains(HydroFlags::SEA_ICE));
    // Ice is climate/hydrology cover, not a bedrock surface. Report the
    // underlying Land/Water first, then flag ice — a snow-capped peak is Land,
    // a frozen sea is Water.
    let wet = water > elev && water.is_finite();
    let base = if wet {
        format!("Water (depth {:.0} m)", water - elev)
    } else {
        "Land".to_string()
    };
    match (ice, wet) {
        (true, true) => format!("{base} · sea ice"),
        (true, false) => format!("{base} · ice cover"),
        (false, _) => base,
    }
}

fn format_terrain(data: &WorldData, i: usize) -> String {
    let elev = data.elevation_mean[i];
    let sea = data.sea_level_m;
    let relief = data.elevation_relief.get(i).copied().unwrap_or(0.0);
    format!(
        "Above sea {:+.0} m\nElevation {:.0} m (abs datum)\nSea level {:.0} m\nRelief {:.0} m\nSurface {}",
        elev - sea,
        elev,
        sea,
        relief,
        surface_summary(data, i)
    )
}

fn format_climate(data: &WorldData, i: usize) -> String {
    let t = data.temperature_mean[i];
    let tr = data.temperature_range.get(i).copied().unwrap_or(0.0);
    let p = data.precipitation[i];
    let regime = format!(
        "{:?}",
        data.climate_regime.get(i).copied().unwrap_or_default()
    );
    // temperature_range / wind are not in HistoryFrame — omit from this tab.
    let _ = tr;
    format!("Temp mean {t:.1} °C\nPrecip {p:.0} mm/yr\nRegime {regime}")
}

fn format_water(data: &WorldData, i: usize) -> String {
    let elev = data.elevation_mean[i];
    let water = data.water_level_m.get(i).copied().unwrap_or(WATER_NONE);
    let depth = if water > elev && water.is_finite() {
        format!("{:.0} m", water - elev)
    } else {
        "dry".into()
    };
    let body_id = data
        .water_body_id
        .get(i)
        .copied()
        .unwrap_or(WaterBodyId::NONE);
    let body_kind = if body_id == WaterBodyId::NONE {
        "none".into()
    } else {
        data.water_bodies
            .get(&body_id)
            .map(|b| format!("{:?}", b.kind))
            .unwrap_or_else(|| "unknown".into())
    };
    let discharge = data.river_discharge_m3_yr.get(i).copied().unwrap_or(0.0);
    let class = format!("{:?}", river_class(f64::from(discharge)));
    let flow = data
        .flow_direction
        .get(i)
        .and_then(|d| *d)
        .map(|d| format!("{d:?}"))
        .unwrap_or_else(|| "—".into());
    let flags = data
        .hydro_flags
        .get(i)
        .copied()
        .unwrap_or(HydroFlags::NONE)
        .0;
    let salt = data.salt_accumulated.get(i).copied().unwrap_or(0.0);
    let ice = data.ice_load_m.get(i).copied().unwrap_or(0.0);
    format!(
        "Depth {depth}\nBody {body_kind} (id {})\nDischarge {:.3e} m³/yr ({class})\nFlow {flow}\nFlags 0x{flags:04x}\nSalt {salt:.1}\nIce load {ice:.0} m",
        body_id.0, discharge
    )
}

fn format_life(data: &WorldData, hex: HexId, biology: Option<&dyn BiologyView>) -> String {
    let i = hex.0 as usize;
    let hab = data.habitability.get(i).copied().unwrap_or(0.0);
    let fert = data.soil_fertility.get(i).copied().unwrap_or(0.0);
    let Some(bio) = biology else {
        return format!("Habitability {hab:.2}\nSoil fertility {fert:.2}\nNot simulated yet.");
    };
    let a = bio.assemblage(data, hex);
    let near_cap = if a.richness > 0.85 { "  (near cap)" } else { "" };
    let mut s = format!(
        "Biome: {}\nRichness R {:.2}{near_cap}\nOccupied guilds {} / {}",
        a.biome_name, a.richness, a.occupied_guilds, a.guild_capacity,
    );
    if let Some(dominant) = a.species.first() {
        s.push_str(&format!(
            "\n\nDominant: {}\n  {} · {}\n  [{}]",
            dominant.name,
            dominant.guild,
            dominant.family,
            dominant.trait_chips.join(", "),
        ));
        s.push_str(&format!(
            "\n\nGenerate assemblage ({} guilds) →  [B]",
            a.occupied_guilds
        ));
    }
    s.push_str(&format!("\n\nHabitability {hab:.2} · Soil fert {fert:.2}"));
    s
}

fn format_debug(data: &WorldData, hex: HexId, i: usize) -> String {
    let plate = data.plate_id.get(i).map(|p| p.0).unwrap_or(u16::MAX);
    let bedrock = format!(
        "{:?}",
        data.bedrock_type
            .get(i)
            .copied()
            .unwrap_or(BedrockType::Unknown)
    );
    let crust = data.continental_crust.get(i).copied().unwrap_or(false);
    let wind_spd = data.wind_speed_m_s.get(i).copied().unwrap_or(0.0);
    let dist = data.distance_to_ocean_km.get(i).copied().unwrap_or(0.0);
    format!(
        "HexId {}\nplate_id {plate}\nbedrock {bedrock}\ncontinental_crust {crust}\nwind_speed {wind_spd:.1} m/s\ndistance_to_ocean_km {dist:.0}\n\nlive world (may not match scrub year)\nplate/bedrock/wind not in HistoryFrame",
        hex.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use genesis_core::parameters::WorldParameters;
    use genesis_core::{HexId, create_world};

    #[test]
    fn hover_text_includes_elevation_and_temp() {
        let params = WorldParameters::default();
        let world = create_world(params).expect("world");
        let text = format_hover_text(&world.data, HexId(0));
        assert!(text.contains("Hex 0"));
        assert!(text.contains("Elev"));
        assert!(text.contains("Temp"));
        // Height above sea must lead; the absolute datum stays secondary.
        assert!(text.contains("above sea"));
        let sea_pos = text.find("above sea").unwrap();
        let abs_pos = text.find("abs").unwrap();
        assert!(sea_pos < abs_pos, "sea-relative height must precede absolute");
    }

    #[test]
    fn terrain_tab_shows_sea_level_and_leads_with_above_sea() {
        let params = WorldParameters::default();
        let world = create_world(params).expect("world");
        let text = format_inspector_tab(&world.data, HexId(2), InspectorTab::Terrain, None);
        assert!(text.contains("Above sea"));
        assert!(text.contains("Sea level"));
        assert!(
            text.find("Above sea").unwrap() < text.find("Elevation").unwrap(),
            "Terrain tab must lead with above-sea height"
        );
    }

    #[test]
    fn snow_capped_peak_reads_as_land_with_ice_flag() {
        let params = WorldParameters::default();
        let mut world = create_world(params).expect("world");
        // Dry land, sea below it, ice on top → "Land · ice cover", not "Ice".
        world.data.sea_level_m = -1000.0;
        world.data.elevation_mean[0] = 3000.0;
        world.data.water_level_m[0] = WATER_NONE;
        if !world.data.ice_mask.is_empty() {
            world.data.ice_mask[0] = true;
        }
        let text = surface_summary(&world.data, 0);
        assert!(text.starts_with("Land"), "peak is land, got {text}");
        assert!(text.contains("ice"), "ice must still be flagged, got {text}");
    }

    #[test]
    fn terrain_tab_mentions_relief() {
        let params = WorldParameters::default();
        let world = create_world(params).expect("world");
        let text = format_inspector_tab(&world.data, HexId(1), InspectorTab::Terrain, None);
        assert!(text.contains("Relief"));
    }

    #[test]
    fn society_tab_is_placeholder() {
        let params = WorldParameters::default();
        let world = create_world(params).expect("world");
        let text = format_inspector_tab(&world.data, HexId(0), InspectorTab::Society, None);
        assert!(text.contains("Not simulated yet"));
    }
}
