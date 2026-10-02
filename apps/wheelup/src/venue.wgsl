// The venue behind the play: a city at night seen from a pirate station's
// rooftop, printed like a 90s rave flyer. Everything is computed here, from a
// few numbers the game moves with the music (see stage.rs): the kick's pulse,
// how much is going on, the lasers of a drop, a WHEEL UP!'s flash.
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
    // Motion (1 normal, 0 reduced), unused.
    c: vec4<f32>,
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

// Distance from a point to a ray (origin, unit direction), forward only.
fn ray_distance(p: vec2<f32>, origin: vec2<f32>, direction: vec2<f32>) -> f32 {
    let along = max(dot(p - origin, direction), 0.0);
    return length(p - origin - direction * along);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = mood.a.xy;
    let time = mood.a.z;
    let pulse = mood.a.w;
    let intensity = mood.b.x;
    let lasers = mood.b.y;
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
