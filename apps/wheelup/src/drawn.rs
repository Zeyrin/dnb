//! Whether the renderer has caught up. The first time something is drawn its
//! shaders compile in the background, and until they have it isn't drawn at
//! all: a song waits for its stage to be on screen before the music starts.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use bevy::render::render_resource::PipelineCache;
use bevy::render::{Render, RenderApp, RenderSystems};

/// How many frames in a row everything asked for has been drawn.
#[derive(Resource, Debug, Default)]
pub struct Drawn {
    settled: Arc<AtomicBool>,
    frames: u32,
}

impl Drawn {
    /// Whether everything on screen has been drawn for `frames` frames in a row.
    pub fn for_frames(&self, frames: u32) -> bool {
        self.frames >= frames
    }

    /// Counts again from now: something new is about to be drawn.
    pub fn restart(&mut self) {
        self.frames = 0;
    }
}

/// The render world's half: nothing left compiling after its last frame.
#[derive(Resource)]
struct Settled(Arc<AtomicBool>);

#[derive(Debug)]
pub struct DrawnPlugin;

impl Plugin for DrawnPlugin {
    fn build(&self, app: &mut App) {
        let settled = Arc::new(AtomicBool::new(false));
        app.insert_resource(Drawn {
            settled: settled.clone(),
            frames: 0,
        })
        .add_systems(First, count);
        match app.get_sub_app_mut(RenderApp) {
            Some(render_app) => {
                render_app
                    .insert_resource(Settled(settled))
                    .add_systems(Render, note.in_set(RenderSystems::Cleanup));
            }
            // Nothing renders: nothing to wait for.
            None => settled.store(true, Ordering::Relaxed),
        }
    }
}

fn note(cache: Res<PipelineCache>, settled: Res<Settled>) {
    settled
        .0
        .store(cache.waiting_pipelines().next().is_none(), Ordering::Relaxed);
}

fn count(mut drawn: ResMut<Drawn>) {
    drawn.frames = if drawn.settled.load(Ordering::Relaxed) {
        drawn.frames.saturating_add(1)
    } else {
        0
    };
}
