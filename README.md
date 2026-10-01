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

Rust stable (1.95 or newer). On Linux, install the audio, input and windowing headers first:

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

### Controls (until controller support lands in M2)

| Key | Does |
|---|---|
| Space | play / stop the demo groove |
| R | back to the start |
| ↑ ↓ ← → | D-pad pads: kick, snare, ghost, rim |
| I J K L | face pads △ □ ✕ ○: jungle snare, low tom, closed hat, open hat |
| Esc | quit |

## Headless tools

```sh
cargo run -p wheelup-cli -- render demo --bars 8 --out demo.wav   # faster than real time
cargo run -p wheelup-cli -- devices                               # list sound cards
cargo run -p wheelup-cli -- play demo --buffer 128 --seconds 20   # play on a sound card
```

## Layout

| Crate | What it is |
|---|---|
| `crates/wu-time` | ticks (960 per beat), tempo maps, swing, the shared monotonic clock |
| `crates/wu-dsp` | oscillators, filters, envelopes, noise, saturation, the "Sampler Era" crusher |
| `crates/wu-instruments` | procedural drum synthesis and kits (no third-party audio) |
| `crates/wu-audio` | the engine: sequencer, voices, clock, offline/null/sound-card outputs |
| `crates/wu-content` | step notation, the demo groove, the licence manifest |
| `apps/wheelup` | the Bevy game: rendering, UI, glue |
| `apps/wheelup-cli` | headless tools |

Before committing: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`. CI runs the same on Linux, Windows and macOS.

## Licences

Every shipped asset is listed with its source and licence in
[`content/licenses.ron`](content/licenses.ron); a test fails on anything unlisted.
Fonts are under the SIL Open Font License. All drum sounds are synthesised from code.
