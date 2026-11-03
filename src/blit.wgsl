// ---------- final copy to the screen: bloom, exposure, the film curve and grading ----------
struct BOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
struct Cfg {
  a: vec4<f32>, // encode to sRGB here?, scene is HDR?, bloom strength, _
  b: vec4<f32>, // exposure, contrast, saturation, warmth
};
@group(0) @binding(0) var blit_tex: texture_2d<f32>;
@group(0) @binding(1) var blit_smp: sampler;
@group(0) @binding(2) var<uniform> cfg: Cfg;
@group(0) @binding(3) var bloom_tex: texture_2d<f32>;

@vertex
fn vs_blit(@builtin(vertex_index) vi: u32) -> BOut {
  let x = f32((vi << 1u) & 2u);
  let y = f32(vi & 2u);
  var o: BOut;
  o.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
  o.uv = vec2<f32>(x, y);
  return o;
}
fn to_srgb(c: vec3<f32>) -> vec3<f32> {
  let lo = c * 12.92;
  let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
  return select(hi, lo, c <= vec3<f32>(0.0031308));
}
@fragment
fn fs_blit(i: BOut) -> @location(0) vec4<f32> {
  var c = textureSampleLevel(blit_tex, blit_smp, i.uv, 0.0).rgb;
  if (cfg.a.y > 0.5) {
    c = c + textureSampleLevel(bloom_tex, blit_smp, i.uv, 0.0).rgb * cfg.a.z;
    // ACES fit (Narkowicz)
    let x = max(c, vec3<f32>(0.0)) * cfg.b.x;
    c = clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
    let lum = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = max(mix(vec3<f32>(lum), c, cfg.b.z), vec3<f32>(0.0));
    var pc = sqrt(c);
    pc = clamp((pc - vec3<f32>(0.5)) * cfg.b.y + vec3<f32>(0.5), vec3<f32>(0.0), vec3<f32>(1.0));
    c = pc * pc * vec3<f32>(1.0 + cfg.b.w, 1.0, 1.0 - cfg.b.w);
  }
  // a lens darkens its corners
  let r = length(i.uv - vec2<f32>(0.5));
  c = c * (1.0 - 0.28 * smoothstep(0.38, 0.8, r));
  if (cfg.a.x > 0.5) { c = to_srgb(c); }
  // a little noise: no bands in the sky and the haze
  let n = fract(sin(dot(i.clip.xy, vec2<f32>(12.9898, 78.233))) * 43758.5453);
  c = c + vec3<f32>((n - 0.5) / 255.0);
  return vec4<f32>(c, 1.0);
}
