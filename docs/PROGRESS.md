# Progress

## M0: skeleton ✅
- Cargo workspace with shared lints and profiles; CI on Linux, Windows and macOS
  (`.github/workflows/wheel-up.yml`, only triggered by changes under `wheel-up/`).
- `wu-time`: ticks, tempo maps (exact to the sample after an hour), swing, the shared clock.
- `wheelup` opens a window; `--screenshot` captures it headless (Xvfb + software Vulkan works).

## M1: audio engine and clock ✅ (sound-card output untested on real hardware)
**Works**
- Drum synthesis (`wu-instruments`): kick, snare, ghost, 12-bit "jungle" snare, rim, clap,
  tom, closed and open hats with a shared choke group. The default kit, Ragga '93,
  bakes in about 0.2 s.
- Engine (`wu-audio`): sample-accurate sequencer, 128-voice pool with stealing and choke
  groups, loops, seeks, live hits (ASAP and Stable scheduling), a log of every voice start,
  garbage handed back to the main thread so the callback never frees memory.
- Clock: a seqlock snapshot per callback; a regression-based estimator on the main thread
  (averages ±1.5 ms of callback jitter down to under 0.5 ms; follows a drifting sound card).
- Outputs: offline (deterministic), null (real-time pace, no device), cpal (any sample format,
  any channel count, Bluetooth detection).
- The game plays the demo groove; pads light at the moment each hit reaches the speaker;
  bar and beat follow the audio clock. Keyboard plays pads live.
- `wheelup-cli render demo` renders 8 bars in about 40 ms.

**Try it**
```sh
cargo run -p wheelup -- --autoplay
cargo run -p wheelup-cli -- render demo --out demo.wav
```

**Verified by tests**
- Hits start on the exact frame the tempo map gives, with buffers of 1, 97, 256 and 4096 frames.
- Loops repeat seamlessly; seeks move the next hit; live hits land where their mode says.
- The audio callback never allocates, through voice stealing, chokes, seeks and live hits
  (`assert_no_alloc` test).

**Known gaps**
- Not yet heard on a real sound card here (the build machine has none): needs a playtest.
- Keyboard pad timestamps are taken when a frame processes them, so live play from the
  keyboard carries up to a frame of extra latency. Controllers get their own thread in M2.

## Next: M2, input and calibration
