// MotionDescriptor GPU evaluate(). Matches nana-ui-core::evaluate_descriptor
// / evaluate_track. Storage layout is MotionGpuDescriptor / MotionGpuKeyframe.

const MOTION_KIND_SCALAR: u32 = 0u;
const MOTION_KIND_COLOR: u32 = 1u;
const MOTION_KIND_TRANSFORM: u32 = 2u;
const MOTION_KIND_DISCRETE: u32 = 3u;

const MOTION_CURVE_EASING: u32 = 0u;
const MOTION_CURVE_STEPS: u32 = 1u;
const MOTION_CURVE_SPRING: u32 = 2u;
const MOTION_CURVE_DECAY: u32 = 3u;

const MOTION_EASING_LINEAR: u32 = 0u;
const MOTION_EASING_OUT_CUBIC: u32 = 1u;
const MOTION_EASING_IN_OUT_CUBIC: u32 = 2u;
const MOTION_EASING_BEZIER: u32 = 3u;

const MOTION_FLAG_LIVE: u32 = 1u;
const MOTION_SETTLE_POS: f32 = 0.001;
const MOTION_SETTLE_VEL: f32 = 0.01;

struct MotionGpuValue {
    kind: u32,
    discrete: u32,
    _pad: vec2<u32>,
    channels: vec4<f32>,
    extra: vec4<f32>,
}

struct MotionGpuDescriptor {
    generation: u32,
    codec: u32,
    property: u32,
    flags: u32,
    track_lo: u32,
    track_hi: u32,
    target_lo: u32,
    target_hi: u32,
    start: f32,
    delay: f32,
    duration: f32,
    paused_at: f32,
    iteration: u32,
    direction: u32,
    fill: u32,
    play_state: u32,
    curve_kind: u32,
    easing_kind: u32,
    steps_count: u32,
    steps_jump: u32,
    bezier: vec4<f32>,
    spring: vec4<f32>,
    from_value: MotionGpuValue,
    to_value: MotionGpuValue,
    velocity: MotionGpuValue,
    keyframe_start: u32,
    keyframe_count: u32,
    version: u32,
    _end_pad: u32,
}

struct MotionGpuKeyframe {
    offset: f32,
    easing_kind: u32,
    _pad: vec2<u32>,
    bezier: vec4<f32>,
    value: MotionGpuValue,
}

struct MotionGpuTime {
    now: f32,
    eval_motion_id: u32,
    _pad: vec2<u32>,
}

struct MotionGpuSample {
    value: MotionGpuValue,
    progress: f32,
    applies: u32,
    finished: u32,
    _pad: u32,
}

@group(1) @binding(0)
var<storage, read> motion_descriptors: array<MotionGpuDescriptor>;
@group(1) @binding(1)
var<storage, read> motion_keyframes: array<MotionGpuKeyframe>;
@group(1) @binding(2)
var<uniform> motion_time: MotionGpuTime;

fn motion_clock(desc: MotionGpuDescriptor, now: f32) -> f32 {
    if (desc.play_state == 1u) {
        if (desc.paused_at >= 0.0) {
            return desc.paused_at;
        }
        return now;
    }
    return now;
}

fn motion_fill_backwards(fill: u32) -> bool {
    return fill == 2u || fill == 3u;
}

fn motion_fill_forwards(fill: u32) -> bool {
    return fill == 1u || fill == 3u;
}

fn motion_start_progress(direction: u32) -> f32 {
    if (direction == 1u || direction == 3u) {
        return 1.0;
    }
    return 0.0;
}

fn motion_map_progress(direction: u32, iteration_index: u32, linear: f32) -> f32 {
    var reverse = false;
    if (direction == 1u) {
        reverse = true;
    } else if (direction == 2u) {
        reverse = (iteration_index % 2u) != 0u;
    } else if (direction == 3u) {
        reverse = (iteration_index % 2u) == 0u;
    }
    if (reverse) {
        return 1.0 - linear;
    }
    return linear;
}

fn motion_end_progress(direction: u32, completed: u32) -> f32 {
    if (completed == 0u) {
        return motion_start_progress(direction);
    }
    return motion_map_progress(direction, completed - 1u, 1.0);
}

fn motion_bezier_axis(t: f32, p1: f32, p2: f32) -> f32 {
    let mt = 1.0 - t;
    return 3.0 * mt * mt * t * p1 + 3.0 * mt * t * t * p2 + t * t * t;
}

fn motion_sample_bezier(points: vec4<f32>, progress: f32) -> f32 {
    if (progress <= 0.0) {
        return 0.0;
    }
    if (progress >= 1.0) {
        return 1.0;
    }
    var low = 0.0;
    var high = 1.0;
    for (var i = 0; i < 24; i = i + 1) {
        let mid = 0.5 * (low + high);
        if (motion_bezier_axis(mid, points.x, points.z) < progress) {
            low = mid;
        } else {
            high = mid;
        }
    }
    return motion_bezier_axis(0.5 * (low + high), points.y, points.w);
}

fn motion_sample_easing(kind: u32, bezier: vec4<f32>, progress: f32) -> f32 {
    let p = clamp(progress, 0.0, 1.0);
    if (kind == MOTION_EASING_OUT_CUBIC) {
        let t = 1.0 - p;
        return 1.0 - t * t * t;
    }
    if (kind == MOTION_EASING_IN_OUT_CUBIC) {
        if (p < 0.5) {
            return 4.0 * p * p * p;
        }
        let t = -2.0 * p + 2.0;
        return 1.0 - (t * t * t) / 2.0;
    }
    if (kind == MOTION_EASING_BEZIER) {
        return motion_sample_bezier(bezier, p);
    }
    return p;
}

fn motion_sample_steps(progress: f32, count: u32, jump: u32) -> f32 {
    if (count == 0u) {
        return progress;
    }
    let n = f32(count);
    let p = clamp(progress, 0.0, 1.0);
    if (jump == 1u) {
        if (p >= 1.0) {
            return 1.0;
        }
        return floor(p * n) / n;
    }
    if (jump == 0u) {
        // Mirrors `sample_steps`: the CSS algorithm increments the current
        // step for `jump-start`, so input 0 is already 1/n.
        return min((floor(p * n) + 1.0) / n, 1.0);
    }
    if (jump == 2u) {
        if (count <= 1u) {
            if (p >= 1.0) { return 1.0; }
            return 0.0;
        }
        if (p >= 1.0) {
            return 1.0;
        }
        return floor(p * (n - 1.0)) / (n - 1.0);
    }
    if (p >= 1.0) {
        return 1.0;
    }
    return (floor(p * n) + 1.0) / (n + 1.0);
}

fn motion_sample_progress(desc: MotionGpuDescriptor, linear: f32) -> f32 {
    let p = clamp(linear, 0.0, 1.0);
    if (desc.curve_kind == MOTION_CURVE_STEPS) {
        return motion_sample_steps(p, desc.steps_count, desc.steps_jump);
    }
    if (desc.curve_kind == MOTION_CURVE_SPRING || desc.curve_kind == MOTION_CURVE_DECAY) {
        return p;
    }
    return motion_sample_easing(desc.easing_kind, desc.bezier, p);
}

// Mirrors `MotionValue::lerp`: `t` is the *eased* progress and may leave
// 0..1, because that is what a `cubic-bezier` with a control point past 1 is
// for. Clamping it would delete the overshoot. A colour clamps per channel
// afterwards instead, the way a browser clamps to the gamut.
fn motion_lerp_value(from_v: MotionGpuValue, to_v: MotionGpuValue, t: f32) -> MotionGpuValue {
    let u = t;
    var out = from_v;
    if (from_v.kind != to_v.kind) {
        if (u >= 1.0) {
            return to_v;
        }
        return from_v;
    }
    out.kind = from_v.kind;
    out.discrete = select(from_v.discrete, to_v.discrete, u >= 1.0);
    out.channels = from_v.channels + (to_v.channels - from_v.channels) * u;
    out.extra = from_v.extra + (to_v.extra - from_v.extra) * u;
    if (from_v.kind == MOTION_KIND_COLOR) {
        out.channels = clamp(out.channels, vec4<f32>(0.0), vec4<f32>(1.0));
        out.extra = clamp(out.extra, vec4<f32>(0.0), vec4<f32>(1.0));
    }
    return out;
}

fn motion_zero_velocity(value: MotionGpuValue) -> MotionGpuValue {
    var out = value;
    out.channels = vec4<f32>(0.0);
    out.extra = vec4<f32>(0.0);
    if (value.kind == MOTION_KIND_TRANSFORM) {
        out.channels = vec4<f32>(0.0);
        out.extra = vec4<f32>(0.0);
    }
    return out;
}

fn motion_local_ease(desc: MotionGpuDescriptor, stop_kind: u32, stop_bezier: vec4<f32>, local: f32) -> f32 {
    if (stop_kind != 0xffffffffu) {
        return motion_sample_easing(stop_kind, stop_bezier, local);
    }
    return motion_sample_progress(desc, local);
}

fn motion_keyframe(index: u32) -> MotionGpuKeyframe {
    return motion_keyframes[min(index, arrayLength(&motion_keyframes) - 1u)];
}

fn motion_interpolate_keyframes(desc: MotionGpuDescriptor, linear: f32) -> MotionGpuValue {
    let count = desc.keyframe_count;
    if (count == 0u) {
        return desc.from_value;
    }
    let p = clamp(linear, 0.0, 1.0);
    var idx = 0u;
    for (var i = 0u; i < count; i = i + 1u) {
        let stop = motion_keyframe(desc.keyframe_start + i);
        if (stop.offset < p) {
            idx = i + 1u;
        }
    }
    if (idx < count) {
        let at = motion_keyframe(desc.keyframe_start + idx);
        if (abs(at.offset - p) <= 1e-7) {
            return at.value;
        }
    }
    if (idx == 0u) {
        let first = motion_keyframe(desc.keyframe_start);
        if (first.offset <= 0.0) {
            return first.value;
        }
        let local = clamp(p / first.offset, 0.0, 1.0);
        // The implicit keyframe at offset 0 is `from_value`, and it declares no
        // easing, so this interval runs on the track's own curve.
        return motion_lerp_value(
            desc.from_value,
            first.value,
            motion_local_ease(desc, 0xffffffffu, vec4<f32>(0.0), local),
        );
    }
    if (idx >= count) {
        return motion_keyframe(desc.keyframe_start + count - 1u).value;
    }
    let prev = motion_keyframe(desc.keyframe_start + idx - 1u);
    let next = motion_keyframe(desc.keyframe_start + idx);
    let span = max(next.offset - prev.offset, 1e-7);
    let local = clamp((p - prev.offset) / span, 0.0, 1.0);
    // Mirrors `interpolate_keyframes`: the stop that *opens* the interval
    // supplies its timing function, as CSS Animations and the WAAPI define it.
    return motion_lerp_value(
        prev.value,
        next.value,
        motion_local_ease(desc, prev.easing_kind, prev.bezier, local),
    );
}

fn motion_interpolate_source(desc: MotionGpuDescriptor, linear: f32, eased: f32, finished: bool) -> MotionGpuValue {
    if (desc.keyframe_count == 0u) {
        return motion_lerp_value(desc.from_value, desc.to_value, eased);
    }
    _ = finished;
    return motion_interpolate_keyframes(desc, linear);
}

struct MotionTimed {
    linear: f32,
    finished: bool,
    applies: bool,
}

fn motion_timed_progress(desc: MotionGpuDescriptor, now: f32) -> MotionTimed {
    var out: MotionTimed;
    let start = desc.start + desc.delay;
    if (!(start == start)) {
        out.linear = motion_end_progress(desc.direction, 0u);
        out.finished = true;
        out.applies = motion_fill_forwards(desc.fill);
        return out;
    }
    if (now < start) {
        let hold = motion_fill_backwards(desc.fill);
        out.linear = select(0.0, motion_start_progress(desc.direction), hold);
        out.finished = false;
        out.applies = hold;
        return out;
    }
    if (desc.iteration == 0u) {
        let duration = desc.duration;
        if (duration <= 0.0) {
            out.linear = motion_end_progress(desc.direction, 1u);
            out.finished = false;
            out.applies = true;
            return out;
        }
        let t = (now - start) / duration;
        let iteration_index = u32(floor(t));
        let linear = clamp(t - f32(iteration_index), 0.0, 1.0);
        out.linear = motion_map_progress(desc.direction, iteration_index, linear);
        out.finished = false;
        out.applies = true;
        return out;
    }
    let end = start + desc.duration * f32(desc.iteration);
    if (now > end) {
        let hold = motion_fill_forwards(desc.fill);
        out.linear = select(0.0, motion_end_progress(desc.direction, desc.iteration), hold);
        out.finished = true;
        out.applies = hold;
        return out;
    }
    if (now == end) {
        out.linear = motion_end_progress(desc.direction, desc.iteration);
        out.finished = true;
        out.applies = true;
        return out;
    }
    let duration = desc.duration;
    if (duration <= 0.0) {
        out.linear = motion_end_progress(desc.direction, desc.iteration);
        out.finished = true;
        out.applies = motion_fill_forwards(desc.fill);
        return out;
    }
    let t = (now - start) / duration;
    let last = desc.iteration - 1u;
    let iteration_index = min(u32(floor(t)), last);
    let linear = clamp(t - f32(iteration_index), 0.0, 1.0);
    out.linear = motion_map_progress(desc.direction, iteration_index, linear);
    out.finished = false;
    out.applies = true;
    return out;
}

fn motion_critically_damped(x0: f32, v0: f32, omega0: f32, t: f32) -> vec2<f32> {
    let a = x0;
    let b = v0 + omega0 * x0;
    let exp = exp(-omega0 * t);
    let x = (a + b * t) * exp;
    let v = b * exp + (a + b * t) * (-omega0) * exp;
    return vec2(x, v);
}

fn motion_damped_harmonic(x0: f32, v0: f32, k: f32, c: f32, m: f32, t: f32) -> vec2<f32> {
    if (t <= 0.0) {
        return vec2(x0, v0);
    }
    if (!(k > 0.0 && m > 0.0 && k == k && m == m)) {
        return vec2(0.0, 0.0);
    }
    let omega0 = sqrt(k / m);
    if (!(omega0 == omega0) || omega0 == 0.0) {
        return vec2(0.0, 0.0);
    }
    let zeta = c / (2.0 * sqrt(k * m));
    if (!(zeta == zeta)) {
        return vec2(0.0, 0.0);
    }
    if (zeta < 1.0 - 1e-5) {
        let omega_d = omega0 * sqrt(1.0 - zeta * zeta);
        if (omega_d == 0.0 || !(omega_d == omega_d)) {
            return motion_critically_damped(x0, v0, omega0, t);
        }
        let a = x0;
        let b = (v0 + zeta * omega0 * x0) / omega_d;
        let e = exp(-zeta * omega0 * t);
        let angle = omega_d * t;
        let s = sin(angle);
        let co = cos(angle);
        let x = e * (a * co + b * s);
        let v = -zeta * omega0 * x + e * (-a * omega_d * s + b * omega_d * co);
        return vec2(x, v);
    }
    if (zeta > 1.0 + 1e-5) {
        let disc = sqrt(zeta * zeta - 1.0);
        let r1 = -omega0 * (zeta - disc);
        let r2 = -omega0 * (zeta + disc);
        if (abs(r1 - r2) <= 1e-7) {
            return motion_critically_damped(x0, v0, omega0, t);
        }
        let a = (v0 - r2 * x0) / (r1 - r2);
        let b = x0 - a;
        let e1 = exp(r1 * t);
        let e2 = exp(r2 * t);
        return vec2(a * e1 + b * e2, a * r1 * e1 + b * r2 * e2);
    }
    return motion_critically_damped(x0, v0, omega0, t);
}

fn motion_spring_channel(x0: f32, rest: f32, v0: f32, params: vec4<f32>, t: f32) -> vec2<f32> {
    let hv = motion_damped_harmonic(x0 - rest, v0, params.x, params.y, params.z, t);
    return vec2(rest + hv.x, hv.y);
}

fn motion_decay_channel(x0: f32, v0: f32, tau: f32, t: f32) -> vec2<f32> {
    if (tau <= 0.0 || !(tau == tau)) {
        return vec2(x0, 0.0);
    }
    let tt = max(t, 0.0);
    let decay = exp(-tt / tau);
    return vec2(x0 + v0 * tau * (1.0 - decay), v0 * decay);
}

fn motion_max_abs(value: MotionGpuValue) -> f32 {
    if (value.kind == MOTION_KIND_SCALAR) {
        return abs(value.channels.x);
    }
    if (value.kind == MOTION_KIND_COLOR) {
        return max(max(abs(value.channels.x), abs(value.channels.y)), max(abs(value.channels.z), abs(value.channels.w)));
    }
    if (value.kind == MOTION_KIND_TRANSFORM) {
        let m0 = max(max(abs(value.channels.x), abs(value.channels.y)), max(abs(value.channels.z), abs(value.channels.w)));
        return max(m0, max(abs(value.extra.x), abs(value.extra.y)));
    }
    return 0.0;
}

fn motion_sub(a: MotionGpuValue, b: MotionGpuValue) -> MotionGpuValue {
    var out = a;
    out.channels = a.channels - b.channels;
    out.extra = a.extra - b.extra;
    return out;
}

fn motion_is_settled(value: MotionGpuValue, rest: MotionGpuValue, velocity: MotionGpuValue) -> bool {
    return motion_max_abs(motion_sub(value, rest)) <= MOTION_SETTLE_POS
        && motion_max_abs(velocity) <= MOTION_SETTLE_VEL;
}

struct MotionPhysicsState {
    pos: MotionGpuValue,
    vel: MotionGpuValue,
}

fn motion_physics_state(
    from_v: MotionGpuValue,
    rest: MotionGpuValue,
    vel: MotionGpuValue,
    desc: MotionGpuDescriptor,
    t: f32,
    decay: bool,
) -> MotionPhysicsState {
    var pos = from_v;
    var vout = vel;
    if (from_v.kind == MOTION_KIND_SCALAR) {
        let ch = select(
            motion_spring_channel(from_v.channels.x, rest.channels.x, vel.channels.x, desc.spring, t),
            motion_decay_channel(from_v.channels.x, vel.channels.x, desc.spring.w, t),
            decay,
        );
        pos.channels.x = ch.x;
        vout.channels.x = ch.y;
        return MotionPhysicsState(pos, vout);
    }
    if (from_v.kind == MOTION_KIND_COLOR) {
        let ch0 = select(
            motion_spring_channel(from_v.channels.x, rest.channels.x, vel.channels.x, desc.spring, t),
            motion_decay_channel(from_v.channels.x, vel.channels.x, desc.spring.w, t),
            decay,
        );
        let ch1 = select(
            motion_spring_channel(from_v.channels.y, rest.channels.y, vel.channels.y, desc.spring, t),
            motion_decay_channel(from_v.channels.y, vel.channels.y, desc.spring.w, t),
            decay,
        );
        let ch2 = select(
            motion_spring_channel(from_v.channels.z, rest.channels.z, vel.channels.z, desc.spring, t),
            motion_decay_channel(from_v.channels.z, vel.channels.z, desc.spring.w, t),
            decay,
        );
        let ch3 = select(
            motion_spring_channel(from_v.channels.w, rest.channels.w, vel.channels.w, desc.spring, t),
            motion_decay_channel(from_v.channels.w, vel.channels.w, desc.spring.w, t),
            decay,
        );
        pos.channels = vec4(ch0.x, ch1.x, ch2.x, ch3.x);
        vout.channels = vec4(ch0.y, ch1.y, ch2.y, ch3.y);
        return MotionPhysicsState(pos, vout);
    }
    if (from_v.kind == MOTION_KIND_TRANSFORM) {
        let a = select(
            motion_spring_channel(from_v.channels.x, rest.channels.x, vel.channels.x, desc.spring, t),
            motion_decay_channel(from_v.channels.x, vel.channels.x, desc.spring.w, t),
            decay,
        );
        let b = select(
            motion_spring_channel(from_v.channels.y, rest.channels.y, vel.channels.y, desc.spring, t),
            motion_decay_channel(from_v.channels.y, vel.channels.y, desc.spring.w, t),
            decay,
        );
        let c = select(
            motion_spring_channel(from_v.channels.z, rest.channels.z, vel.channels.z, desc.spring, t),
            motion_decay_channel(from_v.channels.z, vel.channels.z, desc.spring.w, t),
            decay,
        );
        let d = select(
            motion_spring_channel(from_v.channels.w, rest.channels.w, vel.channels.w, desc.spring, t),
            motion_decay_channel(from_v.channels.w, vel.channels.w, desc.spring.w, t),
            decay,
        );
        let e = select(
            motion_spring_channel(from_v.extra.x, rest.extra.x, vel.extra.x, desc.spring, t),
            motion_decay_channel(from_v.extra.x, vel.extra.x, desc.spring.w, t),
            decay,
        );
        let f = select(
            motion_spring_channel(from_v.extra.y, rest.extra.y, vel.extra.y, desc.spring, t),
            motion_decay_channel(from_v.extra.y, vel.extra.y, desc.spring.w, t),
            decay,
        );
        pos.channels = vec4(a.x, b.x, c.x, d.x);
        pos.extra = vec4(e.x, f.x, 0.0, 0.0);
        vout.channels = vec4(a.y, b.y, c.y, d.y);
        vout.extra = vec4(e.y, f.y, 0.0, 0.0);
        return MotionPhysicsState(pos, vout);
    }
    if (t <= 0.0) {
        return MotionPhysicsState(from_v, vel);
    }
    return MotionPhysicsState(rest, motion_zero_velocity(rest));
}

fn motion_evaluate_physics(desc: MotionGpuDescriptor, now: f32, decay: bool) -> MotionGpuSample {
    var sample: MotionGpuSample;
    let start = desc.start + desc.delay;
    if (now < start) {
        sample.value = desc.from_value;
        sample.progress = 0.0;
        sample.finished = 0u;
        sample.applies = select(0u, 1u, motion_fill_backwards(desc.fill));
        return sample;
    }
    let t = max(now - start, 0.0);
    // A decay has no target: it settles at its own asymptote, x0 + v0 * tau,
    // which is what `evaluate_decay` measures against. Measuring against
    // `to_value` (which a decay track never sets) leaves `finished` and
    // `progress` disagreeing with the CPU reference for the whole fling.
    var rest = desc.to_value;
    if (decay) {
        rest = motion_physics_state(desc.from_value, desc.to_value, desc.velocity, desc, 1000.0, true).pos;
    }
    let state = motion_physics_state(desc.from_value, rest, desc.velocity, desc, t, decay);
    let finished = motion_is_settled(state.pos, rest, state.vel);
    sample.value = state.pos;
    sample.progress = select(0.0, 1.0, finished);
    sample.finished = select(0u, 1u, finished);
    sample.applies = 1u;
    return sample;
}

fn motion_evaluate_timed(desc: MotionGpuDescriptor, now: f32) -> MotionGpuSample {
    let phase = motion_timed_progress(desc, now);
    let linear = clamp(phase.linear, 0.0, 1.0);
    let progress = motion_sample_progress(desc, linear);
    var sample: MotionGpuSample;
    sample.progress = progress;
    sample.finished = select(0u, 1u, phase.finished);
    sample.applies = select(0u, 1u, phase.applies);
    if (!phase.applies) {
        if (phase.finished) {
            sample.value = desc.to_value;
            if (desc.keyframe_count > 0u) {
                sample.value = motion_keyframe(desc.keyframe_start + desc.keyframe_count - 1u).value;
            }
        } else {
            sample.value = desc.from_value;
        }
        return sample;
    }
    sample.value = motion_interpolate_source(desc, linear, progress, phase.finished);
    return sample;
}

fn motion_descriptor_at(index: u32) -> MotionGpuDescriptor {
    return motion_descriptors[min(index, arrayLength(&motion_descriptors) - 1u)];
}

fn motion_evaluate_id(motion_id: u32, expected_generation: u32) -> MotionGpuSample {
    var sample: MotionGpuSample;
    sample.value = MotionGpuValue(MOTION_KIND_SCALAR, 0u, vec2<u32>(0u), vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0));
    sample.progress = 1.0;
    sample.applies = 0u;
    sample.finished = 1u;
    if (motion_id == 0u) {
        return sample;
    }
    let index = motion_id - 1u;
    let desc = motion_descriptor_at(index);
    if ((desc.flags & MOTION_FLAG_LIVE) == 0u) {
        return sample;
    }
    if (expected_generation != 0u && desc.generation != expected_generation) {
        return sample;
    }
    let now = motion_clock(desc, motion_time.now);
    if (desc.curve_kind == MOTION_CURVE_SPRING) {
        return motion_evaluate_physics(desc, now, false);
    }
    if (desc.curve_kind == MOTION_CURVE_DECAY) {
        return motion_evaluate_physics(desc, now, true);
    }
    return motion_evaluate_timed(desc, now);
}

fn motion_evaluate(motion_id: u32) -> MotionGpuSample {
    return motion_evaluate_id(motion_id, 0u);
}

fn motion_sample_scalar(sample: MotionGpuSample, fallback: f32) -> f32 {
    if (sample.applies == 0u) {
        return fallback;
    }
    if (sample.value.kind == MOTION_KIND_SCALAR) {
        return sample.value.channels.x;
    }
    return fallback;
}

fn motion_sample_affine(sample: MotionGpuSample) -> vec4<f32> {
    // abcd
    return sample.value.channels;
}

fn motion_sample_translation(sample: MotionGpuSample) -> vec2<f32> {
    return sample.value.extra.xy;
}

fn motion_apply_local(abcd: vec4<f32>, ef: vec4<f32>, motion_id: u32) -> vec3<f32> {
    // Returns packed abcd in xy unused; caller uses full.
    _ = abcd;
    _ = ef;
    _ = motion_id;
    return vec3<f32>(0.0);
}

struct MotionAffine {
    abcd: vec4<f32>,
    ef: vec4<f32>,
}

// `base · T(origin) · M · T(-origin)`: the overlay pivots on the node's
// transform-origin, matching `LayoutStyle::world_scene_transform`. The base
// may already be projective, so compose and normalize the complete 3×3.
fn motion_compose_projective(base_abcd: vec4<f32>, base_ef: vec4<f32>, origin: vec2<f32>, sample: MotionGpuSample) -> MotionAffine {
    if (sample.applies == 0u || sample.value.kind != MOTION_KIND_TRANSFORM) {
        return MotionAffine(base_abcd, base_ef);
    }
    let a = sample.value.channels.x;
    let b = sample.value.channels.y;
    let c = sample.value.channels.z;
    let d = sample.value.channels.w;
    let e = sample.value.extra.x + origin.x - a * origin.x - c * origin.y;
    let f = sample.value.extra.y + origin.y - b * origin.x - d * origin.y;
    let na = base_abcd.x * a + base_abcd.z * b;
    let nb = base_abcd.y * a + base_abcd.w * b;
    let nc = base_abcd.x * c + base_abcd.z * d;
    let nd = base_abcd.y * c + base_abcd.w * d;
    let ne = base_abcd.x * e + base_abcd.z * f + base_ef.x;
    let nf = base_abcd.y * e + base_abcd.w * f + base_ef.y;
    let ng = base_ef.z * a + base_ef.w * b;
    let nh = base_ef.z * c + base_ef.w * d;
    let k = base_ef.z * e + base_ef.w * f + 1.0;
    let abs_k = abs(k);
    if (!(abs_k <= 3.4028235e+38) || abs_k < 1.0e-6) {
        // Match AffineTransform::then()/then_hit(): a singular composition
        // fails closed instead of handing NaNs or an unnormalized matrix to
        // the perspective divide.
        return MotionAffine(
            vec4(1.0, 0.0, 0.0, 1.0),
            vec4(0.0, 0.0, 0.0, 0.0),
        );
    }
    let inv_k = 1.0 / k;
    return MotionAffine(
        vec4(na, nb, nc, nd) * inv_k,
        vec4(ne, nf, ng, nh) * inv_k,
    );
}

@vertex
fn motion_eval_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}

@fragment
fn motion_eval_fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = motion_evaluate(motion_time.eval_motion_id);
    if (pos.x < 1.0) {
        return sample.value.channels;
    }
    return vec4<f32>(sample.value.extra.xy, sample.progress, f32(sample.applies));
}
