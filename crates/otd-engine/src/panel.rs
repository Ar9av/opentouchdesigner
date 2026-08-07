//! Panel components — the widgets a show is driven from.
//!
//! TouchDesigner's panel COMPs are a control surface built out of the same
//! node graph as everything else, and that is the part worth copying: a button
//! is a node, its state is a parameter, and a parameter is already something
//! the rest of the network can export from, bind to and animate. So there is
//! no new value system here and no widget-to-operator plumbing — a Slider COMP
//! is a node whose `value` parameter happens to have a person moving it.
//!
//! Three consequences fall out of that choice, all of them good:
//!
//!  * A panel's state is **in the project file**, because parameters are.
//!    Reopening a show restores the fader positions.
//!  * Undo works on it, because parameter edits are what undo is made of.
//!  * A widget can be driven by the network as readily as it drives the
//!    network — exporting a CHOP to a slider's `value` moves the slider.
//!
//! Layout is in **fractions of the output**, not pixels. A panel laid out
//! against a 1280×720 viewer has to land in the same place on a 4K projector,
//! and the alternative — pixels plus a design resolution — is a second number
//! to keep in sync for no benefit.

use otd_core::indexmap::IndexMap;
use otd_core::{Connector, Family, Graph, NodeId, OpDef, OpRegistry, Param};

pub const BUTTON: &str = "buttonCOMP";
pub const SLIDER: &str = "sliderCOMP";
pub const FIELD: &str = "fieldCOMP";

/// Whether an operator type is a widget this module draws.
pub fn is_panel(op_type: &str) -> bool {
    matches!(op_type, BUTTON | SLIDER | FIELD)
}

macro_rules! params {
    ($($key:expr => $param:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m: IndexMap<String, Param> = IndexMap::new();
        $( m.insert($key.into(), $param); )*
        m
    }};
}

/// Where a widget sits, as a fraction of the output. Shared by all three so a
/// panel can be laid out without learning three coordinate systems.
fn with_layout(mut m: IndexMap<String, Param>) -> IndexMap<String, Param> {
    for (key, label, default) in [
        ("x", "X", 0.05),
        ("y", "Y", 0.05),
        ("w", "Width", 0.2),
        ("h", "Height", 0.08),
    ] {
        m.insert(
            key.into(),
            Param::float(default).with_label(label).with_range(0.0, 1.0),
        );
    }
    m.insert("label".into(), Param::str("").with_label("Label"));
    m
}

fn params_button() -> IndexMap<String, Param> {
    with_layout(params! {
        "value" => Param::float(0.0).with_label("Value").with_range(0.0, 1.0),
        "mode" => Param::menu("toggle", &["toggle", "momentary"]).with_label("Mode"),
    })
}

fn params_slider() -> IndexMap<String, Param> {
    with_layout(params! {
        "value" => Param::float(0.0).with_label("Value"),
        "min" => Param::float(0.0).with_label("Minimum"),
        "max" => Param::float(1.0).with_label("Maximum"),
        "orientation" => Param::menu("horizontal", &["horizontal", "vertical"])
            .with_label("Orientation"),
    })
}

fn params_field() -> IndexMap<String, Param> {
    with_layout(params! {
        "text" => Param::str("").with_label("Text"),
    })
}

pub fn defs() -> Vec<OpDef> {
    vec![
        OpDef {
            type_name: BUTTON,
            input_families: &[],
            label: "Button",
            family: Family::Comp,
            inputs: &[],
            summary: "A button on the output. Its state is the Value parameter.",
            time_dependent: false,
            params: params_button,
            connector: Connector::None,
        },
        OpDef {
            type_name: SLIDER,
            input_families: &[],
            label: "Slider",
            family: Family::Comp,
            inputs: &[],
            summary: "A fader on the output. Its position is the Value parameter.",
            time_dependent: false,
            params: params_slider,
            connector: Connector::None,
        },
        OpDef {
            type_name: FIELD,
            input_families: &[],
            label: "Field",
            family: Family::Comp,
            inputs: &[],
            summary: "An editable text field on the output.",
            time_dependent: false,
            params: params_field,
            connector: Connector::None,
        },
    ]
}

pub fn register(registry: &mut OpRegistry) {
    for def in defs() {
        registry.register(def);
    }
}

/// One widget, resolved out of the graph and ready to draw.
///
/// The editor gets this rather than a `NodeId` and a pile of parameter
/// lookups, so the "what is on the panel" question is answered here — in a
/// crate with no UI dependency, where it can be tested — and the editor is
/// left with only the drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub id: NodeId,
    pub kind: Kind,
    /// Fractions of the output: x, y, width, height.
    pub rect: [f32; 4],
    pub label: String,
    pub value: f64,
    pub text: String,
    /// Slider ends, and whether it stands up.
    pub range: (f64, f64),
    pub vertical: bool,
    pub momentary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Button,
    Slider,
    Field,
}

/// Every widget in the project, in a stable order.
///
/// Sorted by path so two buttons never swap places between frames, which
/// would make one of them impossible to click.
pub fn widgets(graph: &Graph) -> Vec<Widget> {
    let mut ids: Vec<NodeId> = graph
        .walk()
        .into_iter()
        .filter(|id| *id != graph.root())
        .filter(|id| graph.get(*id).is_some_and(|n| is_panel(&n.op_type)))
        .collect();
    ids.sort_by_key(|id| graph.path(*id));

    let ctx = otd_core::EvalContext::default();
    ids.into_iter()
        .filter_map(|id| {
            let node = graph.get(id)?;
            let f = |key: &str| {
                node.param(key)
                    .map(|p| p.eval(&ctx).as_f64())
                    .unwrap_or(0.0)
            };
            let s = |key: &str| {
                node.param(key)
                    .map(|p| p.eval(&ctx).as_str())
                    .unwrap_or_default()
            };
            let kind = match node.op_type.as_str() {
                BUTTON => Kind::Button,
                SLIDER => Kind::Slider,
                _ => Kind::Field,
            };
            let label = {
                let given = s("label");
                // An unlabelled widget shows its node name, so a fresh button
                // is identifiable without typing anything.
                if given.trim().is_empty() {
                    node.name.clone()
                } else {
                    given
                }
            };
            Some(Widget {
                id,
                kind,
                rect: [f("x") as f32, f("y") as f32, f("w") as f32, f("h") as f32],
                label,
                value: f("value"),
                text: s("text"),
                range: (f("min"), f("max")),
                vertical: s("orientation") == "vertical",
                momentary: s("mode") == "momentary",
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use otd_core::Value;

    fn graph_with(op: &str, name: &str) -> (Graph, NodeId) {
        let mut reg = OpRegistry::new();
        register(&mut reg);
        let mut graph = Graph::new();
        let root = graph.root();
        let id = graph
            .create(root, reg.get(op).unwrap(), Some(name))
            .unwrap();
        (graph, id)
    }

    #[test]
    fn a_widget_reads_its_layout_and_state_from_parameters() {
        let (mut graph, id) = graph_with(SLIDER, "fader1");
        graph.set_param(id, "x", Value::Float(0.25)).unwrap();
        graph.set_param(id, "value", Value::Float(0.7)).unwrap();
        graph.set_param(id, "max", Value::Float(4.0)).unwrap();

        let w = widgets(&graph);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, Kind::Slider);
        assert_eq!(w[0].rect[0], 0.25);
        assert_eq!(w[0].value, 0.7);
        assert_eq!(w[0].range, (0.0, 4.0));
    }

    #[test]
    fn an_unlabelled_widget_shows_its_node_name() {
        let (graph, _) = graph_with(BUTTON, "go");
        assert_eq!(widgets(&graph)[0].label, "go");
    }

    #[test]
    fn widgets_keep_a_stable_order_so_they_stay_clickable() {
        let mut reg = OpRegistry::new();
        register(&mut reg);
        let mut graph = Graph::new();
        let root = graph.root();
        // Created out of order on purpose.
        for name in ["zed", "alpha", "mid"] {
            graph
                .create(root, reg.get(BUTTON).unwrap(), Some(name))
                .unwrap();
        }
        let names: Vec<String> = widgets(&graph).iter().map(|w| w.label.clone()).collect();
        assert_eq!(names, vec!["alpha", "mid", "zed"]);
    }

    #[test]
    fn nothing_but_a_panel_comp_is_a_widget() {
        let mut reg = otd_gpu::ops::registry();
        register(&mut reg);
        let mut graph = Graph::new();
        let root = graph.root();
        graph
            .create(root, reg.get("noiseTOP").unwrap(), Some("noise1"))
            .unwrap();
        graph
            .create(root, reg.get(BUTTON).unwrap(), Some("go"))
            .unwrap();
        assert_eq!(widgets(&graph).len(), 1);
    }
}
