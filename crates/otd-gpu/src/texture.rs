//! Texture allocation and recycling.
//!
//! PLAN.md §3 asks for "a transient texture pool keyed by (size, format)".
//! The wrinkle the plan doesn't spell out: because the cook engine *memoizes*,
//! a TOP's output has to survive for as long as its cache is valid — a static
//! branch cooks on frame 1 and is still being read on frame 10000. So node
//! outputs are retained, and the pool exists to recycle allocations when a
//! node is resized, deleted, or when a multi-pass operator wants scratch.

use std::collections::HashMap;

use wgpu::{Device, Texture, TextureFormat, TextureView};

/// Every TOP is 16-bit float. TD's free tier caps resolution and its default
/// pipeline is 8-bit; uncapped resolution and HDR headroom are two of the
/// wedge features, so there is no reason to start at 8-bit.
pub const TOP_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

pub const MAX_DIMENSION: u32 = 16384;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexKey {
    pub width: u32,
    pub height: u32,
    pub format: TextureFormat,
}

/// A texture plus its default view. Views are created once, not per frame.
#[derive(Debug, Clone)]
pub struct TopTexture {
    pub texture: Texture,
    pub view: TextureView,
    pub key: TexKey,
    /// Bumped every time the underlying texture object is replaced, so the
    /// editor knows when to re-register it with egui.
    pub generation: u64,
}

#[derive(Default)]
pub struct TexturePool {
    free: HashMap<TexKey, Vec<TopTexture>>,
    generation: u64,
    pub created: u64,
    pub reused: u64,
}

impl TexturePool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn acquire(&mut self, device: &Device, width: u32, height: u32) -> TopTexture {
        let key = TexKey {
            width: width.clamp(1, MAX_DIMENSION),
            height: height.clamp(1, MAX_DIMENSION),
            format: TOP_FORMAT,
        };
        if let Some(t) = self.free.get_mut(&key).and_then(|v| v.pop()) {
            self.reused += 1;
            return t;
        }
        self.generation += 1;
        self.created += 1;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("otd top"),
            size: wgpu::Extent3d {
                width: key.width,
                height: key.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: key.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        TopTexture {
            texture,
            view,
            key,
            generation: self.generation,
        }
    }

    pub fn release(&mut self, tex: TopTexture) {
        let bucket = self.free.entry(tex.key).or_default();
        // Cap the free list so a resolution sweep doesn't pin VRAM forever.
        if bucket.len() < 8 {
            bucket.push(tex);
        }
    }

    /// Total bytes held by the free list — reported by the performance panel.
    pub fn pooled_bytes(&self) -> u64 {
        self.free
            .iter()
            .map(|(k, v)| {
                let bpp = 8u64; // Rgba16Float
                k.width as u64 * k.height as u64 * bpp * v.len() as u64
            })
            .sum()
    }

    pub fn clear(&mut self) {
        self.free.clear();
    }
}
