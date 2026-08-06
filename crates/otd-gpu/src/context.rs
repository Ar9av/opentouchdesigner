//! GPU device handles.
//!
//! The editor shares egui's device rather than creating a second one — node
//! thumbnails are the same textures the cook engine renders into, with no
//! copy and no readback (PLAN.md §2.5, "live viewers everywhere").

use wgpu::{Device, Queue};

#[derive(Clone, Debug)]
pub struct GpuContext {
    pub device: Device,
    pub queue: Queue,
}

impl GpuContext {
    pub fn new(device: Device, queue: Queue) -> Self {
        GpuContext { device, queue }
    }

    /// Stand up an offscreen device. Used by tests and, later, by the
    /// headless CLI runtime (PLAN.md Phase 5).
    pub fn headless() -> Result<Self, String> {
        pollster::block_on(Self::headless_async())
    }

    pub async fn headless_async() -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .map_err(|e| format!("no suitable GPU adapter: {e}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("otd headless device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(|e| format!("could not create device: {e}"))?;
        Ok(GpuContext { device, queue })
    }
}
