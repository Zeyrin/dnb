# Decisions

Short records: what was decided, why, and what it costs. Newest last.

## ADR-001: build inside the FFFood repository, for now (superseded by ADR-023)
**Context.** The session that started the game is attached to the FFFood repository.
**Decision.** The game lives in `wheel-up/` on its own branch, with its own CI workflow
that only runs when `wheel-up/` changes. Nothing outside that folder and the workflow
file is touched.
**Consequences.** Moving it to its own repository later keeps the history:
`git subtree split --prefix=wheel-up -b wheel-up-only`, then push that branch to the new repository.

## ADR-002: Bevy 0.19.1, pinned exactly, without its audio and gamepad stacks
**Context.** On 2026-10-01 the latest stable Bevy is 0.19.1; 0.20 is a release candidate.
Bevy changes APIs between minor versions.
**Decision.** Pin `=0.19.1`. Enable features one by one rather than through `default`,
leaving out `bevy_audio` and `bevy_gilrs`: `wu-audio` owns the sound card and `wu-input`
owns controllers.
**Consequences.** Upgrading is a deliberate task. Bevy's own gamepad types are empty;
menus get controller navigation from `wu-input`.

## ADR-003: drums are baked to samples, melodic instruments run live
**Context.** Kits must be procedural (no copyrighted samples), deterministic, and cheap
to play dozens of times per bar.
**Decision.** Drum sounds and breaks are synthesised once into sample buffers when a kit
loads; the audio thread only plays samples back for them. Bass, pads and leads synthesise
in real time because they respond to pressure and sticks while they sound.
**Consequences.** One sampler voice type serves built-in drums, break slices and imported
samples. Baking takes a moment at load and is cached later (M4).

## ADR-004: 4/4, 960 PPQ, integer ticks, step tempo maps
**Context.** Drum & bass and jungle are in 4/4. Charts and patterns must line up exactly
with what plays.
**Decision.** Positions are `i64` ticks at 960 per beat; seconds and frames are derived
from the tempo map, never accumulated. Tempo changes are steps; a ramp is written as many steps.
**Consequences.** No odd meters. Swing applies to odd 16th steps only.

## ADR-005: optimise dependencies in dev builds
**Decision.** `opt-level = 3` for every dependency and `1` for our crates in the dev profile.
**Consequences.** Slower first build, but audio renders in real time and tests that render
songs stay fast without `--release`.

## ADR-006: lints
**Decision.** `unsafe_code` is denied workspace-wide and forbidden in every crate that needs
no FFI. `clippy::unwrap_used` warns (so fails CI under `-D warnings`) outside tests.
**Consequences.** Startup code that may panic uses `expect` with a message.

## ADR-007: gilrs now, SDL3 with the DualSense extras (M7)
**Context.** The prompt asks for SDL3 and gilrs backends in M2. SDL3's advantage is the
DualSense's touchpad, gyro, lightbar and adaptive triggers, none of which M2 uses, and it
needs a C library built per platform.
**Decision.** M2 ships the gilrs backend behind the `Backend` trait; SDL3 arrives in M7
with the features that need it.
**Consequences.** Until M7, Wheel Up! and wobble paths have no touchpad or gyro; buttons,
sticks and analog triggers all work.

## ADR-008: the toolchain is pinned
**Context.** CI's "stable" moved to Rust 1.99 and a new clippy lint failed the build that
passed locally on 1.97.
**Decision.** `rust-toolchain.toml` pins the compiler; CI installs exactly that.
**Consequences.** Upgrading Rust is a deliberate commit that fixes whatever new lints appear.

## ADR-009: keyboard input is for development
**Context.** Keyboard events reach the game through the window's event loop, so their
timestamps are quantised to the frame (about 16 ms at 60 fps).
**Decision.** Keep the keyboard as a full stand-in for testing and menus, but say so on
screen and in the docs; competitive timing needs a controller (its own thread).
**Consequences.** Calibration done on a keyboard is coarse.

## ADR-010: two calibration offsets
**Decision.** Tapping to clicks measures the audio offset (subtracted from every tap before
judging); tapping to flashes measures the video offset. Visuals run ahead by
`video − audio`, so playing along to the highway lands on the sound. Results with a robust
spread above 35 ms are refused. Stored per audio output device name.

## ADR-011: charts come from the song, cut at the song's own tempo
**Decision.** `wu-chart` thins the song's drum part per difficulty (metric strength first,
then pad importance), within thumb rules every chart is validated against. The hits a chart
leaves out play as backing. Charts are cut at the song's tempo, so practising at 70 % plays
the same notes, just slower.
**Consequences.** No hand-made charts yet; the generator's output is the chart. Hand edits
come with the chart editor (M5).

## ADR-012: scores settle in song-time order; replays judge in recorded order
**Context.** Frames discover outcomes out of order (a miss is noticed late, a press arrives
a frame after it happened), and the combo depends on order.
**Decision.** Outcomes reach the score sorted by when they happened in song time, once they
are 100 ms old; judgement feedback still shows at once. Replays store presses in the order
the judge saw them and re-judge in that order, because two input paths can deliver a later
press first, and which note a press takes depends on it. A replay names its song by id and
records everything else that changes the result (difficulty, tempo, No-Fail); the game and
`wheelup-cli replay` build the notes with the same code (`wu_game::play`).
**Consequences.** The score counter lags a tenth of a second. Replays reproduce live scores
exactly (property-tested with random delivery delays and frame rates). A replay is only as
stable as the chart generator: changing it changes the notes old replays are judged against.

## ADR-013: the bass is a baked sample, for now
**Decision.** The sub is synthesised once (a whole number of cycles, so its sustain loop is
seamless) and played at any pitch by playback rate, released when the note ends.
**Consequences.** One sampler voice type covers drums and bass. The real-time synths
(Reese, wobble, pressure on the triggers) arrive with M4.

## ADR-014: Junglist waits for roll segments (superseded by ADR-018)
**Decision.** Charts are generated for all five difficulties and validated, but the song
screen offers Beginner to Hard. Junglist's 85 ms same-thumb gaps need roll strokes (L1/R1)
to be fair, which arrive in M4.

## ADR-015: the game waits for its own program before judging
**Context.** For a frame or two after a song loads, the clock still reports the previous
program's position, which would miss every note up to it.
**Decision.** The engine counts loads and publishes the count in every clock snapshot; the
rhythm screen judges nothing until the snapshot shows its own load, playing.

## ADR-016: slimmer debug builds
**Context.** Two toolchains' worth of dev builds with full debug info filled the 30 GB disk.
**Decision.** Dev builds keep line tables for our crates and no debug info for dependencies.

## ADR-017: the mix is driven by the sequencer, and mastered to −16 LUFS
**Context.** Everything went into one bus and a soft clipper; the drops pinned the clipper
(−10 LUFS) and the sub fought the kick.
**Decision.** Four buses (drums, bass, music, FX). The bass ducks under each kick by a
trigger the engine schedules on the kick's exact frame, not by a detector that reacts after
it. The master runs through a true-peak look-ahead limiter (1.5 ms, ceiling −1.2 dBTP);
its delay is added to the output latency the clock publishes, so judging, calibration and
live scheduling stay exact, and offline renders compensate for it like a DAW bounce. Each
song sets its own mix in its project file and is mastered to −16 LUFS ± 1 LU, at most
−1 dBTP, measured by a BS.1770-4 meter (`wheelup-cli lufs`) and enforced by a test.
**Consequences.** The game is about 6 dB quieter than before, and every song and the JAM
groove sit at the same level; the limiter only shaves peaks. The listener's volume comes
after the limiter, so turning it down never changes the master.

## ADR-018: rolls are single-lane runs, and the shoulder plays them live
**Context.** Junglist keeps 16th-note runs (89 ms apart at 168 BPM), too fast for one thumb.
**Decision.** Same-thumb gaps under 110 ms only appear inside a roll: a run of at least four
notes on one lane, during which that hand's shoulder button (L1 or R1) plays the lane too.
One other note may sit right before or after a roll (the shoulder takes the roll's first
or last stroke, freeing the thumb), so a snare roll can run into the drop's kick. Any other
fast run is thinned a whole layer at a time (its weakest pad on its weakest positions), so
what remains stays regular. While a roll is in reach, the game tells the input thread
which pad each shoulder plays, so roll strokes sound with no extra latency.
**Consequences.** Junglist is on the song screen. Fast runs that hop between lanes stay
out of every chart; letting a thumb play another pad mid-roll would need the validator to
guess which strokes the shoulder takes.

## ADR-019: the player holds the bass line on the triggers
**Context.** The brief puts holds on the L2/R2 rails; the songs' bass lines are the natural
thing to hold.
**Decision.** From Medium, the bass line becomes holds: all of it on R2 at Medium, split by
pitch over L2 and R2 from Hard, back-to-back notes taking turns. On a single rail each hold
ends 120 ms before the next so the trigger can come up. A rail is never used while its hand
rolls. A hold is judged on its press like a tap; it then earns 50 points per beat held
(times the multiplier), with the whole hold counted if let go within the last window, and
completes on its own if the trigger stays down. Letting go early only loses the rest of
those points: no miss, no broken combo. The trigger plays the note itself: the game arms
each rail with its next note's key and the program frame where it ends; the input thread
sends a note-on to the engine as the trigger crosses its threshold, and the engine stops
it at that frame or when the trigger comes up.
**Consequences.** A dropped hold is heard at once (the sub stops). Replays record releases
(format version 2, older replays still load).

## ADR-020: Classic audio mutes the part instead of playing it
**Context.** On Bluetooth or a TV, a press sounds 100 ms or more after the thumb lands:
playing the part live feels broken.
**Decision.** A setting, Audio: Live or Classic, saved with the others. In Classic the song
plays everything, the player's part included but marked; presses make no sound, a miss
mutes the marked events from the next one on, and the next hit brings them back. A new
program always starts unmuted. The song screen suggests Classic when the output is Bluetooth.
**Consequences.** Classic gives feedback that latency can't spoil, at the cost of the
direct link between a press and its sound; it's a fallback, Live stays the default.

## ADR-021: WHEEL UP! is a scheduled jump, and the run has its own timeline
**Context.** Pulling the tune back means the song plays a stretch twice, so song time
repeats; judging, scoring and replays all assumed it only moves forward.
**Decision.** Hype phrases are marked on sections of the song (every 8 bars of a `hype`
section). Clearing one without a miss fills a quarter of the meter; at half, both sticks
(L3 + R3) ask for WHEEL UP!. The game schedules a jump for the next bar line at least
150 ms ahead: there the engine cuts the music, plays the spinback and the horn for two
beats, and drops back to the start of the 8-bar phrase that bar line ends, with the crowd.
The engine reports every transport change at the exact device frame it happened, so any
instant maps to song time even across the cut. The run keeps a timeline that only moves
forward (song time plus every rewind so far): at the cut, notes already judged in the
replayed stretch get fresh copies further along it, the others move, and so do hype
phrases; the multiplier doubles until the timeline is past the stretch. The run takes the
rewind 100 ms after the cut, so every press from before it has arrived. A WHEEL UP! is
recorded in the replay (version 3), and settles the run first, so a replay sees the hype
the live run did. A hold cut by the rewind counts as kept, paid for what was held.
**Consequences.** Everything scored before the cut stays scored; a replayed phrase can
be cleared again for hype. A replay from a newer game is refused rather than judged wrong.
The touchpad swipe of the brief waits for a backend that sees the touchpad.

## ADR-022: one synth engine plays every melodic instrument, with two dub-style returns
**Context.** The brief asks for a dozen instruments and more (ADR-003: melodic ones run
live). Twelve bespoke synths would be twelve things to keep allocation-free and tuned.
**Decision.** One voice type, driven by a `Patch` of plain data: unison oscillators (or
an FM pair with a tine, or formant filters for the vowels), a filter with its own
envelope, an LFO that can sync to the tempo, sweeps spread over the note's length, a
phaser, chord memory. Each instrument is a patch; FX one-shots that never change while
they sound (horn, spinback, crowd) stay baked samples, pitched by rate. Synth voices
have their own fixed pool beside the sample voices; modulation is worked out every 16
samples. The mix gets two returns, a reverb and a dub delay feeding it, and every sound
a send to each, taken before the bus fader.
**Consequences.** New instruments are new patches, and the Studio's synth page (M5) edits
the same fields. A chord costs a voice a note. Sends before the fader keep a ducked bass
from pumping its own reverb, and keep the tails of a cut ringing on through a rewind.

## ADR-023: WHEEL UP! has its own repository
**Context.** ADR-001 put the game in `wheel-up/` on a branch of FFFood, for want of a
repository of its own. Its owner made one, `Zeyrin/dnb`, and asked for the move.
**Decision.** The history of `wheel-up/` was split out with `git subtree split` and is
the start of this repository, the project at its root; the work in progress followed.
CI (`.github/workflows/ci.yml`) runs on every push and pull request. FFFood keeps none of it.
**Consequences.** Every commit kept its message, author and date, under a new hash and
without the `wheel-up/` prefix on its paths.

## ADR-024: breaks are performed and sampled in code; kits are data, baked once
**Context.** Jungle is built on sampled funk breaks, which can't ship: they're someone
else's records. Playing hits from a kit on a grid sounds like a drum machine, not a break.
**Decision.** A break is a drummer's performance written in step notation and played by
the drum synth with human timing and touch, in a room, then put through what the
producers did: sped up in a sampler (which pitches it up), its converters, tape. The loop
is cut into named slices at the performed hits. Kits are plain data (`KitDef`): pads, by
role, from a drum voice or a slice, and their breaks. Baking is deterministic and
fingerprinted with the definition, the sample rate and the synthesis sources' text; the
game keeps baked kits on disk under that fingerprint.
**Consequences.** Every sound stays original and the cache can never go stale without
anyone remembering to bump a version. Changing any synthesis source rebakes every kit once.
A song can run a whole break under its drums, at its own tempo, like a sampler would.
