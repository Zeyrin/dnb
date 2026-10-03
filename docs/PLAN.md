# Plan

The build follows the milestones in [`PROMPT.md`](../PROMPT.md) §15. This file breaks
them into tasks and keeps the risks in view. Status lives in [`PROGRESS.md`](PROGRESS.md);
choices and their reasons in [`DECISIONS.md`](DECISIONS.md).

## M0: skeleton
- Cargo workspace (`crates/*`, `apps/*`), shared lints and profiles.
- `wu-time`: ticks (960 PPQ), tempo maps, swing, the shared monotonic clock.
- `wheelup-cli` with `--help`.
- `wheelup` Bevy window: title card, frame-rate overlay, `--screenshot` for headless captures.
- CI on Linux, Windows and macOS.

## M1: audio engine and clock
- `wu-dsp`: oscillators (PolyBLEP), envelopes, state-variable filter, saturation, noise, smoothing.
- `wu-instruments`: drum synthesis for an 8-pad kit (kick, snare, ghost, clap, snare 2, perc, closed and open hat), baked to samples.
- `wu-audio`: preallocated voice pool with stealing and choke groups, sample-accurate sequencer
  over a compiled event list, transport, mixer with a safety limiter, a log of every voice start.
- Clock snapshots published from the audio callback through a seqlock; a regression-based
  estimator maps any instant to song time.
- Backends: offline (deterministic, for tests and `render`), null (real-time pace, no device), cpal.
- Live-hit queue (ASAP and Stable scheduling).
- `wheelup-cli render demo` writes a WAV; `devices` lists outputs.
- Tests: voice starts land on the exact expected samples; the callback never allocates (`assert_no_alloc`).
- The game plays the demo beat with an on-beat flash driven by the clock.

## M2: input and calibration
- `wu-input`: `InputEvent` model, input thread with ≥ 1 kHz polling and timestamps on the shared clock.
- Backends: gilrs (default), SDL3 (feature `sdl`, DualSense extras), keyboard, scripted.
- Action mapping (pads, roll strokes, rails, sticks, gestures) with the Reel layout as default.
- Pad events fan out to the audio thread (live play) and the main thread (judging, UI).
- Controller monitor screen (the reel's overlay rebuilt) and `wheelup-cli input-monitor`: report rate, jitter.
- Calibration wizard: audio and video offsets, stored per output device.

## M3: vertical slice ✅
- `wu-content`: project format (RON) with step-string patterns, compiled into the engine's event list.
- One original jungle song.
- `wu-chart`: charts from the project's drum part; Hard and Easy.
- `wu-game`: judge (windows per difficulty, earliest-unjudged-note rule), score, combo, vibe, results.
- Highway view, results screen, practice tempo, autoplay ("selecta bot"), replays.
- Tests: perfect scripted input scores 100 % WICKED; replays re-judge identically.
- A playtest checklist for a human with a real controller.

## M4: sound and content engine ✅
In this order, each step playable on its own, so the playtest can redirect it:
1. ✅ **Mix and master.** Buses (drums, bass, music, FX); the sub sidechained to the kick;
   a look-ahead limiter in place of the safety clipper. `wheelup-cli lufs` (EBU R128
   integrated loudness, 4× oversampled true peak); every song checked at −16 LUFS ± 1 LU,
   ≤ −1 dBTP in tests. Sends (reverb, dub delay) come with the instruments that need them.
2. ✅ **Gameplay notes.** Roll segments (the shoulder button as the second stroke; Junglist
   opens up), holds on the L2/R2 rails; Classic audio mode for high-latency outputs.
3. ✅ **Hype and WHEEL UP!.** Hype phrases on the highway; the rewind (L3 + R3 until a
   backend sees the touchpad): spinback, horns, crowd, a transport jump with the notes
   re-armed and the multiplier doubled.
4. ✅ **Instruments.** Real-time synth voices beside the sampled ones (no allocation on the
   audio thread): Reese, Rave Stab, Atmos Pad, Hoover, FM Rhodes, Pluck, Dub Siren, Air
   Horn, Vocal Formant, the FX set (riser, downlifter, impact, spinback), Crowd; reverb
   and dub-delay sends; then the songs use them (tracks, chords). Tape stop works on the
   whole mix, so it comes with the Perform FX in M5.
5. ✅ **Kits and breaks.** The Sampler Era chain; breaks performed by the drum synth, then
   crushed and sliced; kits baked on first launch and cached by content hash.
6. ✅ **Two more songs** in other subgenres (darkside, liquid), five charts each, with the
   musical checklist in tests: a fill every 8 bars, a riser and a one-beat gap before
   each drop, at least one hype phrase.

## Your tune: import
Asked for by the player: drop an audio file on the game, and it becomes a song to play.
1. ✅ **Listen.** Decode MP3, WAV, FLAC, OGG or M4A; find the tempo and the bar grid (to the
   millisecond), every kick, snare and hat, the sub-bass line, the drops.
2. ✅ **Play it.** The recording is the music, following the transport through pauses,
   seeks and WHEEL UP!; a miss muffles it until the next hit. Charts come from the same
   charter as the built-in songs.
3. ✅ **Keep it.** Imported songs live in the player's library, on their own machine only;
   `wheelup-cli import` shows what was heard.
4. ✅ **The feel:** an imported tune's drums are timed where they really sound, and tunes
   an older listener heard are listened to again in the background.
5. ✅ **Drum stems:** a tune's drum stem beside it (`Tune.drums.wav`) is where its beat
   and drums are heard.
6. **Next:** try it on the player's own tunes and tune from there.

## The look (from §11, ahead of M6 and M9)
Asked for by the player: the MVP has to look as good as it plays.
1. ✅ **The stage.** A venue drawn by a shader behind every screen, moved by the music
   (the sequenced kicks, the hype phrases, WHEEL UP!); an HDR camera with a tight bloom;
   the highway drawn in the world, every lane wearing its button's shape.
2. ✅ **The songs screen plays.** The selected tune's first drop loops, the stage moving
   with it, its record turning in its colour.
3. ✅ **A scene per stop:** the Bedroom Studio, the Warehouse Rave, the Sound System
   Clash and the Basement Club besides the rooftop.
4. **Next:** the Festival Main Stage as its tunes arrive; the tracker and pads note
   views (M9).

## M6: game structure (started ahead of M5)
The player asked for the game first: M6 comes before the Studio.
1. ✅ **Records** and **note speed**.
2. ✅ **How to play:** First Steps, a lesson song, first on the songs screen and picked
   for a new player.
3. ✅ **Practice:** loop any section, at any tempo, each pass's accuracy shown.
4. ✅ **The boot:** the photosensitivity notice every launch, then the songs (calibration
   waits on its own screen).
5. ✅ **The Pirate Radio Tour:** six stops, stars from the records, encores, challenges.
6. ✅ **Settings:** language, layout, note speed, audio, the WHEEL UP! flare, motion.
7. ✅ **The game in French:** every screen, the lessons and the tour, in the system's
   language until one is picked.
8. **Next:** tunes and scenes for the three stops on the way; dubplates to spend;
   Practice's Wait mode and metronome.

## M5, M7 → M9
As in the prompt: Studio (M5), controller deluxe and MIDI Bridge (M7), more modes (M8),
content complete and ship (M9). Each gets broken down here when it starts.

## Risks
| Risk | Mitigation |
|---|---|
| Input timestamps: some backends only deliver events when pumped from the main thread (macOS especially) | Prove the input thread on all three OSes early in M2; fall back to main-thread pumping with backend timestamps and measure the cost |
| Bevy API churn | Pinned to 0.19.1; app code kept thin; read the pinned sources rather than memory |
| DualSense effects through SDL3's raw effect packets | Feature-flagged; every effect is optional and the game plays without it |
| Content volume: 12 songs × 5 charts, written without listening | Compact notation, `gen-tune` drafts, the musical checklist enforced in tests, playtest checklists for a human |
| No audio device, controller or GPU on the build machine | Offline render, scripted input, autoplay, Xvfb + software Vulkan screenshots |
| Latency on real hardware | `docs/LATENCY.md` measurement procedure; ASAP/Stable scheduling; Classic audio mode for high-latency outputs |
