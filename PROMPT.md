# WHEEL UP! — build prompt

> **How to use:** paste everything below the line into a coding agent (Claude Code or similar) started in an empty repository, or point the agent at this file. *WHEEL UP!* is a working title; rename freely.

---

# WHEEL UP! — a junglist rhythm game and controller-first DAW, in Rust

## 0. Where this comes from

A reel went around: a PS5 DualSense plugged into a computer over USB-C and played like a finger-drumming pad. An on-screen overlay maps its eight buttons to MIDI notes — D-pad ↑ C1, ↓ D1, ← C#1, → D#1; △ E1, □ F1, ○ G1, ✕ F#1, which is the General MIDI drum map (notes 36–43: kick, side-stick, snare, clap, electric snare, low tom, closed hat, high tom) — with an "M" toggle on each row, live X/Y readouts for both sticks and a "PS5" badge. Its top comment, at 1.1k likes:

> "If someone made a DAW as a game for PS5 with preinstalled kits and instruments, I'd buy it 👌"

You're building that game for **drum & bass and jungle**: a rhythm game on the surface, a real music tool underneath. Every song in the game is a project you can open in the built-in studio, remix, and turn back into a playable chart. Plug in a controller, pick a kit, play — nothing to install, nothing to set up.

## 1. Your job, and how to work

You are a senior Rust engineer with game and real-time audio experience. Build the whole thing, autonomously, milestone by milestone (§15).

- **Vertical slice first.** Until M3 works — one song, real controller, real audio, judged and scored, start to finish — don't spend time on content breadth or polish. The slice has to feel tight before anything else matters.
- **Always runnable.** From M1 on, `cargo run -p wheelup` starts something; from M3 on, something playable.
- **Quality gate on every commit:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Commit after each green step.
- **Write it down.** `docs/PLAN.md` (task breakdown, written first), `docs/PROGRESS.md` (what works, how to try it, known issues — updated every milestone), `docs/DECISIONS.md` (short ADRs: context, choice, consequences).
- **Don't guess APIs.** Bevy and several crates below change between versions. Pin exact versions and read their docs before writing code against them.
- **Scope honestly.** If something here is impractical, don't drop it silently: record the trade-off in DECISIONS.md, take the closest viable alternative, keep going. Stop and ask only for decisions that change what the game is.
- **You probably can't hear audio or hold a controller.** Build the tools that let you verify without them — offline renders, voice-start logs, loudness metering, scripted input, autoplay, replays — and give me short playtest checklists for what only a human can judge (feel, latency, mix).

## 2. Pillars

In priority order; when two collide, the higher one wins.

1. **Timing is sacred.** The audio clock is the master. Inputs are timestamped when they arrive, not when a frame gets around to them. Frame rate never affects judging or sound.
2. **The controller is an instrument.** Every input means something musical — pads, pressure-sensitive triggers, sticks, touchpad, gyro — and haptics and lights answer back.
3. **The game is the DAW.** One engine. Songs are projects, charts come from the music, and anything made in the studio is playable one button press later.
4. **Junglist culture, done with love.** Chopped breaks, rewinds, dub sirens, sound systems, pirate radio — and zero copyrighted material (§9).
5. **Controller-first, couch-friendly.** No screen needs a mouse. Mouse and keyboard still work everywhere.

## 3. Platforms and scope

- **Targets:** Windows 10/11, macOS 13+ (Apple Silicon and Intel), Linux (PipeWire/ALSA/JACK; X11 and Wayland), Steam Deck (native Linux build, 1280×800, controller only).
- **PS5 and other consoles: out of scope.** Shipping on PlayStation requires being a licensed partner with Sony's NDA SDK. Keep platform services (storage, input backend, audio device, achievements) behind traits so a port stays possible, and say so in the README.
- **Controllers:** the DualSense is the hero device (touchpad, gyro, lightbar, player LEDs, adaptive triggers, rumble). Also DualShock 4, Xbox, Switch Pro, Steam Deck, any SDL-mapped pad, MIDI pad controllers (pads → lanes) and keyboard (development and fallback). Every mechanic has a fallback binding for devices without the hardware.
- **Not in v1:** online multiplayer, third-party plugin hosting (VST/CLAP), microphone recording, mobile.

## 4. Stack

Defaults, not dogma: check each crate is maintained when you start; if one isn't, swap it and write an ADR.

| Concern | Default | Notes |
|---|---|---|
| Language | Rust stable, edition 2024, Cargo workspace | `#![forbid(unsafe_code)]` except FFI modules; every `unsafe` block gets a `// SAFETY:` comment |
| App, rendering, UI | **Bevy**, latest stable, pinned | Turn off Bevy's own audio and gamepad features: our crates own both. `bevy_egui` is acceptable for dense Studio panels only if every control stays reachable by gamepad |
| Audio I/O | **cpal** | WASAPI, CoreAudio, ALSA/PipeWire, JACK; ASIO behind a feature on Windows |
| DSP | your own code (`fundsp` is fine for prototyping) | Allocation-free, sample-accurate |
| Thread communication | **rtrb** SPSC queues, atomics, `triple_buffer` or a seqlock for snapshots | Never a `Mutex` on the audio thread |
| Real-time safety checks | **assert_no_alloc** in debug builds and tests | |
| Gamepads | **SDL3** (`sdl3`, dropping to `sdl3-sys` for anything unwrapped, e.g. `SDL_SendGamepadEffect`) | Most complete DualSense support. A **gilrs** backend behind a feature as fallback; `hidapi` if you need raw DualSense reports |
| MIDI | **midir** (in/out; virtual ports on macOS/Linux), **midly** (MIDI file export) | |
| Files | serde + **ron**, **hound** (WAV), **symphonia** (import WAV/FLAC/OGG/MP3), **rubato** (resampling) | |
| Analysis | **realfft**, **ebur128** (LUFS, true peak) | |
| Localisation | Fluent (`fluent-bundle`) | English and French at launch |
| Tooling | clap, tracing, thiserror/anyhow, proptest, insta, criterion | |
| Packaging | cargo-dist or GitHub Actions | zip / dmg / AppImage, plus Steam Deck notes |

## 5. Architecture

### Workspace

```
crates/
  wu-time         musical time: Tick (960 PPQ), TempoMap, ticks ↔ seconds ↔ samples, swing
  wu-dsp          oscillators, envelopes, SVF filters, drive, bitcrush, compressor, reverb, delay, smoothing
  wu-instruments  drum synth, sampler/slicer, synths, FX instruments (§9)
  wu-audio        cpal + null/offline backends, voice pool, graph, mixer, sequencer, transport, clock
  wu-input        backends, input thread, timestamps, action mapping, gestures, haptics/LED output
  wu-midi         MIDI Bridge out, MIDI pad input, MIDI file export
  wu-content      Project / Kit / Preset / Chart / Profile formats, versioning + migrations, licence manifest
  wu-chart        auto-charter, difficulty reduction, playability validator
  wu-game         judge, scoring, vibe, hype & Wheel Up, mode state machines (pure logic)
  wu-studio       DAW model: commands, undo/redo, edit operations (pure logic)
apps/
  wheelup         Bevy app: rendering, UI, menus, glue only
  wheelup-cli     headless tools (§13)
content/          songs, kits, presets (RON) + licences manifest
assets/           fonts (OFL), shaders, textures
```

Everything outside `apps/wheelup` is Bevy-free and testable headless.

### Threads

- **Audio thread** (the cpal callback) owns the engine. It takes commands from SPSC queues, publishes clock snapshots, and sends meters, spectrum frames and a log of what actually played (and on which sample) back to the game, for visuals and tests.
- **Input thread** polls the backend at ≥ 1 kHz (or blocks on events) and stamps each event with `Instant::now()` the moment it's read — or with a better OS/device timestamp when one exists. Events fan out to two queues:
  - **pad events → the audio thread directly** (it knows the current kit mapping), so pad-to-sound never waits for a frame;
  - **all events → the main thread**, for judging and UI.
- **Main thread** (Bevy): game logic, judging from timestamped events, rendering, UI.
- **Workers:** content loading, kit baking, offline renders, auto-charting.

Early in M2, prove the input backend runs off the main thread on Windows, macOS and Linux (macOS is the risky one). Where it can't, pump it on the main thread, keep the backend's timestamps, and measure what that costs.

### Clock and latency

- The transport's sample counter **is** song time. Musical positions are integer ticks (960 PPQ) through a tempo map; seconds and samples are derived from it, never accumulated.
- Each callback publishes `(transport frame at buffer start, Instant when that frame reaches the speaker)`, the latter being `Instant::now()` plus cpal's `playback − callback` timestamp delta (its output-latency estimate). Smooth across callbacks, which arrive with jitter; song time never goes backwards.
- `song_time(instant)` maps any instant to song time. It's used three ways:
  - **judging:** `song_time(event.at) − input_offset`
  - **rendering:** `song_time(now + expected display latency) − video_offset`
  - **live hits:** placed in the audio buffer at the sample offset matching their timestamp.
- Live hits have two scheduling modes: **ASAP** (lowest latency, default) and **Stable** (constant latency of one buffer, zero jitter).
- **Hit snapping** (assist option): a hit up to a BIG window *early* for a charted note is scheduled exactly on the note; late hits play immediately.
- **Latency target:** pad to sound ≤ 15 ms with a wired DualSense at 48 kHz / 128-frame buffer on a low-latency host (WASAPI exclusive or ASIO, CoreAudio, PipeWire/JACK). Describe how to measure it in `docs/LATENCY.md` — a phone's slow-motion camera filming the thumb and a speaker cone, or a contact mic on the controller and a loopback recording.
- Bluetooth audio (100 ms and up) ruins live play. Detect it where the OS lets you, warn, and suggest Classic audio mode (§10).

### Audio thread rules

No allocation, locks, I/O, logging or unbounded loops. A preallocated voice pool (say 128 voices) with stealing (oldest and quietest first), smoothed parameters (no zipper noise), denormal protection, sample-accurate events (split the buffer at event offsets). Enforce it with `assert_no_alloc` in debug builds and a test that renders a dense song under it.

## 6. The controller

### Default layout: "Reel", straight from the video

| Control | Pad | Default role (jungle kit) | MIDI Bridge note |
|---|---|---|---|
| D-pad ↑ | P1 | Kick | C1 (36) |
| D-pad ↓ | P2 | Snare | D1 (38) |
| D-pad ← | P3 | Ghost snare / side-stick | C#1 (37) |
| D-pad → | P4 | Clap / rim | D#1 (39) |
| △ | P5 | Snare 2 / break chop | E1 (40) |
| □ | P6 | Low perc / tom | F1 (41) |
| ✕ | P7 | Closed hat | F#1 (42) |
| ○ | P8 | High perc / open hat | G1 (43) |

The MIDI Bridge names notes the way most DAWs do (middle C = C3, so C1 = 36), to match the video. Content files use scientific pitch (middle C = C4). A kit decides what each pad plays; in chop sections the pads hold break slices, like a hardware sampler.

The layout splits like a drummer: the left thumb owns kick and snare, the right thumb owns hats and percussion, so the most common simultaneous hits (kick + hat, snare + hat) land on different hands.

| Control | During a chart | Free play / studio |
|---|---|---|
| L1 / R1 | **Roll strokes:** inside a roll segment they count as a hit on that hand's rolling lane, so fast 16ths alternate thumb and index finger | Retrigger that hand's last pad |
| L2 (analog) | **Sub rail:** sub-bass hold notes; pressure sets level and drive | Sub bass |
| R2 (analog) | **Bass rail:** Reese/mid-bass hold notes; pressure sets filter cutoff. **Pressure-zone notes** ask for Light (20–45 %), Mid (45–75 %) or Full (75–100 %) | Bass |
| Right stick | **Wobble:** follow the LFO path drawn during bass holds (bonus) | Pitch bend / wobble |
| Left stick | **FX XY** (filter cutoff × resonance) in freestyle zones; menu navigation | FX XY |
| Touchpad | Swipe right to left: **WHEEL UP!** (§10). Two-finger tap: air horn (Style points in freestyle zones) | Scrub / XY pad |
| Gyro | Tilt to sweep the filter on riser notes (bonus). Off by default | Assignable |
| R3 | Dub siren, bent by the right stick (Style points in freestyle zones) | Reset FX |
| L3 | — | Tap tempo |
| Options / Create | Pause / hold to restart | Transport / context menu |

An analog "press" is rising past 20 %; release is falling under 10 % (hysteresis). Fallbacks: no touchpad → View/Select or L3 + R3 for Wheel Up; no adaptive triggers → zones are visual only; no gyro → bind to a stick or skip.

### DualSense extras (feature-flagged, degrade gracefully)

- **Adaptive triggers:** resistance notches on R2 at 45 % and 75 % so the pressure zones are physical; a click on L2 as a sub note lands.
- **Rumble:** follows kick and sub energy, with a distinct pulse per judgement; global strength slider.
- **Lightbar:** section colour, pulsing on the beat, a burst on Wheel Up.
- **Player LEDs:** show the combo multiplier (1–4).
- **Experimental, USB only:** the DualSense shows up as a 4-channel USB audio device whose channels 3–4 drive the haptic actuators. Send a low-passed kick/sub signal there so players feel the bass. Verify on hardware; off by default.

### Remapping and profiles

Fully remappable, per device. Presets: Reel (default), Drummer (↓ kick, ↑ snare), Lefty (hands swapped), One-hand left, One-hand right, MIDI pad controller. Button glyphs follow the connected controller family (PlayStation, Xbox, Nintendo, generic).

### MIDI Bridge mode: the video, done properly

A screen that rebuilds the video's overlay: controller diagram, pad-to-note rows with an M (mute) toggle each, live stick X/Y, trigger bars, touchpad and gyro visualisers. It sends notes (per-pad velocity, fixed or taken from a trigger), sticks, triggers and gyro as CCs or pitch bend, and optionally MIDI clock. On macOS/Linux it opens a virtual port named "Wheel Up Controller"; on Windows it picks an existing port, and the docs explain loopMIDI or Windows MIDI Services loopback. Also available headless as `wheelup-cli midi-bridge`.

## 7. Game modes

1. **Pirate Radio Tour (campaign):** Bedroom Studio → Rooftop Pirate Station → Warehouse Rave → Sound System Clash → Basement Club → Festival Main Stage. Each venue has 3–5 songs, an encore unlocked with stars, and a venue challenge ("no misses through the break roll", "three Wheel Ups in one set"). Rewards are **dubplates**, spent on kits, instruments, visual themes and lightbar themes.
2. **Quickplay:** any song, any difficulty. Modifiers: No-Fail, Tempo 50–150 % (a real tempo change — songs are sequenced, so there are no time-stretch artefacts), Mirror, Hidden, Sudden Death.
3. **Practice:** loop any section, tempo slider, metronome, Wait mode (the song waits for your hit), early/late readout, input monitor.
4. **Pirate Signal (endless):** procedurally generated tunes, seeded per subgenre, back to back — framed as a pirate radio broadcast with on-screen shout-outs. Keep the vibe alive as long as you can.
5. **Soundclash (local, 2–4 players):** players trade 8-bar phrases while a crowd meter swings tug-of-war style; the winner gets the final Wheel Up.
6. **Back2Back (local co-op):** one player on drums, one on bass (triggers and sticks), shared hype.
7. **Studio** (§8), **MIDI Bridge** (§6), **Calibration and Settings** (§12).

## 8. Studio: a DAW you drive with a gamepad

L1/R1 cycle views and Options is the transport. Holding the touchpad click opens the **Perform FX** overlay from anywhere: filter sweep, beat repeat (1/4 to 1/32), tape stop, spinback, dub siren, reverb throw, delay throw.

- **Live:** play pads, shoulders and triggers; record with count-in into 1/2/4/8-bar loops; overdub; quantize (off, 1/8, 1/16, 1/32, with strength and swing); undo per pass.
- **Pattern:** a step grid of 16 steps × 8 pads per bar (zoomable to 32nds), patterns of 1–8 bars. Per step: velocity, probability, **ratchet ×2/×3/×4** (snare rolls), micro-timing nudge, slice, pitch, reverse. D-pad moves, ✕ toggles, hold □ + right stick for velocity/nudge, hold △ + right stick for ratchet/probability, hold R2 to scroll fast, ○ goes back.
- **Chop:** load a break (built-in or imported), slice by transients or on a grid (8/16/32), assign slices to pads; per-slice pitch, reverse, gain, envelope. Classic edit presets ("two-step", "roller", "Amen-style A/B/C") and **Mutate**, a seeded but musical rearrangement.
- **Synth:** four macros per instrument on sticks and triggers, a deep-edit page, a preset browser. A piano roll for melodic tracks on a D-pad grid, with scale lock (project key) and chord memory for stabs.
- **Mixer:** 8 instrument tracks, drum bus, bass bus, two returns (Reverb, Dub Delay), master. Per track: level, pan, mute/solo, two sends, inserts (filter, drive, Sampler Era crusher, compressor), sidechain from the kick. Master: glue compressor and a true-peak limiter at −1 dBTP.
- **Arrange:** section clips per track in a grid (session-view style) plus a linear timeline; automation lanes recorded from sticks, triggers and gyro.
- **Housekeeping:** unlimited undo/redo (command pattern), autosave every 60 s and on exit, crash-safe writes (temp file, then atomic rename).
- **Export:** WAV (16/24-bit, 44.1/48 kHz), stems, Standard MIDI File, and a `.wheelup` bundle (zip of RON plus user samples).
- **Chart It:** one press generates all five difficulties from the project (§10) and drops you into Quickplay. A simple chart editor adds, moves and deletes notes; the validator runs on save.
- **Templates:** one per subgenre (tempo, kit, instruments, starter patterns, arrangement).
- **Studio Missions**, the "DAW as a game" part: goals checked by analysing the project — "make a 4-bar Amen-style edit with a ratchet roll", "write a drop with the sub in F minor", "bounce a mixdown at −16 LUFS ±1". They teach production and unlock content.
- **Onboarding:** a five-minute interactive tutorial, "Make your first jungle tune", that ends with playing it as a chart.

## 9. Sound and content

### Licensing (hard rule)

- **No copyrighted samples, ever.** The famous breaks (Amen, Think, Apache, Funky Drummer…) are commercial recordings: never ship them, never download them. Recreate the *style* with the drum synth and the Sampler Era chain below, or use CC0 sources with recorded provenance.
- Every file shipped under `assets/` and `content/` is listed in `content/licenses.ron` (source, author, licence), and CI fails on any unlisted file. Fonts are OFL. Art and shaders are original, CC0, or CC-BY with attribution.
- No real artists, labels, radio stations or trademarks in game content. Songs are credited to fictional in-house producers.

### Procedural kits (baked on first launch, cached, deterministic)

- **Drum synthesis:** kick (sine with pitch and amp envelopes, click, drive), snare (two tuned modes plus filtered noise), side-stick and rim, clap (3–4 noise bursts plus tail), closed and open hats (six detuned square waves through band-pass and high-pass, 808-style), ride and crash (inharmonic FM/ring-mod partials plus noise), toms, shaker, modal percussion.
- **Breaks:** render 2–4-bar funk- and soul-style drummer performances with the drum synth — humanised timing and velocity, ghost notes, room reverb — then run them through the **Sampler Era** chain, modelled on the trackers and samplers early jungle was made on: *Tracker* (8-bit, ~28 kHz), *Rack Sampler* (12-bit, ~32 kHz), *Drum Machine* (12-bit, 26.04 kHz) or *Clean*, plus tape saturation and a gentle low-pass. Then slice. That's the jungle sound, 100 % original.
- Same seed → same bytes on the same platform; the cache is keyed by a content hash.

### Built-in kits (≥ 8; each has 8 pads and 1–3 breaks)

Ragga '93 · Darkside '92 · Atmos '95 · Liquid Velvet · Jump-Up Tin · Neuro Lab · Halftime Heavy · Minimal Roller

### Built-in instruments (≥ 12)

Sub (sine/triangle plus drive) · Reese (2–7 detuned saws, LFO'd filter, phaser) · Neuro (FM, wavefolder, comb, formant) · Wobble (tempo-synced filter LFO) · Hoover · Rave Stab (chord memory) · Organ Stab · Supersaw Pad · Atmos Pad (granular/strings) · FM Rhodes · Pluck · Vocal Formant (synthesised "ah / oh / yeah", no recorded vocals) · Dub Siren · Air Horn · FX (riser, downlifter, impact, spinback, tape stop, vinyl crackle) · Crowd (synthesised roar, cheer, groan)

### Original songs (≥ 12 for the full game; 1 for the slice)

You write them, as project files, across subgenres: Jungle/Ragga 160–168 BPM, Darkside 150–160, Atmospheric 162–168, Liquid 172–175, Rollers 172–175, Jump-Up 174–176, Neurofunk 172–176, Halftime 170 (half-time feel), and one tutorial song with a tempo ramp.

- 2:00–3:30 "radio edits": intro, build, drop, breakdown, build, second drop, outro, in 8/16/32-bar phrases.
- **Musical checklist**, enforced in code and tests since you can't listen: one key and mode per song (minor, dorian or phrygian); bass stays in scale; the sub is sidechained to the kick; a fill or variation ends every 8 bars; a riser and a one-beat gap before every drop; at least one hype phrase per song; loudness −16 LUFS integrated ±1 LU, true peak ≤ −1 dBTP, checked by `wheelup-cli lufs`.
- Five charts each (Beginner, Easy, Medium, Hard, Junglist): auto-charted, then hand-adjusted and validator-clean.
- Make authoring practical: a compact step notation inside RON (§14), and `wheelup-cli gen-tune --subgenre jungle --seed 42` to draft starting points that you then curate.

## 10. Rhythm gameplay

### Note types

- **Tap** (pad): judged on press.
- **Roll segment:** a run on one lane; that hand's shoulder button counts as an alternate stroke.
- **Hold** (L2/R2 rails): press in the window, stay above the press threshold (or in the target zone), release in the window or let it auto-complete; points per beat held.
- **Pressure zone:** a hold with a target zone (Light, Mid, Full).
- **Wobble path:** follow a drawn curve with the right stick during a hold; scored by correlation.
- **Sweep:** riser notes played with the gyro or the left stick.
- **Freestyle zone:** no notes. On-grid hits score **Style** — timing quantised to 1/16, density within a band, variety across pads, bass in key, siren and horn on the phrase.

### Judgement

| Judgement | Hard / Junglist | Medium | Beginner / Easy |
|---|---|---|---|
| **WICKED** | ±25 ms | ±30 ms | ±35 ms |
| **BIG** | ±50 ms | ±60 ms | ±70 ms |
| **SAFE** | ±90 ms | ±110 ms | ±130 ms |
| **MISS** | outside SAFE, or never hit | | |

- A hit goes to the earliest unjudged note on its lane whose window contains it. Each note is judged exactly once; lanes are judged independently (a chord is several lanes).
- Overhits (no note in the window): no penalty below Hard, a small vibe penalty on Hard and Junglist, never in freestyle zones.
- Optional early/late readout in milliseconds.

### Score, vibe, grades

- WICKED 300, BIG 200, SAFE 100, plus hold ticks. The combo multiplier climbs ×1 → ×4, one step per 10 hits; Wheel Up doubles it (×8 max).
- **Vibe** is the health meter: up on hits, down on misses. At zero it's **PLUG PULLED** — the power cuts, the tape winds down, the crowd groans — and the run fails, unless No-Fail is on.
- Accuracy %, grades S+ / S / A / B / C / D; the results screen shows a timing histogram and per-section accuracy.

### WHEEL UP! (the signature mechanic)

In sound system culture, when a tune goes off, the selecta pulls it back and drops it again. Here, **hype phrases** glow on the highway; clearing one without a miss adds 25 % hype. With at least 50 %, swipe the touchpad: spinback, air horns, crowd roar, a lightbar burst — and the transport jumps back to the start of the current 8-bar phrase. Its notes re-arm, the multiplier doubles while hype drains across the replay, and everything scored before stays scored. The music is sequenced, so the jump is just a transport seek with an FX tail: seamless, no pre-rendered audio.

### Audio modes

- **Live** (default): the backing parts come from the sequencer; *your* hits trigger the part you're charted on. Hit the wrong pad and you hear the wrong sound; miss and there's silence. On easier difficulties, the notes the chart leaves out are auto-played so the groove stays full.
- **Classic:** the whole song plays, and a miss ducks the charted part for that note. Meant for high-latency setups (Bluetooth, TVs).

### Auto-charter and playability validator (`wu-chart`)

Left hand = D-pad (P1–P4), L1, L2. Right hand = face buttons (P5–P8), R1, R2.

- A thumb plays one button, or two *adjacent* ones together (↑+←, ↑+→, ↓+←, ↓+→; △+□, △+○, ✕+□, ✕+○). Opposite pairs (↑+↓, ←+→, △+✕, □+○) are never chords.
- Minimum interval between notes on the same thumb: Beginner 400 ms, Easy 250, Medium 170, Hard 120, Junglist 85 — and anything under 110 ms must sit inside a roll segment.
- No roll strokes on a hand while that hand's trigger is holding.
- Maximum notes per chord: Beginner 1, Easy 2, Medium 2, Hard 3, Junglist 4. Peak density over any 2-bar window: 1.5 / 3 / 5 / 8 / 12 notes per second.
- Lanes per difficulty: Beginner = kick and snare (↑/↓); Easy adds hats (✕); Medium adds ghosts and claps (←/→) and the R2 rail; Hard adds every pad and L2; Junglist adds rolls, pressure zones, wobble paths and sweeps.
- Reduction order: drop low-velocity ghosts first, keep downbeats and backbeats, fold lanes together, then cap density.
- All these numbers are starting points: tune them by playtesting and log the changes. Property test: every generated chart passes the validator.

## 11. Presentation

- **Note views**, player's choice:
  1. **Highway:** four left lanes (D-pad), four right lanes (face buttons), a roll strip per hand, bass rails on the outer edges.
  2. **Tracker:** vertically scrolling rows and channels, after the 90s Amiga trackers a lot of early jungle was written on.
  3. **Pads:** a 2×4 grid with approach rings, for sampler players.
- Lanes use **shapes as well as colours** (colourblind-safe); glyphs match the connected controller.
- **Venues** are audio-reactive 2.5D scenes: tower blocks and a pirate antenna on the rooftop, lasers in the warehouse, a speaker stack whose cones pump with the sub, a basement club, a festival stage. The crowd follows vibe and hype.
- **Look:** 90s rave flyer — halftone, chromatic aberration, VHS noise, neon on black, bold OFL type; menus as a record bag of dubplates and white labels.
- **Visuals sync to the sequencer, not to audio analysis:** the engine knows exactly when every hit sounds, so flashes and pumps are frame-accurate. FFT is only for ambient texture.
- **Photosensitivity:** warning at boot; by default never more than 3 flashes per second (WCAG 2.3.1); strobe intensity slider; reduced-motion mode.
- **Performance:** 60 fps minimum on Steam Deck, 144 fps capable on desktop. Frame pacing never touches audio or judging.

## 12. UX, settings, persistence

- **Flow:** boot → photosensitivity notice → calibration (first launch) → main menu.
- **Calibration wizard:** audio offset (tap along to clicks: 16 taps, median, outliers dropped) and video offset (tap along to flashes). Stored per audio output device.
- **Settings:** audio device, buffer size, sample rate, live-hit mode (ASAP/Stable), audio mode (Live/Classic), offsets, note speed, note view, glyphs, haptics strength, lightbar, gyro, language, accessibility (No-Fail always on, reduced motion, high contrast, one-hand presets, audio cues).
- **Profiles and saves:** progress, scores and settings per profile, in the OS config directory; versioned with migrations; atomic writes.
- **Replays:** every run stores its timestamped input stream — used for ghosts, sharing, and as regression fixtures for the judge.
- **Autoplay ("selecta bot"):** hits everything perfectly; for demos, attract mode and tests.

## 13. Engineering requirements

- **Headless CLI** (`wheelup-cli`): `render <project> --out x.wav [--stems]` · `chart <project> [--difficulty …] [--validate]` · `lufs <wav|project>` · `bake-kits` (prints content hashes) · `gen-tune` · `replay <file>` (re-judges, prints the score) · `input-monitor` (events, timestamps, report rate, jitter histogram) · `midi-bridge` · `latency-test`.
- **Tests:**
  - Time: tempo-map round trips; ticks ↔ samples within half a sample at every supported rate, with no drift over an hour.
  - DSP: never NaN or infinite, bounded output, silence in → silence out.
  - Judge: perfect scripted input → 100 % WICKED; Gaussian jitter of σ = 15 ms → ≥ 99 % WICKED or BIG; every note judged exactly once (property test).
  - Charts: generated charts always pass the validator (property test); insta snapshots of the bundled songs' charts.
  - Audio: offline render of every bundled song — the voice-start log matches the sequence to the sample, onset detection on drum stems agrees within ±1 ms, loudness in range.
  - Replays: recorded input streams re-judge to identical scores.
  - Real-time safety: a dense song renders under `assert_no_alloc`.
  - Content: all bundled content loads and validates; the project loader is fuzzed or property-tested.
- **Benchmarks:** criterion on DSP hot paths and full-mix rendering. Budget: the callback uses ≤ 30 % of the buffer period at 48 kHz / 128 frames with 64 active voices on a mid-range laptop CPU.
- **CI (GitHub Actions):** Linux, Windows and macOS — fmt, clippy `-D warnings`, tests, release build, artifacts uploaded, licence-manifest check. Install what Linux needs (ALSA, udev, Wayland/xkbcommon headers; CMake if SDL3 builds from source).
- **Errors:** no `unwrap()`/`expect()` outside tests and startup; `thiserror` in libraries, `anyhow` at app boundaries; `tracing` spans around loading, baking and rendering.
- **Docs:** README (build, run, controls, the PS5 note), `docs/ARCHITECTURE.md` (thread and clock diagrams), `docs/CONTENT_FORMAT.md`, `docs/LATENCY.md`, and PLAN / PROGRESS / DECISIONS.

## 14. Data formats

A sketch — refine it, version it, document it.

```ron
// content/songs/rooftop-transmission/project.ron
Project(
    version: 1,
    meta: (title: "Rooftop Transmission", artist: "Rooftop Crew", key: "F minor", subgenre: Jungle),
    tempo: [(tick: 0, bpm: 168.0)],
    swing: 0.06,
    kit: "kits/ragga-93",
    tracks: [
        (name: "Break", instrument: Kit,                                     bus: Drums),
        (name: "Reese", instrument: Preset("instruments/reese/dark"),        bus: Bass),
        (name: "Sub",   instrument: Preset("instruments/sub/clean"),         bus: Bass),
        (name: "Siren", instrument: Preset("instruments/dub-siren/classic"), bus: Fx),
    ],
    patterns: {
        // drums: one string per pad, 16 steps per bar, "|" = bar line (validated)
        "break-a": Drums(bars: 2, steps: {
            P1: "x.........x.....|..x.......x.....",
            P2: "....x.......xxxx|....x..x....x...",
            P7: "x.x.x.x.x.x.x.x.|x.x.x.x.x.x.x.x.",
        }),
        // melodic: pitch:steps, "." = one-step rest, scientific pitch (C4 = 60)
        "reese-a": Notes(track: "Reese", bars: 2, notes: "F1:6 . . Ab1:4 C2:2 Bb1:2 | F1:16"),
        // … "sub-a", "siren-a", "riser-a"
    },
    arrangement: [
        (section: Intro, bars: 16, play: ["break-a"]),
        (section: Build, bars: 8,  play: ["break-a", "riser-a"]),
        (section: Drop,  bars: 32, play: ["break-a", "reese-a", "sub-a", "siren-a"]),
        // …
    ],
)
```

```ron
// content/songs/rooftop-transmission/junglist.chart.ron — generated by Chart It, then edited
Chart(
    version: 1,
    difficulty: Junglist,
    notes: [
        (tick: 0,    lane: Pad(P1),  kind: Tap),                                // kick, beat 1
        (tick: 0,    lane: Pad(P7),  kind: Tap),                                // hat, other hand
        (tick: 960,  lane: Pad(P2),  kind: Tap),                                // snare, beat 2
        (tick: 2880, lane: Pad(P2),  kind: Roll(len: 960, min_hits: 4)),        // 16th snare run, beat 4
        (tick: 3840, lane: Rail(R2), kind: Hold(len: 1920, zone: Some(Mid))),   // Reese, bar 2
        // …
    ],
    hype_phrases: [(start: 92160, end: 122880)],                                // bars 25–32
    freestyle: [],
)
```

## 15. Milestones

Each one ends with green gates, an updated PROGRESS.md (including how to try it), and a commit.

| # | Milestone | Done when |
|---|---|---|
| M0 | Skeleton | Workspace and crates; CI green on all three OSes; a Bevy window with a debug overlay; `wheelup-cli --help` |
| M1 | Audio engine and clock | cpal plus null/offline backends; kick/snare/hat synthesis; the sequencer plays a 2-bar DnB beat at 174 BPM; transport and clock snapshots; `wheelup-cli render` writes a WAV whose voice starts land on the exact expected samples; the `assert_no_alloc` test passes |
| M2 | Input and calibration | Input thread with SDL3 and gilrs backends; the **controller monitor** (the video's overlay, rebuilt) with report rate and jitter; pads play sounds live through the direct queue; calibration wizard stores offsets; keyboard and scripted-input backends |
| M3 | **Vertical slice** | One original jungle song with Hard and Easy charts; highway view; judge, score, vibe, results; practice tempo; autoplay; replay test; perfect scripted input = 100 % WICKED; a 10-minute playtest checklist for me to run with a real controller |
| M4 | Sound and content engine | All instruments and kits (baked, cached, hashed); mixer, buses, FX, sidechain, limiter; the LUFS tool; 3 songs × 5 difficulties; auto-charter and validator; hype phrases and Wheel Up; Live and Classic audio modes |
| M5 | Studio | Live, Pattern, Chop, Synth, Mixer and Arrange views; Perform FX; undo/redo; save, load, autosave; WAV, stems and MIDI export; Chart It and the chart editor; templates |
| M6 | Game structure | Campaign (venues, setlists, dubplates, unlocks), Quickplay and modifiers, Practice, profiles, settings, English and French, Studio Missions, tutorial |
| M7 | Controller deluxe | Adaptive triggers, rumble design, lightbar, player LEDs, gyro, touchpad gestures, pressure zones, wobble paths; remapping, presets and glyphs; MIDI Bridge; MIDI pad input; the USB haptics experiment |
| M8 | More modes | Soundclash, Back2Back, Pirate Signal endless with `gen-tune` |
| M9 | Content complete, ship it | ≥ 12 songs × 5 charts, every venue, tracker and pad views, accessibility pass, performance pass on Steam Deck, packaged builds, README with controls |

## 16. Start now

1. Read this whole document.
2. Write `docs/PLAN.md`: milestones broken into tasks, plus the risks you see (input timestamps off the main thread, Bevy version churn, DualSense effects through SDL3, the volume of content to author).
3. Build M0, then keep going through the milestones without waiting for my confirmation.
