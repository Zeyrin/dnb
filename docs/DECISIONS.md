# Decisions

Short records: what was decided, why, and what it costs. Newest last.

## ADR-001: build inside the FFFood repository, for now
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
