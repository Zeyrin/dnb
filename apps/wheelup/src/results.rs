//! The RESULTS screen: grade, score, the count of each judgement, and a
//! histogram of how early or late the hits were. The run's replay is saved.

use bevy::prelude::*;
use wu_game::judge::Judgement;
use wu_game::records::Outcome;
use wu_game::replay::{REPLAY_VERSION, Replay};

use crate::fonts::Fonts;
use crate::input::RawInput;
use crate::palette;
use crate::records::{RecordsStore, describe};
use crate::screens::Screen;
use crate::session::{LastRun, Session};
use crate::settings::SettingsStore;
use crate::songs_screen::{MenuKey, menu_keys};
use crate::ui::{centred_label, centred_on, label, screen_root};
use crate::words::{self, decimal, fill, tr};
use wu_content::settings::Language;

#[derive(Debug)]
pub struct ResultsPlugin;

impl Plugin for ResultsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Results), enter)
            .add_systems(Update, navigate.run_if(in_state(Screen::Results)));
    }
}

/// Histogram buckets: 10 ms wide, from 95 ms early to 95 ms late.
const BUCKET_MS: f64 = 10.0;
const BUCKETS: usize = 19;

fn histogram(offsets: &[f64]) -> [u32; BUCKETS] {
    let mut counts = [0; BUCKETS];
    for &offset in offsets {
        let index = ((offset + BUCKET_MS * BUCKETS as f64 / 2.0) / BUCKET_MS).floor();
        if (0.0..BUCKETS as f64).contains(&index) {
            counts[index as usize] += 1;
        }
    }
    counts
}

fn enter(
    mut commands: Commands,
    last: Option<Res<LastRun>>,
    fonts: Res<Fonts>,
    settings: Res<SettingsStore>,
    mut records: ResMut<RecordsStore>,
) {
    let Some(last) = last else { return };
    let language = settings.language();
    // What the run did to the record on this song at this difficulty.
    let (record, record_colour) = match records.submit(&last) {
        Outcome::First => (
            tr(language, "FIRST RECORD ON THIS TUNE").to_owned(),
            palette::FLYER_YELLOW,
        ),
        Outcome::Beaten(previous) => (
            fill(tr(language, "NEW BEST!   was {}"), &[&describe(&previous, language)]),
            palette::FLYER_YELLOW,
        ),
        Outcome::Kept(best) => (
            fill(tr(language, "best {}"), &[&describe(&best, language)]),
            palette::MUTED,
        ),
        Outcome::NotCounted if last.failed => (String::new(), palette::MUTED),
        Outcome::NotCounted if last.lesson => (
            tr(language, "lessons set no records: pick a tune and a difficulty next").to_owned(),
            palette::MUTED,
        ),
        Outcome::NotCounted => {
            let why = if last.autoplay {
                "the selecta bot played"
            } else if last.no_fail {
                "No-Fail was on"
            } else {
                "practice tempo"
            };
            (
                fill(tr(language, "no record: {}"), &[&tr(language, why)]),
                palette::MUTED,
            )
        }
    };
    let score = &last.score;
    let grade = if last.failed {
        "—".to_owned()
    } else {
        score.grade().label().to_owned()
    };
    let headline = tr(
        language,
        if last.failed {
            "PLUG PULLED"
        } else if last.lesson {
            "LESSON COMPLETE"
        } else {
            "TUNE COMPLETE"
        },
    );
    let counts: Vec<String> = Judgement::ALL
        .iter()
        .map(|j| format!("{} {}", j.label(), score.counts[j.index()]))
        .collect();
    let mean = if score.offsets_ms.is_empty() {
        0.0
    } else {
        score.offsets_ms.iter().sum::<f64>() / score.offsets_ms.len() as f64
    };
    let tendency = match mean {
        m if m > 4.0 => fill(tr(language, "on average {} ms late"), &[&format!("{m:.0}")]),
        m if m < -4.0 => fill(tr(language, "on average {} ms early"), &[&format!("{:.0}", -m)]),
        _ => tr(language, "right on the beat").to_owned(),
    };
    let saved = save_replay(&last, language);
    let bars = histogram(&score.offsets_ms);
    let tallest = bars.iter().copied().max().unwrap_or(0).max(1);

    commands.spawn(screen_root(Screen::Results)).with_children(|screen| {
        screen.spawn(centred_on(0.0, -175.0, 1000.0, 22.0)).with_child(label(
            format!(
                "{} · {} · {} %{}",
                last.title,
                if last.lesson {
                    tr(language, "Lesson")
                } else {
                    words::difficulty(language, last.difficulty)
                },
                last.tempo_percent,
                if last.autoplay {
                    tr(language, " · selecta bot")
                } else {
                    ""
                }
            ),
            15.0,
            palette::MUTED,
        ));
        screen.spawn(centred_on(0.0, -140.0, 1000.0, 36.0)).with_child((
            Text::new(headline),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(28.0)
            },
            TextColor(if last.failed { palette::WARNING } else { palette::SIGNAL }),
        ));
        screen
            .spawn(centred_on(0.0, -110.0, 1000.0, 22.0))
            .with_child(label(record, 16.0, record_colour));
        screen.spawn(centred_on(-300.0, -25.0, 220.0, 140.0)).with_child((
            Text::new(grade),
            TextFont {
                font: fonts.display.clone().into(),
                ..TextFont::from_font_size(110.0)
            },
            TextColor(palette::FLYER_YELLOW),
        ));
        let holds = score.holds_completed + score.holds_dropped;
        let holds = if holds > 0 {
            fill(
                tr(language, "\nholds      {} / {} kept to the end"),
                &[&score.holds_completed, &holds],
            )
        } else {
            String::new()
        };
        screen.spawn(centred_on(110.0, -28.0, 520.0, 150.0)).with_child(label(
            fill(
                tr(
                    language,
                    "score      {}\naccuracy   {} %\nmax combo  {} / {}\n{}\noverhits   {}{}",
                ),
                &[
                    &score.points,
                    &decimal(language, score.accuracy() * 100.0, 2),
                    &score.max_combo,
                    &last.notes,
                    &counts.join("   "),
                    &score.overhits,
                    &holds,
                ],
            ),
            17.0,
            palette::INK,
        ));
        for (i, &count) in bars.iter().enumerate() {
            let x = (i as f32 - (BUCKETS as f32 - 1.0) / 2.0) * 22.0;
            let height = 4.0 + 90.0 * count as f32 / tallest as f32;
            let centre = i == BUCKETS / 2;
            screen.spawn((
                centred_on(x, 150.0 - height / 2.0, 16.0, height),
                BackgroundColor(if centre { palette::FLYER_YELLOW } else { palette::SIGNAL }),
            ));
        }
        screen.spawn(centred_on(0.0, 172.0, 960.0, 18.0)).with_child(label(
            fill(
                tr(
                    language,
                    "early  ←   timing of every hit, 10 ms per bar   →  late      {}",
                ),
                &[&tendency],
            ),
            13.0,
            palette::MUTED,
        ));
        screen
            .spawn(centred_on(0.0, 230.0, 1100.0, 40.0))
            .with_child(centred_label(
                fill(tr(language, "✕ / Space play again · ○ / L back\n{}"), &[&saved]),
                13.0,
                palette::MUTED,
            ));
    });
}

/// Saves the run's presses; returns a line saying where (or why not).
fn save_replay(last: &LastRun, language: Language) -> String {
    let replay = Replay {
        version: REPLAY_VERSION,
        song: last.song.clone(),
        difficulty: last.difficulty.name().to_owned(),
        tempo_percent: last.tempo_percent,
        no_fail: last.no_fail,
        autoplay: last.autoplay,
        sudden_death: last.sudden_death,
        presses: last.presses.clone(),
    };
    let Some(dirs) = directories::ProjectDirs::from("", "", "wheelup") else {
        return tr(language, "replay not saved: no data folder").to_owned();
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = format!("{}-{}-{stamp}.ron", last.song, replay.difficulty.to_lowercase());
    let path = dirs.data_dir().join("replays").join(name);
    match replay.save(&path) {
        Ok(()) => fill(tr(language, "replay saved: {}"), &[&path.display()]),
        Err(error) => fill(tr(language, "replay not saved: {}"), &[&error]),
    }
}

fn navigate(mut raw: MessageReader<RawInput>, session: Res<Session>, mut next: ResMut<NextState<Screen>>) {
    for key in menu_keys(&mut raw) {
        match key {
            MenuKey::Confirm => next.set(Screen::Rhythm),
            MenuKey::Back => next.set(if session.from_tour { Screen::Tour } else { Screen::Songs }),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_land_in_ten_millisecond_buckets() {
        let counts = histogram(&[0.0, 4.9, -5.1, 94.0, 96.0, -95.0]);
        assert_eq!(counts[9], 2, "-5..5 ms");
        assert_eq!(counts[8], 1);
        assert_eq!(counts[18], 1);
        assert_eq!(counts[0], 1);
        assert_eq!(counts.iter().sum::<u32>(), 5, "96 ms is off the chart");
    }
}
