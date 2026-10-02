//! The stage: the camera, and the venue behind every screen. It is drawn in
//! the world, in high dynamic range, so whatever burns brighter than white
//! (a laser, the mast's light, a note) blooms into a neon glow; the text on
//! top is the interface's and stays crisp.
//!
//! Screens tell the venue how the night is going through [`StageMood`]: the
//! kick's pulse, how much is going on, lasers for a drop, a WHEEL UP!'s flash.

use bevy::asset::{AssetPath, embedded_asset, embedded_path};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::{Bloom, BloomCompositeMode, BloomPrefilter};
use bevy::post_process::effect_stack::ChromaticAberration;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{Material2d, Material2dPlugin};

use crate::settings::SettingsStore;

#[derive(Debug)]
pub struct StagePlugin;

impl Plugin for StagePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "venue.wgsl");
        app.add_plugins(Material2dPlugin::<VenueMaterial>::default())
            .init_resource::<StageMood>()
            .add_systems(Startup, spawn_stage)
            .add_systems(Update, (light_the_venue, split_the_picture));
    }
}

/// Where the night is: the stop of the Pirate Radio Tour a tune belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scene {
    /// The pirate station's rooftop: the city at night.
    #[default]
    Rooftop,
    Bedroom,
    Warehouse,
    /// Two stacks face to face across a yard.
    Clash,
}

impl Scene {
    /// A tour stop's scene, by its id; the rooftop for one not drawn yet.
    pub fn of_stop(id: &str) -> Scene {
        match id {
            "bedroom-studio" => Scene::Bedroom,
            "warehouse-rave" => Scene::Warehouse,
            "sound-system-clash" => Scene::Clash,
            _ => Scene::Rooftop,
        }
    }

    /// The scene of the stop that plays `song`, if one does.
    pub fn of_song(tour: &wu_content::tour::Tour, song: &str) -> Scene {
        tour.venues
            .iter()
            .find(|venue| venue.set.iter().chain(&venue.encore).any(|id| id == song))
            .map_or(Scene::Rooftop, |venue| Scene::of_stop(&venue.id))
    }

    /// As the shader numbers it.
    fn index(self) -> f32 {
        match self {
            Scene::Rooftop => 0.0,
            Scene::Bedroom => 1.0,
            Scene::Warehouse => 2.0,
            Scene::Clash => 3.0,
        }
    }
}

/// How the night is going, set by the screen in front of it.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct StageMood {
    /// The kick: 1 as it lands, falling away after (0–1).
    pub pulse: f32,
    /// How much is going on: a drop is 1, a menu or a breakdown far less (0–1).
    pub intensity: f32,
    /// Lasers, for a drop (0–1).
    pub lasers: f32,
    /// A WHEEL UP!, flaring once and fading (0–1).
    pub flash: f32,
    /// The hype meter (0–1): it warms the night toward gold.
    pub hype: f32,
    /// Where: the rooftop, a bedroom studio, a warehouse…
    pub scene: Scene,
}

impl Default for StageMood {
    /// A quiet night: the city, the mast, nothing on.
    fn default() -> StageMood {
        StageMood {
            pulse: 0.0,
            intensity: 0.15,
            lasers: 0.0,
            flash: 0.0,
            hype: 0.0,
            scene: Scene::Rooftop,
        }
    }
}

/// How fast intensity and lasers follow the mood (time constant, seconds): a
/// drop's lights come up over a beat or so rather than snapping on.
const SETTLE_S: f32 = 0.35;

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct VenueMaterial {
    #[uniform(0)]
    mood: VenueUniform,
}

#[derive(ShaderType, Clone, Copy, Debug, Default)]
struct VenueUniform {
    /// Window size in pixels, seconds, the kick's pulse.
    a: Vec4,
    /// Intensity, lasers, flash, hype.
    b: Vec4,
    /// Motion (1, or 0 for reduced motion), the scene, unused.
    c: Vec4,
}

impl Material2d for VenueMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Path(AssetPath::from_path_buf(embedded_path!("venue.wgsl")).with_source("embedded"))
    }
}

/// The backdrop's entity.
#[derive(Component)]
struct Backdrop;

/// Behind everything else in the world.
const VENUE_Z: f32 = -900.0;

fn spawn_stage(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<VenueMaterial>>) {
    commands.spawn((
        Camera2d,
        // Neon: only what burns brighter than white bleeds light, added on
        // top, so the night stays black and the lights glow. Tonemapped
        // gently, so a white-hot core keeps its colour at the edges.
        Bloom {
            intensity: 0.28,
            // A tight halo: the wide, low-frequency glow would wash a haze over
            // the whole highway, coloured by whatever notes are on it.
            low_frequency_boost: 0.1,
            low_frequency_boost_curvature: 0.4,
            prefilter: BloomPrefilter {
                threshold: 1.0,
                threshold_softness: 0.2,
            },
            composite_mode: BloomCompositeMode::Additive,
            ..Bloom::NATURAL
        },
        Tonemapping::SomewhatBoringDisplayTransform,
        // Off until a WHEEL UP! flares: then the picture splits like a tape
        // pulled off its heads.
        ChromaticAberration {
            intensity: 0.0,
            ..default()
        },
    ));
    commands.spawn((
        Backdrop,
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial2d(materials.add(VenueMaterial {
            mood: VenueUniform {
                c: Vec4::new(1.0, 0.0, 0.0, 0.0),
                ..default()
            },
        })),
        Transform::from_xyz(0.0, 0.0, VENUE_Z),
    ));
}

/// Fits the venue to the window and hands it the mood, as the player's
/// settings allow it: the flare as bright as they chose, motion or not.
fn light_the_venue(
    time: Res<Time>,
    mood: Res<StageMood>,
    settings: Res<SettingsStore>,
    windows: Query<&Window>,
    mut venue: Query<(&mut Transform, &MeshMaterial2d<VenueMaterial>), With<Backdrop>>,
    mut materials: ResMut<Assets<VenueMaterial>>,
) {
    let Ok(window) = windows.single() else { return };
    let Ok((mut transform, material)) = venue.single_mut() else {
        return;
    };
    let size = window.size();
    transform.scale = size.extend(1.0);
    let Some(mut material) = materials.get_mut(&material.0) else {
        return;
    };
    let uniform = &mut material.mood;
    let follow = 1.0 - (-time.delta_secs() / SETTLE_S).exp();
    uniform.a = Vec4::new(size.x, size.y, time.elapsed_secs_wrapped(), mood.pulse);
    uniform.b = Vec4::new(
        uniform.b.x + (mood.intensity - uniform.b.x) * follow,
        uniform.b.y + (mood.lasers - uniform.b.y) * follow,
        mood.flash * settings.flare(),
        uniform.b.w + (mood.hype - uniform.b.w) * follow,
    );
    uniform.c.x = if settings.reduced_motion() { 0.0 } else { 1.0 };
    uniform.c.y = mood.scene.index();
}

/// How far a WHEEL UP!'s flare splits the colours apart, at its height.
const FLARE_ABERRATION: f32 = 0.045;

fn split_the_picture(mood: Res<StageMood>, settings: Res<SettingsStore>, mut cameras: Query<&mut ChromaticAberration>) {
    for mut aberration in &mut cameras {
        let intensity = FLARE_ABERRATION * mood.flash * settings.flare();
        if (aberration.intensity - intensity).abs() > 1e-4 {
            aberration.intensity = intensity;
        }
    }
}
