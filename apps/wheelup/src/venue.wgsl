// The venue behind the play, printed like a 90s rave flyer: the stop of the
// Pirate Radio Tour the tune belongs to. A city at night seen from a pirate
// station's rooftop (the default), a bedroom studio, a warehouse rave, a sound
// system clash.
// Everything is computed here, from a few numbers the game moves with the music
// (see stage.rs): the kick's pulse, how much is going on, the lasers of a drop,
// a WHEEL UP!'s flash, and which venue.
//
// Photosensitivity: nothing here flashes on the beat. The kick swells the haze
// and the city's lights by a few per cent; the lasers sweep slowly; the only
// flash is a WHEEL UP!'s, once.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct Mood {
    // Window size in pixels, seconds, the kick's pulse (0–1).
    a: vec4<f32>,
    // Intensity (0–1), lasers (0–1), flash (0–1), hype (0–1).
    b: vec4<f32>,
    // Motion (1 normal, 0 reduced), the venue (0 rooftop, 1 bedroom, 2
    // warehouse, 3 sound system clash), unused.
    c: vec4<f32>,
}

// Where a pixel is and how the night is going, for each venue's scene.
struct Scene {
    // Screen heights from the bottom centre, and pixels from the bottom left.
    p: vec2<f32>,
    pixel: vec2<f32>,
    aspect: f32,
    time: f32,
    pulse: f32,
    intensity: f32,
    lasers: f32,
    motion: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> mood: Mood;

const TAU: f32 = 6.28318530718;

fn hash11(n: f32) -> f32 {
    return fract(sin(n * 127.1) * 43758.5453);
}

fn hash21(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

// A skyline: blocks `width` wide (in screen heights) standing up to `top`,
// each its own height; returns how far under its roof a point is (> 0 inside),
// and the block's index.
fn skyline(x: f32, y: f32, width: f32, low: f32, high: f32, seed: f32) -> vec2<f32> {
    let index = floor(x / width);
    let h = low + (high - low) * pow(hash11(index * 1.37 + seed), 1.6);
    // A gap between some blocks.
    let inside = fract(x / width);
    let gap = select(0.0, 1.0, hash11(index * 7.1 + seed) > 0.8 && (inside < 0.12 || inside > 0.88));
    return vec2<f32>(select(h - y, -1.0, gap > 0.5), index);
}

// Whether a point is inside the box centred on `centre`, `half` its half-size.
fn in_box(p: vec2<f32>, centre: vec2<f32>, half: vec2<f32>) -> bool {
    let d = abs(p - centre);
    return d.x < half.x && d.y < half.y;
}

// Distance from a point to a ray (origin, unit direction), forward only.
fn ray_distance(p: vec2<f32>, origin: vec2<f32>, direction: vec2<f32>) -> f32 {
    let along = max(dot(p - origin, direction), 0.0);
    return length(p - origin - direction * along);
}

// The pirate station's rooftop: the city at night, a haze over it, a mast on a
// roof sending its signal out with the kick, lasers behind the city in a drop.
fn rooftop(s: Scene) -> vec3<f32> {
    let p = s.p;
    let pixel = s.pixel;
    let aspect = s.aspect;
    let time = s.time;
    let pulse = s.pulse;
    let intensity = s.intensity;
    let lasers = s.lasers;
    let motion = s.motion;

    // The sky: violet-black overhead, a haze glowing over the city.
    let horizon = 0.34;
    var colour = mix(vec3<f32>(0.03, 0.012, 0.06), vec3<f32>(0.004, 0.002, 0.01), smoothstep(0.1, 0.9, p.y));
    let haze_hue = mix(vec3<f32>(0.30, 0.04, 0.22), vec3<f32>(0.04, 0.14, 0.26), 0.5 + 0.5 * sin(time * 0.07));
    let haze = exp(-max(p.y - horizon + 0.12, 0.0) * 7.0);
    colour += haze * haze_hue * (0.22 + 0.3 * intensity + 0.05 * pulse);

    // Stars, faint, twinkling slowly.
    let star_cell = floor(pixel / 3.0);
    let star = step(0.9975, hash21(star_cell)) * (0.5 + 0.5 * sin(time * 1.3 + hash21(star_cell + 7.0) * TAU));
    colour += vec3<f32>(0.5, 0.5, 0.7) * star * smoothstep(horizon, 1.0, p.y);

    // Lasers in a drop, from behind the city, sweeping slowly.
    if lasers > 0.001 {
        for (var i = 0; i < 4; i++) {
            let fi = f32(i);
            let side = select(-1.0, 1.0, i % 2 == 0);
            let origin = vec2<f32>(side * (0.25 + 0.2 * fi) * aspect * 0.5, 0.18);
            let angle = 1.5708 - side * (0.35 + 0.3 * sin(time * (0.21 + 0.07 * fi) + fi * 1.7) * motion);
            let direction = vec2<f32>(cos(angle), sin(angle));
            let d = ray_distance(p, origin, direction);
            let beam = exp(-pow(d / 0.0025, 2.0)) + 0.25 * exp(-pow(d / 0.012, 2.0));
            let hue = select(vec3<f32>(0.2, 1.0, 0.45), vec3<f32>(1.0, 0.2, 0.75), i % 2 == 1);
            colour += hue * beam * lasers * 1.5;
        }
    }

    // The far city: low, blue-grey, a few lit windows.
    let far = skyline(p.x + 3.0, p.y, 0.045, 0.16, 0.30, 11.0);
    if far.x > 0.0 {
        colour = mix(colour, vec3<f32>(0.016, 0.014, 0.035) + haze_hue * 0.05, 0.94);
        let window = floor(vec2<f32>(p.x / 0.007, p.y / 0.011));
        let lit = step(0.93, hash21(window + far.y * 3.1));
        colour += vec3<f32>(0.35, 0.3, 0.55) * lit * 0.22 * (1.0 + 0.06 * pulse);
    }

    // The near blocks: tower blocks, black against the haze, windows burning.
    let near = skyline(p.x, p.y, 0.09, 0.08, 0.27, 3.0);
    if near.x > 0.0 {
        colour = vec3<f32>(0.012, 0.008, 0.02);
        let window = floor(vec2<f32>(p.x / 0.011, p.y / 0.016));
        let local = fract(vec2<f32>(p.x / 0.011, p.y / 0.016));
        let pane = step(0.25, local.x) * step(local.x, 0.75) * step(0.3, local.y) * step(local.y, 0.8);
        let on = step(0.72, hash21(window + near.y * 17.0));
        // A few windows change, slowly, as people come and go.
        let change = step(0.5, hash11(window.x * 3.0 + window.y * 13.0 + floor(time * 0.2 + hash21(window) * 5.0)));
        let warm = mix(vec3<f32>(1.0, 0.7, 0.3), vec3<f32>(0.3, 0.8, 1.0), step(0.7, hash21(window + 2.0)));
        // Dimmer in the menus, so the text over them reads; all lit in a drop.
        colour += warm * pane * on * change * (0.18 + 0.17 * intensity + 0.05 * pulse);
    }

    // The pirate station: a mast on a roof right of centre, its light blinking
    // slowly, its signal going out in rings with the kick.
    let mast_x = 0.32 * aspect;
    let roof = 0.08 + 0.19 * pow(hash11(floor(mast_x / 0.09) * 1.37 + 3.0), 1.6);
    let tip = vec2<f32>(mast_x, roof + 0.16);
    let on_mast = abs(p.x - mast_x) < 0.0016 && p.y > roof && p.y < tip.y;
    let rung = fract((p.y - roof) / 0.02) < 0.12 && abs(p.x - mast_x) < 0.006 * (1.0 - (p.y - roof) / 0.16) + 0.0015 && p.y > roof && p.y < tip.y;
    if on_mast || rung {
        colour = vec3<f32>(0.08, 0.07, 0.1);
    }
    let blink = 0.5 + 0.5 * sin(time * 2.4);
    let light = exp(-pow(length(p - tip) / 0.004, 2.0));
    colour += vec3<f32>(3.0, 0.2, 0.15) * light * (0.35 + 0.65 * blink);
    let from_tip = length(p - tip);
    let rings = pow(0.5 + 0.5 * cos((from_tip - time * 0.09 * motion) * TAU * 12.0), 30.0);
    colour += vec3<f32>(0.3, 0.9, 1.0) * rings * exp(-from_tip * 11.0) * (0.06 + 0.4 * pulse) * step(0.004, from_tip);

    return colour;
}

// The bedroom studio: a wall in the dark, warm by a lamp, a window onto the
// city, an LED strip along the ceiling, a poster, and the monitors on the desk
// breathing with the kick. In a drop, a cheap laser throws its dots on the wall.
fn bedroom(s: Scene) -> vec3<f32> {
    let p = s.p;
    let half_w = 0.5 * s.aspect;
    var colour = vec3<f32>(0.018, 0.009, 0.03);
    let lamp = vec2<f32>(-half_w * 0.95, 0.92);
    colour += vec3<f32>(0.35, 0.16, 0.06) * exp(-length(p - lamp) * 3.5) * (0.2 + 0.25 * s.intensity);

    // The laser's dots, drifting over the wall.
    if s.lasers > 0.001 {
        let g = (p + vec2<f32>(s.time * 0.025, 0.04 * sin(s.time * 0.2)) * s.motion) * 16.0;
        let id = floor(g);
        let cell = fract(g) - 0.5;
        let shown = step(0.78, hash21(id));
        let spot = exp(-dot(cell, cell) / 0.003);
        let hue = select(vec3<f32>(0.2, 1.0, 0.4), vec3<f32>(1.0, 0.15, 0.35), hash21(id + 3.0) > 0.5);
        colour += hue * spot * shown * s.lasers * 1.3;
    }

    // The LED strip along the ceiling: a slow rainbow, brighter in a drop, its
    // light spilling down the wall.
    let strip_y = 0.965;
    let led = 0.5 + 0.5 * cos(TAU * (vec3<f32>(0.0, 0.33, 0.67) + p.x * 1.2 + s.time * 0.08 * s.motion));
    let lit = 0.5 + 1.1 * s.intensity + 0.15 * s.pulse;
    colour += led * exp(-pow((p.y - strip_y) / 0.004, 2.0)) * lit;
    colour += led * exp(-max(strip_y - p.y, 0.0) * 10.0) * 0.025 * lit;

    // A poster on the left: a dubplate, its print faded.
    let poster = vec2<f32>(-0.56 * half_w, 0.6);
    if in_box(p, poster, vec2<f32>(0.12, 0.16)) {
        colour = vec3<f32>(0.04, 0.018, 0.045);
        let r = length(p - poster - vec2<f32>(0.0, 0.03));
        colour += vec3<f32>(0.9, 0.2, 0.6) * exp(-pow((r - 0.07) / 0.004, 2.0)) * 0.2;
        colour += vec3<f32>(0.9, 0.8, 0.2) * step(r, 0.022) * 0.12;
        let line = fract((p.y - poster.y + 0.16) / 0.022);
        let text = step(0.6, line) * step(p.y, poster.y - 0.07) * step(0.35, hash11(floor((p.y - poster.y) / 0.022) + 4.0));
        colour += vec3<f32>(0.4, 0.35, 0.5) * text * 0.15;
    }

    // The window, right of centre: the night through it, sky and city.
    let window = vec2<f32>(0.4 * half_w, 0.6);
    let window_half = vec2<f32>(0.27, 0.19);
    if in_box(p, window, window_half) {
        let local = (p - window + window_half) / (2.0 * window_half.y);
        colour = mix(vec3<f32>(0.04, 0.015, 0.07), vec3<f32>(0.008, 0.004, 0.016), local.y);
        colour += vec3<f32>(0.25, 0.05, 0.2) * exp(-local.y * 4.0) * (0.15 + 0.3 * s.intensity);
        let star = step(0.996, hash21(floor(s.pixel / 3.0)));
        colour += vec3<f32>(0.5, 0.5, 0.7) * star * 0.6;
        let city = skyline(local.x + 2.0, local.y, 0.11, 0.15, 0.6, 9.0);
        if city.x > 0.0 {
            colour = vec3<f32>(0.012, 0.008, 0.02);
            let pane = floor(vec2<f32>(local.x / 0.018, local.y / 0.026));
            let on = step(0.7, hash21(pane + city.y * 5.0));
            let warm = mix(vec3<f32>(1.0, 0.7, 0.3), vec3<f32>(0.3, 0.8, 1.0), step(0.7, hash21(pane + 2.0)));
            colour += warm * on * (0.08 + 0.14 * s.intensity + 0.04 * s.pulse);
        }
        // The frame and its cross.
        let edge = window_half - abs(p - window);
        let bar = min(abs(p.x - window.x), abs(p.y - window.y));
        if min(edge.x, edge.y) < 0.012 || bar < 0.006 {
            colour = vec3<f32>(0.02, 0.015, 0.03);
        }
    }

    // The desk along the bottom, its edge catching the strip's light.
    if p.y < 0.17 {
        colour = vec3<f32>(0.016, 0.011, 0.022) + led * exp(-(0.17 - p.y) / 0.004) * 0.08 * lit;
    }

    // The monitors, a cone and a tweeter each, the cones pushing out on the kick.
    for (var i = 0; i < 2; i++) {
        let side = select(-1.0, 1.0, i == 1);
        let cabinet = vec2<f32>(side * 0.72 * half_w, 0.3);
        if in_box(p, cabinet, vec2<f32>(0.085, 0.13)) {
            colour = vec3<f32>(0.022, 0.02, 0.028);
            let cone = cabinet - vec2<f32>(0.0, 0.04);
            let r = length(p - cone) / (1.0 + 0.05 * s.pulse * s.motion);
            colour += vec3<f32>(0.18, 0.16, 0.22) * exp(-pow((r - 0.055) / 0.004, 2.0));
            colour += vec3<f32>(0.04, 0.035, 0.05) * step(r, 0.05) * (1.0 - r / 0.05);
            colour += vec3<f32>(0.3, 0.9, 1.0) * exp(-pow((r - 0.012) / 0.003, 2.0)) * (0.1 + 0.4 * s.pulse);
            let tweeter = length(p - cabinet - vec2<f32>(0.0, 0.085));
            colour += vec3<f32>(0.16, 0.15, 0.2) * exp(-pow((tweeter - 0.016) / 0.003, 2.0));
        }
    }
    return colour;
}

// The warehouse rave: steel trusses under the roof, pillars, the booth's glow at
// the back with lasers fanning out of it in a drop, searchlights sweeping, and
// a crowd along the bottom moving with the kick.
fn warehouse(s: Scene) -> vec3<f32> {
    let p = s.p;
    let half_w = 0.5 * s.aspect;
    let booth = vec2<f32>(0.0, 0.33);
    let glow_hue = mix(vec3<f32>(0.9, 0.12, 0.6), vec3<f32>(0.1, 0.5, 1.0), 0.5 + 0.5 * sin(s.time * 0.11));
    var colour = vec3<f32>(0.012, 0.012, 0.02);
    colour += glow_hue * exp(-length((p - booth) * vec2<f32>(0.8, 1.6)) * 3.0) * (0.12 + 0.35 * s.intensity + 0.06 * s.pulse);
    colour += vec3<f32>(0.05, 0.04, 0.07) * exp(-abs(p.y - 0.3) * 6.0) * (0.3 + 0.4 * s.intensity);

    // Searchlights from the roof, two soft beams crossing slowly.
    for (var i = 0; i < 2; i++) {
        let side = select(-1.0, 1.0, i == 1);
        let origin = vec2<f32>(side * 0.6 * half_w, 1.02);
        let angle = -1.5708 - side * (0.35 + 0.25 * sin(s.time * (0.13 + 0.05 * f32(i)) + f32(i) * 2.0) * s.motion);
        let d = ray_distance(p, origin, vec2<f32>(cos(angle), sin(angle)));
        colour += vec3<f32>(0.6, 0.6, 0.8) * exp(-pow(d / 0.05, 2.0)) * 0.07 * (0.4 + s.intensity);
    }

    // Lasers fanning from the booth in a drop.
    if s.lasers > 0.001 {
        for (var i = 0; i < 6; i++) {
            let fi = f32(i);
            let angle = 1.5708 + (fi - 2.5) * 0.3 + 0.22 * sin(s.time * (0.23 + 0.04 * fi) + fi * 1.3) * s.motion;
            let d = ray_distance(p, booth, vec2<f32>(cos(angle), sin(angle)));
            let beam = exp(-pow(d / 0.0022, 2.0)) + 0.2 * exp(-pow(d / 0.01, 2.0));
            let hue = select(vec3<f32>(0.2, 1.0, 0.45), vec3<f32>(1.0, 0.15, 0.3), i % 2 == 1);
            colour += hue * beam * s.lasers * 1.6;
        }
    }

    // The roof: two steel chords and the zigzag between them, black on the haze.
    let chord_top = 0.88;
    let chord_low = 0.78;
    if p.y > chord_top {
        colour = mix(colour, vec3<f32>(0.006, 0.006, 0.01), 0.85);
    }
    let along = fract(p.x / 0.13);
    let zigzag = chord_low + (chord_top - chord_low) * (1.0 - abs(2.0 * along - 1.0));
    let steel = abs(p.y - chord_top) < 0.006 || abs(p.y - chord_low) < 0.005
        || (p.y > chord_low && p.y < chord_top && abs(p.y - zigzag) < 0.0045);
    if steel {
        colour = vec3<f32>(0.01, 0.01, 0.014);
    }

    // Pillars down to the floor.
    for (var i = 0; i < 4; i++) {
        let x = (f32(i) - 1.5) / 1.5 * 0.9 * half_w;
        if abs(p.x - x) < 0.012 && p.y < chord_low {
            colour = vec3<f32>(0.01, 0.01, 0.015);
        }
    }

    // The booth: a desk at the back, its front a strip of light.
    if in_box(p, booth - vec2<f32>(0.0, 0.07), vec2<f32>(0.15, 0.035)) {
        colour = vec3<f32>(0.008, 0.008, 0.012);
        colour += glow_hue * exp(-pow((p.y - booth.y + 0.085) / 0.004, 2.0)) * (0.8 + 0.8 * s.intensity);
    }

    // The crowd: two rows of heads and shoulders, black against the light,
    // bobbing on the kick (some a little late); in a drop, hands go up.
    for (var row = 0; row < 2; row++) {
        let fr = f32(row);
        let width = 0.055 + 0.03 * fr;
        let base = 0.2 - 0.12 * fr;
        let radius = 0.018 + 0.01 * fr;
        let id = floor(p.x / width + fr * 0.5);
        let local = (fract(p.x / width + fr * 0.5) - 0.5) * width;
        let bob = s.pulse * (0.006 + 0.01 * hash11(id + fr * 9.0)) * s.motion;
        let head_y = base + 0.02 * hash11(id * 3.1 + fr) + bob;
        let head = length(vec2<f32>(local, p.y - head_y)) < radius;
        let body = p.y < head_y - radius * 0.7 && abs(local) < radius * 1.7;
        let hand_up = s.intensity > 0.6 && hash11(id * 5.3 + fr) > 0.8;
        let arm = hand_up && abs(local - radius * 1.2) < 0.004 && p.y > head_y && p.y < head_y + radius * 3.2;
        if head || body || arm {
            colour = vec3<f32>(0.004, 0.004, 0.006);
        }
    }
    return colour;
}

// The sound system clash: a yard at night, two stacks of speakers face to face
// at its sides, their bass bins breathing with the kick, string lights hung
// between them, and the crowd in the middle; in a drop, hands and lighters go up.
fn clash(s: Scene) -> vec3<f32> {
    let p = s.p;
    let half_w = 0.5 * s.aspect;
    // The stacks stand outside this, the yard between them.
    let span = 0.62 * half_w;

    // The sky: blue-black, warmed low by the yard's lights and the smoke.
    var colour = mix(vec3<f32>(0.022, 0.016, 0.04), vec3<f32>(0.004, 0.004, 0.012), smoothstep(0.1, 0.95, p.y));
    let warm = vec3<f32>(0.35, 0.12, 0.04);
    colour += warm * exp(-p.y * 3.2) * (0.16 + 0.3 * s.intensity + 0.04 * s.pulse);
    let star_cell = floor(s.pixel / 3.0);
    let star = step(0.997, hash21(star_cell)) * (0.5 + 0.5 * sin(s.time * 1.1 + hash21(star_cell + 5.0) * TAU));
    colour += vec3<f32>(0.45, 0.45, 0.6) * star * smoothstep(0.45, 1.0, p.y);
    // Smoke drifting through the light.
    let drift = s.time * s.motion;
    let smoke = 0.5 + 0.5 * sin(p.x * 7.0 + drift * 0.15 + 1.5 * sin(p.y * 9.0 - drift * 0.1));
    colour += vec3<f32>(0.05, 0.03, 0.04) * smoke * exp(-abs(p.y - 0.35) * 4.0) * (0.3 + 0.5 * s.intensity);

    // String lights: three wires of bulbs from stack to stack, red, gold and
    // green in turn, swaying a little, brighter in a drop.
    let glow = 0.45 + 1.1 * s.intensity + 0.5 * s.lasers + 0.08 * s.pulse;
    let spacing = 0.055;
    for (var i = 0; i < 3; i++) {
        let fi = f32(i);
        let top = 0.88 - fi * 0.075;
        let sag = 0.1 + 0.035 * fi - 0.006 * sin(s.time * 0.6 + fi * 1.7) * s.motion;
        if abs(p.x) < span {
            let u = p.x / span;
            let wire_y = top - sag * (1.0 - u * u);
            colour = mix(colour, vec3<f32>(0.01, 0.008, 0.012), exp(-pow((p.y - wire_y) / 0.0018, 2.0)) * 0.9);
        }
        // The nearest bulb, if the wire reaches it: its halo fades out on its own.
        let k = round(p.x / spacing);
        if abs(k * spacing) < span - 0.01 {
            let bu = k * spacing / span;
            let bulb = vec2<f32>(k * spacing, top - sag * (1.0 - bu * bu) - 0.009);
            let d = length(p - bulb);
            let turn = i32(abs(k) + fi) % 3;
            var hue = vec3<f32>(1.0, 0.18, 0.1);
            if turn == 1 {
                hue = vec3<f32>(1.0, 0.72, 0.15);
            } else if turn == 2 {
                hue = vec3<f32>(0.25, 1.0, 0.3);
            }
            colour += hue * (1.4 * exp(-pow(d / 0.0045, 2.0)) + 0.06 * exp(-d / 0.025)) * glow;
        }
    }

    // The stacks: two columns of boxes each side, two rows of bass bins at the
    // bottom, mid boxes above with two cones each, horns on top; the bins push
    // out on the kick.
    let stack_top = 0.74;
    if abs(p.x) >= span && p.y < stack_top {
        let side = sign(p.x);
        let local_x = (abs(p.x) - span) / 0.17;
        let column = floor(local_x);
        let fx = (fract(local_x) - 0.5) * 0.17;
        colour = vec3<f32>(0.02, 0.017, 0.02);
        // The light from the yard catching the stack's inner edge.
        colour += warm * exp(-(abs(p.x) - span) / 0.02) * 0.2 * glow;
        var row_base = 0.0;
        var row_height = 0.17;
        var kind = 0;
        if p.y >= 0.34 && p.y < 0.54 {
            row_base = 0.34;
            row_height = 0.2;
            kind = 1;
        } else if p.y >= 0.54 {
            row_base = 0.54;
            row_height = 0.2;
            kind = 2;
        } else if p.y >= 0.17 {
            row_base = 0.17;
        }
        let fy = p.y - row_base - 0.5 * row_height;
        // The boxes' edges.
        let edge = min(0.085 - abs(fx), 0.5 * row_height - abs(fy));
        if edge < 0.004 {
            colour = vec3<f32>(0.055, 0.04, 0.032) + warm * 0.05 * glow;
        } else if kind == 0 {
            // A bin: one big cone, its surround catching the light.
            let r = length(vec2<f32>(fx, fy)) / (1.0 + 0.06 * s.pulse * s.motion);
            colour += vec3<f32>(0.16, 0.13, 0.15) * exp(-pow((r - 0.066) / 0.004, 2.0));
            colour += vec3<f32>(0.035, 0.03, 0.04) * step(r, 0.062) * (1.0 - r / 0.062);
            colour += vec3<f32>(1.0, 0.65, 0.15) * exp(-pow(r / 0.012, 2.0)) * (0.06 + 0.25 * s.pulse);
        } else if kind == 1 {
            // A mid box: two cones, one over the other.
            let cy = select(fy + 0.048, fy - 0.048, fy > 0.0);
            let r = length(vec2<f32>(fx, cy)) / (1.0 + 0.03 * s.pulse * s.motion);
            colour += vec3<f32>(0.14, 0.12, 0.14) * exp(-pow((r - 0.038) / 0.0035, 2.0));
            colour += vec3<f32>(0.03, 0.026, 0.035) * step(r, 0.035) * (1.0 - r / 0.035);
        } else {
            // A horn: its mouth flaring out, ribbed.
            let flare = 0.075 * (0.35 + 0.65 * smoothstep(-0.07, 0.07, -side * fx));
            if abs(fy) < flare {
                colour = vec3<f32>(0.006, 0.006, 0.008);
                let rib = fract((fx + 0.1) / 0.018);
                colour += vec3<f32>(0.06, 0.05, 0.06) * step(0.85, rib) * (1.0 - abs(fy) / flare);
                colour += warm * exp(-pow((abs(fy) - flare) / 0.003, 2.0)) * 0.3 * glow;
            }
        }
        // The far column sinks into the dark: the stacks go back from the yard.
        colour *= select(1.0, 0.55, column >= 1.0);
    }

    // The crowd: two rows of heads and shoulders, black against the lights,
    // bobbing on the kick; in a drop, hands go up, a few holding a lighter.
    for (var row = 0; row < 2; row++) {
        let fr = f32(row);
        let width = 0.055 + 0.03 * fr;
        let base = 0.2 - 0.12 * fr;
        let radius = 0.018 + 0.01 * fr;
        let id = floor(p.x / width + fr * 0.5);
        let local = (fract(p.x / width + fr * 0.5) - 0.5) * width;
        let bob = s.pulse * (0.006 + 0.01 * hash11(id + fr * 9.0)) * s.motion;
        let head_y = base + 0.02 * hash11(id * 3.1 + fr) + bob;
        let head = length(vec2<f32>(local, p.y - head_y)) < radius;
        let body = p.y < head_y - radius * 0.7 && abs(local) < radius * 1.7;
        let hand_up = s.intensity > 0.6 && hash11(id * 5.3 + fr) > 0.72;
        let arm_top = head_y + radius * 3.2;
        let arm = hand_up && abs(local - radius * 1.2) < 0.004 && p.y > head_y && p.y < arm_top;
        if head || body || arm {
            colour = vec3<f32>(0.004, 0.004, 0.006);
        }
        // A lighter: a small flame over the raised hand, wavering gently.
        if hand_up && hash11(id * 7.7 + fr) > 0.55 {
            let flame = vec2<f32>(radius * 1.2, arm_top + 0.012);
            let waver = 0.9 + 0.1 * sin(s.time * 5.0 + id * 2.3) * s.motion;
            let d = length((vec2<f32>(local, p.y) - flame) * vec2<f32>(1.0, 0.6));
            colour += vec3<f32>(1.0, 0.55, 0.15) * (exp(-pow(d / 0.004, 2.0)) * 1.5 + exp(-d / 0.02) * 0.08) * waver;
        }
    }
    return colour;
}

fn basement(s: Scene) -> vec3<f32> {
    let p = s.p;
    let half_w = 0.5 * s.aspect;
    let drift = s.time * s.motion;
    // The light: a red wash from the booth, swelling in a drop, and the UV
    // tube's violet when the lasers would be on.
    let glow = 0.35 + 1.0 * s.intensity + 0.06 * s.pulse;
    let red = vec3<f32>(0.75, 0.06, 0.05);
    let uv = vec3<f32>(0.35, 0.08, 0.85);

    // The back wall: bare brick, sweating, lit from below.
    let ceiling = 0.8;
    let brick = vec2<f32>(0.09, 0.034);
    let course = floor(p.y / brick.y);
    let shifted = p.x / brick.x + 0.5 * (course % 2.0);
    let in_brick = vec2<f32>(fract(shifted), fract(p.y / brick.y));
    let mortar = min(min(in_brick.x, 1.0 - in_brick.x) * brick.x, min(in_brick.y, 1.0 - in_brick.y) * brick.y);
    let tone = 0.75 + 0.5 * hash21(vec2<f32>(floor(shifted), course));
    var colour = vec3<f32>(0.05, 0.018, 0.014) * tone * smoothstep(0.0, 0.003, mortar);
    colour *= 0.3 + 0.65 * glow * exp(-p.y * 1.8);
    colour += red * exp(-p.y * 2.6) * (0.08 + 0.18 * s.intensity);
    colour += uv * exp(-pow((p.y - 0.55) / 0.3, 2.0)) * 0.05 * s.lasers;
    // Sweat on the bricks: a few beads catching the light, running slowly.
    let bead_at = vec2<f32>(p.x / 0.012, (p.y + 0.001 * drift) / 0.02);
    let bead_cell = floor(bead_at);
    let in_cell = (fract(bead_at) - 0.5) * vec2<f32>(0.012, 0.02);
    let shine = 0.5 + 0.5 * sin(s.time * 2.0 + hash21(bead_cell + 3.0) * TAU);
    let bead = step(0.985, hash21(bead_cell)) * exp(-pow(length(in_cell) / 0.0012, 2.0)) * shine;
    colour += vec3<f32>(1.0, 0.6, 0.5) * bead * 0.6 * glow * step(p.y, ceiling);

    // The ceiling: low and close, concrete, three pipes along it.
    if p.y > ceiling {
        colour = vec3<f32>(0.016, 0.013, 0.016) + red * 0.02 * glow;
        for (var i = 0; i < 3; i++) {
            let fi = f32(i);
            let y = ceiling + 0.04 + 0.05 * fi;
            let r = 0.012 + 0.004 * fi;
            let d = abs(p.y - y);
            if d < r {
                let shade = sqrt(1.0 - (d / r) * (d / r));
                colour = vec3<f32>(0.03, 0.025, 0.028) * shade + red * 0.12 * glow * pow(shade, 6.0);
            }
        }
    }
    // Bare bulbs hanging from it on short leads, swaying a little.
    let spacing = 0.32;
    let k = round(p.x / spacing);
    let sway = 0.006 * sin(s.time * 0.9 + k * 1.3) * s.motion;
    let bulb = vec2<f32>(k * spacing + sway, ceiling - 0.045);
    if abs(p.x - k * spacing - sway * (ceiling - p.y) / 0.045) < 0.0012 && p.y > bulb.y && p.y < ceiling {
        colour = vec3<f32>(0.01, 0.008, 0.01);
    }
    let d = length(p - bulb);
    colour += vec3<f32>(1.0, 0.55, 0.25) * (1.5 * exp(-pow(d / 0.006, 2.0)) + 0.1 * exp(-d / 0.05)) * (0.35 + 0.5 * glow);

    // Haze under the ceiling, rolling slowly through the light.
    let haze = 0.5 + 0.5 * sin(p.x * 5.0 + drift * 0.12 + 2.0 * sin(p.y * 7.0 - drift * 0.08));
    colour += (red * 0.6 + uv * 0.4 * s.lasers) * haze * exp(-abs(p.y - 0.62) * 5.0) * 0.06 * glow;

    // The crowd, packed in close: big heads and shoulders, black against the
    // wash, bobbing on the kick; in a drop, hands up to the ceiling.
    for (var row = 0; row < 2; row++) {
        let fr = f32(row);
        let width = 0.08 + 0.04 * fr;
        let base = 0.24 - 0.13 * fr;
        let radius = 0.026 + 0.012 * fr;
        let id = floor(p.x / width + fr * 0.5);
        let local = (fract(p.x / width + fr * 0.5) - 0.5) * width;
        let bob = s.pulse * (0.008 + 0.012 * hash11(id + fr * 9.0)) * s.motion;
        let head_y = base + 0.025 * hash11(id * 3.1 + fr) + bob;
        let head = length(vec2<f32>(local, p.y - head_y)) < radius;
        let body = p.y < head_y - radius * 0.7 && abs(local) < radius * 1.8;
        let hand_up = s.intensity > 0.6 && hash11(id * 5.3 + fr) > 0.6;
        let arm = hand_up && abs(local + radius * 1.1) < 0.005 && p.y > head_y && p.y < head_y + radius * 3.5;
        if head || body || arm {
            colour = vec3<f32>(0.004, 0.003, 0.004);
            // The wash catching the tops of heads.
            colour += red * 0.05 * glow * smoothstep(head_y - radius, head_y + radius, p.y) * select(0.0, 1.0, head);
        }
    }
    return colour * (1.0 - 0.25 * smoothstep(half_w * 0.6, half_w, abs(p.x)));
}

// The festival's main stage: a night sky full of smoke, the truss's beams
// sweeping over it, LED screens at the sides, fifty thousand heads to the
// horizon, hands going up in a drop and phone lights between them.
fn festival(s: Scene) -> vec3<f32> {
    let p = s.p;
    let half_w = 0.5 * s.aspect;
    let drift = s.time * s.motion;
    let glow = 0.4 + 1.0 * s.intensity + 0.06 * s.pulse;

    // The sky: deep blue going violet down where the stage lights it.
    var colour = mix(vec3<f32>(0.03, 0.02, 0.08), vec3<f32>(0.005, 0.006, 0.02), smoothstep(0.2, 1.0, p.y));
    colour += vec3<f32>(0.25, 0.06, 0.3) * exp(-p.y * 2.4) * (0.15 + 0.3 * s.intensity);
    let star_cell = floor(s.pixel / 3.0);
    let star = step(0.997, hash21(star_cell)) * (0.5 + 0.5 * sin(s.time * 1.3 + hash21(star_cell + 7.0) * TAU));
    colour += vec3<f32>(0.5, 0.5, 0.7) * star * smoothstep(0.6, 1.0, p.y);
    // Smoke rolling over the field, catching every light.
    let smoke = 0.5 + 0.5 * sin(p.x * 4.0 + drift * 0.1 + 1.7 * sin(p.y * 6.0 - drift * 0.07));
    colour += vec3<f32>(0.08, 0.05, 0.12) * smoke * exp(-abs(p.y - 0.45) * 3.0) * glow * 0.4;

    // The beams: moving heads on the truss, sweeping slowly, cyan, magenta and
    // gold in turn; more of them, and brighter, in a drop.
    let beams = 0.25 + 0.75 * max(s.intensity, s.lasers);
    for (var i = 0; i < 6; i++) {
        let fi = f32(i);
        let x = (fi - 2.5) / 2.5 * half_w * 0.9;
        let origin = vec2<f32>(x, 0.86);
        let angle = -1.5708 + 0.55 * sin(s.time * (0.18 + 0.04 * fi) + fi * 2.1) * s.motion + (fi - 2.5) * 0.08;
        let direction = vec2<f32>(cos(angle), sin(angle));
        let d = ray_distance(p, origin, direction);
        let reach = max(dot(p - origin, direction), 0.0);
        let beam = (exp(-pow(d / (0.004 + 0.03 * reach), 2.0)) * 0.6) * exp(-reach * 0.9);
        var hue = vec3<f32>(0.2, 0.85, 1.0);
        if i % 3 == 1 {
            hue = vec3<f32>(1.0, 0.2, 0.8);
        } else if i % 3 == 2 {
            hue = vec3<f32>(1.0, 0.75, 0.2);
        }
        colour += hue * beam * beams;
    }

    // The truss across the top: a lattice of steel, lamps hanging from it.
    let truss_y = 0.88;
    if abs(p.y - truss_y) < 0.018 {
        let lattice = abs(fract((p.x + (p.y - truss_y)) / 0.036) - 0.5);
        let rails = step(0.013, abs(p.y - truss_y));
        colour = mix(colour, vec3<f32>(0.03, 0.03, 0.04), max(rails, step(lattice, 0.08)) * 0.9);
    }
    let lamp = round(p.x / 0.09);
    let lamp_at = vec2<f32>(lamp * 0.09, truss_y - 0.022);
    colour += vec3<f32>(1.0, 0.9, 0.7) * exp(-pow(length(p - lamp_at) / 0.004, 2.0)) * (0.4 + 0.6 * glow);

    // The side screens: tall LED walls, a slow gradient rolling up them, their
    // bars rising with the music.
    let screen_x = 0.78 * half_w;
    if abs(abs(p.x) - screen_x) < 0.11 * half_w && p.y > 0.3 && p.y < 0.78 {
        let u = (abs(p.x) - screen_x) / (0.11 * half_w);
        let v = (p.y - 0.3) / 0.48;
        let column = floor((u + 1.0) * 6.0);
        let level = 0.35 + 0.45 * s.intensity * (0.6 + 0.4 * sin(column * 1.7 + s.time * 0.8 * s.motion)) + 0.1 * s.pulse;
        let hue = 0.5 + 0.5 * cos(TAU * (v * 0.6 - s.time * 0.05 * s.motion + vec3<f32>(0.0, 0.33, 0.67)));
        let lit = step(v, level);
        let led = step(0.25, fract(s.pixel.x / 4.0)) * step(0.25, fract(s.pixel.y / 4.0));
        colour = vec3<f32>(0.01, 0.01, 0.015) + hue * (0.12 + 0.75 * lit) * led * (0.5 + 0.5 * glow);
    }

    // The field: rows of heads to the horizon, smaller as they go, bobbing on
    // the kick; in a drop, hands up, and phone lights among them.
    for (var row = 0; row < 4; row++) {
        let fr = f32(row);
        let scale = 1.0 / (1.0 + fr * 0.7);
        let width = 0.06 * scale;
        let base = 0.06 + 0.07 * fr * scale + 0.1 * (1.0 - scale);
        let radius = 0.019 * scale;
        let id = floor(p.x / width + fr * 0.37);
        let local = (fract(p.x / width + fr * 0.37) - 0.5) * width;
        let bob = s.pulse * 0.008 * scale * hash11(id + fr * 13.0) * s.motion;
        let head_y = base + 0.012 * scale * hash11(id * 3.7 + fr) + bob;
        let head = length(vec2<f32>(local, p.y - head_y)) < radius;
        let body = p.y < head_y - radius * 0.7 && abs(local) < radius * 1.7;
        let hand_up = s.intensity > 0.55 && hash11(id * 5.1 + fr) > 0.45;
        let arm = hand_up && abs(local - radius * 1.2) < 0.003 * scale && p.y > head_y && p.y < head_y + radius * 3.0;
        if head || body || arm {
            colour = vec3<f32>(0.006, 0.005, 0.01) + vec3<f32>(0.08, 0.03, 0.1) * 0.15 * glow * (1.0 - fr * 0.2);
        }
        // A phone held up, its screen glowing.
        if hand_up && hash11(id * 9.1 + fr) > 0.8 {
            let phone = vec2<f32>(radius * 1.2, head_y + radius * 3.2);
            let d = length(vec2<f32>(local, p.y) - phone);
            colour += vec3<f32>(0.8, 0.85, 1.0) * (exp(-pow(d / (0.0035 * scale), 2.0)) + 0.05 * exp(-d / 0.02));
        }
    }
    return colour;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = mood.a.xy;
    let time = mood.a.z;
    let flash = mood.b.z;
    let hype = mood.b.w;
    let motion = mood.c.x;

    // Pixels from the bottom left, then screen heights from the bottom centre.
    let pixel = vec2<f32>(in.uv.x * size.x, (1.0 - in.uv.y) * size.y);
    let aspect = size.x / size.y;
    var p = vec2<f32>(pixel.x / size.y - 0.5 * aspect, pixel.y / size.y);

    // VHS: now and then a tracking band rolls up the screen, pulling the picture sideways.
    let band_y = fract(time * 0.03) * 1.6 - 0.3;
    let in_band = exp(-pow((p.y - band_y) / 0.018, 2.0)) * motion;
    p.x += in_band * 0.004 * sin(time * 9.0 + p.y * 40.0);

    let scene = Scene(p, pixel, aspect, time, mood.a.w, mood.b.x, mood.b.y, motion);
    var colour: vec3<f32>;
    let venue = i32(mood.c.y + 0.5);
    if venue == 1 {
        colour = bedroom(scene);
    } else if venue == 2 {
        colour = warehouse(scene);
    } else if venue == 3 {
        colour = clash(scene);
    } else if venue == 4 {
        colour = basement(scene);
    } else if venue == 5 {
        colour = festival(scene);
    } else {
        colour = rooftop(scene);
    }

    // Printed: a halftone screen over the haze, scan lines, grain.
    let cell = 5.0;
    let dot_at = (fract(pixel / cell) - 0.5) * cell;
    let lum = dot(colour, vec3<f32>(0.3, 0.55, 0.15));
    let radius = clamp(sqrt(lum) * cell * 1.1, 0.0, cell * 0.7);
    let dot_mask = smoothstep(radius + 0.6, radius - 0.6, length(dot_at));
    colour *= mix(1.0, 0.6 + 0.8 * dot_mask, 0.3);
    colour *= 0.92 + 0.08 * sin(pixel.y * 3.14159);
    colour += (hash21(pixel + floor(time * 24.0) * 1.7) - 0.5) * 0.012 * (1.0 + in_band);

    // The vibe of the night: hype warms it toward gold; a WHEEL UP! flares once.
    colour = mix(colour, colour * vec3<f32>(1.25, 1.05, 0.7), 0.4 * hype);
    colour += vec3<f32>(1.0, 0.9, 0.5) * flash * 0.06;

    // A vignette, like a cheap lens.
    let centred = in.uv - 0.5;
    colour *= 1.0 - 0.55 * dot(centred, centred);

    return vec4<f32>(max(colour, vec3<f32>(0.0)), 1.0);
}
