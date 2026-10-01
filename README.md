# WHEEL UP!

A junglist rhythm game and controller-first DAW, in Rust. You play drum & bass and
jungle on a gamepad: a rhythm game on top, a real music tool underneath, and every
song in the game is a project you can open, remix and turn back into a chart.

The full brief is [`PROMPT.md`](PROMPT.md). Progress is in [`docs/PROGRESS.md`](docs/PROGRESS.md),
the plan in [`docs/PLAN.md`](docs/PLAN.md), and the reasons behind choices in
[`docs/DECISIONS.md`](docs/DECISIONS.md).

> **PS5:** shipping on PlayStation requires being a licensed Sony partner with their
> NDA SDK, so WHEEL UP! targets Windows, macOS, Linux and Steam Deck, with the
> DualSense as its hero controller. Platform services sit behind traits so a console
> port stays possible.

## Build and run

Rust: `rust-toolchain.toml` pins the version; rustup installs it on first build. On Linux, install the audio, input and windowing headers first:

```sh
sudo apt-get install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
```

Then, from this folder:

```sh
cargo run -p wheelup                 # the game (first build takes a while: Bevy)
cargo run -p wheelup -- --autoplay   # start the demo groove straight away
cargo run -p wheelup -- --buffer 128 # ask the sound card for a smaller buffer
cargo run -p wheelup -- --silent     # no sound card: the engine runs silently
```

### Controls

Plug in a controller (DualSense, DualShock 4, Xbox, Switch Pro…) and press pads; the
keyboard stands in for one (its timing is only as fine as the frame rate).

| Controller | Keyboard | Does |
|---|---|---|
| D-pad ↑ ↓ ← → | ↑ ↓ ← → | pads: kick, snare, ghost, rim (Reel layout) |
| △ □ ✕ ○ | I J K L | pads: jungle snare, low tom, closed hat, open hat |
| L1 / R1 | E / O | roll strokes |
| L2 / R2 | Z / N | sub and bass rails (analog on a controller) |
| OPTIONS | Space / Enter | play / stop |
| CREATE | Tab | next screen: Play, Controller, Calibrate |
| L3 (Controller screen) | X | swap layout: Reel ↔ Drummer (kick on ↓) |
| | R | back to the start |
| | F12 | screenshot to `screenshots/` |
| | Esc | quit |

Calibrate once per audio output: the **Calibrate** screen measures how late you tap after
the sound and after the picture, and saves both.

## Headless tools

```sh
cargo run -p wheelup-cli -- render demo --bars 8 --out demo.wav   # faster than real time
cargo run -p wheelup-cli -- devices                               # list sound cards
cargo run -p wheelup-cli -- play demo --buffer 128 --seconds 20   # play on a sound card
cargo run -p wheelup-cli -- input-monitor                         # controller events, rate, jitter
```

## Layout

| Crate | What it is |
|---|---|
| `crates/wu-time` | ticks (960 per beat), tempo maps, swing, the shared monotonic clock |
| `crates/wu-dsp` | oscillators, filters, envelopes, noise, saturation, the "Sampler Era" crusher |
| `crates/wu-instruments` | procedural drum synthesis and kits (no third-party audio) |
| `crates/wu-audio` | the engine: sequencer, voices, clock, offline/null/sound-card outputs |
| `crates/wu-input` | controllers on their own thread, layouts, trigger thresholds, statistics |
| `crates/wu-game` | rules: calibration now, judging and scoring from M3 |
| `crates/wu-content` | step notation, the demo groove, settings, the licence manifest |
| `apps/wheelup` | the Bevy game: rendering, UI, glue |
| `apps/wheelup-cli` | headless tools |

Before committing: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`. CI runs the same on Linux, Windows and macOS.

## Licences

Every shipped asset is listed with its source and licence in
[`content/licenses.ron`](content/licenses.ron); a test fails on anything unlisted.
Fonts are under the SIL Open Font License. All drum sounds are synthesised from code.
