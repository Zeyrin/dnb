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
