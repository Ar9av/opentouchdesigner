//! `otd-engine` — the cross-family cook.
//!
//! `otd-core` drives scheduling and knows nothing about what an operator
//! produces; `otd-gpu` and `otd-chop` each produce one family and know nothing
//! about each other. This crate is the only place the two meet, and it exists
//! for one reason: a TOP parameter in Export mode has to read a CHOP channel
//! cooked in the same frame (PLAN.md §2.3).
//!
//! The channel store is owned here rather than by the CHOP engine so that,
//! while a node is cooking, the freshly cooked channels can be read
//! immutably by the parameters of whatever cooks next.

pub mod demo;

use otd_chop::InputState;
use otd_chop::engine::{ChannelStore, ChopEngine, Network};
use otd_core::{CookContext, CookError, Cooker, Family, Graph, NodeId, OpRegistry};
use otd_gpu::{GpuContext, TopEngine};

pub use otd_chop::{Channel, ChopData};

/// Every operator this build knows about, across all families.
pub fn registry() -> OpRegistry {
    let mut r = otd_gpu::ops::registry();
    for spec in otd_chop::ops::all() {
        r.register(spec.def.clone());
    }
    r
}

pub struct Engines {
    pub top: TopEngine,
    pub chop: ChopEngine,
    /// Cooked channels, owned here — see the module docs.
    pub channels: ChannelStore,
}

impl Engines {
    pub fn new(gpu: GpuContext) -> Self {
        Engines {
            top: TopEngine::new(gpu),
            chop: ChopEngine::new(),
            channels: ChannelStore::new(),
        }
    }

    pub fn begin_frame(&mut self) {
        self.top.begin_frame();
    }

    pub fn end_frame(&mut self) {
        self.top.end_frame();
    }

    /// Mouse and keyboard state for the input CHOPs, pushed in by the editor.
    pub fn set_input_state(&mut self, state: InputState) {
        self.chop.io.input = state;
    }

    pub fn chop_data(&self, id: NodeId) -> Option<&ChopData> {
        self.channels.get(id)
    }

    /// The value a parameter would read if it exported from this channel.
    pub fn channel_value(&self, graph: &Graph, path: &str, channel: &str) -> Option<f32> {
        use otd_core::ChannelSource;
        Network {
            graph,
            chops: &self.channels,
        }
        .channel(path, channel)
    }

    /// The value a parameter would read if it were bound to this one.
    pub fn param_value(&self, graph: &Graph, path: &str, param: &str) -> Option<otd_core::Value> {
        use otd_core::ChannelSource;
        Network {
            graph,
            chops: &self.channels,
        }
        .param_value(path, param)
    }

    /// A device or shader message for a node, for display on its body.
    pub fn node_status(&self, graph: &Graph, id: NodeId) -> Option<String> {
        if let Some(err) = self.top.shader_error(id) {
            return Some(err.to_string());
        }
        self.chop.status(&graph.path(id)).map(|s| s.to_string())
    }

    pub fn forget(&mut self, id: NodeId) {
        self.top.forget(id);
        self.chop.forget(id);
        self.channels.remove(id);
    }

    pub fn reset(&mut self) {
        self.top.reset();
        self.chop.reset();
        self.channels.clear();
    }
}

impl Cooker for Engines {
    fn extra_inputs(&self, graph: &Graph, id: NodeId) -> Vec<NodeId> {
        self.top.extra_inputs(graph, id)
    }

    fn cook(&mut self, graph: &Graph, id: NodeId, ctx: &CookContext) -> Result<(), CookError> {
        let Some(family) = graph.get(id).map(|n| n.family) else {
            return Err(CookError::NoSuchNode);
        };
        let Engines {
            top,
            chop,
            channels,
        } = self;

        match family {
            Family::Chop => {
                let data = {
                    let net = Network {
                        graph,
                        chops: channels,
                    };
                    let eval = ctx.eval_ctx_with(&net);
                    chop.cook_node(graph, id, ctx, &eval, channels)?
                };
                channels.insert(id, data);
                Ok(())
            }
            Family::Top => {
                let net = Network {
                    graph,
                    chops: channels,
                };
                let eval = ctx.eval_ctx_with(&net);
                top.cook_node(graph, id, ctx, &eval)
            }
            // COMPs hold sub-networks; the other families are later phases.
            _ => Ok(()),
        }
    }
}
