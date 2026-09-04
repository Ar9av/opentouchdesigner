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

use std::cell::RefCell;

use otd_chop::InputState;
use otd_chop::engine::{ChannelStore, ChopEngine, Network};
use otd_core::{ChannelSource, CookContext, CookError, Cooker, Family, Graph, NodeId, OpRegistry};
use otd_dat::{DatEngine, DatStore, ScriptHost};
use otd_gpu::{GpuContext, TopEngine};
use otd_py::PyEngine;
use otd_sop::{Geometry, GeometryStore, SopEngine};

pub use otd_chop::{Channel, ChopData};
pub use otd_dat::DatData;
pub use otd_sop::Geometry as SopGeometry;

/// Every operator this build knows about, across all families.
pub fn registry() -> OpRegistry {
    let mut r = otd_gpu::ops::registry();
    for spec in otd_chop::ops::all() {
        r.register(spec.def.clone());
    }
    for spec in otd_dat::ops::all() {
        r.register(spec.def.clone());
    }
    for spec in otd_sop::ops::all() {
        r.register(spec.def.clone());
    }
    // The scene components and materials live with the renderer.
    otd_gpu::scene::register(&mut r);
    r
}

/// The network plus the interpreter: everything a parameter can read.
///
/// `otd-chop`'s `Network` answers channel and parameter lookups; this adds
/// the Python fallback for expressions the built-in language cannot parse.
/// The interpreter is behind a `RefCell` because compiling caches, and a
/// parameter evaluation is a `&self` call all the way down.
struct FullNetwork<'a> {
    chops: Network<'a>,
    python: &'a RefCell<PyEngine>,
}

impl otd_core::ChannelSource for FullNetwork<'_> {
    fn channel(&self, op_path: &str, channel: &str) -> Option<f32> {
        self.chops.channel(op_path, channel)
    }
    fn param_value(&self, op_path: &str, param: &str) -> Option<otd_core::Value> {
        self.chops.param_value(op_path, param)
    }
    fn parent_param(&self, node_path: &str, param: &str) -> Option<otd_core::Value> {
        self.chops.parent_param(node_path, param)
    }
    fn eval_python(
        &self,
        source: &str,
        ctx: &otd_core::EvalContext,
        path: &str,
    ) -> Option<Result<otd_core::Value, String>> {
        // A re-entrant evaluation — Python reading a parameter that is itself
        // Python — would deadlock on the RefCell. Refusing is the honest
        // outcome, and the parameter falls back to its constant.
        let mut py = self.python.try_borrow_mut().ok()?;
        Some(py.eval(source, ctx, path))
    }
}

/// Lets a Script DAT run its source without `otd-dat` depending on PyO3.
struct PythonScripts<'a> {
    python: &'a RefCell<PyEngine>,
}

impl ScriptHost for PythonScripts<'_> {
    fn run_table(
        &self,
        source: &str,
        ctx: &otd_core::EvalContext,
        path: &str,
    ) -> Result<Vec<Vec<String>>, String> {
        self.python
            .try_borrow_mut()
            .map_err(|_| "a script cannot run inside another script".to_string())?
            .run_table(source, ctx, path)
    }
}

/// What the renderer is allowed to ask the rest of the network for.
struct SceneView<'a> {
    graph: &'a Graph,
    geometry: &'a GeometryStore,
    channels: &'a ChannelStore,
}

impl otd_gpu::scene::Scene for SceneView<'_> {
    fn geometry(&self, path: &str) -> Option<&Geometry> {
        // Resolving means a path can name a component and get the geometry
        // its Out operator produces.
        let id = self.graph.resolve_output(self.graph.find(path)?)?;
        self.geometry.get(id)
    }

    fn channels(&self, path: &str) -> Option<Vec<(String, Vec<f32>)>> {
        let id = self.graph.resolve_output(self.graph.find(path)?)?;
        let data = self.channels.get(id)?;
        Some(
            data.channels
                .iter()
                .map(|c| (c.name.clone(), c.samples.clone()))
                .collect(),
        )
    }
}

pub struct Engines {
    pub top: TopEngine,
    pub chop: ChopEngine,
    pub dat: DatEngine,
    pub dats: DatStore,
    pub sop: SopEngine,
    pub geometry: GeometryStore,
    /// Cooked channels, owned here — see the module docs.
    pub channels: ChannelStore,
    /// The embedded interpreter. Started once; a machine where it cannot
    /// start still opens projects, with Python expressions reporting why.
    pub python: RefCell<PyEngine>,
}

impl Engines {
    pub fn new(gpu: GpuContext) -> Self {
        Engines {
            top: TopEngine::new(gpu),
            chop: ChopEngine::new(),
            dat: DatEngine::new(),
            dats: DatStore::new(),
            sop: SopEngine::new(),
            geometry: GeometryStore::new(),
            channels: ChannelStore::new(),
            python: RefCell::new(PyEngine::new()),
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
        Network {
            graph,
            chops: &self.channels,
        }
        .channel(path, channel)
    }

    /// Evaluate a parameter the way the cook would, for the editor's display.
    pub fn eval_param(
        &self,
        graph: &Graph,
        id: NodeId,
        key: &str,
        ctx: &CookContext,
    ) -> Option<otd_core::Value> {
        let path = graph.path(id);
        let net = FullNetwork {
            chops: Network {
                graph,
                chops: &self.channels,
            },
            python: &self.python,
        };
        let eval = otd_core::EvalContext {
            path: Some(&path),
            ..ctx.eval_ctx_with(&net)
        };
        Some(graph.get(id)?.param(key)?.eval(&eval))
    }

    /// Why Python is unavailable, if it is.
    pub fn python_error(&self) -> Option<String> {
        self.python.borrow().startup_error.clone()
    }

    /// The value a parameter would read if it were bound to this one.
    pub fn param_value(&self, graph: &Graph, path: &str, param: &str) -> Option<otd_core::Value> {
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
        let path = graph.path(id);
        self.chop
            .status(&path)
            .or_else(|| self.dat.status_of(&path))
            .map(|s| s.to_string())
    }

    pub fn dat_data(&self, id: NodeId) -> Option<&DatData> {
        self.dats.get(id)
    }

    pub fn geometry_of(&self, id: NodeId) -> Option<&Geometry> {
        self.geometry.get(id)
    }

    pub fn forget(&mut self, id: NodeId) {
        self.top.forget(id);
        self.chop.forget(id);
        self.channels.remove(id);
        self.dats.remove(id);
        self.geometry.remove(id);
    }

    pub fn reset(&mut self) {
        self.top.reset();
        self.chop.reset();
        self.dat.reset();
        self.dats.clear();
        self.geometry.clear();
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
            dat,
            dats,
            sop,
            geometry,
            channels,
            python,
        } = self;

        match family {
            Family::Chop => {
                let data = {
                    let net = FullNetwork {
                        chops: Network {
                            graph,
                            chops: channels,
                        },
                        python,
                    };
                    let eval = ctx.eval_ctx_with(&net);
                    chop.cook_node(graph, id, ctx, &eval, channels)?
                };
                channels.insert(id, data);
                Ok(())
            }
            Family::Sop => {
                let data = {
                    let net = FullNetwork {
                        chops: Network {
                            graph,
                            chops: channels,
                        },
                        python,
                    };
                    let eval = ctx.eval_ctx_with(&net);
                    sop.cook_node(graph, id, ctx, &eval, geometry)?
                };
                geometry.insert(id, data);
                Ok(())
            }
            Family::Top => {
                let net = FullNetwork {
                    chops: Network {
                        graph,
                        chops: channels,
                    },
                    python,
                };
                let eval = ctx.eval_ctx_with(&net);
                let view = SceneView {
                    graph,
                    geometry,
                    channels,
                };
                top.cook_node(graph, id, ctx, &eval, Some(&view))
            }
            Family::Dat => {
                let data = {
                    let net = FullNetwork {
                        chops: Network {
                            graph,
                            chops: channels,
                        },
                        python,
                    };
                    let eval = ctx.eval_ctx_with(&net);
                    let scripts = PythonScripts { python };
                    dat.cook_node(graph, id, ctx, &eval, dats, Some(&scripts))?
                };
                dats.insert(id, data);
                Ok(())
            }
            // COMPs hold sub-networks; the other families are later phases.
            _ => Ok(()),
        }
    }
}
