//! The assistant can actually reach the TouchDesigner-parity TOPs.
//!
//! Two things have to be true before "put a corner pin on it" works, and
//! neither is proved by the operator existing.
//!
//! **The model has to know what the operator is for.** The catalogue is
//! generated, so a new operator appears in it the day it is registered — but
//! it appeared for a long time as a name and a list of parameter keys, which
//! is enough to spell `slopeTOP` correctly and not enough to know when to
//! reach for it. This checks the prose is carried through.
//!
//! **The plan the model writes has to survive validation.** Menu values with
//! spaces in them (`"input2 g"`), two-input operators wired in the right
//! order, vec2 corner parameters: each is a shape the older operators never
//! exercised, and a plan is refused whole rather than in part, so one of them
//! being rejected means the user gets nothing rather than most of it.

use otd_ai::patch;
use otd_core::{Graph, Value};

/// The operators added for TouchDesigner parity, and what a model has to be
/// able to say about each of them.
const NEW_OPS: &[&str] = &[
    "monochromeTOP",
    "rgbtohsvTOP",
    "hsvtorgbTOP",
    "channelmixTOP",
    "reorderTOP",
    "matteTOP",
    "rgbkeyTOP",
    "lumalevelTOP",
    "functionTOP",
    "limitTOP",
    "embossTOP",
    "slopeTOP",
    "normalmapTOP",
    "antialiasTOP",
    "convolveTOP",
    "cornerpinTOP",
    "cropTOP",
    "fitTOP",
    "lensdistortTOP",
    "remapTOP",
    "tileTOP",
    "lumablurTOP",
    "overTOP",
    "underTOP",
    "addTOP",
    "subtractTOP",
    "multiplyTOP",
    "screenTOP",
    "differenceTOP",
    "insideTOP",
    "outsideTOP",
    "crossTOP",
];

#[test]
fn the_catalogue_says_what_each_new_operator_is_for() {
    let registry = otd_engine::registry();
    let text = patch::catalogue(&registry);

    for op in NEW_OPS {
        let line = text
            .lines()
            .find(|l| l.starts_with(&format!("{op} ")))
            .unwrap_or_else(|| panic!("{op} is not in the catalogue at all"));

        let def = registry.get(op).unwrap();
        assert!(
            line.contains(def.summary),
            "{op} is listed without its summary: {line}"
        );
        // Which input is which matters more here than usual: half of these
        // are two-input operators where the inputs are not interchangeable.
        assert!(
            line.contains(&format!("in[{}]", def.inputs.join(","))),
            "{op} does not say what its inputs are: {line}"
        );
    }
}

/// A plan of the shape a model actually writes, using the new operators,
/// applied against the real registry.
#[test]
fn a_plan_built_out_of_the_new_operators_applies_cleanly() {
    let registry = otd_engine::registry();
    let mut graph = Graph::new();
    let root = graph.root();

    let json = serde_json::json!({
        "notes": "grade, key, warp",
        "nodes": [
            { "name": "src", "op": "noiseTOP", "pos": [0, 0] },
            { "name": "mono", "op": "monochromeTOP", "pos": [200, 0],
              "params": { "mode": "maximum" } },
            // A menu value with a space in it — the shape that had never
            // been sent through the parser before these operators existed.
            { "name": "swap", "op": "reorderTOP", "pos": [400, 0],
              "params": { "red": "input2 g", "alpha": "one" } },
            { "name": "pin", "op": "cornerpinTOP", "pos": [600, 0],
              "params": { "topleft": [0.2, 1.0], "extend": "hold" } },
            { "name": "soft", "op": "lumablurTOP", "pos": [800, 0],
              "params": { "white": 24.0 } },
            { "name": "mixed", "op": "screenTOP", "pos": [1000, 0] },
            { "name": "out1", "op": "nullTOP", "pos": [1200, 0] }
        ],
        "connections": [
            { "from": "src", "to": "mono", "input": 0 },
            { "from": "mono", "to": "swap", "input": 0 },
            { "from": "src", "to": "swap", "input": 1 },
            { "from": "swap", "to": "pin", "input": 0 },
            { "from": "pin", "to": "soft", "input": 0 },
            { "from": "mono", "to": "soft", "input": 1 },
            { "from": "soft", "to": "mixed", "input": 0 },
            { "from": "src", "to": "mixed", "input": 1 },
            { "from": "mixed", "to": "out1", "input": 0 }
        ],
        "viewer": "out1"
    });

    let plan = patch::parse_plan(&json, &registry).expect("plan validates");
    let (applied, viewer) = patch::apply(&mut graph, root, &registry, &plan).expect("plan applies");

    assert!(
        applied.warnings.is_empty(),
        "the plan was accepted but not applied whole: {:?}",
        applied.warnings
    );
    assert_eq!(applied.created.len(), 7);
    assert_eq!(applied.wired, 9);
    assert!(viewer.is_some(), "the viewer was not set");

    // The parameters landed as the values they name, not as the index of a
    // menu or the first item of one.
    let swap = graph.find_from(root, "swap").unwrap();
    assert_eq!(
        graph.node(swap).param("red").unwrap().value,
        Value::Str("input2 g".into()),
        "a menu value with a space in it survived the round trip"
    );
    let pin = graph.find_from(root, "pin").unwrap();
    assert_eq!(
        graph.node(pin).param("topleft").unwrap().value,
        Value::Vec2([0.2, 1.0])
    );

    // And the second input of a two-input operator is the one that was named
    // for it — an operator wired to itself twice looks built and is not.
    let soft = graph.find_from(root, "soft").unwrap();
    let mono = graph.find_from(root, "mono").unwrap();
    assert_eq!(graph.node(soft).inputs[1], Some(mono));
}

/// An operator that does not exist still fails the whole plan.
///
/// Thirty-two new names is thirty-two more things for a model to nearly
/// remember — `blurLumaTOP`, `keyRGBTOP` — and the guarantee that a plan is
/// applied whole or not at all is what stops a near-miss becoming half a
/// patch the user has to unpick.
#[test]
fn a_near_miss_on_a_new_operator_name_is_still_refused() {
    let registry = otd_engine::registry();
    let json = serde_json::json!({
        "nodes": [
            { "name": "a", "op": "monochromeTOP" },
            { "name": "b", "op": "blurLumaTOP" }
        ]
    });
    let err = patch::parse_plan(&json, &registry).unwrap_err();
    assert!(err.contains("blurLumaTOP"), "{err}");
}
