//! How the highway looks, drawn in the world so it glows (see `stage.rs`):
//! the lane field, the hit line, receptors shaped like their buttons, notes
//! with a white-hot core and their button's shape on them, holds, and the
//! bursts of light a hit throws. The rhythm screen says where things are; this
//! module makes them.
//!
//! Every lane wears its button's shape as well as its colour (←, ↑, ↓, →, □,
//! △, ×, ○), so no one needs to tell the colours apart.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::sprite_render::AlphaMode2d;
use wu_input::Button;

/// Draw order in the world, back to front (the venue is far behind, at −900).
pub const Z_FIELD: f32 = 0.0;
pub const Z_LANE_GLOW: f32 = 1.0;
pub const Z_BAND: f32 = 2.0;
pub const Z_BEAM: f32 = 3.0;
pub const Z_HIT_LINE: f32 = 4.0;
pub const Z_RECEPTOR: f32 = 5.0;
pub const Z_NOTE: f32 = 10.0;
pub const Z_BURST: f32 = 20.0;

/// How much brighter than white the lights are: what blooms.
pub const GEM_GLOW: f32 = 1.7;
pub const CORE_GLOW: f32 = 2.6;

/// Meshes and materials shared by everything on the highway.
#[derive(Resource)]
pub struct Looks {
    /// A 1 × 1 square, scaled to any rectangle.
    pub unit: Handle<Mesh>,
    /// A thin ring a burst grows from.
    pub ring: Handle<Mesh>,
    /// White fading to nothing upwards: beams and the lanes' glow.
    pub fade: Handle<Image>,
    /// Each lane's button shape, as pieces: a mesh and where it sits, for a
    /// shape about one unit across.
    pub glyphs: Vec<Vec<(Handle<Mesh>, Transform)>>,
    /// Near-black, for a shape drawn on a lit note.
    pub ink: Handle<ColorMaterial>,
    /// White-hot, for a note's core.
    pub core: Handle<ColorMaterial>,
}

impl Looks {
    pub fn new(
        lanes: &[Button],
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<ColorMaterial>,
        images: &mut Assets<Image>,
    ) -> Looks {
        Looks {
            unit: meshes.add(Rectangle::new(1.0, 1.0)),
            ring: meshes.add(Annulus::new(0.42, 0.5)),
            fade: images.add(fade_up()),
            glyphs: lanes.iter().map(|&button| glyph(button, meshes)).collect(),
            ink: materials.add(ColorMaterial::from(Color::srgb(0.02, 0.015, 0.035))),
            core: materials.add(ColorMaterial::from(glowing(Color::WHITE, CORE_GLOW))),
        }
    }

    /// Spawns lane `lane`'s shape, `size` across, in `material`, as children of
    /// `parent`, at `z` above it.
    pub fn spawn_glyph(
        &self,
        parent: &mut ChildSpawnerCommands,
        lane: usize,
        size: f32,
        z: f32,
        material: &Handle<ColorMaterial>,
        marker: impl Bundle + Clone,
    ) {
        for (mesh, at) in &self.glyphs[lane] {
            parent.spawn((
                Mesh2d(mesh.clone()),
                MeshMaterial2d(material.clone()),
                Transform {
                    translation: (at.translation.truncate() * size).extend(z),
                    rotation: at.rotation,
                    scale: Vec3::new(size, size, 1.0),
                },
                marker.clone(),
            ));
        }
    }
}

/// `colour`, `brightness` times brighter than white allows: it blooms.
pub fn glowing(colour: Color, brightness: f32) -> Color {
    let linear = colour.to_linear();
    Color::LinearRgba(LinearRgba::new(
        linear.red * brightness,
        linear.green * brightness,
        linear.blue * brightness,
        linear.alpha,
    ))
}

/// `colour` at `alpha`, blended over what is behind.
pub fn see_through(colour: Color, alpha: f32) -> ColorMaterial {
    ColorMaterial {
        color: colour.with_alpha(alpha),
        alpha_mode: AlphaMode2d::Blend,
        ..default()
    }
}

/// A texture white at the bottom, fading to nothing at the top.
fn fade_up() -> Image {
    const HEIGHT: u32 = 64;
    let data = (0..HEIGHT)
        .flat_map(|row| {
            // Row 0 is the top of the texture.
            let up = 1.0 - row as f32 / (HEIGHT - 1) as f32;
            let alpha = ((1.0 - up).powf(2.2) * 255.0).round() as u8;
            [255, 255, 255, alpha]
        })
        .collect();
    Image::new(
        Extent3d {
            width: 1,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A button's shape, about one unit across, as meshes and where they sit.
fn glyph(button: Button, meshes: &mut Assets<Mesh>) -> Vec<(Handle<Mesh>, Transform)> {
    const STROKE: f32 = 0.16;
    let mut pieces = Vec::new();
    let mut bar = |meshes: &mut Assets<Mesh>, from: Vec2, to: Vec2| {
        let along = to - from;
        pieces.push((
            meshes.add(Rectangle::new(along.length() + STROKE * 0.9, STROKE)),
            Transform {
                translation: ((from + to) / 2.0).extend(0.0),
                rotation: Quat::from_rotation_z(along.y.atan2(along.x)),
                ..default()
            },
        ));
    };
    let arrow = |turn: f32, meshes: &mut Assets<Mesh>| {
        // Pointing left, then turned.
        let rotation = Quat::from_rotation_z(turn);
        vec![
            (
                meshes.add(Triangle2d::new(
                    Vec2::new(-0.5, 0.0),
                    Vec2::new(0.0, 0.45),
                    Vec2::new(0.0, -0.45),
                )),
                Transform::from_rotation(rotation),
            ),
            (
                meshes.add(Rectangle::new(0.5, 0.24)),
                Transform {
                    translation: rotation * Vec3::new(0.24, 0.0, 0.0),
                    rotation,
                    ..default()
                },
            ),
        ]
    };
    match button {
        Button::DPadLeft => return arrow(0.0, meshes),
        Button::DPadUp => return arrow(-std::f32::consts::FRAC_PI_2, meshes),
        Button::DPadDown => return arrow(std::f32::consts::FRAC_PI_2, meshes),
        Button::DPadRight => return arrow(std::f32::consts::PI, meshes),
        Button::West => {
            let c = 0.38;
            for (from, to) in [
                (Vec2::new(-c, -c), Vec2::new(c, -c)),
                (Vec2::new(c, -c), Vec2::new(c, c)),
                (Vec2::new(c, c), Vec2::new(-c, c)),
                (Vec2::new(-c, c), Vec2::new(-c, -c)),
            ] {
                bar(meshes, from, to);
            }
        }
        Button::North => {
            let corner = |angle: f32| Vec2::new(angle.cos(), angle.sin()) * 0.5 + Vec2::new(0.0, -0.06);
            let [a, b, c] = [90.0f32, 210.0, 330.0].map(|degrees| corner(degrees.to_radians()));
            for (from, to) in [(a, b), (b, c), (c, a)] {
                bar(meshes, from, to);
            }
        }
        Button::South => {
            bar(meshes, Vec2::new(-0.38, -0.38), Vec2::new(0.38, 0.38));
            bar(meshes, Vec2::new(-0.38, 0.38), Vec2::new(0.38, -0.38));
        }
        Button::East => {
            pieces.push((meshes.add(Annulus::new(0.31, 0.47)), Transform::IDENTITY));
        }
        _ => pieces.push((meshes.add(Circle::new(0.3)), Transform::IDENTITY)),
    }
    pieces
}

/// A flash of light thrown by a hit: it grows and fades.
#[derive(Component)]
pub struct Burst {
    pub born_s: f32,
    pub life_s: f32,
    pub from_scale: Vec2,
    pub to_scale: Vec2,
    pub colour: LinearRgba,
}

/// Grows and fades every burst, and clears the spent ones.
pub fn fade_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut bursts: Query<(Entity, &Burst, &mut Transform, &MeshMaterial2d<ColorMaterial>)>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let now = time.elapsed_secs();
    for (entity, burst, mut transform, material) in &mut bursts {
        let age = ((now - burst.born_s) / burst.life_s).clamp(0.0, 1.0);
        if age >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        let eased = 1.0 - (1.0 - age).powi(3);
        transform.scale = burst.from_scale.lerp(burst.to_scale, eased).extend(1.0);
        if let Some(mut material) = materials.get_mut(&material.0) {
            let fade = (1.0 - age).powi(2);
            let c = burst.colour;
            material.color = Color::LinearRgba(LinearRgba::new(
                c.red * fade,
                c.green * fade,
                c.blue * fade,
                c.alpha * fade,
            ));
        }
    }
}
