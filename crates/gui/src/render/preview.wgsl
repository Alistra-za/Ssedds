// The preview pass (M3a spec §6.1): one texture, one explicit mip, drawn into the rect egui sets as the viewport.
struct Params {
    uv_offset: vec2<f32>,
    uv_scale: vec2<f32>,
    mip: f32,
    // 0 RGBA, 1 R, 2 G, 3 B, 4 A, 5 alpha as a mask over a checkerboard.
    channel: u32,
    // 1: the stored values are linear data; re-encode to sRGB for display.
    view_linear: u32,
    // 1: Reinhard tone map (BC6H).
    tonemap: u32,
    // How the texture's channels map to colour. 0: as sampled. 1: BC4 (the R channel is grey, as the CPU decode
    // makes it). 2: BC5 (a normal map: blue is rebuilt from red and green).
    kind: u32,
    // What an HDR value is multiplied by before the tone map: 2^EV.
    exposure: f32,
    // 1: the picture repeats 3x3 (uv -1..2 wraps into 0..1).
    tile: u32,
    // 1: the magnifier lens: only the circle inscribed in the rect is drawn.
    lens: u32,
};

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle covering the viewport egui sets to the callback rect.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(xy * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(xy.x, 1.0 - xy.y) * params.uv_scale + params.uv_offset;
    return o;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if (params.lens == 1u) {
        let d = (in.uv - (params.uv_offset + params.uv_scale * 0.5)) / (params.uv_scale * 0.5);
        if (dot(d, d) > 1.0) {
            return vec4<f32>(0.0, 0.0, 0.0, 0.0);
        }
    }
    var uv = in.uv;
    if (params.tile == 1u) {
        // The same rule as `model::view::tile_uv`.
        if (any(uv < vec2<f32>(-1.0)) || any(uv >= vec2<f32>(2.0))) {
            return vec4<f32>(0.0, 0.0, 0.0, 0.0);
        }
        uv = fract(uv);
    }
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    var c = textureSampleLevel(tex, samp, uv, params.mip);
    if (params.kind == 1u) {
        c = vec4<f32>(c.r, c.r, c.r, 1.0);
    } else if (params.kind == 2u) {
        let xy = c.rg * 2.0 - 1.0;
        let z = sqrt(max(0.0, 1.0 - dot(xy, xy)));
        c = vec4<f32>(c.rg, z * 0.5 + 0.5, 1.0);
    }
    if (params.tonemap == 1u) {
        let e = c.rgb * params.exposure;
        c = vec4<f32>(linear_to_srgb(e / (vec3<f32>(1.0) + e)), 1.0);
    }
    var rgb = c.rgb;
    switch params.channel {
        case 1u: { rgb = vec3<f32>(c.r); }
        case 2u: { rgb = vec3<f32>(c.g); }
        case 3u: { rgb = vec3<f32>(c.b); }
        case 4u: { rgb = vec3<f32>(c.a); }
        case 5u: {
            let cell = (u32(in.pos.x / 8.0) + u32(in.pos.y / 8.0)) % 2u;
            let checker = select(vec3<f32>(0.35), vec3<f32>(0.65), cell == 1u);
            rgb = mix(checker, c.rgb, c.a);
        }
        default: {}
    }
    if (params.view_linear == 1u) {
        rgb = linear_to_srgb(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
