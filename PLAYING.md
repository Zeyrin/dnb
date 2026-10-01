# Playing WHEEL UP!

A junglist rhythm game: play the drums and hold the bass of real drum & bass and jungle
tunes on a gamepad (or the keyboard).

## Start it

- **Windows:** double-click `wheelup.exe`. Windows may say it "protected your PC" (the
  game isn't signed yet): click **More info**, then **Run anyway**.
- **macOS (Apple silicon):** open Terminal in this folder and run
  `xattr -dr com.apple.quarantine . && ./wheelup`. (Or right-click `wheelup`, **Open**,
  and confirm.) Intel Macs aren't built yet: say if you need one.
- **Linux:** `./wheelup`. It needs ALSA and udev, present on most desktops.

Play on **wired headphones or speakers**. Bluetooth and TVs add so much delay that your
hits sound late: if that's all you have, set **Audio** to **Classic** on the song screen.

## First five minutes

1. Plug in your controller (DualSense, DualShock 4, Xbox, Switch Pro…) before starting.
2. **CREATE** (or **Tab**) moves between screens. Go to **CALIBRATE** first and follow
   it: it measures your audio and video delay, once per headphones or speakers.
3. Go to **SONGS**, pick **Rooftop Transmission**, **Easy**, and press **OPTIONS**
   (**Space**). Hit each note as it reaches the line.
4. Turn **Autoplay** on to watch the selecta bot play it first.

## Controls

| Controller | Keyboard | Does |
|---|---|---|
| D-pad ↑ ↓ ← → | ↑ ↓ ← → | kick, snare, ghost, rim |
| △ □ ✕ ○ | I J K L | jungle snare, low tom, closed hat, open hat |
| L1 / R1 | E / O | inside a roll band: the roll's pad (left hand L1, right R1) |
| L2 / R2 | Z / N | the bass rails: hold for as long as the bass note lasts |
| L3 + R3 | X + M | WHEEL UP!: pull the tune back once the hype meter is half full |
| OPTIONS | Space / Enter | play, pause |
| CREATE | Tab | next screen; quit a song |
| ✕ / ○ | K / L | in menus: confirm / back |
| | F12 | screenshot |
| | Esc | quit |

**JAM** is free play over a groove: press pads and listen.

## Tell us

What felt late, too hard, too easy, or just wrong, and what sounded great or awful.
The checklist we test with is `docs/PLAYTEST.md` in the repository.

Fonts: Bungee and JetBrains Mono, under the SIL Open Font License (see `licenses/`).
Every sound in the game is synthesised from code.
