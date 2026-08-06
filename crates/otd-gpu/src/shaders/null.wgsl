// Null / Out TOP — a straight copy. Also used to blit a Feedback TOP's target.

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
  return sample0(in.uv);
}
