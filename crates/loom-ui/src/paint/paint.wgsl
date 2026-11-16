// LoomDSP UI layer: the shaders that draw painted boxes and glyphs
// (`paint/gpu.rs`), and the parts a host's own shaders build on. A host
// that draws more puts its own stages after this source in one module, so
// what is shared here exists once.
//
// Naming: wgpu translates this file to Metal, HLSL, and SPIR-V, and does not
// rename every identifier that is reserved there. Avoid names that are types
// or keywords in Metal or HLSL, such as `half`, `in`, `out`,
// `vector`, `matrix`, `sample`, or `texture`.
//
// Cost model: every box and every glyph is one small quad, and its fragment
// shader runs only inside that quad. Nothing here runs over the whole
// target.

struct Globals {
    // The host's point at the viewport's top-left corner, in its own units
    // (world units for a canvas, logical pixels for painted boxes).
    origin: vec2<f32>,
    // Viewport size in logical pixels.
    size: vec2<f32>,
    // Logical pixels per host unit: 1 for painted boxes.
    zoom: f32,
    // Physical pixels per logical pixel (2.0 on Retina).
    scale: f32,
    // A host's own value for its own stages; painted boxes and glyphs do not
    // read it. The canvas keeps the opacity of cables out of focus here.
    dim: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> g: Globals;

fn to_clip(world: vec2<f32>) -> vec4<f32> {
    let s = (world - g.origin) * g.zoom;
    return vec4<f32>(s.x / g.size.x * 2.0 - 1.0, 1.0 - s.y / g.size.y * 2.0, 0.0, 1.0);
}

// Physical pixels per host unit.
fn px_per_world() -> f32 {
    return g.zoom * g.scale;
}

fn premultiply(c: vec4<f32>, coverage: f32) -> vec4<f32> {
    let a = c.a * coverage;
    return vec4<f32>(c.rgb * a, a);
}

// ---------------------------------------------------------------------------
// Rounded rectangles: painted boxes, and a host's own shapes.

struct ShapeIn {
    // x, y, width, height in host units.
    @location(0) rect: vec4<f32>,
    @location(1) fill: vec4<f32>,
    @location(2) border: vec4<f32>,
    // corner radius and border width (host units), minimum border (physical px), unused.
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
// Glyphs, from the atlas pages (`atlas.rs`).
//
// Each glyph is one quad on whole physical pixels, so a texel of the page
// is one pixel of the target and the pages are read without a sampler.
// Drawn with the screen globals: no zoom, positions in logical pixels
// from the viewport's corner.

@group(1) @binding(0) var glyph_coverage: texture_2d<f32>;
@group(1) @binding(1) var glyph_colours: texture_2d<f32>;

struct GlyphIn {
    // x, y, width, height in logical pixels from the viewport's corner.
    @location(0) rect: vec4<f32>,
    // The image's corners in its page, in texels: left, top, right, bottom.
    @location(1) uv: vec4<f32>,
    // The colour of a coverage glyph's letters.
    @location(2) colour: vec4<f32>,
    // x: 1 for a colour glyph, read from the colour page.
    @location(3) params: vec4<f32>,
};

struct GlyphOut {
    @builtin(position) position: vec4<f32>,
    @location(0) texel: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) kind: f32,
};

@vertex
fn vs_glyph(@builtin(vertex_index) vi: u32, glyph: GlyphIn) -> GlyphOut {
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let p = glyph.rect.xy + corner * glyph.rect.zw;
    var result: GlyphOut;
    result.position = vec4<f32>(p.x / g.size.x * 2.0 - 1.0, 1.0 - p.y / g.size.y * 2.0, 0.0, 1.0);
    result.texel = mix(glyph.uv.xy, glyph.uv.zw, corner);
    result.colour = glyph.colour;
    result.kind = glyph.params.x;
    return result;
}

@fragment
fn fs_glyph(v: GlyphOut) -> @location(0) vec4<f32> {
    let at = vec2<i32>(floor(v.texel));
    let coverage = textureLoad(glyph_coverage, at, 0).r;
    let colour = textureLoad(glyph_colours, at, 0);
    if (v.kind > 0.5) {
        // Premultiplied already; the run's alpha fades it as a whole.
        return colour * v.colour.a;
    }
    return premultiply(v.colour, coverage);
}
