//! `otd-gpu` — the TOP (texture operator) engine.
//!
//! Everything here is wgpu; nothing here knows about the editor. The public
//! surface is [`TopEngine`], which implements `otd_core::Cooker`.

pub mod context;
pub mod engine;
pub mod ops;
pub mod texture;

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
                let v = f16_to_f32(bits).clamp(0.0, 1.0);
                out.push((v * 255.0 + 0.5) as u8);
            }
        }
    }
    drop(data);
    buffer.unmap();
    Ok((w, h, out))
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
                // Subnormal: renormalise.
                let mut e = -1i32;
                let mut m = mant;
                while m & 0x400 == 0 {
                    m <<= 1;
                    e -= 1;
                }
                let m = m & 0x3ff;
                (sign << 31) | (((127 - 15 + e + 1) as u32) << 23) | (m << 13)
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

    #[test]
    fn feedback_reads_the_previous_frame() {
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
        graph.set_param(src, "resw", Value::Int(32)).unwrap();
        graph.set_param(src, "resh", Value::Int(32)).unwrap();
        graph.connect(src, target, 0).unwrap();
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
        cook.pull(&graph, fb, &ctxt, &mut engine).unwrap();
        cook.pull(&graph, target, &ctxt, &mut engine).unwrap();
        engine.end_frame();

        // Frame 2: source goes black. Feedback must still show white.
        ctxt.advance(1.0 / 60.0);
        graph
            .set_param(src, "color", Value::Vec4([0.0, 0.0, 0.0, 1.0]))
            .unwrap();
        engine.begin_frame();
        cook.pull(&graph, fb, &ctxt, &mut engine).unwrap();
        cook.pull(&graph, target, &ctxt, &mut engine).unwrap();
        engine.end_frame();

        let fb_tex = engine.output(&graph, fb).unwrap().clone();
        let (w, _, pixels) = read_pixels_rgba8(&ctx, &fb_tex).unwrap();
        assert!(
            px(&pixels, w, 4, 4)[0] > 250,
            "feedback showed this frame's black instead of last frame's white"
        );

        let target_tex = engine.output(&graph, target).unwrap().clone();
        let (w, _, pixels) = read_pixels_rgba8(&ctx, &target_tex).unwrap();
        assert!(px(&pixels, w, 4, 4)[0] < 5, "target should be black now");
    }
}
