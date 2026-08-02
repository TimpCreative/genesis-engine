//! End-to-end UI smoke driver (enabled by `GENESIS_UI_SMOKE_DIR`).
//!
//! Walks the real screen flow — menu → setup → generation → viewer — using
//! the same resources and systems as a user, screenshots each screen into the
//! smoke dir, scrubs the timeline, and exits. Gives CI and review loops visual
//! proof that the interactive shell works without needing input injection.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use genesis_render::{ColorsDirty, WorldResource};

use crate::ui::{ActiveConfig, AppScreen, GenerationTask, WorldTimeline, start_generation};

/// Frame-counter driven script state.
#[derive(Resource, Default)]
pub struct SmokeDriver {
    pub dir: String,
    frames_in_screen: u32,
    shots_taken: u32,
    scrub_steps_done: u32,
    last_screen: Option<AppScreen>,
}

pub struct SmokePlugin {
    pub dir: String,
}

impl Plugin for SmokePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SmokeDriver {
            dir: self.dir.clone(),
            ..Default::default()
        })
        .add_systems(Update, drive_smoke);
    }
}

const SETTLE_FRAMES: u32 = 12;

#[allow(clippy::too_many_arguments)]
fn drive_smoke(
    mut commands: Commands,
    mut driver: ResMut<SmokeDriver>,
    screen: Res<State<AppScreen>>,
    mut next_screen: ResMut<NextState<AppScreen>>,
    mut config: ResMut<ActiveConfig>,
    task: Option<Res<GenerationTask>>,
    timeline: Option<ResMut<WorldTimeline>>,
    world_res: Option<ResMut<WorldResource>>,
    colors_dirty: Option<ResMut<ColorsDirty>>,
    mut exit: MessageWriter<AppExit>,
) {
    let current = *screen.get();
    if driver.last_screen != Some(current) {
        driver.last_screen = Some(current);
        driver.frames_in_screen = 0;
    } else {
        driver.frames_in_screen += 1;
    }
    if driver.frames_in_screen != SETTLE_FRAMES {
        // Screenshot/act once per screen after a settle delay; scrubbing below
        // uses larger frame marks within Viewing.
        if current != AppScreen::Viewing || !driver.frames_in_screen.is_multiple_of(SETTLE_FRAMES) {
            return;
        }
    }

    match current {
        AppScreen::MainMenu => {
            snapshot(&mut commands, &mut driver, "1_menu");
            next_screen.set(AppScreen::Setup);
        }
        AppScreen::Setup => {
            // Small, fast world; 1B years gives visible plate drift between
            // the first and last frames when scrubbing.
            config.0.subdivision_level = 5;
            config.0.target_year = 1_000_000_000;
            config.0.seed_text = "42".to_string();
            snapshot(&mut commands, &mut driver, "2_setup");
            start_generation(&mut commands, config.0.clone());
            next_screen.set(AppScreen::Generating);
        }
        AppScreen::Generating => {
            if driver.shots_taken < 3 {
                snapshot(&mut commands, &mut driver, "3_generating");
            }
            // poll_generation flips the state when the thread reports Done.
            let _ = task;
        }
        AppScreen::Viewing => {
            let (Some(mut timeline), Some(mut world_res), Some(mut colors_dirty)) =
                (timeline, world_res, colors_dirty)
            else {
                return;
            };
            // Screenshots capture asynchronously a frame or two later, so each
            // shot and the next mutation happen in separate steps.
            match driver.scrub_steps_done {
                // While (probably) still buffering: capture the partial bar.
                0 => {
                    snapshot(&mut commands, &mut driver, "4_viewing_buffering");
                    driver.scrub_steps_done = 1;
                }
                // Wait for generation to finish, then jump to the final year.
                1 => {
                    if !timeline.complete {
                        return;
                    }
                    timeline.playing = false;
                    timeline.current = timeline.frames.len().saturating_sub(1);
                    if let Some(frame) = timeline.frames.get(timeline.current) {
                        frame.apply(&mut world_res.0.data);
                        colors_dirty.0 = true;
                    }
                    driver.scrub_steps_done = 2;
                }
                2 => {
                    snapshot(&mut commands, &mut driver, "5_viewing_final_year");
                    driver.scrub_steps_done = 3;
                }
                3 => {
                    timeline.current = 0;
                    if let Some(frame) = timeline.frames.first() {
                        frame.apply(&mut world_res.0.data);
                        colors_dirty.0 = true;
                    }
                    driver.scrub_steps_done = 4;
                }
                4 => {
                    snapshot(&mut commands, &mut driver, "6_viewing_scrubbed_to_start");
                    driver.scrub_steps_done = 5;
                }
                _ => {
                    exit.write(AppExit::Success);
                }
            }
        }
    }
}

fn snapshot(commands: &mut Commands, driver: &mut SmokeDriver, name: &str) {
    let path = format!("{}/{name}.png", driver.dir);
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    driver.shots_taken += 1;
}
