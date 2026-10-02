// The venue behind the play, printed like a 90s rave flyer: the stop of the
// Pirate Radio Tour the tune belongs to. A city at night seen from a pirate
// station's rooftop (the default), a bedroom studio, a warehouse rave.
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
    // warehouse), unused.
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
