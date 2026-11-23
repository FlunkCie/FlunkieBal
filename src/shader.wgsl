// FlunkieBal scene shader: one uber-shader, the instance's `params.x` picks the material.
// 0 lit · 1 unlit · 2 soft disc (smoke, glow) · 3 lawn · 4 referee gaze fan · 5 sky · 6 water · 7 volleyball
// 8 wood · 9 pavers · 10 dance floor · 11 facades · 12 brick · 13 bark · 14 leaves · 15 grass blades (vs_grass) · 16 plaster · 17 asphalt · 18 light beam
//
// Lighting is physically based (GGX specular, Fresnel, energy-conserving diffuse) with a sun, up to six
// lamps with inverse-square falloff, a hemisphere/bounce ambient with reflections, soft shadows from
// the sun or from a bare bulb, and corner darkening inside rooms.

struct Globals {
  view_proj: mat4x4<f32>,
  light_vp: mat4x4<f32>,
  cam_pos: vec4<f32>,
  sun_dir: vec4<f32>,   // w > 0: shadows come from lamp 0 (perspective, w = near plane)
  sun_col: vec4<f32>,
  sky_top: vec4<f32>,
  sky_hor: vec4<f32>,
  ground_col: vec4<f32>,
  fog: vec4<f32>,
  misc: vec4<f32>,      // time, shadow texel, exposure, ambient strength
  grade: vec4<f32>,     // contrast, saturation, warmth, _
  lp: array<vec4<f32>, 6>, // lamp position, range
  lc: array<vec4<f32>, 6>, // lamp colour × intensity
  rooms: array<vec4<f32>, 4>, // (min, max) × 2; max.w = 1 when used
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_smp: sampler_comparison;

const PI: f32 = 3.14159265;

struct VIn {
  @location(0) pos: vec3<f32>,
  @location(1) nrm: vec3<f32>,
  @location(2) uv: vec2<f32>,
  @location(3) m0: vec4<f32>,
  @location(4) m1: vec4<f32>,
  @location(5) m2: vec4<f32>,
  @location(6) m3: vec4<f32>,
  @location(7) color: vec4<f32>,
  @location(8) params: vec4<f32>,
};
struct VOut {
  @builtin(position) clip: vec4<f32>,
  @location(0) wpos: vec3<f32>,
  @location(1) nrm: vec3<f32>,
  @location(2) uv: vec2<f32>,
  @location(3) color: vec4<f32>,
  @location(4) @interpolate(flat) params: vec4<f32>,
};

// ---------- noise ----------
fn hash2(p: vec2<f32>) -> f32 {
  let q = fract(p * vec2<f32>(127.1, 311.7));
  return fract(sin(dot(q, vec2<f32>(12.9898, 78.233)) + q.x * 3.1) * 43758.5453);
}
fn vnoise(p: vec2<f32>) -> f32 {
  let i = floor(p); let f = fract(p);
  let u = f * f * (3.0 - 2.0 * f);
  let a = hash2(i); let b = hash2(i + vec2<f32>(1.0, 0.0));
  let c = hash2(i + vec2<f32>(0.0, 1.0)); let d = hash2(i + vec2<f32>(1.0, 1.0));
  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
fn fbm(p: vec2<f32>) -> f32 {
  return vnoise(p) * 0.5 + vnoise(p * 2.03 + vec2<f32>(5.2, 1.3)) * 0.3 + vnoise(p * 4.1 + vec2<f32>(2.7, 8.1)) * 0.2;
}

// ---------- the lawn (shared by the ground and the blades growing on it) ----------
fn pitch_wear(p: vec2<f32>) -> f32 {
  let n2 = vnoise(p * 2.1 + vec2<f32>(7.0, 3.0));
  let n3 = vnoise(p * 9.0);
  let dz = abs(abs(p.y) - 6.45);
  return max(exp(-dot(p, p) * 0.9), exp(-dz * dz * 3.0) * smoothstep(3.6, 2.0, abs(p.x))) * smoothstep(0.25, 0.7, n2 + n3 * 0.4);
}
fn lawn_col(p: vec2<f32>) -> vec3<f32> {
  let n1 = fbm(p * 0.22);
  let n2 = vnoise(p * 1.7 + vec2<f32>(7.0, 3.0));
  var c = mix(vec3<f32>(0.075, 0.2, 0.035), vec3<f32>(0.2, 0.33, 0.06), n1);
  c = mix(c, vec3<f32>(0.28, 0.3, 0.1), smoothstep(0.6, 0.9, n2) * 0.35); // drier tufts
  c = c * (0.95 + 0.07 * step(0.5, fract(p.x * 0.18))); // mowing stripes
  return c;
}
fn path_x(z: f32) -> f32 { return -11.5 + sin(z * 0.07) * 3.0; }
/// How tall the grass grows here (`worn`: `pitch_wear` there): short where it is worn, gone on the chalk lines and the path.
fn grass_height(p: vec2<f32>, worn: f32) -> f32 {
  var h = (1.0 - worn * 0.95) * (0.65 + 0.7 * vnoise(p * 0.6));
  h = h * smoothstep(0.3, 0.55, length(p));
  h = h * smoothstep(1.25, 1.6, abs(p.x - path_x(p.y)));
  // none in the pond, on the cross path (slabs 7 m long) or under the picnic blankets
  let pond = (p - vec2<f32>(20.0, -22.0)) / vec2<f32>(11.9, 7.9);
  h = h * smoothstep(1.0, 1.06, dot(pond, pond));
  let xc = -8.0 + 7.0 * clamp(round((p.x + 8.0) / 7.0), 0.0, 9.0);
  let on_path = step(-11.7, p.x) * step(p.x, 55.7) * step(abs(p.y - 19.0 - sin(xc * 0.09) * 1.5), 1.12);
  h = h * (1.0 - on_path);
  h = h * step(1.05, length(p - vec2<f32>(14.0, 10.0))) * step(1.05, length(p - vec2<f32>(-6.0, -18.5))) * step(1.05, length(p - vec2<f32>(15.0, -9.0)));
  return h;
}
fn wind(p: vec2<f32>, t: f32) -> vec2<f32> {
  let gust = 0.6 + 0.4 * sin(t * 0.37 + p.x * 0.05);
  return vec2<f32>(sin(t * 1.9 + p.x * 0.9 + p.y * 0.5) + 0.5 * sin(t * 3.7 + p.x * 2.7 - p.y), cos(t * 1.5 + p.y * 0.8) * 0.6) * gust;
}

// ---------- vertices ----------
fn deform(v: VIn, m: mat4x4<f32>) -> vec4<f32> {
  let kind = i32(v.params.x + 0.5);
  let t = g.misc.x;
  var w = m * vec4<f32>(v.pos, 1.0);
  if (kind == 14) {
    let s = sin(t * 1.6 + w.x * 0.7 + w.y * 0.9) * 0.022 + sin(t * 4.3 + w.z * 3.1 + w.x) * 0.008;
    w.x = w.x + s; w.z = w.z + s * 0.6; w.y = w.y + s * 0.3;
  }
  return w;
}

@vertex
fn vs_main(v: VIn) -> VOut {
  let m = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
  let w = deform(v, m);
  let c0 = v.m0.xyz; let c1 = v.m1.xyz; let c2 = v.m2.xyz;
  let nm = mat3x3<f32>(cross(c1, c2), cross(c2, c0), cross(c0, c1)); // cofactor: right normals under non-uniform scale
  var o: VOut;
  o.clip = g.view_proj * w;
  o.wpos = w.xyz;
  o.nrm = nm * v.nrm;
  o.uv = v.uv;
  o.color = v.color;
  let kind = i32(v.params.x + 0.5);
  if (kind == 14) { o.color.a = length(v.pos); } // how deep in the clump: inner leaves get less light
  o.params = v.params;
  return o;
}

struct ShOut {
  @builtin(position) clip: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) @interpolate(flat) params: vec4<f32>,
};
@vertex
fn vs_shadow(v: VIn) -> ShOut {
  let m = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
  var o: ShOut;
  o.clip = g.light_vp * deform(v, m);
  o.uv = v.uv;
  o.params = v.params;
  return o;
}
fn leaf_mask(uv: vec2<f32>) -> f32 {
  let x = fract(uv.x) * 2.0 - 1.0;
  let y = uv.y;
  let w = pow(max(sin(PI * y), 0.0), 0.7) * (1.0 - 0.3 * y);
  return w - abs(x);
}
@fragment
fn fs_shadow(i: ShOut) {
  if (i32(i.params.x + 0.5) == 14 && leaf_mask(i.uv) < 0.0) { discard; }
}

// ---------- the park's plants and stones (`nature`) ----------
@group(1) @binding(0) var nat_tex: texture_2d_array<f32>;
@group(1) @binding(1) var nat_smp: sampler;
struct NIn {
  @location(0) pos: vec3<f32>,
  @location(1) nrm: vec3<f32>,
  @location(2) uv: vec2<f32>,
  @location(9) col: vec3<f32>,
  @location(10) ls: vec2<f32>, // texture layer, how much the wind moves it
  @location(3) m0: vec4<f32>,
  @location(4) m1: vec4<f32>,
  @location(5) m2: vec4<f32>,
  @location(6) m3: vec4<f32>,
  @location(7) color: vec4<f32>,
  @location(8) params: vec4<f32>,
};
fn nature_pos(v: NIn) -> vec4<f32> {
  let m = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
  var w = m * vec4<f32>(v.pos, 1.0);
  // the wind: more the higher up a leaf or blade is; each plant a little out of step
  let wv = wind(w.xz, g.misc.x + v.params.w) * v.ls.y * 0.18 * length(v.m1.xyz);
  w = vec4<f32>(w.x + wv.x, w.y, w.z + wv.y, w.w);
  return w;
}
@vertex
fn vs_nature(v: NIn) -> VOut {
  let w = nature_pos(v);
  let c0 = v.m0.xyz; let c1 = v.m1.xyz; let c2 = v.m2.xyz;
  let nm = mat3x3<f32>(cross(c1, c2), cross(c2, c0), cross(c0, c1));
  var o: VOut;
  o.clip = g.view_proj * w;
  o.wpos = w.xyz;
  o.nrm = nm * v.nrm;
  o.uv = v.uv;
  o.color = vec4<f32>(v.col * v.color.rgb, 1.0);
  o.params = vec4<f32>(v.ls.x, v.ls.y, 0.0, 0.0);
  return o;
}
@fragment
fn fs_nature(i: VOut, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {
  let tx = textureSample(nat_tex, nat_smp, i.uv, i32(i.params.x + 0.5));
  if (tx.a < 0.5) { discard; }
  var n = normalize(i.nrm);
  if (!ff) { n = -n; }
  let p = i.wpos;
  let base = tx.rgb * i.color.rgb;
  // leaves and blades let some light through from behind
  let trans = select(0.0, 0.35, i.params.y > 0.0 || i32(i.params.x + 0.5) == 2 || i32(i.params.x + 0.5) == 3);
  let col = shade(Surf(base, n, 0.85, 0.03, room_ao(p, n), trans), p);
  return finish(fogged(col, p), 1.0);
}
struct NShOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) @interpolate(flat) layer: f32 };
@vertex
fn vs_nature_shadow(v: NIn) -> NShOut {
  var o: NShOut;
  o.clip = g.light_vp * nature_pos(v);
  o.uv = v.uv;
  o.layer = v.ls.x;
  return o;
}
@fragment
fn fs_nature_shadow(i: NShOut) {
  if (textureSample(nat_tex, nat_smp, i.uv, i32(i.layer + 0.5)).a < 0.5) { discard; }
}

// ---------- skinned people ----------
struct Skin { bones: array<mat4x4<f32>, 32>, colors: array<vec4<f32>, 8>, misc: vec4<f32> };
@group(1) @binding(0) var<uniform> sk: Skin;
@group(1) @binding(1) var people_tex: texture_2d<f32>;
@group(1) @binding(2) var people_smp: sampler;
struct SIn {
  @location(0) pos: vec3<f32>,
  @location(1) nrm: vec3<f32>,
  @location(2) joints: vec4<f32>,
  @location(3) weights: vec4<f32>,
  @location(4) uv: vec2<f32>,
  @location(5) slot: f32,
};
fn skin_m(v: SIn) -> mat4x4<f32> {
  let j = vec4<u32>(v.joints + vec4<f32>(0.5));
  return sk.bones[j.x] * v.weights.x + sk.bones[j.y] * v.weights.y + sk.bones[j.z] * v.weights.z + sk.bones[j.w] * v.weights.w;
}
@vertex
fn vs_skin(v: SIn) -> VOut {
  let m = skin_m(v);
  let w = m * vec4<f32>(v.pos, 1.0);
  var o: VOut;
  o.clip = g.view_proj * w;
  o.wpos = w.xyz;
  o.nrm = (m * vec4<f32>(v.nrm, 0.0)).xyz;
  o.uv = v.uv;
  // color: the person's colour these texels may take (a = 1), see `fs_person`; on the referee's shirt
  // a is 2 + where across the stripes it is
  let slot = u32(v.slot + 0.5);
  o.color = vec4<f32>(1.0, 1.0, 1.0, 0.0);
  if (slot < 8u) { o.color = vec4<f32>(sk.colors[slot].rgb, 1.0); }
  if (slot == 0u && sk.misc.x > 0.0) { o.color.a = 2.0 + v.pos.x * sk.misc.x + 64.25; }
  // params.z: how rough (skin has a little shine)
  var rough = 0.75;
  if (slot == 1u) { rough = 0.5; }
  o.params = vec4<f32>(0.0, 0.0, rough, 0.0);
  return o;
}
// People: their texture, where its alpha says so tinted by one of their colours (the texel then holds a
// shade, 0.5 = the colour itself), lit like everything else, with a soft rim of sky light along their outline.
@fragment
fn fs_person(i: VOut) -> @location(0) vec4<f32> {
  let n = normalize(i.nrm);
  let p = i.wpos;
  let tx = textureSample(people_tex, people_smp, i.uv);
  var base = tx.rgb;
  if (i.color.a > 0.5) {
    var tint = i.color.rgb;
    if (i.color.a > 1.5 && fract(i.color.a) < 0.5) { tint = vec3<f32>(0.03, 0.03, 0.035); } // the referee's stripes
    base = mix(tx.rgb, tx.rgb * 2.0 * tint, tx.a);
  }
  var col = shade(Surf(base, n, i.params.z, 0.04, room_ao(p, n), 0.0), p);
  let v = normalize(g.cam_pos.xyz - p);
  let ndv = max(dot(n, v), 0.0);
  let sky = mix(g.ground_col.rgb, g.sky_top.rgb, n.y * 0.4 + 0.6);
  col = col + (base * 0.5 + vec3<f32>(0.5)) * sky * g.misc.w * pow(1.0 - ndv, 3.0) * 0.25;
  return finish(fogged(col, p), 1.0);
}
@vertex
fn vs_skin_shadow(v: SIn) -> @builtin(position) vec4<f32> {
  return g.light_vp * skin_m(v) * vec4<f32>(v.pos, 1.0);
}

// ---------- grass: every blade made up here from its number, no mesh ----------
// One instance is a tile of GRASS_TILE metres (its corner, in whole tiles, in m3.xz; color.x = how dense);
// vertex_index / 7 is the blade, % 7 the point on it: base pair, two joint pairs and the tip. A blade
// further off than its density reaches shrinks away, and those left widen to cover what the others did.
const GRASS_TILE: f32 = 1.0;
const GRASS_NB: f32 = 768.0;
const GRASS_D0: f32 = 5.0;
const GRASS_FAR: f32 = 30.0;
struct GIn {
  @builtin(vertex_index) vi: u32,
  @location(3) m0: vec4<f32>,
  @location(4) m1: vec4<f32>,
  @location(5) m2: vec4<f32>,
  @location(6) m3: vec4<f32>,
  @location(7) color: vec4<f32>,
  @location(8) params: vec4<f32>,
};
fn pcg(v: u32) -> u32 {
  let s = v * 747796405u + 2891336453u;
  let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
  return (w >> 22u) ^ w;
}
fn unit(h: u32) -> f32 { return f32(h >> 8u) / 16777216.0; }
@vertex
fn vs_grass(v: GIn) -> VOut {
  let blade = v.vi / 7u;
  let j = v.vi % 7u;
  let tile = vec2<i32>(i32(floor(v.m3.x + 0.5)), i32(floor(v.m3.z + 0.5)));
  var h0 = pcg(blade * 2654435769u ^ (u32(tile.x) * 73856093u) ^ (u32(tile.y) * 19349663u));
  let h1 = pcg(h0); let h2 = pcg(h1); let h3 = pcg(h2); let h4 = pcg(h3); let h5 = pcg(h4); let h6 = pcg(h5); let h7 = pcg(h6);
  let root2 = (vec2<f32>(tile) + vec2<f32>(unit(h0), unit(h1))) * GRASS_TILE;
  let root = vec3<f32>(root2.x, 0.0, root2.y);
  let d = distance(g.cam_pos.xyz, root);
  let n = GRASS_NB * v.color.x * min(1.0, pow(GRASS_D0 / max(d, 0.01), 1.5));
  let worn = pitch_wear(root2);
  // worn ground keeps only a few blades, and short ones
  let sparse = step(worn * 0.9, unit(pcg(h6 + 1u)));
  let fade = clamp(n - f32(blade), 0.0, 1.0) * smoothstep(GRASS_FAR, GRASS_FAR - 5.0, d) * sparse;
  let r2 = unit(h2);
  let hf = grass_height(root2, worn);
  let len = (0.05 + r2 * 0.08 + r2 * r2 * 0.05) * hf * fade;
  let wide = clamp(sqrt(GRASS_NB * v.color.x / max(n, 1.0)), 1.0, 3.5);
  let wid = (0.005 + unit(h3) * 0.004) * wide * min(fade * 4.0, 1.0);
  let yaw = unit(h4) * 6.2831853;
  let side = vec3<f32>(cos(yaw), 0.0, sin(yaw));
  let fwd = vec3<f32>(-sin(yaw), 0.0, cos(yaw));
  let bend = 0.1 + unit(h5) * 0.55;
  let row = j / 2u;
  var tt = array<f32, 4>(0.0, 0.4, 0.75, 1.0);
  var ww = array<f32, 4>(1.0, 0.9, 0.62, 0.0);
  let t = tt[row];
  let sd = select(-1.0, 1.0, (j & 1u) == 1u);
  // a breeze rolling over the lawn in waves; every blade gives way a little differently
  let time = g.misc.x;
  let wave = 0.55 + 0.45 * sin(dot(root2, vec2<f32>(0.31, 0.22)) - time * 1.7);
  let s = wind(root2, time) * (wave * 0.32 * len * t * t * (0.7 + 0.6 * unit(h6)));
  let p = root + vec3<f32>(0.0, len * t * (1.0 - 0.3 * bend * t), 0.0) + fwd * (bend * len * t * t) + side * (sd * wid * ww[row]) + vec3<f32>(s.x, 0.0, s.y);
  var o: VOut;
  o.clip = g.view_proj * vec4<f32>(p, 1.0);
  o.wpos = p;
  // mostly up, so the lawn reads as one surface, curling round the blade's edges
  o.nrm = normalize(fwd * 0.45 + side * (sd * ww[row] * 0.35) + vec3<f32>(0.0, 0.85, 0.0));
  let r7 = unit(h7);
  var c = lawn_col(root2) * (0.8 + 0.4 * r7);
  // a few blades dried out, more where the lawn is worn
  let dry = step(0.94 - worn * 0.3, unit(pcg(h7)));
  c = mix(c, vec3<f32>(0.36, 0.32, 0.13) * (0.7 + 0.5 * r7), dry * 0.75);
  o.color = vec4<f32>(c, 1.0);
  o.uv = vec2<f32>(r7, t);
  o.params = vec4<f32>(15.0, 0.0, 0.0, 0.0);
  return o;
}
@fragment
fn fs_grass(i: VOut, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {
  var n = normalize(i.nrm);
  if (!ff) { n = normalize(vec3<f32>(-n.x, n.y, -n.z)); }
  let h = i.uv.y;
  // darker down among the roots, lighter and a touch yellow at the tip
  var base = i.color.rgb * mix(0.55, 1.08, h);
  base = mix(base, base * vec3<f32>(1.25, 1.15, 0.65), smoothstep(0.75, 1.0, h) * (0.15 + 0.4 * i.uv.x));
  var col = shade(Surf(base, n, 0.6, 0.04, mix(0.55, 1.0, h), 0.45), i.wpos);
  col = fogged(col, i.wpos);
  return finish(col, 1.0);
}

// ---------- light ----------
fn shadow_at(wpos: vec3<f32>, n: vec3<f32>) -> f32 {
  if (g.misc.y <= 0.0) { return 1.0; } // shadows switched off
  let spot = g.sun_dir.w > 0.0;
  let lp = g.light_vp * vec4<f32>(wpos + n * select(0.03, 0.02, spot), 1.0);
  if (lp.w <= 0.0) { return 1.0; }
  let p = lp.xyz / lp.w;
  let uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
  if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || p.z > 1.0) { return 1.0; }
  var bias = 0.0012;
  var r = g.misc.y * 1.8;
  if (spot) { bias = g.sun_dir.w * 0.012 / (lp.w * lp.w); r = g.misc.y * 2.6; }
  let d = p.z - bias;
  // eight taps on a rotated disc: soft edges without a visible pattern
  let a = hash2(wpos.xz * 91.7 + wpos.y * 13.1) * 6.2831;
  let cs = vec2<f32>(cos(a), sin(a));
  let rot = mat2x2<f32>(cs.x, cs.y, -cs.y, cs.x);
  var s = 0.0;
  var k = array<vec2<f32>, 8>(vec2<f32>(-0.61, 0.21), vec2<f32>(0.41, -0.79), vec2<f32>(0.93, 0.32), vec2<f32>(-0.18, -0.48), vec2<f32>(0.17, 0.95), vec2<f32>(-0.95, -0.27), vec2<f32>(0.55, 0.43), vec2<f32>(-0.39, 0.68));
  for (var j = 0; j < 8; j = j + 1) {
    s = s + textureSampleCompareLevel(shadow_map, shadow_smp, uv + rot * k[j] * r, d);
  }
  // no hard line where the shadow map ends
  let edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
  return mix(1.0, s * 0.125, smoothstep(0.0, 0.06, edge));
}

/// Corners and edges of rooms get less light (a cheap stand-in for ambient occlusion).
fn room_ao(p: vec3<f32>, n: vec3<f32>) -> f32 {
  var ao = 1.0;
  for (var i = 0; i < 2; i = i + 1) {
    let mx = g.rooms[i * 2 + 1];
    if (mx.w < 0.5) { continue; }
    let mn = g.rooms[i * 2].xyz;
    if (any(p < mn - vec3<f32>(0.25)) || any(p > mx.xyz + vec3<f32>(0.25))) { continue; }
    let d = max(min(p - mn, mx.xyz - p), vec3<f32>(0.0));
    let f = mix(vec3<f32>(0.5), vec3<f32>(1.0), smoothstep(vec3<f32>(0.0), vec3<f32>(1.4), d));
    let w = vec3<f32>(1.0) - abs(n);
    ao = ao * mix(1.0, f.x, w.x) * mix(1.0, f.y, w.y) * mix(1.0, f.z, w.z);
  }
  return ao;
}

fn brdf(base: vec3<f32>, n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, rough: f32, f0: vec3<f32>) -> vec3<f32> {
  let ndl = max(dot(n, l), 0.0);
  if (ndl <= 0.0) { return vec3<f32>(0.0); }
  let h = normalize(l + v);
  let ndh = max(dot(n, h), 0.0);
  let ndv = max(dot(n, v), 0.001);
  let vdh = max(dot(v, h), 0.0);
  let a = rough * rough;
  let a2 = a * a;
  let dd = ndh * ndh * (a2 - 1.0) + 1.0;
  let D = a2 / (PI * dd * dd);
  let k = (rough + 1.0) * (rough + 1.0) / 8.0;
  let G = ndl / (ndl * (1.0 - k) + k) * ndv / (ndv * (1.0 - k) + k);
  let F = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - vdh, 5.0);
  let spec = D * G * F / (4.0 * ndl * ndv + 0.0001);
  // diffuse kept at albedo scale (light colours are authored that way), specular scaled to match
  return (base * (vec3<f32>(1.0) - F) + min(spec * PI, vec3<f32>(40.0))) * ndl;
}

struct Surf { base: vec3<f32>, n: vec3<f32>, rough: f32, f0: f32, ao: f32, trans: f32 };

fn shade(s: Surf, wpos: vec3<f32>) -> vec3<f32> {
  let v = normalize(g.cam_pos.xyz - wpos);
  let n = s.n;
  let f0 = vec3<f32>(s.f0);
  let rough = clamp(s.rough, 0.08, 1.0);
  let sh = shadow_at(wpos, n);
  let spot = g.sun_dir.w > 0.0;
  let sun_sh = select(sh, 1.0, spot);
  let l = normalize(g.sun_dir.xyz);
  var col = brdf(s.base, n, v, l, rough, f0) * g.sun_col.rgb * sun_sh;
  // light through leaves and blades from behind
  if (s.trans > 0.0) {
    let back = max(dot(-n, l), 0.0) * 0.6 + pow(max(dot(-v, l), 0.0), 4.0);
    col = col + s.base * g.sun_col.rgb * sun_sh * back * s.trans;
  }
  for (var i = 0; i < 6; i = i + 1) {
    let r = g.lp[i].w;
    if (r <= 0.0) { continue; }
    let pv = g.lp[i].xyz - wpos;
    let d = length(pv);
    let q = d / r;
    let win = clamp(1.0 - q * q * q * q, 0.0, 1.0);
    if (win <= 0.0) { continue; }
    let att = win * win / (1.0 + d * d);
    var ls = 1.0;
    if (spot && i == 0) { ls = sh; }
    let pl = pv / max(d, 0.0001);
    col = col + brdf(s.base, n, v, pl, rough, f0) * g.lc[i].rgb * (att * ls);
    if (s.trans > 0.0) { col = col + s.base * g.lc[i].rgb * att * max(dot(-n, pl), 0.0) * s.trans * 0.5; }
    // indoors a lamp also lights the room: what comes down bounces off the floor and back up on to things
    if (g.grade.w > 0.0) { col = col + s.base * g.lc[i].rgb * att * (0.04 + 0.07 * clamp(0.5 - n.y * 0.5, 0.0, 1.0)); }
  }
  // ambient: sky and ground (indoors: the light bouncing off ceiling, walls and floor)
  // what stands on the ground gets darker where it meets it: less sky reaches there
  let contact = mix(mix(0.5, 1.0, smoothstep(0.0, 0.4, wpos.y)), 1.0, clamp(n.y, 0.0, 1.0));
  let amb = g.misc.w * s.ao * contact;
  // the sky is brighter on the sun's side, and the lit ground throws its colour back up
  let toward = select(dot(n, l) * 0.5 + 0.5, 0.5, g.grade.w > 0.0);
  let hemi = mix(g.ground_col.rgb, g.sky_top.rgb, n.y * 0.5 + 0.5) * mix(0.82, 1.14, toward) + g.sun_col.rgb * g.ground_col.rgb * max(-n.y * 0.5 + 0.5, 0.0) * 0.35 * max(l.y, 0.0);
  let ndv = max(dot(n, v), 0.0);
  let fr = f0 + (max(vec3<f32>(1.0 - rough), f0) - f0) * pow(1.0 - ndv, 5.0);
  col = col + s.base * hemi * amb * (vec3<f32>(1.0) - fr * 0.5);
  let rv = reflect(-v, n);
  let env = mix(g.ground_col.rgb, mix(g.sky_hor.rgb, g.sky_top.rgb, clamp(rv.y * 1.5, 0.0, 1.0)), smoothstep(-0.25, 0.15, rv.y));
  col = col + env * fr * amb * (1.0 - rough * 0.75);
  return col;
}

/// The old lighting call (specular strength + shininess), now on the physical model.
fn light(base: vec3<f32>, n: vec3<f32>, wpos: vec3<f32>, spec_k: f32, shin: f32) -> vec3<f32> {
  let rough = clamp(sqrt(2.0 / (shin + 2.0)), 0.12, 0.95);
  return shade(Surf(base, n, rough, 0.02 + spec_k * 0.04, room_ao(wpos, n), 0.0), wpos);
}

/// The colour of the haze along a view direction: it lights up towards the sun, the way air does.
fn fog_col(dir: vec3<f32>) -> vec3<f32> {
  let s = max(dot(dir, normalize(g.sun_dir.xyz)), 0.0);
  return g.fog.rgb + g.sun_col.rgb * (pow(s, 6.0) * 0.045 + pow(s, 40.0) * 0.16);
}
/// Haze lies thickest close to the ground.
fn fogged(c: vec3<f32>, wpos: vec3<f32>) -> vec3<f32> {
  let v = wpos - g.cam_pos.xyz;
  let d = length(v);
  let f = 1.0 - exp(-d * g.fog.w * exp(-max(wpos.y, 0.0) * 0.04));
  return mix(c, fog_col(v / max(d, 0.001)), clamp(f, 0.0, 1.0)) + airlight(v / max(d, 0.001), d);
}
/// Light scattered towards the eye by the smoky air between it and this point: for each lamp the exact
/// integral of inverse-square light along the view ray. This is what makes a room's air glow.
fn airlight(dir: vec3<f32>, len: f32) -> vec3<f32> {
  let k = g.grade.w;
  if (k <= 0.0) { return vec3<f32>(0.0); }
  var sum = vec3<f32>(0.0);
  for (var i = 0; i < 6; i = i + 1) {
    if (g.lp[i].w <= 0.0) { continue; }
    let q = g.cam_pos.xyz - g.lp[i].xyz;
    let b = dot(dir, q);
    let h = sqrt(max(dot(q, q) - b * b, 0.0025));
    sum = sum + g.lc[i].rgb * (atan((len + b) / h) - atan(b / h)) / h;
  }
  return sum * k * 0.08;
}

fn tonemap(c: vec3<f32>) -> vec3<f32> {
  // ACES fit (Narkowicz), exposure per arena
  let x = c * g.misc.z;
  return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

/// Roughly how wide one pixel is on this surface, in metres (for fading out detail that would alias).
fn footprint(p: vec3<f32>, n: vec3<f32>) -> f32 {
  let v = g.cam_pos.xyz - p;
  let d = length(v);
  return d * 0.0011 / max(abs(dot(n, v / d)), 0.15);
}
/// 1 where a pattern of this size is resolved, fading to 0 where it would turn into moiré.
fn resolve(fp: f32, size: f32) -> f32 {
  return 1.0 - smoothstep(0.15, 0.5, fp / size);
}

/// Tilt a normal by the slope of a height field given along two surface directions.
fn bump(n: vec3<f32>, ta: vec3<f32>, tb: vec3<f32>, da: f32, db: f32) -> vec3<f32> {
  return normalize(n - ta * da - tb * db);
}

// ---------- materials ----------
fn wood_h(al: f32, ac: f32, pw: f32, seglen: f32) -> vec4<f32> {
  // returns (height, tone, grain, row)
  let row = floor(ac / pw);
  let off = hash2(vec2<f32>(row, 3.0)) * seglen;
  let seg = floor((al + off) / seglen);
  let fa = fract(ac / pw) * pw;
  let fl = fract((al + off) / seglen) * seglen;
  let edge = min(min(fa, pw - fa), min(fl, seglen - fl));
  let groove = smoothstep(0.0, 0.006, edge);
  let rnd = hash2(vec2<f32>(row, seg));
  // grain: long wavy lines with knots now and then
  let warp = fbm(vec2<f32>(al * 0.9, row * 7.3 + seg * 3.1)) * 3.0;
  let lines = sin((fa / pw) * 26.0 + warp * 2.2 + rnd * 9.0) * 0.5 + 0.5;
  let knot_c = vec2<f32>(hash2(vec2<f32>(row, seg + 0.5)) * seglen, 0.5 * pw);
  let kd = length(vec2<f32>(fl - knot_c.x, (fa - knot_c.y) * 2.5));
  let knot = step(0.72, rnd) * smoothstep(0.05, 0.0, kd);
  let grain = 0.86 + 0.14 * lines - knot * 0.35;
  let tone = 0.74 + 0.36 * rnd;
  return vec4<f32>(groove, tone, grain, row);
}

// The materials that cover most of the screen have their own entry points too (fs_lit, fs_lawn, fs_sky,
// fs_leaf): a shader holding only one material needs far fewer registers than the whole of fs_main.
// 0: lit
fn m_lit(i: VOut) -> vec4<f32> {
  let n = normalize(i.nrm);
  var col = i.color.rgb;
  var a = i.color.a;
  let p = i.wpos;
  var base = i.color.rgb;
  var rough = 0.62;
  if (i.params.w > 0.5) { // vertical stripes (the referee's shirt)
    if (fract(i.uv.x * i.params.w) < 0.5) { base = vec3<f32>(0.03, 0.03, 0.035); }
  }
  col = shade(Surf(base, n, rough, 0.04, room_ao(p, n), 0.0), p) + base * i.params.y * 1.6;
  col = fogged(col, p);
  return vec4<f32>(col, a);
}
// 3: the lawn
fn m_lawn(i: VOut) -> vec4<f32> {
  var col = i.color.rgb;
  var a = i.color.a;
  let p = i.wpos;
  let q = p.xz;
  var base = lawn_col(q);
  let worn = pitch_wear(q);
  let dirt = vec3<f32>(0.33, 0.27, 0.17) * (0.8 + 0.4 * vnoise(q * 13.0));
  base = mix(base, dirt, worn * 0.8);
  base = base * (0.9 + 0.2 * vnoise(q * 31.0));
  // the gravel path
  let pd = abs(q.x - path_x(q.y));
  let gravel = vec3<f32>(0.62, 0.55, 0.42) * (0.75 + 0.5 * hash2(floor(q * 40.0)));
  base = mix(base, gravel, smoothstep(1.5, 1.25, pd) * step(q.y, 60.0) * step(-61.0, q.y));
  let hx = vnoise(q * 18.0 + vec2<f32>(0.37, 0.0)) - vnoise(q * 18.0);
  let hz = vnoise(q * 18.0 + vec2<f32>(0.0, 0.37)) - vnoise(q * 18.0);
  let nn = bump(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), hx * 0.5, hz * 0.5);
  // grass that isn't worn shades its own roots: deep down among the blades round the camera, a little further off
  let under = mix(0.5, 0.75, smoothstep(10.0, 24.0, distance(g.cam_pos.xz, q)));
  col = shade(Surf(base, nn, 0.92, 0.03, mix(under, 1.0, worn), 0.0), p);
  col = fogged(col, p);
  return vec4<f32>(col, a);
}
// 5: sky
fn m_sky(i: VOut) -> vec4<f32> {
  var col = i.color.rgb;
  var a = i.color.a;
  let t = g.misc.x;
  let p = i.wpos;
  let d = normalize(p - g.cam_pos.xyz);
  let h = clamp(d.y, -0.2, 1.0);
  col = mix(g.sky_hor.rgb, g.sky_top.rgb, pow(max(h, 0.0), 0.45)) * 1.1;
  let s = max(dot(d, normalize(g.sun_dir.xyz)), 0.0);
  col = col + g.sun_col.rgb * (pow(s, 900.0) * 6.0 + pow(s, 14.0) * 0.16 + pow(s, 3.0) * 0.05);
  // layered clouds, lit from the sun's side
  if (h > 0.02) {
    let cp = d.xz / (h + 0.15) * 1.6 + vec2<f32>(t * 0.008, 0.0);
    let dens = fbm(cp) * 0.7 + vnoise(cp * 3.7) * 0.3;
    let cl = smoothstep(0.5, 0.82, dens);
    let lit = 0.75 + 0.45 * s;
    col = mix(col, vec3<f32>(1.0, 0.96, 0.92) * lit * 1.25, cl * 0.6 * smoothstep(0.02, 0.22, h));
  }
  if (d.y < 0.0) { col = mix(g.sky_hor.rgb, g.ground_col.rgb * 0.8, clamp(-d.y * 6.0, 0.0, 1.0)); }
  // the horizon dissolves into the same haze the far trees do
  let sky_fog = 1.0 - exp(-g.fog.w * 45.0 / max(d.y, 0.03));
  col = mix(col, fog_col(d), clamp(sky_fog, 0.0, 1.0));
  return vec4<f32>(col, a);
}
// 14: leaves
fn m_leaf(i: VOut) -> vec4<f32> {
  let n = normalize(i.nrm);
  var col = i.color.rgb;
  var a = i.color.a;
  let p = i.wpos;
  // leaves: the card is cut to a leaf; each leaf has its own shade of green
  let m = leaf_mask(i.uv);
  if (m < 0.0) { discard; }
  let id = floor(i.uv.x);
  let h = hash2(vec2<f32>(id, i.color.r * 37.0 + i.params.w));
  var base = i.color.rgb * (0.7 + 0.5 * h);
  base = mix(base, base * vec3<f32>(1.25, 1.1, 0.55), step(0.9, h) * 0.6); // a few yellowing
  let vein = 1.0 - 0.18 * smoothstep(0.06, 0.0, abs(fract(i.uv.x) * 2.0 - 1.0));
  base = base * vein;
  let nn = n;
  let depth = clamp(i.color.a, 0.0, 1.0); // 0.3 in the middle of the clump … 1 at the outside
  let ao = mix(0.35, 1.0, smoothstep(0.3, 1.0, depth));
  col = shade(Surf(base, nn, 0.5, 0.045, ao, 0.4), p);
  col = fogged(col, p);
  a = 1.0;
  return vec4<f32>(col, a);
}
@fragment fn fs_lit(i: VOut) -> @location(0) vec4<f32> { let r = m_lit(i); return finish(r.rgb, r.a); }
@fragment fn fs_lawn(i: VOut) -> @location(0) vec4<f32> { let r = m_lawn(i); return finish(r.rgb, r.a); }
@fragment fn fs_sky(i: VOut) -> @location(0) vec4<f32> { let r = m_sky(i); return finish(r.rgb, r.a); }
@fragment fn fs_leaf(i: VOut) -> @location(0) vec4<f32> { let r = m_leaf(i); return finish(r.rgb, 1.0); }

@fragment
fn fs_main(i: VOut, @builtin(front_facing) ff: bool) -> @location(0) vec4<f32> {
  let kind = i32(i.params.x + 0.5);
  var n = normalize(i.nrm);
  var col = i.color.rgb;
  var a = i.color.a;
  let t = g.misc.x;
  let p = i.wpos;
  if (kind == 0) {
    let r = m_lit(i); col = r.rgb; a = r.a;
  } else if (kind == 1) {
    col = i.color.rgb * 1.6;
    if (i.params.w > 0.5) { col = fogged(col, p); }
  } else if (kind == 2) {
    let d = length(i.uv * 2.0 - vec2<f32>(1.0));
    let s = clamp(1.0 - d, 0.0, 1.0);
    a = a * s * s;
    let hemi = mix(g.ground_col.rgb, g.sky_top.rgb, 0.8);
    col = mix(i.color.rgb, i.color.rgb * (hemi * 0.7 + g.sun_col.rgb * 0.45), i.params.w);
  } else if (kind == 3) {
    col = m_lawn(i).rgb;
  } else if (kind == 4) {
    let u = i.uv.x; let v = i.uv.y;
    let e = max(exp(-u / 0.035), exp(-(1.0 - u) / 0.035));
    let f = min(1.0, v * 7.0) * min(1.0, (1.0 - v) * 2.2);
    let fill = f * (0.22 + 0.78 * e);
    let bv = fract(v * 6.0 - t * 0.55 * 6.0 / 6.0);
    let band = smoothstep(0.36, 0.5, bv) * (1.0 - smoothstep(0.5, 0.64, bv));
    let fade = min(1.0, v * 6.0) * pow(1.0 - v, 1.3) * (0.42 + 0.58 * e);
    a = clamp(i.color.a * (fill * 0.5 + band * fade * 0.45), 0.0, 1.0);
    col = i.color.rgb * 1.15;
  } else if (kind == 5) {
    col = m_sky(i).rgb;
  } else if (kind == 6) {
    let w1 = vnoise(p.xz * 1.5 + vec2<f32>(t * 0.3, t * 0.2)) - 0.5;
    let w2 = vnoise(p.xz * 4.3 - vec2<f32>(t * 0.5, -t * 0.1)) - 0.5;
    let nn = normalize(vec3<f32>(w1 * 0.25 + w2 * 0.1, 1.0, vnoise(p.xz * 1.7 - vec2<f32>(t * 0.2, 0.0)) * 0.25 - 0.12 + w2 * 0.1));
    col = shade(Surf(i.color.rgb * 0.6, nn, 0.06, 0.02, 1.0, 0.0), p);
    col = fogged(col, p);
  } else if (kind == 7) {
    // a worn three-colour volleyball: panels around the ball, yellow · blue · white
    let k = fract(i.uv.x * 3.0 + floor(i.uv.y * 3.0) * 0.5);
    var base = vec3<f32>(0.95, 0.93, 0.86);
    if (k < 0.333) { base = vec3<f32>(0.96, 0.74, 0.12); } else if (k < 0.666) { base = vec3<f32>(0.1, 0.3, 0.7); }
    let seam = smoothstep(0.0, 0.03, abs(fract(i.uv.x * 3.0) - 0.5) - 0.47);
    base = base * (1.0 - seam * 0.5);
    col = shade(Surf(base, n, 0.38, 0.04, 1.0, 0.0), p) + i.color.rgb * i.params.y;
  } else if (kind == 8) {
    // wooden planks: floors run along x, walls are vertical boards; params.w = 1 for a varnished floor
    let wallish = abs(n.y) < 0.5;
    var A = vec3<f32>(1.0, 0.0, 0.0);
    var C = vec3<f32>(0.0, 0.0, 1.0);
    var pw = 0.14;
    var sl = 1.8;
    if (wallish) {
      A = vec3<f32>(0.0, 1.0, 0.0);
      C = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(n.x) > 0.5);
      pw = 0.19; sl = 3.1;
    } else if (abs(n.x) > 0.5) {
      A = vec3<f32>(0.0, 0.0, 1.0); C = vec3<f32>(0.0, 1.0, 0.0);
    }
    let al = dot(p, A); let ac = dot(p, C);
    let w0 = wood_h(al, ac, pw, sl);
    let e = 0.004;
    let wa = wood_h(al + e, ac, pw, sl);
    let wc = wood_h(al, ac + e, pw, sl);
    let hs = 0.0025 * resolve(footprint(p, n), 0.012); // groove depth, metres
    let gb = 0.0004 * smoothstep(6.0, 2.0, length(g.cam_pos.xyz - p));
    let nn = bump(n, A, C, (wa.x - w0.x) / e * hs + (wa.z - w0.z) / e * gb, (wc.x - w0.x) / e * hs + (wc.z - w0.z) / e * gb);
    // fine grain only up close: further away it would shimmer into moiré
    let dist = length(g.cam_pos.xyz - p);
    let gk = smoothstep(9.0, 3.0, dist);
    let fpw = footprint(p, n);
    let rg = resolve(fpw, 0.012); // grooves
    let rp = resolve(fpw, pw);    // boards
    var base = i.color.rgb * mix(0.92, w0.y, rp) * mix(0.93, w0.z, gk * rg);
    base = base * mix(vec3<f32>(0.8), mix(vec3<f32>(0.35), vec3<f32>(1.0), w0.x), rg);
    // a slight hue drift between boards
    base = base * mix(vec3<f32>(1.04, 0.98, 0.92), vec3<f32>(0.95, 1.0, 1.06), hash2(vec2<f32>(w0.w, 9.0)));
    let varnish = i.params.w > 0.5;
    let rough = select(0.62 - 0.1 * w0.z, 0.2 + 0.12 * vnoise(p.xz * 2.0), varnish);
    col = shade(Surf(base, nn, rough, select(0.035, 0.05, varnish), room_ao(p, n) * mix(0.6, 1.0, w0.x), 0.0), p);
    col = fogged(col, p);
  } else if (kind == 9) {
    // concrete pavers with grout, size params.w metres
    let sz = max(i.params.w, 0.3);
    let c = floor(p.xz / sz);
    let f = fract(p.xz / sz);
    let ex = min(f.x, 1.0 - f.x); let ez = min(f.y, 1.0 - f.y);
    let grout = smoothstep(0.015, 0.035, min(ex, ez));
    let speck = vnoise(p.xz * 23.0);
    let rgt = resolve(footprint(p, n), sz * 0.03);
    var base = i.color.rgb * (0.84 + 0.24 * hash2(c)) * (0.88 + 0.18 * speck) * mix(0.9, mix(0.5, 1.0, grout), rgt);
    base = base * (0.92 + 0.12 * fbm(p.xz * 0.7));
    let gx = select(-1.0, 1.0, f.x > 0.5) * (1.0 - smoothstep(0.015, 0.035, ex));
    let gz = select(-1.0, 1.0, f.y > 0.5) * (1.0 - smoothstep(0.015, 0.035, ez));
    let nn = normalize(n + vec3<f32>(gx, 0.0, gz) * 0.35 * rgt);
    col = shade(Surf(base, nn, 0.82 - 0.1 * speck, 0.035, room_ao(p, n), 0.0), p);
    col = fogged(col, p);
  } else if (kind == 10) {
    // the dance floor: glass tiles lighting up in turn
    let c = floor(p.xz / 1.0);
    let f = fract(p.xz);
    let h = hash2(c);
    let beat = fract(t * 2.07 + h);
    let on = smoothstep(0.0, 0.05, beat) * (1.0 - smoothstep(0.35, 0.8, beat));
    let hue = fract(h * 3.7 + floor(t * 0.5) * 0.17);
    let rgb = clamp(abs(fract(vec3<f32>(hue) + vec3<f32>(0.0, 0.333, 0.667)) * 6.0 - 3.0) - 1.0, vec3<f32>(0.0), vec3<f32>(1.0));
    let edge = step(0.04, f.x) * step(f.x, 0.96) * step(0.04, f.y) * step(f.y, 0.96);
    col = shade(Surf(vec3<f32>(0.03, 0.03, 0.04), n, 0.12, 0.05, 1.0, 0.0), p) + rgb * (0.12 + on * 0.8) * edge;
  } else if (kind == 11) {
    // building facades with lit windows
    let w = vec2<f32>(p.x + p.z, p.y);
    let cell = floor(w / vec2<f32>(2.4, 3.2));
    let f = fract(w / vec2<f32>(2.4, 3.2));
    let win = step(0.18, f.x) * step(f.x, 0.82) * step(0.22, f.y) * step(f.y, 0.78) * step(0.2, abs(n.y - 1.0));
    let lit = step(0.55, hash2(cell));
    let base = i.color.rgb * (0.9 + 0.2 * hash2(cell + vec2<f32>(3.0, 1.0)));
    col = shade(Surf(mix(base, vec3<f32>(0.05, 0.06, 0.08), win), n, mix(0.85, 0.1, win), mix(0.03, 0.06, win), 1.0, 0.0), p);
    col = col + win * lit * vec3<f32>(1.0, 0.72, 0.38) * 1.5;
    col = fogged(col, p);
  } else if (kind == 12) {
    // brick walls: bricks stand proud of sunken mortar
    let C = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(n.x) > 0.5);
    let along = dot(p, C);
    let r = floor(p.y / 0.075);
    let off = fract(r * 0.5) * 0.11;
    let b = floor((along + off) / 0.22);
    let fy = fract(p.y / 0.075) * 0.075;
    let fx = fract((along + off) / 0.22) * 0.22;
    let ey = min(fy, 0.075 - fy); let ex = min(fx, 0.22 - fx);
    let mortar = smoothstep(0.003, 0.008, min(ey, ex));
    let rb = hash2(vec2<f32>(b, r));
    var brick = i.color.rgb * (0.72 + 0.4 * rb) * (0.85 + 0.25 * vnoise(vec2<f32>(along, p.y) * 30.0));
    brick = mix(brick, brick * vec3<f32>(0.75, 0.7, 0.75), step(0.85, rb)); // a few burnt ones
    let rb2 = resolve(footprint(p, n), 0.02);
    let mortar_c = vec3<f32>(0.46, 0.43, 0.38);
    var base = mix(mortar_c, brick, mortar);
    base = mix(mix(mortar_c, i.color.rgb * 0.9, 0.78), base, rb2);
    let dy = select(-1.0, 1.0, fy > 0.0375) * (1.0 - smoothstep(0.003, 0.008, ey));
    let dx = select(-1.0, 1.0, fx > 0.11) * (1.0 - smoothstep(0.003, 0.008, ex));
    let nn = normalize(n + (vec3<f32>(0.0, dy, 0.0) * 0.5 + C * dx * 0.5) * rb2);
    col = shade(Surf(base, nn, 0.88, 0.03, room_ao(p, n) * mix(1.0, mix(0.7, 1.0, mortar), rb2), 0.0), p);
    col = fogged(col, p);
  } else if (kind == 13) {
    // bark: furrows running up the trunk
    let u = i.uv.x * 6.2831;
    let ridge = abs(sin(u * 7.0 + fbm(vec2<f32>(u * 2.0, p.y * 2.5)) * 4.0));
    let fur = smoothstep(0.0, 0.6, ridge);
    let base = i.color.rgb * (0.55 + 0.5 * fur) * (0.85 + 0.3 * vnoise(vec2<f32>(u * 3.0, p.y * 9.0)));
    let tang = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), n) + vec3<f32>(0.0001));
    let nn = normalize(n + tang * (cos(u * 7.0) * 0.35));
    col = shade(Surf(base, nn, 0.92, 0.03, mix(0.6, 1.0, fur), 0.0), p);
    col = fogged(col, p);
  } else if (kind == 14) {
    col = m_leaf(i).rgb;
    a = 1.0;
  } else if (kind == 18) {
    // a beam of light in smoky air: soft at its edges, dying away from the lamp
    let view = normalize(g.cam_pos.xyz - p);
    let core = pow(abs(dot(n, view)), 2.5);
    a = a * core * smoothstep(0.0, 5.5, p.y + 0.4);
    col = i.color.rgb * 2.2;
  } else if (kind == 17) {
    // asphalt: grit, tar patches and damp spots that catch the lamps
    let fine = resolve(footprint(p, n), 0.03);
    let grit = (hash2(floor(p.xz * 55.0)) - 0.5) * 0.3 * fine;
    let tar = fbm(p.xz * 0.45);
    let damp = smoothstep(0.58, 0.72, fbm(p.xz * 0.22 + vec2<f32>(7.0, 3.0)));
    let base = i.color.rgb * (0.8 + 0.4 * tar + grit) * (1.0 - 0.35 * damp);
    col = shade(Surf(base, n, mix(0.92, 0.2, damp), mix(0.04, 0.06, damp), 1.0, 0.0), p);
    col = fogged(col, p);
  } else if (kind == 16) {
    // painted plaster
    let q = vec2<f32>(p.x + p.z, p.y);
    let nz = fbm(q * 3.0);
    let base = i.color.rgb * (0.93 + 0.1 * nz);
    col = shade(Surf(base, n, 0.85, 0.03, room_ao(p, n), 0.0), p);
    col = fogged(col, p);
  }
  // in a dark room the referee's gaze is a hint on the floor, not a floodlight
  if (kind == 4 && g.grade.w > 0.0) { a = a * 0.32; }
  return finish(col, a);
}

fn finish(col: vec3<f32>, a: f32) -> vec4<f32> {
  // with an HDR target the film curve and the grading happen in the final copy, after bloom
  if (g.cam_pos.w > 1.5) { return vec4<f32>(clamp(col, vec3<f32>(0.0), vec3<f32>(48.0)), a); }
  // colour grading: a little contrast and saturation
  var c = tonemap(col);
  let lum = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
  c = max(mix(vec3<f32>(lum), c, g.grade.y), vec3<f32>(0.0));
  var pc = sqrt(c);
  pc = clamp((pc - vec3<f32>(0.5)) * g.grade.x + vec3<f32>(0.5), vec3<f32>(0.0), vec3<f32>(1.0));
  c = pc * pc * vec3<f32>(1.0 + g.grade.z, 1.0, 1.0 - g.grade.z);
  return vec4<f32>(c, a);
}
