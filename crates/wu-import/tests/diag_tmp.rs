//! TEMPORARY: prints a song's bass line as played and as heard, bar by bar.
use wu_audio::render_offline;
use wu_content::songs::BUILTIN;
use wu_import::bass::hear_bass;
use wu_import::find_grid;
use wu_import::hits::{Drum, hear_drums};
use wu_import::spectrum::analyse;
use wu_instruments::Pad;
use wu_time::{TICKS_PER_STEP, Tick};

const SR: u32 = 48_000;

fn name(key: Option<u8>) -> String {
    const N: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    key.map_or("  . ".to_owned(), |k| {
        format!("{:>3}{}", N[usize::from(k % 12)], i32::from(k / 12) - 1)
    })
}

#[test]
fn diag() {
    let id = std::env::var("DIAG_SONG").unwrap_or_default();
    let Some(builtin) = BUILTIN.iter().find(|b| b.id == id) else {
        return;
    };
    let song = builtin.load().expect("compiles");
    let program = song.whole_program(SR, &song.tempo, 0);
    let frames = program.tempo.frame_at(song.length + Tick::from_bars(1), SR) as usize;
    let render = render_offline(program, frames, 512);
    let (fr, _) = render.audio.as_chunks::<2>();
    let mono: Vec<f32> = fr.iter().map(|[l, r]| 0.5 * (l + r)).collect();
    let spec = analyse(&mono, SR);
    let grid = find_grid(&mono, &spec).expect("grid");
    let steps = song.length.0 / TICKS_PER_STEP;
    let kicks: Vec<i64> = hear_drums(&spec, &grid, steps)
        .iter()
        .filter(|h| h.drum == Drum::Kick)
        .map(|h| h.step)
        .collect();
    let heard = hear_bass(&mono, SR, &grid, steps, &kicks);
    let keyed = |notes: &[wu_audio::Note]| {
        let mut keys = vec![None; steps as usize];
        let mut starts = vec![false; steps as usize];
        for n in notes {
            let (start, length) = (n.tick.0 / TICKS_PER_STEP, n.length.0 / TICKS_PER_STEP);
            if start < steps {
                starts[start as usize] = true;
            }
            for s in start..(start + length).min(steps) {
                keys[s as usize] = Some(n.key);
            }
        }
        (keys, starts)
    };
    let (truth, tstarts) = keyed(&song.bass);
    let (found, fstarts) = keyed(&heard);
    for bar in 0..steps / 16 {
        let range = (bar * 16) as usize..(bar * 16 + 16) as usize;
        if truth[range.clone()].iter().all(Option::is_none) && found[range.clone()].iter().all(Option::is_none) {
            continue;
        }
        let wrong = range.clone().filter(|&s| truth[s] != found[s]).count();
        let t: String = range
            .clone()
            .map(|s| format!("{}{}", if tstarts[s] { '>' } else { ' ' }, name(truth[s])))
            .collect();
        let f: String = range
            .clone()
            .map(|s| format!("{}{}", if fstarts[s] { '>' } else { ' ' }, name(found[s])))
            .collect();
        eprintln!("bar {:>3} wrong {:>2}\n  played {t}\n  heard  {f}", bar + 1, wrong);
    }
    let k: Vec<i64> = kicks.iter().copied().filter(|&s| s < 16 * 40).collect();
    eprintln!("kicks heard (first 40 bars): {k:?}");
}

#[test]
fn diag_feel() {
    if std::env::var("DIAG_FEEL").is_err() {
        return;
    }
    for swing in [0.0, 0.2] {
        let mut song = BUILTIN[0].load().expect("compiles");
        song.tracks.retain(|(_, t, _)| !t.instrument.starts_with("break/"));
        for hit in &mut song.drums {
            let step = (hit.tick.0 as f64 / TICKS_PER_STEP as f64).round();
            let late = if step as i64 % 2 == 1 { swing } else { 0.0 };
            hit.tick = Tick(((step + late) * TICKS_PER_STEP as f64).round() as i64);
        }
        let program = song.whole_program(SR, &song.tempo, 0);
        let frames = program.tempo.frame_at(song.length + Tick::from_bars(1), SR) as usize;
        let render = render_offline(program, frames, 512);
        let (fr, _) = render.audio.as_chunks::<2>();
        let mono: Vec<f32> = fr.iter().map(|[l, r]| 0.5 * (l + r)).collect();
        let spec = analyse(&mono, SR);
        let grid = find_grid(&mono, &spec).expect("grid");
        let steps = song.length.0 / TICKS_PER_STEP;
        let hits = hear_drums(&spec, &grid, steps);
        eprintln!("swing {swing}: grid first bar {:.2} ms", grid.first_bar_s * 1000.0);
        for (name, drums) in [
            ("kick", vec![Drum::Kick]),
            ("snare", vec![Drum::Snare]),
            ("ghost", vec![Drum::Ghost]),
            ("hat", vec![Drum::Hat]),
        ] {
            let mut line = format!("  {name:6}");
            for pos in 0..16 {
                let n = hits
                    .iter()
                    .filter(|h| drums.contains(&h.drum) && h.step.rem_euclid(16) == pos)
                    .count();
                line += &format!(" {pos}:{n}");
            }
            eprintln!("{line}");
        }
        // The true kit hits per pad per position.
        for pad in [Pad::P1, Pad::P2, Pad::P3, Pad::P5, Pad::P7] {
            let mut line = format!("  true {pad:?}");
            for pos in 0..16i64 {
                let n = song
                    .drums
                    .iter()
                    .filter(|h| {
                        h.pad == pad && ((h.tick.0 as f64 / TICKS_PER_STEP as f64).round() as i64).rem_euclid(16) == pos
                    })
                    .count();
                line += &format!(" {pos}:{n}");
            }
            eprintln!("{line}");
        }
        let feel = wu_import::feel::hear_feel(&mono, SR, &grid, &hits);
        eprintln!("  feel {feel:?}");
    }
}

#[test]
fn diag_offsets() {
    if std::env::var("DIAG_OFFSETS").is_err() {
        return;
    }
    let mut song = BUILTIN[0].load().expect("compiles");
    song.tracks.retain(|(_, t, _)| !t.instrument.starts_with("break/"));
    for hit in &mut song.drums {
        let step = (hit.tick.0 as f64 / TICKS_PER_STEP as f64).round();
        hit.tick = Tick((step * TICKS_PER_STEP as f64).round() as i64);
    }
    let program = song.whole_program(SR, &song.tempo, 0);
    let frames = program.tempo.frame_at(song.length + Tick::from_bars(1), SR) as usize;
    let render = render_offline(program, frames, 512);
    let (fr, _) = render.audio.as_chunks::<2>();
    let mono: Vec<f32> = fr.iter().map(|[l, r]| 0.5 * (l + r)).collect();
    let spec = analyse(&mono, SR);
    let grid = find_grid(&mono, &spec).expect("grid");
    let hits = hear_drums(&spec, &grid, song.length.0 / TICKS_PER_STEP);
    let gaps = wu_import::feel::attack_offsets(&mono, SR, &grid, &hits);
    for (f, name) in ["kick", "snare", "hat"].iter().enumerate() {
        for pos in [0usize, 2, 4, 7, 8, 10, 11, 12, 15] {
            let mut g = gaps[f][pos].clone();
            g.sort_by(f64::total_cmp);
            if g.is_empty() {
                continue;
            }
            let shown: Vec<String> = g.iter().map(|x| format!("{x:.0}")).collect();
            eprintln!("{name} {pos}: n={} [{}]", g.len(), shown.join(" "));
        }
    }
}

#[test]
fn diag_pad_onsets() {
    if std::env::var("DIAG_PADS").is_err() {
        return;
    }
    for id in [
        "ragga-93",
        "darkside-92",
        "neuro-lab",
        "jump-up-tin",
        "minimal-roller",
        "liquid-velvet",
        "atmos-95",
        "halftime-heavy",
    ] {
        let kit = wu_content::kits::kit(id, SR).expect("kit");
        let mut line = format!("{id:15}");
        for pad in &kit.pads {
            let data = pad.sample.data();
            let (frames, _) = data.as_chunks::<2>();
            let mono: Vec<f32> = frames.iter().map(|[l, r]| 0.5 * (l.abs() + r.abs())).collect();
            let peak = mono.iter().copied().fold(0.0f32, f32::max);
            let first = mono.iter().position(|&x| x > 0.1 * peak).unwrap_or(0);
            let peak_at = mono.iter().position(|&x| x >= peak).unwrap_or(0);
            line += &format!(
                " {}:{:.1}/{:.1}",
                pad.name.split_whitespace().last().unwrap_or(""),
                first as f64 / 48.0,
                peak_at as f64 / 48.0
            );
        }
        eprintln!("{line}");
    }
}

#[test]
fn diag_feel_all() {
    if std::env::var("DIAG_ALL").is_err() {
        return;
    }
    for builtin in BUILTIN.iter() {
        let song = builtin.load().expect("compiles");
        let program = song.whole_program(SR, &song.tempo, 0);
        let frames = program.tempo.frame_at(song.length + Tick::from_bars(1), SR) as usize;
        let render = render_offline(program, frames, 512);
        let (fr, _) = render.audio.as_chunks::<2>();
        let mono: Vec<f32> = fr.iter().map(|[l, r]| 0.5 * (l + r)).collect();
        let spec = analyse(&mono, SR);
        let grid = find_grid(&mono, &spec).expect("grid");
        let steps = song.length.0 / TICKS_PER_STEP;
        let hits = hear_drums(&spec, &grid, steps);
        let feel = wu_import::feel::hear_feel(&mono, SR, &grid, &hits);
        let fam = |pad: Pad| match pad {
            Pad::P1 => Some(0),
            Pad::P2 | Pad::P3 | Pad::P4 | Pad::P5 => Some(1),
            Pad::P7 | Pad::P8 => Some(2),
            Pad::P6 => None,
        };
        let mut with = Vec::new();
        let mut without = Vec::new();
        for h in &hits {
            let hf = match h.drum {
                Drum::Kick => 0,
                Drum::Snare | Drum::Ghost => 1,
                Drum::Hat => 2,
            };
            let Some(t) = song
                .drums
                .iter()
                .find(|d| (d.tick.0 as f64 / TICKS_PER_STEP as f64).round() as i64 == h.step && fam(d.pad) == Some(hf))
            else {
                continue;
            };
            let truth = song.tempo.seconds_at(t.tick.0 as f64);
            let line = grid.time_of_step(h.step as f64);
            with.push(((line + f64::from(feel.offset_ms(h.drum, h.step)) / 1000.0) - truth).abs() * 1000.0);
            without.push((line - truth).abs() * 1000.0);
        }
        let stat = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            (v[v.len() / 2], v[v.len() * 9 / 10], v.len())
        };
        let (m1, p1, n) = stat(with);
        let (m0, p0, _) = stat(without);
        let moved: Vec<String> = [("k", feel.kick), ("s", feel.snare), ("h", feel.hat)]
            .iter()
            .flat_map(|(name, row)| {
                row.iter()
                    .enumerate()
                    .filter(|(_, v)| **v != 0.0)
                    .map(move |(i, v)| format!("{name}{i}:{v:+.0}"))
            })
            .collect();
        eprintln!(
            "{:22} n={n:4} grid median {m0:4.1} p90 {p0:4.1} | feel median {m1:4.1} p90 {p1:4.1} | {}",
            builtin.id,
            moved.join(" ")
        );
    }
}

#[test]
fn diag_backbeat() {
    let Ok(file) = std::env::var("DIAG_MP3") else { return };
    let tune = wu_import::decode(std::path::Path::new(&file)).expect("decodes");
    let mono = tune.mono();
    let spec = analyse(&mono, tune.sample_rate);
    let grid = find_grid(&mono, &spec).expect("grid");
    let bars = ((tune.seconds() - grid.first_bar_s) / grid.bar_s()).floor() as i64;
    let ev = wu_import::hits::step_evidence(&spec, &grid, bars * 16);
    let from: i64 = std::env::var("FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let to: i64 = std::env::var("TO").ok().and_then(|v| v.parse().ok()).unwrap_or(bars);
    let positions: Vec<i64> = std::env::var("POS")
        .ok()
        .map(|v| v.split(",").map(|p| p.parse().unwrap()).collect())
        .unwrap_or(vec![4, 12]);
    for pos in positions {
        let rows: Vec<_> = ev
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i as i64) % 16 == pos && (*i as i64) / 16 >= from && (*i as i64) / 16 < to)
            .map(|(_, r)| r)
            .collect();
        let n = rows.len() as f32;
        let mut x = [0.0f32; wu_import::hits::FEATURES];
        let mut p = [0.0f32; 4];
        for (f, q) in &rows {
            for i in 0..x.len() {
                x[i] += f[i] / n;
            }
            for i in 0..4 {
                p[i] += q[i] / n;
            }
        }
        let xs: Vec<String> = x.iter().map(|v| format!("{v:.2}")).collect();
        eprintln!(
            "pos {pos}: p(kick,snare,ghost,hat) {:.2} {:.2} {:.2} {:.2}\n  x {}",
            p[0],
            p[1],
            p[2],
            p[3],
            xs.join(" ")
        );
    }
}
