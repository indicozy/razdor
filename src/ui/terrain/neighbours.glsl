#version 100
// The neighbours shader, one source for every variant (see `variant_text`): the `#define`s after the version line
// choose the blocks a variant holds, so a material carries only the code its draws use. Each block is kept as it
// was in the single text before (the same expressions, in the same order), so a variant's pixels are the old
// shader's pixels. The colour and blend-weight values use the precision the variant names: highp (the default) or
// mediump (option 4). Coordinates stay highp. The sharp sampling is the same as before.
//   LIVE_WATER  the live water («Живая вода», `water`): the flow, the ripple, the de-tiled water, the foam, the shore's
//               waves and the glints. Only in the draws of a live map's full pass and overlay.
//   WATER_CODE  the water stage (the water frame and its multiply) and the overlay's test for it: no water code in the
//               ground pass, which draws the land only.
//   OVERLAY_PASS the cache's overlay: the pixels with no water and no border fog are discarded (they keep the cached
//               ground), the others are drawn as the full shader draws them.
//   EDGE_MIST   the border's fog («Продолжать карту за краем»), drawn over the ground past the map's edges.
precision highp float;
varying highp vec2 uv;
uniform sampler2D Neighbours;
uniform sampler2D Atlas;
uniform sampler2D Water;
uniform float WaterOn;
uniform vec4 View;
uniform vec2 MapSize;
uniform vec2 CellPx;
uniform vec2 Tile;
uniform vec2 Slot;
uniform vec2 AtlasSize;
uniform vec2 AtlasScale;
uniform vec2 WaterSize;
uniform vec2 WaterScale;
// x: 1.0 when the ground goes on past the map's edges (Razdor's «Продолжать карту за краем»), else 0.0; yzw: the
// fog's colour there (the war fog's black). Ramp: the cells where the fog thickens to solid (x) and where it is solid
// (y), counted from the border. Clock: seconds.
uniform vec4 Edge;
uniform vec2 Ramp;
uniform float Clock;
// 1.0 in the cache's overlay pass (the overlay variant discards on it; the uniform is kept for the uniform list).
uniform float Overlay;
// Live water («Живая вода», `water`): 1.0 when on. Flow, one texel per cell (linear, so the shore is a soft edge): rg = the
// flow as (128 + 127 · flow) / 255, so the still water (128) is exactly zero; b = the river's drift speed; a = the water
// mask (255 on the water cells, 0 on land). See `flow_texels`.
// Sky: the sky's colour, which the water takes by its share of the mask.
uniform float LiveWater;
// The water's detail by the map's zoom (`water::lod_detail`): 1 whole, 0 zoomed out. It scales the ripple and the
// foam's lapping; zoomed out the water keeps its texture, colour and foam.
uniform float Detail;
// The shore (`water::bank_distance`, `wide_water`): red the water's distance from the bank in steps / 8, green 1 on
// the wide water (a lake, the sea), 0 on a river.
uniform sampler2D Bank;
// The sun's and the moon's glint (`water::glint`): rgb the light's colour, a the strength.
uniform vec4 Glint;
uniform sampler2D Flow;
uniform vec3 Sky;
// The water's noise texture (`water::noise_texels`): a wrapped 64×64 table of the lattice's hashes.
uniform sampler2D Noise;
// @SHARP_UV@

#if EDGE_MIST
float hash2(vec2 q) {
    return fract(sin(dot(q, vec2(127.1, 311.7))) * 43758.5453);
}

// The lattice repeats every 64 cells, so a drift that moves the noise by a whole number of periods
// over CLOCK_PERIOD comes back to itself: the Clock's wrap is seamless (see the tests).
float vnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    vec2 a = mod(i, 64.0);
    vec2 b = mod(i + 1.0, 64.0);
    return mix(mix(hash2(a), hash2(vec2(b.x, a.y)), u.x), mix(hash2(vec2(a.x, b.y)), hash2(b), u.x), u.y);
}
#endif

#if LIVE_WATER
// The water's value noise: the same lattice as `vnoise`, one texture fetch. The lookup sits on the lattice
// point's centre plus the smoothed fraction, so the hardware's bilinear step is the smooth interpolation.
float wnoise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return texture2D(Noise, (mod(i, 64.0) + u + 0.5) / 64.0).r;
}

// The ripple's offset of the water at `q` (the water's uv distortion, in frame units).
vec2 ripple(vec2 q) {
    return vec2(sin(q.y * 2.1 + Clock * 1.29983) + 0.5 * wnoise(q * 0.7 + Clock * 0.2), cos(q.x * 1.7 + Clock * 1.09956));
}

// The cell's random byte (live water's de-tiling): the noise table's texel at the cell, wrapped, 0..255
// (`water::hash2_ref` at the lattice point, as `water::detile_corner` reads it).
float cell_byte(vec2 c) {
    return floor(texture2D(Noise, (mod(c, 64.0) + 0.5) / 64.0).r * 255.0 + 0.5);
}

// One cell centre's share of the water frame at uv `uvp` (of the pixel, with its flow and ripple): the uv moved by
// the cell's offset (a fraction of the frame, from the byte's golden-ratio steps), and mirrored on still water (a
// mirror would reverse a river's drift, so a flowing cell keeps its direction). `w` is the cell's weight.
vec3 water_corner(float n, float w, vec2 uvp, float still) {
    vec2 off = fract(n * vec2(0.618034, 0.754878));
    float mir = step(0.5, fract(n * 0.381966)) * step(0.5, still);
    vec2 uvc = mix(uvp, -uvp, mir) + off;
    return w * texture2D(Water, sharp_uv(uvc, WaterSize, WaterScale)).rgb;
}

// The water frame at uv `uvp`, de-tiled (live water, «Живая вода» only): four samples, one per cell centre round the
// pixel, each with its own cell's offset, blended by the smooth partition `wgt` of the cells (the same centres as the
// ground's blend). A cell's pattern fades into its neighbour's across the boundary, so no seam shows and the frame's
// 3-cell tile does not repeat. `nb` holds the four cells' bytes (x: the cell at floor(p), y: +x, z: +y, w: +x+y).
vec3 water_detiled(vec4 nb, vec4 wgt, vec2 uvp, float still) {
    return water_corner(nb.x, wgt.x, uvp, still) + water_corner(nb.y, wgt.y, uvp, still)
        + water_corner(nb.z, wgt.z, uvp, still) + water_corner(nb.w, wgt.w, uvp, still);
}

// The shore foam at a water pixel: `cov` is the water's share of the pixel's four cells (0.5 on the waterline, 1
// deep), `p` the cell coordinate and `fl` the flow there; s = cov - 0.5 is the distance into the water, in cells.
// The main line is near-white at the waterline, full for a few hundredths of a cell and fading by a third of a cell into the water; its inner
// edge laps between the waterline and a tenth of a cell out (a wave of about 6.4 s, its phase varying along the
// shore), and a noise breaks it into patches. A fainter second line lies further out, on a wave of its own. A still
// shore (a lake, the sea) foams at full strength, a river at 0.55. The periods fit CLOCK_PERIOD (6400 s): the wave is
// 1000 cycles of it, and the drifts move the noise by whole lattice periods. Returns the foam's colour and alpha.
// The foam patch's noise at one cell centre (`n` its byte, `w` its weight): the two octaves at the pixel's flow
// coordinates, moved by the cell's offset in the noise (a cell's patches differ from its neighbour's, and the
// partition blends them across the boundary, so no seam shows).
float foam_patch_at(float n, float w, vec2 q0, vec2 q1) {
    vec2 oc = fract(n * vec2(0.618034, 0.754878)) * 64.0;
    return w * (0.6 * wnoise(q0 * 2.4 + Clock * vec2(0.01, 0.0) + oc) + 0.4 * wnoise(q1 * 5.1 + Clock * vec2(0.0, -0.02) + oc.yx));
}

vec4 shore_foam(float cov, vec2 p, vec2 fl, vec2 dr, vec4 nb, vec4 wgt) {
    float s = cov - 0.5;
    if (s <= 0.0) {
        return vec4(0.0);
    }
    float t0 = fract(Clock * 0.05);
    float t1 = fract(Clock * 0.05 + 0.5);
    vec2 q0 = p - dr * t0 * 7.0;
    vec2 q1 = p - dr * t1 * 7.0;
    // Zoomed out (Detail 0) the line stands at its middle phase: the lapping is the moving detail.
    float lap = mix(0.5, 0.5 + 0.5 * sin(Clock * 0.98174770 + 6.0 * wnoise(q0 * 0.8)), Detail);
    float lap2 = mix(0.5, 0.5 + 0.5 * sin(Clock * 0.98174770 + 2.5 + 6.0 * wnoise(q1 * 0.6)), Detail);
    // De-tiled (live water): the patch noise is the four cells' own noise, blended by the cell weights, then the step.
    float raw = foam_patch_at(nb.x, wgt.x, q0, q1) + foam_patch_at(nb.y, wgt.y, q0, q1)
        + foam_patch_at(nb.z, wgt.z, q0, q1) + foam_patch_at(nb.w, wgt.w, q0, q1);
    float patch = smoothstep(0.38, 0.62, raw);
    float inner = 0.1 * lap;
    float line = smoothstep(inner - 0.015, inner + 0.015, s) * (1.0 - smoothstep(inner + 0.08, inner + 0.33, s));
    float inner2 = 0.36 + 0.05 * lap2;
    float far = smoothstep(inner2 - 0.01, inner2 + 0.01, s) * (1.0 - smoothstep(inner2 + 0.03, inner2 + 0.07, s));
    float still = 0.55 + 0.45 * (1.0 - smoothstep(0.2, 0.8, length(fl)));
    float a1 = 0.85 * line * (0.3 + 0.7 * patch) * still;
    float a2 = 0.3 * far * (0.2 + 0.8 * patch) * still;
    return vec4(0.97, 0.98, 1.0, a1 + a2 - a1 * a2);
}
#endif

@CP@ vec3 ground(float code, vec2 px, @CP@ vec3 water, @CP@ float sea) {
    vec2 slot = vec2(mod(code, 4.0), floor(code / 4.0));
    vec2 t = mod(px, Tile);
    vec2 atlas_uv = (slot * Slot + (Slot - Tile) * 0.5 + t) / AtlasSize;
    @CP@ vec3 c = texture2D(Atlas, sharp_uv(atlas_uv, AtlasSize, AtlasScale)).rgb;
#if WATER_CODE
    if (code < 2.5 && WaterOn > 0.5) {
        c = min(c * water * 2.0, 1.0);
#if LIVE_WATER
        // Live water: the sky's colour lies on the water by its share of the mask (the foam is laid on the colour below).
        c = mix(c, Sky, 0.18 * sea);
#endif
    }
#endif
    return c;
}

void main() {
    vec2 p = mix(View.xy, View.zw, uv);
    vec2 i = floor(p);
    vec2 f = p - i;
    vec2 px = p * CellPx;
#if EDGE_MIST
    // Past the border (p in cell units, the border at -0.5 and MapSize - 0.5): the war fog.
    vec2 gap = max(max(vec2(-0.5) - p, p - (MapSize - 0.5)), vec2(0.0));
    float d = length(gap);
    bool mist = Edge.x > 0.5 && d > 0.0;
#else
    const bool mist = false;
#endif
    // The four codes of the cell: code(i), code(i + (1, 0)), code(i + (0, 1)), code(i + (1, 1)),
    // each clamped to the map as `code_at` does. The texel of cell i (clamped to one cell past
    // the border, see `neighbour_texels`) holds them; its channels are code · 17 / 255.
    vec4 k = floor(texture2D(Neighbours, (clamp(i, vec2(-1.0), MapSize) + 1.5) / (MapSize + 2.0)) * 15.0 + 0.5);
#if WATER_CODE
    bool wet = WaterOn > 0.5 && any(lessThan(k, vec4(2.5)));
#if OVERLAY_PASS
    if (!wet && !mist) {
        discard;
    }
#endif
#endif
    @CP@ float fx = f.x;
    @CP@ float fy = f.y;
#if WATER_CODE
    // The water stage's coordinates (as the old shader): read only where a code is water.
    @CP@ vec3 w = vec3(1.0);
    // Live water: this pixel's mask (1 deep, 0.5 on the shore line, 0 on land) and its ripple, the same for each
    // of the four codes. The ripple reads the noise along the flow (a river drifts; zero flow stands still).
    @CP@ float sea = 0.0;
#if LIVE_WATER
    // The flow, the de-tiling's cell weights and bytes (set below, read by the foam).
    vec2 fl = vec2(0.0);
    vec2 dr = vec2(0.0);
    vec4 nb = vec4(0.0);
    vec4 wgt = vec4(0.0);
#endif
    if (wet) {
#if LIVE_WATER
        vec2 cell_uv = (p + 0.5) / MapSize;
        vec4 fd = texture2D(Flow, cell_uv);
        sea = fd.a;
        // The flow: its direction (rg) and the water's speed (b: 1 next to a bank, slower as a river widens, 0 on a
        // lake or the sea). The drift is the flow times the speed.
        fl = (fd.rg * 255.0 - 128.0) / 127.0;
        dr = fl * fd.b;
        // The flow scroll: two phases half a cycle apart (a 20 s cycle), each faded out at its own wrap, so the
        // pattern never jumps when Clock wraps. The surface moves 6 cells a cycle at full speed (two of its 3-cell
        // tiles, so the two phases show the same tile pattern and the crossfade does not ghost at full speed).
        float t0 = fract(Clock * 0.05);
        float t1 = fract(Clock * 0.05 + 0.5);
        float wt = abs(2.0 * t0 - 1.0);
        vec2 q0 = p - dr * t0 * 7.0;
        vec2 q1 = p - dr * t1 * 7.0;
        vec2 u0 = (p - dr * t0 * 6.0 + 1.5) / 3.0 + 0.015 * Detail * ripple(q0);
        vec2 u1 = (p - dr * t1 * 6.0 + 1.5) / 3.0 + 0.015 * Detail * ripple(q1);
        // De-tiled (`water_detiled`): the four cell centres round the pixel, each with its own offset, blended by
        // the smooth partition of the cells (the ground's centres: whole numbers).
        vec2 bc = floor(p);
        vec2 g = p - bc;
        vec2 sw = g * g * (3.0 - 2.0 * g);
        wgt = vec4((1.0 - sw.x) * (1.0 - sw.y), sw.x * (1.0 - sw.y), (1.0 - sw.x) * sw.y, sw.x * sw.y);
        nb = vec4(cell_byte(bc), cell_byte(bc + vec2(1.0, 0.0)), cell_byte(bc + vec2(0.0, 1.0)), cell_byte(bc + vec2(1.0, 1.0)));
        float stillw = 1.0 - smoothstep(0.2, 0.8, length(fl));
        w = mix(water_detiled(nb, wgt, u0, stillw), water_detiled(nb, wgt, u1, stillw), wt);
        // The low-frequency variation: a slow large-scale noise (its drift whole lattice periods per CLOCK_PERIOD),
        // a few per cent brighter or darker and a little warmer or cooler across the water.
        float lf = wnoise(p * 0.04 + Clock * vec2(0.01, 0.02));
        float lh = wnoise(p * 0.03 + vec2(31.0, 7.0) + Clock * vec2(-0.01, 0.01));
        w *= vec3(1.0 + 0.10 * (lh - 0.5), 1.0 + 0.16 * (lf - 0.5), 1.0 - 0.10 * (lh - 0.5));
#else
        vec2 uvw = (p + 1.5) / 3.0;
        w = texture2D(Water, sharp_uv(uvw, WaterSize, WaterScale)).rgb;
#endif
    }
#else
    const vec3 w = vec3(1.0);
    const float sea = 0.0;
#endif
    @CP@ vec3 col;
    if (k.x == k.y && k.y == k.z && k.z == k.w) {
        col = ground(k.x, px, w, sea);
    } else {
        @CP@ vec3 top = mix(ground(k.x, px, w, sea), ground(k.y, px, w, sea), fx);
        @CP@ vec3 bottom = mix(ground(k.z, px, w, sea), ground(k.w, px, w, sea), fx);
        col = mix(top, bottom, fy);
    }
#if LIVE_WATER
    // The shore foam (live water, the water's pixels only): laid on the coast by the water's share of the four
    // cells, with the weights the colour blend uses, so it sits on the visible shore and not on the mask's edge.
    if (wet) {
        float cov = (k.x < 2.5 ? 1.0 : 0.0) * (1.0 - fx) * (1.0 - fy) + (k.y < 2.5 ? 1.0 : 0.0) * fx * (1.0 - fy)
            + (k.z < 2.5 ? 1.0 : 0.0) * (1.0 - fx) * fy + (k.w < 2.5 ? 1.0 : 0.0) * fx * fy;
        vec4 fo = shore_foam(cov, p, fl, dr, nb, wgt);
        col = mix(col, fo.rgb, fo.a);
        vec4 bk = texture2D(Bank, (p + 0.5) / MapSize);
        float wide = step(0.5, bk.g);
        float still = 1.0 - smoothstep(0.2, 0.8, length(fl));
        // (c) The shore waves: soft lines rolling from the open water onto the beach, on the wide water only (a
        // lake's or the sea's shore), fading into the foam at the waterline and out by 1.4 cells. A crest moves
        // 1.1 cells a 4 s cycle toward the land (Clock · 0.25 wraps at a whole number of cycles, as CLOCK_PERIOD does).
        float s = bk.r * 8.0 - 0.5;
        float f = fract(s / 1.1 + Clock * 0.25 + 0.6 * sin(p.x * 0.9 + p.y * 0.5));
        float crest = smoothstep(0.0, 0.15, f) * (1.0 - smoothstep(0.15, 0.4, f));
        float near = smoothstep(0.1, 0.3, s) * (1.0 - smoothstep(0.9, 1.4, s));
        col = mix(col, vec3(0.92, 0.96, 1.0), 0.3 * crest * near * wide * still * Detail);
        // (b) The glints: small points that twinkle and drift, brighter on the wide water than on a river. The drift
        // moves the noise by whole lattice periods per CLOCK_PERIOD (5760 and 2240 of 64), and the twinkle is 2000 cycles
        // of it (1.9634954 rad/s), so the wrap is invisible. The mist and the war fog cover them after.
        float g = wnoise(p * 6.0 + Clock * vec2(0.9, 0.35));
        float tw = 0.5 + 0.5 * sin(Clock * 1.9634954 + 40.0 * g);
        float glint = smoothstep(0.87, 0.95, g) * tw * (0.4 + 0.6 * wide) * still * Detail;
        col = min(col + Glint.rgb * (glint * Glint.a * 0.7), vec3(1.0));
    }
#endif
#if EDGE_MIST
    if (mist) {
        float n = 0.6 * vnoise(p * 0.11 + Clock * vec2(0.05, 0.02)) + 0.4 * vnoise(p * 0.27 + Clock * vec2(-0.08, 0.05));
        float o = smoothstep(0.0, Ramp.y, d) * (0.35 + 0.65 * n);
        o = mix(o, 1.0, smoothstep(Ramp.x, Ramp.y, d));
        col = mix(col, Edge.yzw, clamp(o, 0.0, 1.0));
    }
#endif
    gl_FragColor = vec4(col, 1.0);
}
