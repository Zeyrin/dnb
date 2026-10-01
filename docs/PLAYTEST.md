# Playtest checklist (about 10 minutes)

What only a person with a controller and speakers can judge. Note anything that
feels off, with the answer to each question, and send it back: it decides what
gets tuned next.

**You need** a wired controller (a DualSense if you have one), wired headphones
or speakers (not Bluetooth), and the game built from this folder.

## 1. Sound and latency (2 min)
```sh
cargo run --release -p wheelup -- --screen jam --buffer 128
```
1. Press Space (or OPTIONS) and listen to the groove. Do the kick, snare and hats
   sound like drum & bass and jungle? Too clean? Too harsh?
2. Tap pads along with the groove. Does the sound feel attached to your thumb, or
   does it lag? If it lags, try `--buffer 64`, then `--buffer 256`.
3. The top-left overlay shows the output latency: write it down.

## 2. The controller (2 min)
Press Tab (or CREATE) until **CONTROLLER**.
1. Every button lights its row; ↑ ↓ ← → △ □ ○ ✕ show C1 D1 C#1 D#1 E1 F1 G1 F#1.
2. Move both sticks in circles: the dots follow, and the readout under
   "event interval" settles. Write down the median and the 95th percentile.
3. Squeeze L2 and R2 slowly: the bars fill smoothly.
4. Press L3: the layout switches to Drummer (kick on ↓). Which do you prefer?

## 3. Calibration (2 min)
Tab to **CALIBRATE** and follow it: 16 taps on the clicks, 16 on the flashes.
Write down both numbers. Did "taps too uneven" come up, and was it fair?

## 4. The tune (4 min)
Tab to **SONGS**. Play **Rooftop Transmission** on Easy, then on Hard.
1. Do the notes line up with what you hear? (If every hit reads "late" or "early",
   calibration is off: say which way.)
2. Easy: too easy, right, too hard? Hard: same question.
3. Do any notes feel impossible for your thumbs (two at once on the same thumb,
   too fast in a row)?
4. Is the highway readable? Note speed too fast or too slow?
5. Try 70 % tempo: does it help practise?
6. At the end, does the grade feel deserved?

## 5. Anything else
Crashes, freezes, confusing screens, things you expected a button to do.
