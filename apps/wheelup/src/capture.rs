//! `--screenshot`: render a few frames, save the window to a PNG, quit.
//! Lets a headless machine (CI, an agent under Xvfb) show what the game looks like.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

#[derive(Debug)]
pub struct CapturePlugin {
    pub path: PathBuf,
    pub after_frames: u32,
}

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Capture {
            path: self.path.clone(),
            frames_left: self.after_frames,
            saved: false,
        })
        .add_systems(Update, capture);
    }
}

#[derive(Resource)]
struct Capture {
    path: PathBuf,
    frames_left: u32,
    saved: bool,
}

#[derive(Component)]
struct PendingShot;

fn capture(
    mut commands: Commands,
    mut state: ResMut<Capture>,
    pending: Query<(), With<PendingShot>>,
    mut exit: MessageWriter<AppExit>,
) {
    if state.saved {
        exit.write(AppExit::Success);
        return;
    }
    if state.frames_left > 0 {
        state.frames_left -= 1;
        return;
    }
    if pending.is_empty() {
        commands
            .spawn((Screenshot::primary_window(), PendingShot))
            .observe(save_to_disk(state.path.clone()))
            .observe(|_: On<ScreenshotCaptured>, mut state: ResMut<Capture>| state.saved = true);
    }
}
