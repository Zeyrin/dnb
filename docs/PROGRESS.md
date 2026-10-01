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

## M2: input and calibration ✅ (needs a real controller to confirm feel and timing)
**Works**
- `wu-input`: controllers on their own thread (gilrs backend), every event stamped on the
  shared clock; on Linux the kernel's own event time is used when it is plausible.
  Pad presses go straight to the audio engine from that thread, and to the game with their
  timestamps. Layouts: Reel (the video's mapping) and Drummer (kick on ↓).
- Analog triggers press at 20 % and release under 10 % (hysteresis).
- Screens (Tab or CREATE switches): **Play** (the demo groove and the pads), **Controller**
  (the reel's overlay rebuilt: button → pad → MIDI note, sticks, triggers, shoulders,
  report rate and jitter, an event log; L3 or X swaps the layout), **Calibrate** (tap to
  clicks, then to flashes; refuses to save uneven results; saved per audio output).
- Settings saved to `<config dir>/wheelup/settings.ron` (layout, calibration), written atomically.
- `wheelup-cli input-monitor` prints controller events, timestamps, report rate and jitter.
- F12 saves a screenshot to `screenshots/`.

**Verified**
- Tests: mapping and layouts, trigger hysteresis, the input thread (ordering, timestamps,
  live play on and off), interval statistics, calibration maths (including a randomized test),
  settings round trips and corrupt files.
- End to end under Xvfb, driven by xdotool: both calibration tests ran and the settings file
  was written.

**Known gaps**
- Not yet tried with a real controller here (none attached): the report rate, jitter and the
  feel of live play need a playtest. Run `wheelup-cli input-monitor` and the Controller screen.
- SDL3 (DualSense touchpad, gyro, lightbar, adaptive triggers) moves to M7, where those
  features are used (ADR-007).
- Keyboard timestamps are quantised to the frame (ADR-009).

## M3: the vertical slice ✅ (needs a playtest with a controller and speakers)
**Works**
- **Rooftop Transmission**, the first original tune: 168 BPM, F minor, 72 bars (1:43),
  intro, build, a snare-roll fill, a two-step drop, a chopped-break section, a breakdown,
  a second drop, outro, with a sub bass throughout.
- Song projects in RON (`content/songs/…/project.ron`): drum patterns in step notation,
  bass lines in note notation, an arrangement of sections; compiled to hits and notes.
- The sub bass: baked once, played at any pitch, sustained through a seamless loop,
  released when the note ends.
- `wu-chart`: charts cut from the song's drums for five difficulties, strongest beats and
  most important pads first, within thumb rules (no opposite buttons together, minimum
  gaps per thumb, peak density); a validator checks every rule.
- `wu-game`: the judge (WICKED / BIG / SAFE windows per difficulty; each note judged once;
  lanes independent), scoring (combo multiplier ×1–×4, vibe meter, PLUG PULLED at zero,
  accuracy, grades S+ to D), runs and replays.
- Screens: **SONGS** (difficulty, practice tempo 50–150 %, selecta bot, No-Fail),
  **RHYTHM** (the highway: lanes as the thumbs sit, count-in, WICKED/BIG/SAFE/MISS pop-ups,
  early/late readout, combo, vibe meter, pause), **RESULTS** (grade, score, counts, a
  timing histogram, the replay saved). The demo pads moved to **JAM**.
- The player's pads sound at once; the backing plays everything the chart leaves out;
  a miss is silence.
- CLI: `songs`, `render <song>`, `chart <song> --show-bars N`, `replay <file>` (judges a
  saved run again from its presses alone).

**Verified by tests**
- Perfect presses score 100 % WICKED; presses with σ = 15 ms jitter score ≥ 99 % WICKED or BIG.
- Every note is judged exactly once, whatever the presses (property test).
- Replays re-judge to exactly the live score, with random delivery delays and frame rates
  (property test).
- Every chart of every bundled song is playable at every difficulty, and each difficulty
  has more notes than the one below; randomly generated drum parts always chart validly.
- Songs compile; the bass stays in the song's key; held notes loop and release; the audio
  callback never allocates with notes playing.

**Known gaps**
- Needs a playtest: feel, chart difficulty, highway speed, mix (see `docs/PLAYTEST.md`).
- Junglist is generated but not offered until roll segments arrive (M4).

## M4: the sound and content engine (in progress; the steps are in [`PLAN.md`](PLAN.md))
**Step 1, mix and master ✅**
- Four buses (drums, bass, music, FX), each with its level; every sound knows its bus.
- The sub ducks under every kick, the sequenced ones and the player's, on the kick's exact
  frame (−6 dB, back within 120 ms; each song can set its own).
- A true-peak look-ahead limiter on the master (ceiling −1.2 dBTP) instead of the soft
  clipper. Its 1.5 ms delay counts as output latency in the clock, so timing stays exact.
- A BS.1770-4 loudness meter (K-weighting, gating, momentary and short-term maxima, 4×
  true peak) and `wheelup-cli lufs <song | demo | file.wav>`.
- Songs carry a `mix` section; Rooftop Transmission measures −15.7 LUFS, −2.2 dBTP, and
  the JAM groove −16.0 LUFS.

**Verified by tests**
- The meter reads the EBU Tech 3341 signals within 0.1 LU, including both gates; the
  K-weighting matches the standard's 48 kHz coefficients.
- The limiter passes quiet audio bit for bit and never lets random bursts over the ceiling
  (property test); the true-peak detector finds the crest a quarter-rate sine hides.
- The bass dips on the kick's frame and comes back; buses level and sum as set.
- Every bundled song renders at −16 LUFS ± 1 LU and at most −1 dBTP.
- Hits still land on their exact frames in offline renders; the callback still never
  allocates.

**Step 2, notes beyond taps: rolls ✅, holds ✅** (Classic mode next)
- Junglist charts keep fast single-lane runs as rolls; the highway draws them as a band
  marked L1 or R1, and that shoulder button plays the roll's lane while it's in reach,
  straight from the input thread. Junglist is on the song screen.
- Rooftop Transmission on Junglist: 801 notes, 2 rolls (the snare roll into the drop, the
  tom turnaround). `wheelup-cli chart` counts rolls and marks their notes `r`.
- Tests: a single-lane run becomes a roll, a run that hops lanes is thinned instead, a roll
  may run into the next downbeat; the validator rejects fast notes outside rolls, a roll
  interrupted by another pad, and rolls below Junglist; every generated chart stays playable.
- The bass line is played on the triggers from Medium: R2 alone, then L2 and R2 from Hard
  (split by pitch, taking turns when the line is legato), 94 holds in Rooftop Transmission.
  The trigger plays the sub itself, from the input thread, and the engine stops it at the
  charted end or when the trigger comes up. The highway draws the rails at its edges and
  eats each hold at the hit line while it's held; the results count holds kept.
- Replays record releases (version 2; older replays still load).
- Tests: holds on one rail leave room to let go; two rails share a legato line without
  cutting it; no rail while its hand rolls; a hold pays for the share held and completes
  on its own; a rail note stops at its charted end or on release, without allocating;
  the input thread plays the armed bass note and roll pad. The selecta bot plays Hard at
  150 % to S+, 827 / 827 WICKED, 94 / 94 holds, and `wheelup-cli replay` matches it.

