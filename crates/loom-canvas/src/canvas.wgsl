// LoomDSP patch canvas shaders.
//
// Naming: wgpu translates this file to Metal, HLSL, and SPIR-V, and does not
// rename every identifier that is reserved there. Avoid names that are types
// or keywords in Metal or HLSL, such as `half`, `in`, `out`,
// `vector`, `matrix`, `sample`, or `texture`. The test `shader_names` checks
// for the ones that have bitten us.
//
// Cost model: every shape is one small quad, and its fragment shader runs
// only inside that quad. Cables are thin triangle strips. Nothing runs a
// shader over the whole canvas except the background dot grid, which is a
// few arithmetic operations per pixel. This keeps GPU cost tied to what is
// drawn, not to the number of pixels on a Retina display.

struct Globals {
    // World point at the widget's top-left corner.
    origin: vec2<f32>,
    // Widget size in logical pixels.
    size: vec2<f32>,
    // Logical pixels per world unit.
    zoom: f32,
    // Physical pixels per logical pixel (2.0 on Retina).
    scale: f32,
    // Opacity of cables that are not in focus (not on a selected or hovered
    // module). Lower while something is selected, so its cables stand out.
    cable_dim: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> g: Globals;

fn to_clip(world: vec2<f32>) -> vec4<f32> {
    let s = (world - g.origin) * g.zoom;
    return vec4<f32>(s.x / g.size.x * 2.0 - 1.0, 1.0 - s.y / g.size.y * 2.0, 0.0, 1.0);
}

// Physical pixels per world unit.
fn px_per_world() -> f32 {
    return g.zoom * g.scale;
}

fn premultiply(c: vec4<f32>, coverage: f32) -> vec4<f32> {
    let a = c.a * coverage;
    return vec4<f32>(c.rgb * a, a);
}

// ---------------------------------------------------------------------------
// Rounded rectangles (tiles, headers, ports, outlines).

struct ShapeIn {
    // x, y, width, height in world units.
    @location(0) rect: vec4<f32>,
    @location(1) fill: vec4<f32>,
    @location(2) border: vec4<f32>,
    // corner radius (world), border width (world), minimum border (physical px), unused.
    @location(3) params: vec4<f32>,
};

struct ShapeOut {
    @builtin(position) position: vec4<f32>,
    // Position relative to the centre, in physical pixels.
    @location(0) local: vec2<f32>,
    // Half size in physical pixels.
    @location(1) half_size: vec2<f32>,
    @location(2) fill: vec4<f32>,
    @location(3) border: vec4<f32>,
    // Corner radius and border width in physical pixels.
    @location(4) radius_border: vec2<f32>,
};

@vertex
fn vs_shape(@builtin(vertex_index) vi: u32, shape: ShapeIn) -> ShapeOut {
    // Triangle strip: 0 = top-left, 1 = top-right, 2 = bottom-left, 3 = bottom-right.
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u)) * 2.0 - 1.0;
    let px = px_per_world();
    let half_size = shape.rect.zw * 0.5 * px;
    // One extra pixel on every side for the anti-aliased edge.
    let local = corner * (half_size + vec2<f32>(1.0));
    let centre = shape.rect.xy + shape.rect.zw * 0.5;

    var result: ShapeOut;
    result.position = to_clip(centre + local / px);
    result.local = local;
    result.half_size = half_size;
    result.fill = shape.fill;
    result.border = shape.border;
    var border = shape.params.y * px;
    if (shape.params.y > 0.0) {
        border = max(border, shape.params.z);
    }
    result.radius_border = vec2<f32>(min(shape.params.x * px, min(half_size.x, half_size.y)), border);
    return result;
}

@fragment
fn fs_shape(v: ShapeOut) -> @location(0) vec4<f32> {
    let r = v.radius_border.x;
    let q = abs(v.local) - v.half_size + vec2<f32>(r);
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let coverage = clamp(0.5 - d, 0.0, 1.0);
    if (coverage <= 0.0) {
        discard;
    }
    var colour = v.fill;
    let b = v.radius_border.y;
    if (b > 0.0) {
        let t = clamp(d + b + 0.5, 0.0, 1.0);
        colour = mix(v.fill, v.border, t);
    }
    return premultiply(colour, coverage);
}

// ---------------------------------------------------------------------------
// Cables: cubic Bezier curves, built in the vertex shader.

const SEGMENTS: u32 = 32u;

struct CableIn {
    // Start and end points in world units.
    @location(0) ends: vec4<f32>,
    @location(1) colour_from: vec4<f32>,
    @location(2) colour_to: vec4<f32>,
    // width (world), dashed (0 or 1), minimum width (physical px),
    // focused (1: full opacity, 0: dimmed by `cable_dim`).
    @location(3) params: vec4<f32>,
};

struct CableOut {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
    // Signed distance from the centre line, physical px.
    @location(1) across: f32,
    // Half width in physical px.
    @location(2) half_width: f32,
    // Distance along the cable in world units (for dashes).
    @location(3) along: f32,
    @location(4) dashed: f32,
    @location(5) opacity: f32,
};

@vertex
fn vs_cable(@builtin(vertex_index) vi: u32, cable: CableIn) -> CableOut {
    let i = vi / 2u;
    let side = f32(vi & 1u) * 2.0 - 1.0;
    let t = f32(i) / f32(SEGMENTS);

    let p0 = cable.ends.xy;
    let p3 = cable.ends.zw;
    // Horizontal tangents at both ends. Cables that run backwards (right
    // to left, usually feedback) loop out and sag below the modules, so they
    // do not run straight across the tiles between their ends.
    let span = abs(p3.x - p0.x);
    var reach = max(span * 0.5, 30.0);
    var sag = 0.0;
    if (p3.x < p0.x) {
        reach = 40.0 + span * 0.15;
        sag = 30.0 + min(span * 0.2, 120.0);
    }
    let p1 = p0 + vec2<f32>(reach, sag);
    let p2 = p3 + vec2<f32>(-reach, sag);

    let u = 1.0 - t;
    let pos = u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3;
    var tangent = 3.0 * u * u * (p1 - p0) + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (p3 - p2);
    if (dot(tangent, tangent) < 1e-6) {
        tangent = vec2<f32>(1.0, 0.0);
    }
    let normal = normalize(vec2<f32>(-tangent.y, tangent.x));

    let px = px_per_world();
    let half_width = max(cable.params.x * 0.5 * px, cable.params.z * 0.5);
    let offset = side * (half_width + 1.0);

    // Rough length of the curve: average of the chord and the control polygon.
    let length_estimate = 0.5 * (distance(p0, p3) + distance(p0, p1) + distance(p1, p2) + distance(p2, p3));

    var result: CableOut;
    result.position = to_clip(pos + normal * offset / px);
    result.colour = mix(cable.colour_from, cable.colour_to, t);
    result.across = offset;
    result.half_width = half_width;
    result.along = t * length_estimate;
    result.dashed = cable.params.y;
    result.opacity = select(g.cable_dim, 1.0, cable.params.w > 0.5);
    return result;
}

@fragment
fn fs_cable(v: CableOut) -> @location(0) vec4<f32> {
    var coverage = clamp(v.half_width + 0.5 - abs(v.across), 0.0, 1.0) * v.opacity;
    if (v.dashed > 0.5 && fract(v.along / 10.0) > 0.55) {
        // Gaps stay faintly visible, so the cable never seems to vanish.
        coverage = coverage * 0.2;
    }
    if (coverage <= 0.0) {
        discard;
    }
    return premultiply(v.colour, coverage);
}

// ---------------------------------------------------------------------------
// Background dot grid. One full-size quad with very cheap shading.

struct GridOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world: vec2<f32>,
};

@vertex
fn vs_grid(@builtin(vertex_index) vi: u32) -> GridOut {
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    var result: GridOut;
    result.position = vec4<f32>(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0, 0.0, 1.0);
    result.world = g.origin + corner * g.size / g.zoom;
    return result;
}

const GRID_SPACING: f32 = 24.0;

@fragment
fn fs_grid(v: GridOut) -> @location(0) vec4<f32> {
    let px = px_per_world();
    let spacing_px = GRID_SPACING * px;
    // Fade the dots out as they get dense, instead of drawing noise.
    let fade = clamp((spacing_px - 8.0 * g.scale) / (8.0 * g.scale), 0.0, 1.0);
    if (fade <= 0.0) {
        discard;
    }
    let cell = (v.world / GRID_SPACING - round(v.world / GRID_SPACING)) * spacing_px;
    let d = length(cell);
    let radius = 0.9 * g.scale;
    let coverage = clamp(radius + 0.5 - d, 0.0, 1.0) * fade;
    if (coverage <= 0.0) {
        discard;
    }
    return premultiply(vec4<f32>(1.0, 1.0, 1.0, 0.09), coverage);
}
