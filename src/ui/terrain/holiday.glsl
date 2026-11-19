// «Новогодний вид» (the holiday look, `holiday.rs`): the ground's snow and the water's ice. One text for both terrain
// shaders: the holiday placeholder line of each is replaced by it (`variant_text`, and the old `FRAGMENT`). The shader that
// includes it declares `Holiday`, `HolidayCheap` and `SnowByCode` (a 16×1 texture: red the snow share of each code,
// `snow_amount`).

// The holiday look's noise of the cells: a smooth value noise of world position `q` (in cells), 0 to 1, from a hash
// (no texture), so it stands still with the map.
float snow_hash(vec2 q) {
    return fract(sin(dot(q, vec2(127.1, 311.7))) * 43758.5453);
}

float snow_noise(vec2 q) {
    vec2 i = floor(q);
    vec2 f = fract(q);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(snow_hash(i), snow_hash(i + vec2(1.0, 0.0)), u.x), mix(snow_hash(i + vec2(0.0, 1.0)), snow_hash(i + vec2(1.0, 1.0)), u.x), u.y);
}
const vec3 HOLIDAY_SNOW = vec3(0.93, 0.95, 1.0);
const vec3 HOLIDAY_ICE = vec3(0.75, 0.85, 0.95);

// The snow share of one code (0 to 1).
float snow_of(float code) {
    return texture2D(SnowByCode, (vec2(code, 0.0) + 0.5) / vec2(16.0, 1.0)).r;
}

// The ground's snow share at the pixel: the four cells' shares blended by the same fractions as the colour, so the soft
// edge between a snowy and a bare cell carries the snow too (no square edge).
float snow_blend(vec4 k, float fx, float fy) {
    return mix(mix(snow_of(k.x), snow_of(k.y), fx), mix(snow_of(k.z), snow_of(k.w), fx), fy);
}

// The water's share at the pixel (codes 0 to 2), blended as the colour is.
float water_blend(vec4 k, float fx, float fy) {
    vec4 wv = vec4(1.0) - step(vec4(2.5), k);
    return mix(mix(wv.x, wv.y, fx), mix(wv.z, wv.w, fx), fy);
}

// The ground colour `col` under the snow. `bare` (0 to 1) keeps a share of the pixel bare (a winding path's road). Full
// look: a noise of the cells varies the snow by 75 to 100 per cent, and the darks (luma under about 0.15: the tree gaps,
// the shadows) keep 85 per cent of the ground. Cheap look (`HolidayCheap`): a flat mix by the shares, no noise, no dark keep.
vec3 holiday_ground(vec3 col, vec4 k, float fx, float fy, vec2 p, float bare) {
    if (Holiday < 0.5) {
        return col;
    }
    float sa = snow_blend(k, fx, fy) * (1.0 - bare);
    if (sa <= 0.0) {
        return col;
    }
    if (HolidayCheap > 0.5) {
        return mix(col, HOLIDAY_SNOW, clamp(sa, 0.0, 1.0));
    }
    sa *= 0.75 + 0.25 * snow_noise(p * 0.35);
    float luma = dot(col, vec3(0.299, 0.587, 0.114));
    sa *= mix(0.15, 1.0, smoothstep(0.12, 0.18, luma));
    return mix(col, HOLIDAY_SNOW, clamp(sa, 0.0, 1.0));
}

// The water's colour `col` under the ice, by the water's share `wsh`: desaturated by 60 per cent, then lifted toward the
// pale blue by half.
vec3 holiday_ice(vec3 col, float wsh) {
    float luma = dot(col, vec3(0.299, 0.587, 0.114));
    vec3 ice = mix(mix(col, vec3(luma), 0.6), HOLIDAY_ICE, 0.5);
    return mix(col, ice, wsh);
}
