//! Your own tunes: drop an audio file on the window and it is listened to in
//! the background, kept in the library and selected. An imported song's
//! recording is decoded and brought to the output's rate while it sits
//! selected, so pressing play starts it at once.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy::window::FileDragAndDrop;
use wu_content::settings::Language;
use wu_import::listen::{FEWEST_BARS, ListenError};
use wu_import::{DecodeError, ImportError, ImportedSong, Stage};

use crate::audio::AudioLink;
use crate::session::Session;
use crate::settings::SettingsStore;
use crate::songs_screen::SongLibrary;
use crate::words::{fill, tr};

#[derive(Debug)]
pub struct ImportPlugin;

impl Plugin for ImportPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Importing>()
            .init_resource::<Recordings>()
            .add_systems(Update, (take_drops, follow_import, ready_recording).chain());
    }
}

/// An import under way, and what the songs screen says about importing.
#[derive(Resource, Default)]
pub struct Importing {
    job: Option<Job>,
    /// How it is going, or how the last one went.
    pub status: Option<String>,
}

struct Job {
    name: String,
    /// Behind a lock only so the resource can be shared: one system reads it.
    news: Mutex<Receiver<News>>,
}

enum News {
    Stage(Stage),
    Done(Box<Result<ImportedSong, ImportError>>),
}

/// A recording being read and brought to the output's rate on its own thread.
type Reading = Mutex<Receiver<Result<Arc<[f32]>, String>>>;

/// The selected song's recording, at the output's rate: interleaved stereo.
#[derive(Resource, Default)]
pub struct Recordings {
    ready: Option<(PathBuf, u32, Arc<[f32]>)>,
    loading: Option<(PathBuf, u32, Reading)>,
    /// Why the selected song's recording can't be played, if it can't.
    pub problem: Option<String>,
}

impl Recordings {
    /// The recording at `path`, at `sample_rate`, once it is ready.
    pub fn get(&self, path: &std::path::Path, sample_rate: u32) -> Option<Arc<[f32]>> {
        self.ready
            .as_ref()
            .filter(|(p, rate, _)| p == path && *rate == sample_rate)
            .map(|(_, _, audio)| Arc::clone(audio))
    }
}

/// Why an import failed, in `language`.
fn why(error: &ImportError, language: Language) -> String {
    match error {
        ImportError::Decode(DecodeError::Open(error)) => fill(tr(language, "can't open it: {}"), &[error]),
        ImportError::Decode(DecodeError::Format(error)) => fill(
            tr(
                language,
                "not an audio file this game can read (MP3, WAV, FLAC, OGG or M4A): {}",
            ),
            &[error],
        ),
        ImportError::Decode(DecodeError::NoAudio) => tr(language, "it has no audio in it").to_owned(),
        ImportError::Listen(ListenError::NoBeat) => {
            tr(language, "there is no steady beat in it to play along to").to_owned()
        }
        ImportError::Listen(ListenError::TooShort { bars }) => fill(
            tr(language, "it is too short to play: {} bars (a song needs at least {})"),
            &[bars, &FEWEST_BARS],
        ),
        ImportError::Keep(error) => fill(tr(language, "can't keep it: {}"), &[error]),
        ImportError::Damaged(error) => fill(tr(language, "its song file is damaged: {}"), &[error]),
        ImportError::Outdated => tr(language, "it was heard by an older listener").to_owned(),
    }
}

/// Takes in a file dropped on the window, unless one is still being listened to.
fn take_drops(
    mut drops: MessageReader<FileDragAndDrop>,
    mut importing: ResMut<Importing>,
    settings: Res<SettingsStore>,
) {
    let language = settings.language();
    for drop in drops.read() {
        let FileDragAndDrop::DroppedFile { path_buf, .. } = drop else {
            continue;
        };
        let name = path_buf
            .file_name()
            .map_or_else(|| path_buf.display().to_string(), |n| n.to_string_lossy().into_owned());
        if let Some(job) = &importing.job {
            importing.status = Some(fill(
                tr(language, "Still listening to {}: drop {} again after"),
                &[&job.name, &name],
            ));
            continue;
        }
        let Some(library) = wu_import::library::default_dir() else {
            importing.status = Some(
                tr(
                    language,
                    "Can't import: this system has no data folder to keep tunes in",
                )
                .to_owned(),
            );
            continue;
        };
        let (tell, news) = channel();
        let file = path_buf.clone();
        let spawned = std::thread::Builder::new().name("import".into()).spawn(move || {
            let result = wu_import::import(&file, &library, |stage| {
                let _ = tell.send(News::Stage(stage));
            });
            let _ = tell.send(News::Done(Box::new(result)));
        });
        importing.status = Some(match spawned {
            Ok(_) => {
                importing.job = Some(Job {
                    name: name.clone(),
                    news: Mutex::new(news),
                });
                fill(
                    tr(language, "{}: {}…"),
                    &[&name, &tr(language, Stage::Spectrum.describe())],
                )
            }
            Err(error) => fill(tr(language, "Can't import {}: {}"), &[&name, &error]),
        });
    }
}

/// Follows the import under way; a tune imported joins the library, selected.
fn follow_import(
    mut importing: ResMut<Importing>,
    mut library: ResMut<SongLibrary>,
    mut session: ResMut<Session>,
    settings: Res<SettingsStore>,
) {
    let language = settings.language();
    let Some(job) = importing.job.as_ref() else { return };
    let name = job.name.clone();
    let mut done = None;
    let mut stage = None;
    let Ok(news) = job.news.lock() else { return };
    loop {
        match news.try_recv() {
            Ok(News::Stage(now)) => stage = Some(now),
            Ok(News::Done(result)) => {
                done = Some(*result);
                break;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                done = Some(Err(ImportError::Damaged("the listener stopped".to_owned())));
                break;
            }
        }
    }
    drop(news);
    if let Some(stage) = stage {
        importing.status = Some(fill(
            tr(language, "{}: {}… {} %"),
            &[
                &name,
                &tr(language, stage.describe()),
                &format!("{:.0}", 100.0 * stage.done()),
            ],
        ));
    }
    let Some(result) = done else { return };
    importing.job = None;
    importing.status = Some(match result {
        Ok(imported) => {
            let song = imported.song();
            let drops = imported.imported.sections.iter().filter(|s| s.3).count();
            let line = fill(
                tr(language, "Ready to play: {} · {} BPM · {} bars · {} drop{}"),
                &[
                    &song.meta.title,
                    &format!("{:.0}", imported.imported.bpm),
                    &imported.imported.bars,
                    &drops,
                    &if drops == 1 { "" } else { "s" },
                ],
            );
            session.song = library.add_import(&imported);
            line
        }
        Err(error) => fill(tr(language, "Couldn't import {}: {}"), &[&name, &why(&error, language)]),
    });
}

/// Readies the selected song's recording, if it has one, at the output's rate.
fn ready_recording(
    library: Res<SongLibrary>,
    session: Res<Session>,
    audio: NonSend<AudioLink>,
    mut recordings: ResMut<Recordings>,
    settings: Res<SettingsStore>,
) {
    let language = settings.language();
    let rate = audio.sample_rate();
    if let Some((path, at, news)) = &recordings.loading {
        let heard = news
            .lock()
            .map_or(Err(TryRecvError::Disconnected), |news| news.try_recv());
        match heard {
            Ok(Ok(audio)) => {
                recordings.ready = Some((path.clone(), *at, audio));
                recordings.loading = None;
            }
            Ok(Err(problem)) => {
                recordings.problem = Some(problem);
                recordings.loading = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => recordings.loading = None,
        }
    }
    let Some(recording) = library.get(session.song).and_then(|song| song.recording.as_ref()) else {
        return;
    };
    let path = recording.path.clone();
    let wanted = |p: &PathBuf, at: u32| *p == path && at == rate;
    if recordings.ready.as_ref().is_some_and(|(p, at, _)| wanted(p, *at))
        || recordings.loading.as_ref().is_some_and(|(p, at, _)| wanted(p, *at))
    {
        return;
    }
    // Another song: let its recording go, and read this one.
    recordings.ready = None;
    recordings.problem = None;
    let (tell, news) = channel();
    let file = path.clone();
    let spawned = std::thread::Builder::new().name("recording".into()).spawn(move || {
        let result = wu_import::decode(&file)
            .map(|tune| wu_dsp::resample(&tune.stereo, 2, tune.sample_rate, rate).into())
            .map_err(|error| {
                let why = why(&ImportError::Decode(error), language);
                fill(tr(language, "Can't play {}: {}"), &[&file.display(), &why])
            });
        let _ = tell.send(result);
    });
    match spawned {
        Ok(_) => recordings.loading = Some((path, rate, Mutex::new(news))),
        Err(error) => recordings.problem = Some(fill(tr(language, "Can't read the tune: {}"), &[&error])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_import_says_why_it_failed_as_the_listener_does() {
        let errors = [
            ImportError::Decode(DecodeError::Open(std::io::Error::other("no such file"))),
            ImportError::Decode(DecodeError::Format("unknown codec".to_owned())),
            ImportError::Decode(DecodeError::NoAudio),
            ImportError::Listen(ListenError::NoBeat),
            ImportError::Listen(ListenError::TooShort { bars: 9 }),
            ImportError::Keep(std::io::Error::other("disk full")),
            ImportError::Damaged("bad RON".to_owned()),
            ImportError::Outdated,
        ];
        for error in &errors {
            assert_eq!(why(error, Language::English), error.to_string());
            assert_ne!(why(error, Language::French), error.to_string(), "{error}");
        }
    }
}
