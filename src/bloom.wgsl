// Bloom: what's brighter than white bleeds out, the way it does in a lens and in smoky air.
// A chain of ever smaller images: down (keeping what's bright), then back up, each adding to the next.
struct BOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32> };
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;

@vertex
fn vs(@builtin(vertex_index) vi: u32) -> BOut {
  let x = f32((vi << 1u) & 2u);
  let y = f32(vi & 2u);
  var o: BOut;
  o.clip = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
  o.uv = vec2<f32>(x, y);
  return o;
}
fn px() -> vec2<f32> {
  let d = textureDimensions(src);
  return vec2<f32>(1.0 / f32(d.x), 1.0 / f32(d.y));
}
fn down13(uv: vec2<f32>) -> vec3<f32> {
  let p = px();
  let s = textureSampleLevel(src, smp, uv, 0.0).rgb * 0.25;
  var c = s;
  c = c + textureSampleLevel(src, smp, uv + vec2<f32>(-p.x, -p.y), 0.0).rgb * 0.1875;
  c = c + textureSampleLevel(src, smp, uv + vec2<f32>(p.x, -p.y), 0.0).rgb * 0.1875;
  c = c + textureSampleLevel(src, smp, uv + vec2<f32>(-p.x, p.y), 0.0).rgb * 0.1875;
  c = c + textureSampleLevel(src, smp, uv + vec2<f32>(p.x, p.y), 0.0).rgb * 0.1875;
  return c;
}
/// The first step: only what's over the knee goes in.
@fragment
fn fs_bright(i: BOut) -> @location(0) vec4<f32> {
  let c = min(down13(i.uv), vec3<f32>(24.0));
  let l = max(c.r, max(c.g, c.b));
  let over = max(l - 1.0, 0.0);
  let k = over * over / (over + 0.5) / max(l, 0.0001);
  return vec4<f32>(c * k, 1.0);
}
@fragment
fn fs_down(i: BOut) -> @location(0) vec4<f32> {
  return vec4<f32>(down13(i.uv), 1.0);
}
/// Going up, a soft tent over the smaller image, added onto the bigger one.
@fragment
fn fs_up(i: BOut) -> @location(0) vec4<f32> {
  let p = px();
  var c = textureSampleLevel(src, smp, i.uv, 0.0).rgb * 0.25;
  c = c + textureSampleLevel(src, smp, i.uv + vec2<f32>(p.x, 0.0), 0.0).rgb * 0.125;
  c = c + textureSampleLevel(src, smp, i.uv - vec2<f32>(p.x, 0.0), 0.0).rgb * 0.125;
  c = c + textureSampleLevel(src, smp, i.uv + vec2<f32>(0.0, p.y), 0.0).rgb * 0.125;
  c = c + textureSampleLevel(src, smp, i.uv - vec2<f32>(0.0, p.y), 0.0).rgb * 0.125;
  c = c + textureSampleLevel(src, smp, i.uv + p, 0.0).rgb * 0.0625;
  c = c + textureSampleLevel(src, smp, i.uv - p, 0.0).rgb * 0.0625;
  c = c + textureSampleLevel(src, smp, i.uv + vec2<f32>(p.x, -p.y), 0.0).rgb * 0.0625;
  c = c + textureSampleLevel(src, smp, i.uv + vec2<f32>(-p.x, p.y), 0.0).rgb * 0.0625;
  return vec4<f32>(c, 1.0);
}
