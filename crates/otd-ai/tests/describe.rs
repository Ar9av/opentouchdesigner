//! What the network description tells the model about the things a wire
//! does not show: exports, feedback loops, and which null is being looked at.
//!
//! Each marker is here because its absence produced a specific wrong plan
//! against a real patch: "make it simpler" deleted the CHOP chain a shader
//! uniform was exported from; "make it look better" put a hue shift inside
//! the feedback loop; "combine everything" added a fourth null.

use otd_ai::patch::describe;
use otd_core::{Graph, Value};
use otd_engine::registry;

#[test]
fn exports_loops_and_the_viewer_are_said_out_loud() {
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let mk = |g: &mut Graph, op: &str, name: &str| {
        g.create(root, reg.get(op).unwrap(), Some(name)).unwrap()
    };
    let src = mk(&mut graph, "noiseTOP", "src1");
    let fb = mk(&mut graph, "feedbackTOP", "fb1");
    let decay = mk(&mut graph, "levelTOP", "decay1");
    let mix = mk(&mut graph, "compositeTOP", "mix1");
    let out = mk(&mut graph, "nullTOP", "out1");
    let _lfo = mk(&mut graph, "lfoCHOP", "lfo1");
    graph.connect(src, mix, 0).unwrap();
    graph.connect(fb, decay, 0).unwrap();
    graph.connect(decay, mix, 1).unwrap();
    graph.connect(mix, out, 0).unwrap();
    graph
        .set_param(fb, "target", Value::Str("out1".into()))
        .unwrap();
    graph
        .set_export(decay, "brightness", "/lfo1", "lfo")
        .unwrap();

    let text = describe(&graph, root, None, Some(out), &[], &reg);
    let line = |name: &str| {
        text.lines()
            .find(|l| l.starts_with(&format!("- {name} ")))
            .unwrap_or_else(|| panic!("no line for {name} in:\n{text}"))
            .to_string()
    };
    assert!(
        line("decay1").contains("brightness=export(lfo1:lfo)"),
        "{}",
        line("decay1")
    );
    assert!(
        line("lfo1").contains("[READ BY decay1.brightness"),
        "{}",
        line("lfo1")
    );
    for name in ["decay1", "mix1", "out1"] {
        assert!(
            line(name).contains("IN THE FEEDBACK LOOP of fb1"),
            "{}",
            line(name)
        );
    }
    assert!(
        !line("src1").contains("FEEDBACK LOOP"),
        "the source is not the loop"
    );
    assert!(
        !line("fb1").contains("FEEDBACK LOOP"),
        "the feedback node itself is the loop's owner"
    );
    assert!(line("out1").contains("[VIEWER"), "{}", line("out1"));
}
