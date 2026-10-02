# Progress

## M0: skeleton ✅
- Cargo workspace with shared lints and profiles; CI on Linux, Windows and macOS
  (`.github/workflows/ci.yml`).
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

## M4: the sound and content engine ✅ (needs a playtest by ear and by thumb; the steps are in [`PLAN.md`](PLAN.md))
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

**Step 2, notes beyond taps: rolls, holds and Classic audio ✅**
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
- Classic audio (song screen, Audio row, saved): the song plays the player's part too, a
  miss mutes it until the next hit, presses stay silent. The song screen suggests it on
  Bluetooth. Tested in the engine: a muted part stays silent until unmuted, the backing
  never does; a Classic backing keeps every event and marks the player's.

**Step 3, hype and WHEEL UP! ✅**
- Songs mark hype sections; every 8 bars of one is a hype phrase, drawn on the highway as a
  gold band (grey once a note in it is missed). Each cleared phrase fills a quarter of the
  hype meter under the score.
- At half full, L3 + R3 (X + M on the keyboard): the music cuts on the next bar line, the
  spinback and the air horn play for two beats, the crowd roars and the tune drops back at
  the start of its 8-bar phrase. Its notes come round again, the multiplier doubles (×8 at
  most) while they do, and everything scored before stays scored. The selecta bot pulls
  up at the end of a phrase when it can.
- All three sounds are synthesised; the engine schedules the jump to the frame and reports
  every transport change, so presses either side of the cut are judged exactly.
- Replays record the rewind (version 3); a replay from a newer game is refused.
- Tests: the jump waits out its gap and replays the phrase on exact frames; the judge's
  splice keeps every index; WHEEL UP! needs the hype, replays its phrase with the
  multiplier doubled, and re-judges identically from the replay (a miss just before the
  cut counted once); two sticks together wheel up; every song has a hype phrase. Under
  Xvfb the selecta bot pulls up after the first drop and finishes S+, 936 / 936 WICKED;
  `wheelup-cli replay` agrees.

**Step 4, instruments ✅**
- 20 instruments: synth patches played live, a voice per note (Reese, Wobble, Rave Stab,
  Organ Stab, Hoover, Atmos Pad, Supersaw Pad, FM Rhodes, Pluck, Vocal Ah / Oh / Yeah, Dub
  Siren, Riser, Downlifter, Impact) and baked one-shots pitched by rate (Sub, Air Horn,
  Spinback, Crowd). One synth engine makes them all: up to seven unison oscillators or an
  FM pair with a tine, or a voice sung through three gliding formants; a filter with its
  own envelope, an LFO (tempo-synced if asked), sweeps over the note, a phaser.
- Chord memory: a stab plays its whole chord on one key. The note notation takes chords
  too (`F3+Ab3+C4:16`).
- The mix gets two returns: a reverb (an eight-line feedback delay network) and a dub
  delay (ping-pong, each repeat darker, thinner and saturated, the tape wowing), the
  echoes feeding the reverb. Every sound has a send to each.
- Programs hold several instruments: the bass line plays on every bass sound (a Reese can
  layer over the sub), and the rails play them all live; other parts are tracks.
- `wheelup-cli instruments` lists them, `wheelup-cli audition reese --out reese.wav`
  plays one (alone or over the demo beat).
- Tests: synth voices play their pitch, release and stop, stay as loud with seven
  oscillators as with one, open on the filter envelope, sweep, sing and ring; every
  instrument sits between −34 and −8 dBFS at a peak under full scale; the reverb tail
  falls 60 dB in its decay time; the delay's echoes alternate sides and fade; the pool
  starts and steals chords, and lets a rail go; bass notes reach every bass sound and
  track notes only their own; a dense song with synths, chords and sends never
  allocates on the audio thread.
- Tape stop needs the whole mix, not a note: it comes with the Perform FX (M5).
- Songs name their parts: `bass` lists the sounds the bass line plays on (the rails play
  them all), `tracks` give the other parts an instrument, a level, a pan and sends, and
  `Notes` patterns write their notes, chords and long rests (`.:12`) included. An unknown
  instrument, a missing track or a chord in the bass line is refused with the reason.
- Rooftop Transmission gets its instruments: a Reese over the sub, an atmos pad through
  the intro and the breakdown, rave stabs (minor sevenths on F, Bb and C) in the drops, a
  dub siren as drops one and three land and the air horn on drop two, FM Rhodes and a
  delayed pluck in the breakdown, a four-bar riser and a sung "yeah" into each drop. The
  build and the breakdown are each split in two so the riser ends on the drop; the drums
  and the bass line are unchanged, and so are all five charts. −16.0 LUFS, −2.7 dBTP; the
  whole song renders in 2.3 s.
- Tests: tracks play their notes on their own instruments, the bass on every bass sound;
  each mistake is explained; every part of every song stays in its key, chord memory
  included.

**Step 5, kits and breaks ✅**
- Six breaks, each an original drummer's performance written in step notation: Rough Rider
  (syncopated funk), Sunday Service (a gospel shuffle on the ride), Bunker Funk (sparse
  and heavy), Velvet Ride (smooth, rolling), Tin Can (tight, four bars ending in a tom
  fill), Half Step (half time). The drum synth plays them with a person's timing and touch
  (a few milliseconds off the grid, softer hits darker), in a room; the sampler speeds
  them up to jungle tempo, pitching them up like the records were, through its converters
  (8-bit tracker, 12-bit rack sampler or drum machine, or clean) and onto tape. The room's
  tail wraps round so the loop is seamless, and it is cut into slices at the performed hits.
- Eight kits, every pad in the same role so any pattern plays on any of them: Ragga '93
  (unchanged), Darkside '92, Atmos '95, Liquid Velvet, Jump-Up Tin, Neuro Lab, Halftime
  Heavy, Minimal Roller. Each has one or two breaks; most play a slice of one on their
  jungle-snare pad. New drum voices for them: ride, crash, shaker.
- Kits bake once: the game bakes every one on a thread at launch and keeps them on disk,
  under a fingerprint of the definition, the sample rate and the synthesis code itself, so
  any change bakes afresh and a damaged file is simply baked again.
- Songs can run a break under the drums (`break/rough-rider`), played at the song's tempo.
  Rooftop Transmission runs Rough Rider under every drop. Its mix was set by measuring each
  bus in each section: in the drops drums −17, bass −20, stabs −24 LUFS; the keys and pad
  lead the breakdown. Still −16 LUFS overall.
- `wheelup-cli kits`, `kit <id> --out kit.wav` (each pad, then the breaks), `breaks`,
  `break <name> --out break.wav` (looped, then each slice).
- Tests: every break performs whole bars at its tempo, a human but never lost drummer, the
  same bytes every time, a loop without a seam; every kit bakes eight pads and its breaks,
  deterministically, with its own fingerprint; a kit survives the disk bit for bit and a
  damaged file is baked again; the song's break plays at its tempo, once round every two bars.

**Step 6, two more songs ✅**
- The musical checklist is tested on every song: each eight-bar phrase with drums turns
  round on a fill (a section's `fill` swaps the last bar of its phrases for a fill
  pattern); each drop lands after a riser and a beat of silence (`gap` cuts the drums,
  the bass and the break for the last beat before it); every song has a hype phrase.
- Underpass Spirits (Grey Static): darkside at 170 BPM in E phrygian, on the Darkside '92
  kit. A choir in the tunnel over a lonely kick, Bunker Funk stirring far off, a build
  where the hoover teases E then F, a heavy two-step drop with hoover stabs answering
  the sub and the Reese, a drone breakdown, and a second drop on the break chopped across
  the jungle snare. Five charts, 122 to 813 notes. −16.0 LUFS, −1.6 dBTP.
- Its mix was set by measuring each part alone in each section (`wheelup-cli render SONG
  --only drums,bass,hoover`): the Darkside kick's boom, 6 dB more sub than Ragga '93's,
  buried the top of the drops, so it is shorter (amplitude decay 0.25 → 0.18 s).
- Tidewater Lights (Lune Avenue): liquid funk at 174 BPM in A minor, on the Liquid Velvet
  kit. The Rhodes and strings walk the liquid turnaround (Am9, Fmaj7, Dm9, Em7, voiced
  close), the sub walks it under them with a breath of Reese, Velvet Ride rolls its ride
  under a two-step, a plucked call answers itself down the dub delay, and a voice holds
  each chord's colour through the breakdown. The rise drops the bass so the second drop
  brings it back, on the break chopped across the jungle snare. Five charts, 126 to 719
  notes: the shaker plays on the beat against the hat off it (the lanes alternate), and
  is left out of the drops, where the break's ride already rolls, so no chart is a wall
  of sixteenths. −16.0 LUFS, −1.2 dBTP.
- Bounce Patrol (Tin Can Crew): jump-up at 175 BPM in G minor, on the Jump-Up Tin kit. The
  bass is the hook: a wobble bouncing over the sub with stops and octave jumps, organ stabs
  answering it, a horn and a siren as the drops land, a half-time switch-up where the
  wobble holds, and the Tin Can break chopped across the jungle snare in drop two. Five
  charts, 152 to 889 notes. −16.0 LUFS, −1.2 dBTP. The Wobble lost its sine an octave
  under its notes: over the sub, it put the bass an octave below what was written, down
  where nothing plays it; it is a mid-bass now, as the Reese is.
- Satellite Drift (Lumen Atlas): atmospheric at 172 BPM in D dorian, on the Atmos '95 kit.
  Pads and Rhodes sway between Dm9, G9, Cmaj9 and Am7, the sub walks under them, a plucked
  call floats up the mode with the delay answering, Velvet Ride rolls its ride under the
  drops and Sunday Service plays far off before them; the breakdown floats without the
  bass, a voice holding each chord's colour. The gentlest charts so far, 125 to 618 notes.
  −15.8 LUFS, −1.2 dBTP.

## Your tune: import
- Drop an MP3, WAV, FLAC, OGG or M4A on the game window (or run `wheelup-cli import FILE`):
  it is listened to in the background, kept in the player's library (their machine only,
  `<data dir>/wheelup/imports/<fingerprint>/`: a copy of the audio and `song.ron`, what
  was heard), and selected on the songs screen, whose new first row picks the song.
- `wu-import` listens: a third-octave spectrogram; the tempo from the onsets'
  autocorrelation (expecting drum & bass, so no half or double tempo), fitted to every
  strong onset from the first ones outwards, then pinned to the drums' real attacks,
  read in the crack band (zero-phase filtered: the lows smear a long kick's attack
  early); the bar line where the lows lift most with the snare on two and four, or on
  three in half time, among all sixteen sixteenths.
- The drums, step by step: a small logistic model each for kick, snare, ghost and hat
  over how nine bands jump, start and ring, fitted on our song played on all eight kits.
- The bass line: the kick is learnt from the tune (its lows lined up to a fraction of a
  sample and averaged over the middle of the pile, then again over the kicks heard
  clean), placed where its neighbours on the same sixteenth lie (swing is followed)
  unless it is clearly heard where it is (a played break's kicks each fall their own
  way), found where the drum listener missed it, and subtracted. A break's own kick,
  ringing on a note between the programmed ones, is learnt from what the first leaves:
  from the leftover most of the others sound like, kept only if its pitch falls as it
  opens and it dies away (a bass note does neither), and subtracted too. YIN reads every
  step's pitch in what is left; a Viterbi decoder finds the likeliest line, a change of
  note cheaper on a kick or a beat, so a note the kick blurs still starts on the kick.
- The sections: four-bar blocks are full when they play almost as much as the fullest
  (drums, bass, level, a snare on two and four); runs of full blocks are the drops, and
  their eight-bar phrases the hype phrases; the rest is intro, build, breakdown or outro.
- An imported song plays its own recording: the engine reads it where the transport is
  (the count-in plays its pickup; loops and WHEEL UP! follow it, faded in over 3 ms), at
  a practice tempo slower and lower like a turntable, turned to the game's −16 LUFS; a
  miss muffles it (a low-pass down to 420 Hz) until the next hit. Presses never sound
  over it. The recording is decoded and resampled (Kaiser-windowed sinc) while the song
  sits selected, so play starts at once.
- Tests: Rooftop Transmission on all eight kits is heard at 168.00 BPM with its first
  bar line within 3 ms; its bass line on the right key 97 % of the time, 96 % of its
  notes starting on the right step; its sections exactly (Intro, Drop, Breakdown,
  Drop 2, Outro); bounced to a WAV, it imports, reloads and charts playably at every
  difficulty. MP3, FLAC, OGG and M4A bounces import alike, tags and all. Underpass
  Spirits, with Bunker Funk's kick ringing on a D under its own, is heard at 170.00 BPM,
  its bass line on the right key 93 % of the time (71 % before the break's kick was
  taken out), its drops exactly. A break's kick laid under a two-step, a few
  milliseconds off each time, comes out 19 dB down (7 dB with the first kick alone).

## The look: a rave flyer at night (groundwork for M6 and M9)
- Behind every screen, a city at night from a pirate station's rooftop, drawn by one
  shader: a violet sky and a haze over two layers of tower blocks with burning windows,
  the station's mast with its red light blinking slowly and its signal going out in
  rings with the kick, printed in halftone with scan lines, grain and a VHS tracking band
  rolling by now and then. In a hype phrase the lasers come on behind the city; hype
  warms the night toward gold; a WHEEL UP! flares once.
- The camera is in high dynamic range with a tight bloom: only what burns brighter than
  white glows. The highway is drawn in the world now: dark glass over the venue, lane
  lines, each lane lit from below in its colour, a hit line that swells with the kick,
  receptors shaped like their buttons (←, ↑, ↓, →, □, △, ×, ○), notes with a white-hot
  core and their button's shape on them, holds, rolls tinting their lane, hype phrases
  framed in gold. A hit throws a ring of light, the best ones a beam up the lane; a
  judgement pops and settles; a big faint combo counts behind the notes.
- The header and the tabs go away while a song plays; the developer readout (frame rate,
  audio) is on F3, and only a warning the player must see (no sound, Bluetooth) shows
  anyway.
- The songs screen plays what is selected: once the selection rests on a tune for a
  moment, its first drop loops, 5 dB under the game, and the city pulses on its kicks
  with the lasers out. The tune's record turns beside the menu at 33⅓ (it gets up to
  speed and winds down like a deck), its label in the tune's colour (jungle yellow,
  darkside violet, liquid teal, your tunes white), its halo on the kick.
- WHEEL UP! has its moment: as the record is pulled back, a vinyl leaps up over the
  highway with a bounce and spins backwards, slowing as the rewind runs out, then drops
  away as the tune comes back; the picture splits its colours for an instant like a tape
  pulled off its heads, the lasers go full and the banner pops.
- Photosensitivity: nothing flashes on the beat; the kick swells light by a few per cent,
  and a WHEEL UP!'s flare is a single warm glow. A strobe slider and reduced motion come
  with the settings (M6).
- `wheelup --screenshot PATH --screenshot-at SECONDS` captures a given moment of a song
  (the music runs on the clock, however slowly a software renderer draws).

## Game structure (M6, started)
- Records: the best run on every song at every difficulty, kept in the player's data
  folder (`records.ron`, written to a temporary file then renamed over the old one). A
  run sets one only at the song's own tempo, played by a person, with No-Fail off and
  the plug left in. The results say FIRST RECORD, NEW BEST! (and what it beat), the
  best still standing, or why the run didn't count; the songs screen shows the record
  under the tune's vinyl. A records file from a newer game is never overwritten.
- Note speed, on the songs screen and kept in the settings: 0.75× to 3×, where 1× shows two
  seconds of the song ahead on the highway and 2× one.

