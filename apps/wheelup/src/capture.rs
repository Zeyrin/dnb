//! Screenshots. F12 saves one to `screenshots/`; `--screenshot` renders a few
//! frames, saves the window and quits, so a headless machine (CI, an agent
//! under Xvfb) can show what the game looks like.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

#[derive(Debug)]
pub struct CapturePlugin {
    /// Save here after this many frames, then quit.
    pub auto: Option<(PathBuf, u32)>,
}

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, screenshot_on_f12);
        if let Some((path, after_frames)) = &self.auto {
            app.insert_resource(Capture {
                path: path.clone(),
                frames_left: *after_frames,
                saved: false,
            })
            .add_systems(Update, capture_and_quit);
        }
    }
}

fn screenshot_on_f12(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if keys.just_pressed(KeyCode::F12) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = PathBuf::from("screenshots").join(format!("wheelup-{stamp}.png"));
        if let Some(dir) = path.parent()
            && let Err(error) = std::fs::create_dir_all(dir)
        {
            warn!("screenshot folder: {error}");
        }
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
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

fn capture_and_quit(
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
