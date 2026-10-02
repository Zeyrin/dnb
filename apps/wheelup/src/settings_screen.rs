//! The SETTINGS screen: the language, the controller layout, how fast notes
//! fall, how the song sounds, and what the picture may do (the WHEEL UP! flare,
//! motion). Every change is saved at once.

use bevy::prelude::*;
use wu_content::settings::{AudioMode, FLARES, Language, NOTE_SPEEDS};
use wu_input::Layout;

use crate::input::{InputLink, RawInput};
use crate::palette;
use crate::screens::Screen;
use crate::settings::SettingsStore;
use crate::songs_screen::{MenuKey, menu_keys};
use crate::ui::{centred_label, centred_on, label, screen_root};

#[derive(Debug)]
pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsRow>()
            .add_systems(OnEnter(Screen::Settings), enter)
            .add_systems(Update, (navigate, show).chain().run_if(in_state(Screen::Settings)));
    }
}

#[derive(Resource, Debug, Default)]
struct SettingsRow(usize);

/// What the rows set, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setting {
    Language,
    Layout,
    NoteSpeed,
    Audio,
    Flare,
    Motion,
}

const ROWS: [Setting; 6] = [
    Setting::Language,
    Setting::Layout,
    Setting::NoteSpeed,
    Setting::Audio,
    Setting::Flare,
    Setting::Motion,
];

#[derive(Component)]
enum Part {
    Row(usize),
    About,
}

fn enter(mut commands: Commands, mut row: ResMut<SettingsRow>) {
    row.set_changed();
    commands.spawn(screen_root(Screen::Settings)).with_children(|screen| {
        screen
            .spawn(centred_on(0.0, -175.0, 600.0, 20.0))
            .with_child(label("SETTINGS", 13.0, palette::MUTED));
        for (i, _) in ROWS.iter().enumerate() {
            screen
                .spawn(centred_on(0.0, -130.0 + i as f32 * 36.0, 760.0, 30.0))
                .with_child((Part::Row(i), label("", 19.0, palette::INK)));
        }
        screen
            .spawn(centred_on(0.0, 130.0, 900.0, 60.0))
            .with_child((Part::About, centred_label("", 15.0, palette::SIGNAL)));
        screen.spawn(centred_on(0.0, 290.0, 1000.0, 20.0)).with_child(label(
            "↑ ↓ choose · ← → change · saved at once",
            14.0,
            palette::MUTED,
        ));
    });
}

fn step<T: Copy + PartialEq>(options: &[T], current: T, by: i32) -> T {
    let i = options.iter().position(|&o| o == current).unwrap_or(0) as i32;
    options[(i + by).rem_euclid(options.len() as i32) as usize]
}

/// The offered value closest to `value` (the settings file may hold any).
fn nearest(options: &[f32], value: f32) -> f32 {
    options
        .iter()
        .copied()
        .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
        .unwrap_or(options[0])
}

fn navigate(
    mut raw: MessageReader<RawInput>,
    mut row: ResMut<SettingsRow>,
    mut settings: ResMut<SettingsStore>,
    mut input: NonSendMut<InputLink>,
) {
    for key in menu_keys(&mut raw) {
        let by = match key {
            MenuKey::Up => {
                row.0 = (row.0 + ROWS.len() - 1) % ROWS.len();
                continue;
            }
            MenuKey::Down => {
                row.0 = (row.0 + 1) % ROWS.len();
                continue;
            }
            MenuKey::Left => -1,
            MenuKey::Right | MenuKey::Confirm => 1,
            MenuKey::Back => continue,
        };
        match ROWS[row.0] {
            Setting::Language => {
                let language = step(&Language::ALL, settings.language(), by);
                settings.set_language(language);
            }
            Setting::Layout => {
                let layout = step(&Layout::ALL, input.layout(), by);
                input.set_layout(layout);
                settings.settings.layout = layout.name().to_owned();
                settings.save();
            }
            Setting::NoteSpeed => {
                let speed = step(&NOTE_SPEEDS, nearest(&NOTE_SPEEDS, settings.note_speed()), by);
                settings.set_note_speed(speed);
            }
            Setting::Audio => {
                let mode = match settings.audio_mode() {
                    AudioMode::Live => AudioMode::Classic,
                    AudioMode::Classic => AudioMode::Live,
                };
                settings.set_audio_mode(mode);
            }
            Setting::Flare => {
                let flare = step(&FLARES, nearest(&FLARES, settings.flare()), by);
                settings.set_flare(flare);
            }
            Setting::Motion => {
                let reduced = !settings.reduced_motion();
                settings.set_reduced_motion(reduced);
            }
        }
    }
}

fn layout_name(layout: Layout) -> &'static str {
    match layout {
        Layout::Reel => "Reel: ↑ kick, ↓ snare",
        Layout::Drummer => "Drummer: ↓ kick, ↑ snare",
    }
}

fn flare_name(flare: f32) -> &'static str {
    match nearest(&FLARES, flare) {
        f if f >= 1.0 => "full",
        f if f > 0.0 => "half",
        _ => "off",
    }
}

fn show(
    row: Res<SettingsRow>,
    settings: Res<SettingsStore>,
    input: NonSend<InputLink>,
    mut parts: Query<(&Part, &mut Text, &mut TextColor)>,
) {
    if !row.is_changed() && !settings.is_changed() {
        return;
    }
    let value = |setting: Setting| -> String {
        match setting {
            Setting::Language => settings.language().name().to_owned(),
            Setting::Layout => layout_name(input.layout()).to_owned(),
            Setting::NoteSpeed => format!("{}×", nearest(&NOTE_SPEEDS, settings.note_speed())),
            Setting::Audio => settings.audio_mode().name().to_owned(),
            Setting::Flare => flare_name(settings.flare()).to_owned(),
            Setting::Motion => if settings.reduced_motion() { "reduced" } else { "full" }.to_owned(),
        }
    };
    let name = |setting: Setting| match setting {
        Setting::Language => "Language",
        Setting::Layout => "Controller layout",
        Setting::NoteSpeed => "Note speed",
        Setting::Audio => "Audio",
        Setting::Flare => "WHEEL UP! flare",
        Setting::Motion => "Motion",
    };
    let about = match ROWS[row.0] {
        Setting::Language => "The language the game speaks.",
        Setting::Layout => {
            "Which D-pad button plays the kick: the reel's own mapping, or the kick under\n\
             the thumb's resting point like a drummer's foot."
        }
        Setting::NoteSpeed => "How fast notes fall: 1× shows two seconds of the song ahead, 2× one.",
        Setting::Audio => {
            "Live: your presses play your part. Classic: the whole song plays and a miss\n\
             mutes your part, for outputs too slow to play along to (Bluetooth, TVs)."
        }
        Setting::Flare => "The one warm glow and colour split a WHEEL UP! throws, as bright as you like.",
        Setting::Motion => {
            "Reduced: lasers and searchlights hold still, no tape band rolls by, the crowd\n\
             and the speakers stop moving, and no record leaps over the highway."
        }
    };
    for (part, mut text, mut colour) in &mut parts {
        match part {
            Part::Row(i) => {
                let setting = ROWS[*i];
                let selected = *i == row.0;
                text.0 = format!(
                    "{} {:<20} ◀ {:^28} ▶",
                    if selected { "›" } else { " " },
                    name(setting),
                    value(setting)
                );
                colour.0 = if selected { palette::FLYER_YELLOW } else { palette::INK };
            }
            Part::About => text.0 = about.to_owned(),
        }
    }
}
