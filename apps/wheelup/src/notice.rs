//! The first thing on screen: the photosensitivity notice, every launch. Then
//! the songs.

use bevy::prelude::*;

use crate::fonts::Fonts;
use crate::input::RawInput;
use crate::palette;
use crate::screens::Screen;
use crate::settings::SettingsStore;
use crate::songs_screen::{MenuKey, menu_keys};
use crate::ui::{centred_label, centred_on, screen_root};
use crate::words::tr;

/// Long enough to see what it is before a press can skip it.
const READ_S: f32 = 1.5;

#[derive(Debug)]
pub struct NoticePlugin;

impl Plugin for NoticePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Notice), enter)
            .add_systems(Update, carry_on.run_if(in_state(Screen::Notice)));
    }
}

#[derive(Resource, Debug)]
struct Shown(f32);

fn enter(mut commands: Commands, fonts: Res<Fonts>, time: Res<Time<Real>>, settings: Res<SettingsStore>) {
    let language = settings.language();
    commands.insert_resource(Shown(time.elapsed_secs()));
    commands.spawn(screen_root(Screen::Notice)).with_children(|screen| {
        screen.spawn(centred_on(0.0, -230.0, 1000.0, 70.0)).with_child((
            Text::new("WHEEL UP!"),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(52.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
        screen.spawn(centred_on(0.0, -150.0, 1000.0, 34.0)).with_child((
            Text::new(tr(language, "PHOTOSENSITIVITY WARNING")),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(24.0)
            },
            TextColor(palette::WARNING),
        ));
        screen
            .spawn(centred_on(0.0, -20.0, 900.0, 200.0))
            .with_child(centred_label(
                tr(
                    language,
                    "A very small share of people may have a seizure when they see certain light\n\
                 patterns or flashing lights, even with no history of epilepsy. If you or anyone\n\
                 in your family has an epileptic condition, ask a doctor before playing. Stop\n\
                 at once if you feel dizzy, your sight blurs, your eyes or muscles twitch or you\n\
                 feel disoriented, and see a doctor.\n\n\
                 WHEEL UP! never flashes more than three times a second: nothing flashes on the\n\
                 beat, the lights only swell. Play in a lit room, sitting back from the screen,\n\
                 and take a break every hour.",
                ),
                16.0,
                palette::INK,
            ));
        screen
            .spawn(centred_on(0.0, 150.0, 900.0, 24.0))
            .with_child(centred_label(
                tr(language, "✕ / Space: carry on"),
                16.0,
                palette::FLYER_YELLOW,
            ));
    });
}

/// A press, once the notice has been up long enough: the songs.
fn carry_on(
    mut raw: MessageReader<RawInput>,
    time: Res<Time<Real>>,
    shown: Res<Shown>,
    mut next: ResMut<NextState<Screen>>,
) {
    let keys = menu_keys(&mut raw);
    if time.elapsed_secs() - shown.0 >= READ_S && keys.contains(&MenuKey::Confirm) {
        next.set(Screen::Songs);
    }
}
