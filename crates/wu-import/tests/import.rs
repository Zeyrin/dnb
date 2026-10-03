//! A tune, start to finish: rendered to a file as a player would bring it,
//! imported, kept, loaded back, charted and played.

use std::path::{Path, PathBuf};

use wu_audio::render_offline;
use wu_chart::{Difficulty, auto_chart, validate};
use wu_content::songs::BUILTIN;
use wu_import::library::{LISTENER_VERSION, load, load_all};
use wu_import::{ImportError, Stage, import, relisten};
use wu_time::Tick;

const SR: u32 = 48_000;

/// A folder of its own for each test, emptied first.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wheelup-import-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

/// Rooftop Transmission, bounced to a 16-bit WAV as a producer would.
fn bounce(into: &Path) -> PathBuf {
    let song = BUILTIN[0].load().expect("compiles");
    let program = song.whole_program(SR, &song.tempo, 0);
    let frames = program.tempo.frame_at(song.length + Tick::from_bars(1), SR) as usize;
    let render = render_offline(program, frames, 512);
    let path = into.join("Rooftop Transmission.wav");
    wu_audio::write_wav(&path, &render.audio, SR).expect("bounce");
    path
}

#[test]
fn a_bounced_tune_is_imported_kept_and_played_like_our_own() {
    let dir = scratch("whole");
    let file = bounce(&dir);
    let library = dir.join("imports");
    let mut stages = Vec::new();
    let imported = import(&file, &library, |stage| stages.push(stage)).expect("imports");
    assert_eq!(stages.first(), Some(&Stage::Spectrum));
    assert_eq!(stages.last(), Some(&Stage::Loudness));

    let kept = &imported.imported;
    assert_eq!(kept.title, "Rooftop Transmission", "named after the file without tags");
    assert!((kept.bpm - 168.0).abs() < 0.01, "{} BPM", kept.bpm);
    assert!(kept.first_bar_s.abs() < 0.003, "first bar at {} s", kept.first_bar_s);
    assert!((72..=73).contains(&kept.bars), "{} bars", kept.bars);
    // Mastered to the game's loudness already: hardly turned up or down.
    assert!(kept.gain_db.abs() < 1.5, "{} dB", kept.gain_db);
    assert!(imported.folder.join(&kept.audio).is_file(), "the audio is copied in");

    // Kept: loaded back the same, once even when imported twice.
    assert_eq!(load(&imported.folder).expect("loads"), imported);
    let again = import(&file, &library, |_| {}).expect("imports again");
    assert_eq!(again.id, imported.id);
    let all = load_all(&library);
    assert_eq!(all.len(), 1, "{all:?}");

    let song = imported.song();
    let drops: Vec<&str> = song.sections.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(drops, ["Intro", "Drop", "Breakdown", "Drop 2", "Outro"]);
    assert_eq!(song.hype.len(), 5, "three phrases in the first drop, two in the second");
    // About as many hits and notes as were played (with the break's hits on top).
    let played = BUILTIN[0].load().expect("compiles");
    let (hits, notes) = (song.drums.len() as f64, song.bass.len() as f64);
    eprintln!(
        "{hits} hits heard of {} played, {notes} notes of {}",
        played.drums.len(),
        played.bass.len()
    );
    assert!(hits > 0.8 * played.drums.len() as f64, "{hits} hits");
    assert!((notes - played.bass.len() as f64).abs() < 10.0, "{notes} notes");
    let recording = song.recording.as_ref().expect("plays its recording");
    assert!(recording.path.is_file());
    // Nothing of its own sounds but the count-in: the recording is the music.
    let program = song.whole_program(SR, &song.tempo, 2);
    assert_eq!(program.events().len(), 8, "two bars of count-in");

    for difficulty in Difficulty::ALL {
        let chart = auto_chart(&song.drums, &song.bass, &song.tempo, difficulty);
        let problems = validate(&chart, &song.tempo);
        assert!(problems.is_empty(), "{difficulty:?}: {problems:?}");
        assert!(!chart.notes.is_empty());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn what_cannot_be_played_is_refused_and_nothing_is_kept() {
    let dir = scratch("refused");
    let library = dir.join("imports");
    // Five seconds of a steady tone: no beat in it.
    let path = dir.join("tone.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).expect("create");
    for i in 0..5 * 44_100 {
        let x = (std::f32::consts::TAU * 220.0 * i as f32 / 44_100.0).sin();
        writer.write_sample((x * 8_000.0) as i16).expect("write");
    }
    writer.finalize().expect("finalize");
    let refused = import(&path, &library, |_| {});
    assert!(refused.is_err(), "{refused:?}");
    assert!(load_all(&library).is_empty());
    let missing = import(&dir.join("nowhere.mp3"), &library, |_| {});
    assert!(missing.is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_tune_an_older_listener_heard_plays_as_heard_until_heard_again() {
    let dir = scratch("older");
    let file = bounce(&dir);
    let library = dir.join("imports");
    let imported = import(&file, &library, |_| {}).expect("imports");
    // As the first listener kept it: no feel, and the player's own title.
    let song_file = imported.folder.join("song.ron");
    // Kept with the system's line ends (\r\n on Windows): edited here with plain ones.
    let text = std::fs::read_to_string(&song_file).expect("kept").replace("\r\n", "\n");
    let start = text.find("    feel: (").expect("a feel");
    let end = start + text[start..].find("\n    ),\n").expect("its end") + "\n    ),\n".len();
    let older = format!("{}{}", &text[..start], &text[end..])
        .replacen(&format!("version: {LISTENER_VERSION}"), "version: 1", 1)
        .replacen("title: \"Rooftop Transmission\"", "title: \"My Rooftop\"", 1);
    std::fs::write(&song_file, older).expect("written");

    let kept = load(&imported.folder).expect("an older listener's tune still loads");
    assert!(kept.heard_by_an_older_listener());
    assert!(kept.imported.feel.is_straight(), "it had no feel: on the grid");
    assert!(kept.song().recording.is_some(), "and it plays");

    let heard = relisten(&kept, |_| {}).expect("heard again");
    assert!(!heard.heard_by_an_older_listener());
    assert_eq!(heard.imported.title, "My Rooftop", "its title stays the player's");
    assert!(!heard.imported.feel.is_straight(), "the rooftop's light swing is felt");
    assert_eq!(load(&imported.folder).expect("loads"), heard, "and kept");

    // A newer game's listener: this one can't read what it kept.
    let newer = std::fs::read_to_string(&song_file).expect("kept").replacen(
        &format!("version: {LISTENER_VERSION}"),
        &format!("version: {}", LISTENER_VERSION + 1),
        1,
    );
    std::fs::write(&song_file, newer).expect("written");
    assert!(matches!(load(&imported.folder), Err(ImportError::Newer)));
    let _ = std::fs::remove_dir_all(dir);
}
