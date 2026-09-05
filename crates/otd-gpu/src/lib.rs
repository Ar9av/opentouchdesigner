//! `otd-gpu` — the TOP (texture operator) engine.
//!
//! Everything here is wgpu; nothing here knows about the editor. The public
//! surface is [`TopEngine`], which implements `otd_core::Cooker`.

pub mod context;
pub mod demo;
pub mod engine;
pub mod isf;
pub mod math;
pub mod ops;
pub mod record;
pub mod render3d;
pub mod scene;
pub mod shader;
pub mod text;
pub mod texture;
pub mod video;

pub use context::GpuContext;
pub use engine::TopEngine;
pub use texture::{TOP_FORMAT, TopTexture};

/// Copy a TOP's texture back to the CPU as 8-bit RGBA.
///
/// Slow by design — it stalls the pipeline. It exists for tests and for
/// still-image export, not for the render path.
pub fn read_pixels_rgba8(
    ctx: &GpuContext,
    tex: &TopTexture,
) -> Result<(u32, u32, Vec<u8>), String> {
    let (w, h, floats) = read_pixels_rgba_f32(ctx, tex)?;
    let out = floats
        .into_iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
        .collect();
    Ok((w, h, out))
}

/// The same copy, keeping the full 16-bit float range.
///
/// TOPs are `Rgba16Float` throughout — that is the headline the README makes —
/// so anything that reads pixels back in order to *compute* with them, rather
/// than to write a PNG, has to see the values that are actually there.
/// A TOP to CHOP that clamped to 0..1 would quietly destroy the HDR range the
/// rest of the pipeline goes to some trouble to keep.
pub fn read_pixels_rgba_f32(
    ctx: &GpuContext,
    tex: &TopTexture,
) -> Result<(u32, u32, Vec<f32>), String> {
    let (w, h) = (tex.key.width, tex.key.height);
    // Rgba16Float: 8 bytes per pixel, and rows must be 256-byte aligned.
    let unpadded = w as u64 * 8;
    let padded = unpadded.div_ceil(256) * 256;

    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("otd readback"),
        size: padded * h as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("otd readback"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &tex.texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    ctx.queue.submit(Some(encoder.finish()));

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| format!("device poll failed: {e}"))?;
    rx.recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("buffer map failed: {e}"))?;

    let data = slice.get_mapped_range();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let row = &data[y * padded as usize..y * padded as usize + unpadded as usize];
        for x in 0..w as usize {
            for c in 0..4 {
                let i = x * 8 + c * 2;
                let bits = u16::from_le_bytes([row[i], row[i + 1]]);
                out.push(f16_to_f32(bits));
            }
        }
    }
    drop(data);
    buffer.unmap();
    Ok((w, h, out))
}

/// IEEE 754 binary32 -> binary16, round-to-nearest-even.
///
/// The inverse of [`f16_to_f32`], for the CPU side of an upload into a
/// `Rgba16Float` texture. Values beyond half's range saturate to infinity
/// rather than wrapping, which is what a shader reading them would want.
pub(crate) fn f32_to_f16(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x7f_ffff;

    if exp == 0xff {
        // Inf or NaN. A NaN must stay a NaN: zeroing the mantissa would turn
        // it into an infinity.
        let m = if mant != 0 { 0x200 } else { 0 };
        return sign | 0x7c00 | m;
    }
    let unbiased = exp - 127 + 15;
    if unbiased >= 0x1f {
        return sign | 0x7c00; // Overflow to infinity.
    }
    if unbiased <= 0 {
        if unbiased < -10 {
            return sign; // Smaller than the smallest subnormal.
        }
        let m = mant | 0x80_0000;
        let shift = (14 - unbiased) as u32;
        let half = (m >> shift) as u16;
        // Round to nearest even.
        let round = (m >> (shift - 1)) & 1;
        return sign | (half + round as u16);
    }
    let half = ((unbiased as u16) << 10) | (mant >> 13) as u16;
    let round = if (mant & 0x1000) != 0 && ((mant & 0x0fff) != 0 || (half & 1) == 1) {
        1
    } else {
        0
    };
    sign | (half + round)
}

/// IEEE 754 binary16 -> binary32.
fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits >> 15) & 1) as u32;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let mant = (bits & 0x3ff) as u32;
    let f = match exp {
        0 => {
            if mant == 0 {
                sign << 31
            } else {
                // Subnormal: renormalise. Its value is `mant * 2^-24`, so
                // shifting the mantissa left until the implicit bit appears
                // leaves `(1 + m/1024) * 2^(-14 - shifts)`, and the biased
                // binary32 exponent is `127 - 14 - shifts`.
                let mut shifts = 0i32;
                let mut m = mant;
                while m & 0x400 == 0 {
                    m <<= 1;
                    shifts += 1;
                }
                let m = m & 0x3ff;
                (sign << 31) | (((113 - shifts) as u32) << 23) | (m << 13)
            }
        }
        0x1f => (sign << 31) | (0xff << 23) | (mant << 13),
        _ => (sign << 31) | ((exp + 127 - 15) << 23) | (mant << 13),
    };
    f32::from_bits(f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use otd_core::{CookContext, CookEngine, Graph, Value};

    /// Both halves of the half-float conversion are hand-rolled, which is
    /// exactly the sort of code that is silently wrong in one exponent range
    /// and nowhere else.
    #[test]
    fn half_floats_survive_the_round_trip() {
        for v in [
            0.0, -0.0, 1.0, -1.0, 0.5, 0.25, 0.1, -0.75, 65504.0, -65504.0, 1e-5, 1234.0,
        ] {
            let back = f16_to_f32(f32_to_f16(v));
            let tolerance = v.abs() * 1e-3 + 1e-7;
            assert!((back - v).abs() <= tolerance, "{v} came back as {back}",);
        }
        // Out of range saturates rather than wrapping to something small.
        assert!(f16_to_f32(f32_to_f16(1e30)).is_infinite());
        assert!(f16_to_f32(f32_to_f16(f32::NAN)).is_nan());
    }

    /// CI runners without a GPU (and without lavapipe) skip these rather
    /// than failing the build.
    macro_rules! gpu_or_skip {
        () => {
            match GpuContext::headless() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("skipping GPU test: {e}");
                    return;
                }
            }
        };
    }

    fn px(pixels: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    #[test]
    fn f16_decode_matches_known_values() {
        assert_eq!(f16_to_f32(0x0000), 0.0);
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0x4000), 2.0);
        assert_eq!(f16_to_f32(0xbc00), -1.0);
        assert!((f16_to_f32(0x3555) - 0.333).abs() < 0.001);
    }

    #[test]
    fn every_shader_compiles() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx);
        let mut graph = Graph::new();
        let reg = ops::registry();
        let root = graph.root();

        let mut ids = Vec::new();
        for spec in ops::all() {
            ids.push(
                graph
                    .create(root, reg.get(spec.def.type_name).unwrap(), None)
                    .unwrap(),
            );
        }
        let mut cook = CookEngine::new();
        engine.begin_frame();
        for id in &ids {
            cook.pull(&graph, *id, &CookContext::default(), &mut engine)
                .unwrap_or_else(|e| panic!("{} failed: {e}", graph.path(*id)));
        }
        engine.end_frame();
    }

    #[test]
    fn a_constant_top_renders_its_colour() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let c = graph
            .create(root, reg.get("constantTOP").unwrap(), None)
            .unwrap();
        graph
            .set_param(c, "color", Value::Vec4([1.0, 0.5, 0.0, 1.0]))
            .unwrap();
        graph.set_param(c, "resw", Value::Int(64)).unwrap();
        graph.set_param(c, "resh", Value::Int(32)).unwrap();

        let mut cook = CookEngine::new();
        engine.begin_frame();
        cook.pull(&graph, c, &CookContext::default(), &mut engine)
            .unwrap();
        engine.end_frame();

        let tex = engine.output(&graph, c).unwrap().clone();
        let (w, h, pixels) = read_pixels_rgba8(&ctx, &tex).unwrap();
        assert_eq!((w, h), (64, 32));
        let p = px(&pixels, w, 10, 10);
        assert_eq!(p[0], 255);
        assert!((p[1] as i32 - 128).abs() <= 2, "green was {}", p[1]);
        assert_eq!(p[2], 0);
    }

    #[test]
    fn a_level_top_actually_changes_its_input() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let c = graph
            .create(root, reg.get("constantTOP").unwrap(), None)
            .unwrap();
        let l = graph
            .create(root, reg.get("levelTOP").unwrap(), None)
            .unwrap();
        graph
            .set_param(c, "color", Value::Vec4([0.5, 0.5, 0.5, 1.0]))
            .unwrap();
        graph.set_param(c, "resw", Value::Int(32)).unwrap();
        graph.set_param(c, "resh", Value::Int(32)).unwrap();
        graph.connect(c, l, 0).unwrap();
        graph.set_param(l, "brightness", Value::Float(2.0)).unwrap();

        let mut cook = CookEngine::new();
        engine.begin_frame();
        cook.pull(&graph, l, &CookContext::default(), &mut engine)
            .unwrap();
        engine.end_frame();

        let tex = engine.output(&graph, l).unwrap().clone();
        let (w, _, pixels) = read_pixels_rgba8(&ctx, &tex).unwrap();
        let p = px(&pixels, w, 4, 4);
        assert!(p[0] > 250, "brightness 2.0 on 0.5 grey gave {}", p[0]);
    }

    #[test]
    fn a_filter_inherits_its_input_resolution() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx);
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let c = graph
            .create(root, reg.get("noiseTOP").unwrap(), None)
            .unwrap();
        let l = graph
            .create(root, reg.get("blurTOP").unwrap(), None)
            .unwrap();
        graph.set_param(c, "resw", Value::Int(200)).unwrap();
        graph.set_param(c, "resh", Value::Int(100)).unwrap();
        graph.connect(c, l, 0).unwrap();

        let mut cook = CookEngine::new();
        engine.begin_frame();
        cook.pull(&graph, l, &CookContext::default(), &mut engine)
            .unwrap();
        engine.end_frame();

        let t = engine.output(&graph, l).unwrap();
        assert_eq!((t.key.width, t.key.height), (200, 100));
    }

    #[test]
    fn resizing_recycles_the_old_texture() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx);
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let c = graph
            .create(root, reg.get("constantTOP").unwrap(), None)
            .unwrap();
        let mut cook = CookEngine::new();
        let mut ctxt = CookContext::default();

        for w in [64i64, 128, 64] {
            graph.set_param(c, "resw", Value::Int(w)).unwrap();
            engine.begin_frame();
            cook.pull(&graph, c, &ctxt, &mut engine).unwrap();
            engine.end_frame();
            ctxt.advance(1.0 / 60.0);
        }

        // Three cooks at two distinct sizes must not allocate three textures.
        assert!(
            engine.textures_created() <= 2,
            "texture pool is not recycling: {} allocations",
            engine.textures_created()
        );
    }

    /// Cook `roots` for one frame and read back the first root's pixels.
    fn cook_and_read(
        ctx: &GpuContext,
        engine: &mut TopEngine,
        cook: &mut CookEngine,
        graph: &Graph,
        time: &CookContext,
        roots: &[otd_core::NodeId],
    ) -> (u32, Vec<u8>) {
        engine.begin_frame();
        for r in roots {
            cook.pull(graph, *r, time, engine).unwrap();
        }
        engine.end_frame();
        let tex = engine.output(graph, roots[0]).unwrap().clone();
        let (w, _, pixels) = read_pixels_rgba8(ctx, &tex).unwrap();
        (w, pixels)
    }

    #[test]
    fn a_glsl_top_renders_its_default_wgsl_source() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let g = graph
            .create(root, reg.get(ops::GLSL).unwrap(), None)
            .unwrap();
        graph.set_param(g, "resw", Value::Int(64)).unwrap();
        graph.set_param(g, "resh", Value::Int(64)).unwrap();

        let mut cook = CookEngine::new();
        let (w, pixels) = cook_and_read(
            &ctx,
            &mut engine,
            &mut cook,
            &graph,
            &CookContext::default(),
            &[g],
        );
        assert!(
            engine.shader_error(g).is_none(),
            "{:?}",
            engine.shader_error(g)
        );
        assert!(
            pixels.iter().step_by(4).any(|p| *p > 8),
            "default GLSL TOP shader rendered nothing"
        );
        assert_eq!(w, 64);
    }

    #[test]
    fn a_glsl_top_runs_shadertoy_style_glsl() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let g = graph
            .create(root, reg.get(ops::GLSL).unwrap(), None)
            .unwrap();
        graph.set_param(g, "resw", Value::Int(32)).unwrap();
        graph.set_param(g, "resh", Value::Int(32)).unwrap();
        graph
            .set_param(g, "language", Value::Str("glsl".into()))
            .unwrap();
        graph
            .set_param(
                g,
                "source",
                Value::Str(
                    "void mainImage(out vec4 fragColor, in vec2 fragCoord) {\n\
                     fragColor = vec4(1.0, 0.0, 0.0, 1.0);\n}"
                        .into(),
                ),
            )
            .unwrap();

        let mut cook = CookEngine::new();
        let (w, pixels) = cook_and_read(
            &ctx,
            &mut engine,
            &mut cook,
            &graph,
            &CookContext::default(),
            &[g],
        );
        assert!(
            engine.shader_error(g).is_none(),
            "{:?}",
            engine.shader_error(g)
        );
        assert_eq!(px(&pixels, w, 4, 4), [255, 0, 0, 255]);
    }

    #[test]
    fn a_broken_shader_reports_the_error_and_holds_the_last_good_one() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let g = graph
            .create(root, reg.get(ops::GLSL).unwrap(), None)
            .unwrap();
        graph.set_param(g, "resw", Value::Int(16)).unwrap();
        graph.set_param(g, "resh", Value::Int(16)).unwrap();
        graph
            .set_param(g, "source", Value::Str("return vec4<f32>(1.0);".into()))
            .unwrap();

        let mut cook = CookEngine::new();
        let mut time = CookContext::default();
        let (w, good) = cook_and_read(&ctx, &mut engine, &mut cook, &graph, &time, &[g]);
        assert_eq!(px(&good, w, 2, 2), [255, 255, 255, 255]);

        // Now break it mid-edit.
        time.advance(1.0 / 60.0);
        graph
            .set_param(g, "source", Value::Str("return vec4<f32>(".into()))
            .unwrap();
        let (w, after) = cook_and_read(&ctx, &mut engine, &mut cook, &graph, &time, &[g]);
        assert!(engine.shader_error(g).is_some(), "error should be reported");
        assert_eq!(
            px(&after, w, 2, 2),
            [255, 255, 255, 255],
            "a typo must not black out a running patch"
        );
    }

    #[test]
    fn a_select_top_reads_this_frame_where_feedback_reads_the_last() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();

        let src = graph
            .create(root, reg.get("constantTOP").unwrap(), Some("src"))
            .unwrap();
        let sel = graph
            .create(root, reg.get(ops::SELECT).unwrap(), Some("sel"))
            .unwrap();
        graph.set_param(src, "resw", Value::Int(16)).unwrap();
        graph.set_param(src, "resh", Value::Int(16)).unwrap();
        graph
            .set_param(sel, "top", Value::Str("/src".into()))
            .unwrap();
        graph
            .set_param(src, "color", Value::Vec4([1.0, 1.0, 1.0, 1.0]))
            .unwrap();

        let mut cook = CookEngine::new();
        let mut time = CookContext::default();
        // Pulling only the Select must drag its target in and show it.
        let (w, pixels) = cook_and_read(&ctx, &mut engine, &mut cook, &graph, &time, &[sel]);
        assert!(px(&pixels, w, 2, 2)[0] > 250);
        assert_eq!(cook.cook_count(src), 1, "the target must have cooked");

        // Change the source: Select shows the new value the same frame.
        time.advance(1.0 / 60.0);
        graph
            .set_param(src, "color", Value::Vec4([0.0, 0.0, 0.0, 1.0]))
            .unwrap();
        let (w, pixels) = cook_and_read(&ctx, &mut engine, &mut cook, &graph, &time, &[sel]);
        assert!(px(&pixels, w, 2, 2)[0] < 5, "Select must not lag a frame");
    }

    #[test]
    fn a_cache_top_freezes_its_input() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let src = graph
            .create(root, reg.get("constantTOP").unwrap(), None)
            .unwrap();
        let cache = graph
            .create(root, reg.get(ops::CACHE).unwrap(), None)
            .unwrap();
        graph.set_param(src, "resw", Value::Int(16)).unwrap();
        graph.set_param(src, "resh", Value::Int(16)).unwrap();
        graph.connect(src, cache, 0).unwrap();
        graph
            .set_param(src, "color", Value::Vec4([1.0, 1.0, 1.0, 1.0]))
            .unwrap();

        let mut cook = CookEngine::new();
        let mut time = CookContext::default();
        cook_and_read(&ctx, &mut engine, &mut cook, &graph, &time, &[cache]);

        time.advance(1.0 / 60.0);
        graph
            .set_param(cache, "active", Value::Bool(false))
            .unwrap();
        graph
            .set_param(src, "color", Value::Vec4([0.0, 0.0, 0.0, 1.0]))
            .unwrap();
        let (w, pixels) = cook_and_read(&ctx, &mut engine, &mut cook, &graph, &time, &[cache]);
        assert!(
            px(&pixels, w, 2, 2)[0] > 250,
            "an inactive Cache TOP must hold the frame it had"
        );
    }

    #[test]
    fn a_resolution_top_resamples_to_an_explicit_size() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx);
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        let src = graph
            .create(root, reg.get("noiseTOP").unwrap(), None)
            .unwrap();
        let res = graph
            .create(root, reg.get("resolutionTOP").unwrap(), None)
            .unwrap();
        graph.set_param(src, "resw", Value::Int(64)).unwrap();
        graph.set_param(src, "resh", Value::Int(64)).unwrap();
        graph.connect(src, res, 0).unwrap();
        graph.set_param(res, "resw", Value::Int(200)).unwrap();
        graph.set_param(res, "resh", Value::Int(150)).unwrap();

        let mut cook = CookEngine::new();
        engine.begin_frame();
        cook.pull(&graph, res, &CookContext::default(), &mut engine)
            .unwrap();
        engine.end_frame();
        let t = engine.output(&graph, res).unwrap();
        assert_eq!((t.key.width, t.key.height), (200, 150));
    }

    #[test]
    fn a_displace_top_moves_pixels() {
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();
        // A horizontal ramp displaced by a constant map shifts sideways.
        let ramp = graph
            .create(root, reg.get("rampTOP").unwrap(), None)
            .unwrap();
        let map = graph
            .create(root, reg.get("constantTOP").unwrap(), None)
            .unwrap();
        let disp = graph
            .create(root, reg.get("displaceTOP").unwrap(), None)
            .unwrap();
        for n in [ramp, map] {
            graph.set_param(n, "resw", Value::Int(64)).unwrap();
            graph.set_param(n, "resh", Value::Int(64)).unwrap();
        }
        graph
            .set_param(map, "color", Value::Vec4([1.0, 0.5, 0.0, 1.0]))
            .unwrap();
        graph.connect(ramp, disp, 0).unwrap();
        graph.connect(map, disp, 1).unwrap();
        graph.set_param(disp, "amount", Value::Float(0.25)).unwrap();

        let mut cook = CookEngine::new();
        let time = CookContext::default();
        let (w, displaced) = cook_and_read(&ctx, &mut engine, &mut cook, &graph, &time, &[disp]);

        // Displacement X = (r=1.0 + offset -0.5) * 0.25 = +0.125 in uv.
        // The ramp is black-to-white left-to-right, so sampling 0.125 further
        // right makes every pixel brighter.
        let plain = engine.output(&graph, ramp).unwrap().clone();
        let base = read_pixels_rgba8(&ctx, &plain).unwrap().2;
        assert!(
            px(&displaced, w, 16, 16)[0] > px(&base, w, 16, 16)[0],
            "displace did not shift the lookup"
        );
    }

    #[test]
    fn feedback_reads_the_previous_frame() {
        // Observed through a consumer, not by reading the Feedback TOP's own
        // texture after the frame: the delay is a promise to whatever samples
        // it *during* a cook, and the node's own texture is mid-hand-off by
        // the time the frame has ended.
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();

        let src = graph
            .create(root, reg.get("constantTOP").unwrap(), Some("src"))
            .unwrap();
        let target = graph
            .create(root, reg.get(ops::NULL).unwrap(), Some("target"))
            .unwrap();
        let fb = graph
            .create(root, reg.get(ops::FEEDBACK).unwrap(), Some("fb"))
            .unwrap();
        let seen = graph
            .create(root, reg.get(ops::NULL).unwrap(), Some("seen"))
            .unwrap();
        graph.set_param(src, "resw", Value::Int(32)).unwrap();
        graph.set_param(src, "resh", Value::Int(32)).unwrap();
        graph.connect(src, target, 0).unwrap();
        graph.connect(fb, seen, 0).unwrap();
        graph
            .set_param(fb, "target", Value::Str("/target".into()))
            .unwrap();

        let mut cook = CookEngine::new();
        let mut ctxt = CookContext::default();

        // Frame 1: white source.
        graph
            .set_param(src, "color", Value::Vec4([1.0, 1.0, 1.0, 1.0]))
            .unwrap();
        engine.begin_frame();
        cook.pull(&graph, seen, &ctxt, &mut engine).unwrap();
        cook.pull(&graph, target, &ctxt, &mut engine).unwrap();
        engine.end_frame();

        // Frame 2: source goes black. Anything sampling the feedback this
        // frame must still get last frame's white.
        ctxt.advance(1.0 / 60.0);
        graph
            .set_param(src, "color", Value::Vec4([0.0, 0.0, 0.0, 1.0]))
            .unwrap();
        engine.begin_frame();
        cook.pull(&graph, seen, &ctxt, &mut engine).unwrap();
        cook.pull(&graph, target, &ctxt, &mut engine).unwrap();
        engine.end_frame();

        let seen_tex = engine.output(&graph, seen).unwrap().clone();
        let (w, _, pixels) = read_pixels_rgba8(&ctx, &seen_tex).unwrap();
        assert!(
            px(&pixels, w, 4, 4)[0] > 250,
            "feedback handed on this frame's black instead of last frame's white"
        );

        let target_tex = engine.output(&graph, target).unwrap().clone();
        let (w, _, pixels) = read_pixels_rgba8(&ctx, &target_tex).unwrap();
        assert!(px(&pixels, w, 4, 4)[0] < 5, "target should be black now");
    }

    #[test]
    fn feedback_lags_a_target_that_is_upstream_of_it() {
        // The camera-motion patch: `source` differenced against
        // `feedback(target = source)`. The target cooks FIRST here, and the
        // delay used to come from the cook order rather than from the
        // operator — so the feedback copied the very frame it was being
        // subtracted from and the difference was identically black. Eleven
        // operators, no warning, no picture.
        let ctx = gpu_or_skip!();
        let mut engine = TopEngine::new(ctx.clone());
        let reg = ops::registry();
        let mut graph = Graph::new();
        let root = graph.root();

        let src = graph
            .create(root, reg.get("constantTOP").unwrap(), Some("src"))
            .unwrap();
        let fb = graph
            .create(root, reg.get(ops::FEEDBACK).unwrap(), Some("fb"))
            .unwrap();
        let diff = graph
            .create(root, reg.get("compositeTOP").unwrap(), Some("diff"))
            .unwrap();
        graph.set_param(src, "resw", Value::Int(32)).unwrap();
        graph.set_param(src, "resh", Value::Int(32)).unwrap();
        graph
            .set_param(fb, "target", Value::Str("/src".into()))
            .unwrap();
        graph
            .set_param(diff, "operation", Value::Str("difference".into()))
            .unwrap();
        graph.connect(src, diff, 0).unwrap();
        graph.connect(fb, diff, 1).unwrap();

        let mut cook = CookEngine::new();
        let mut ctxt = CookContext::default();

        graph
            .set_param(src, "color", Value::Vec4([0.0, 0.0, 0.0, 1.0]))
            .unwrap();
        engine.begin_frame();
        cook.pull(&graph, diff, &ctxt, &mut engine).unwrap();
        engine.end_frame();

        // The source jumps to white. The difference against last frame's
        // black is the whole picture.
        ctxt.advance(1.0 / 60.0);
        graph
            .set_param(src, "color", Value::Vec4([1.0, 1.0, 1.0, 1.0]))
            .unwrap();
        engine.begin_frame();
        cook.pull(&graph, diff, &ctxt, &mut engine).unwrap();
        engine.end_frame();

        let tex = engine.output(&graph, diff).unwrap().clone();
        let (w, _, pixels) = read_pixels_rgba8(&ctx, &tex).unwrap();
        assert!(
            px(&pixels, w, 4, 4)[0] > 250,
            "a change in the source produced no difference — the feedback \
             copied the same frame it was subtracted from"
        );

        // And when nothing changes, nothing is painted.
        ctxt.advance(1.0 / 60.0);
        engine.begin_frame();
        cook.pull(&graph, diff, &ctxt, &mut engine).unwrap();
        engine.end_frame();
        let tex = engine.output(&graph, diff).unwrap().clone();
        let (w, _, pixels) = read_pixels_rgba8(&ctx, &tex).unwrap();
        assert!(
            px(&pixels, w, 4, 4)[0] < 5,
            "a still source should paint nothing"
        );
    }
}
