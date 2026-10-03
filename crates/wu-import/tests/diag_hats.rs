//! TEMPORARY, never committed: hat evidence per sixteenth of the bar on a tune.
use std::path::Path;
use wu_import::decode::decode;
use wu_import::find_grid;
use wu_import::hits::{Drum, hear_drums, step_evidence};
use wu_import::spectrum::analyse;

#[test]
fn diag_hats() {
    let Ok(file) = std::env::var("DIAG_FILE") else { return };
    let from: i64 = std::env::var("DIAG_FROM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32);
    let to: i64 = std::env::var("DIAG_TO").ok().and_then(|v| v.parse().ok()).unwrap_or(64);
    let decoded = decode(Path::new(&file)).expect("decodes");
    let (fr, _) = decoded.stereo.as_chunks::<2>();
    let mono: Vec<f32> = fr.iter().map(|[l, r]| 0.5 * (l + r)).collect();
    let spec = analyse(&mono, decoded.sample_rate);
    let grid = find_grid(&mono, &spec).expect("grid");
    let steps = to * 16;
    let ev = step_evidence(&spec, &grid, steps);
    let heard = hear_drums(&spec, &grid, steps);
    let mut p = [0.0f32; 16];
    let mut air = [0.0f32; 16];
    let mut hats = [0usize; 16];
    let bars = (to - from) as f32;
    for s in from * 16..to * 16 {
        let (x, probs) = &ev[s as usize];
        let at = (s % 16) as usize;
        p[at] += probs[3] / bars;
        air[at] += (x[21] + x[24]) / bars;
    }
    for h in heard
        .iter()
        .filter(|h| h.drum == Drum::Hat && h.step >= from * 16 && h.step < to * 16)
    {
        hats[(h.step % 16) as usize] += 1;
    }
    println!("pos  P(hat)  air-jump  heard/{bars}");
    for at in 0..16 {
        println!("{at:>3}  {:>6.2}  {:>8.2}  {:>5}", p[at], air[at], hats[at]);
    }
    for drum in [Drum::Kick, Drum::Snare, Drum::Ghost] {
        let mut at = [0usize; 16];
        for h in heard
            .iter()
            .filter(|h| h.drum == drum && h.step >= from * 16 && h.step < to * 16)
        {
            at[(h.step % 16) as usize] += 1;
        }
        println!("{drum:?}: {at:?}");
    }
    // A few bars as heard: H for a hat.
    for bar in from..(from + 8).min(to) {
        let line: String = (0..16)
            .map(|i| {
                let s = bar * 16 + i;
                if heard.iter().any(|h| h.step == s && h.drum == Drum::Hat) {
                    'H'
                } else {
                    '.'
                }
            })
            .collect();
        println!("bar {bar:>3} {line}");
    }
}
