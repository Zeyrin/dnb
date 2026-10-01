# Plan

The build follows the milestones in [`PROMPT.md`](../PROMPT.md) §15. This file breaks
them into tasks and keeps the risks in view. Status lives in [`PROGRESS.md`](PROGRESS.md);
choices and their reasons in [`DECISIONS.md`](DECISIONS.md).

## M0: skeleton
- Cargo workspace (`crates/*`, `apps/*`), shared lints and profiles.
- `wu-time`: ticks (960 PPQ), tempo maps, swing, the shared monotonic clock.
- `wheelup-cli` with `--help`.
- `wheelup` Bevy window: title card, frame-rate overlay, `--screenshot` for headless captures.
- CI on Linux, Windows and macOS, triggered only by changes under `wheel-up/`.

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

## M3: vertical slice
- `wu-content`: project format (RON) with step-string patterns, compiled into the engine's event list.
- One original jungle song.
- `wu-chart`: charts from the project's drum part; Hard and Easy.
- `wu-game`: judge (windows per difficulty, earliest-unjudged-note rule), score, combo, vibe, results.
- Highway view, results screen, practice tempo, autoplay ("selecta bot"), replays.
- Tests: perfect scripted input scores 100 % WICKED; replays re-judge identically.
- A playtest checklist for a human with a real controller.

## M4 → M9
As in the prompt: sound and content engine (M4), Studio (M5), game structure (M6),
controller deluxe and MIDI Bridge (M7), more modes (M8), content complete and ship (M9).
Each gets broken down here when it starts.

## Risks
| Risk | Mitigation |
|---|---|
| Input timestamps: some backends only deliver events when pumped from the main thread (macOS especially) | Prove the input thread on all three OSes early in M2; fall back to main-thread pumping with backend timestamps and measure the cost |
| Bevy API churn | Pinned to 0.19.1; app code kept thin; read the pinned sources rather than memory |
| DualSense effects through SDL3's raw effect packets | Feature-flagged; every effect is optional and the game plays without it |
| Content volume: 12 songs × 5 charts, written without listening | Compact notation, `gen-tune` drafts, the musical checklist enforced in tests, playtest checklists for a human |
| No audio device, controller or GPU on the build machine | Offline render, scripted input, autoplay, Xvfb + software Vulkan screenshots |
| Latency on real hardware | `docs/LATENCY.md` measurement procedure; ASAP/Stable scheduling; Classic audio mode for high-latency outputs |
