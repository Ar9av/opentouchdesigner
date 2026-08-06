//! External component files and clones — the sharing half of Phase 3.
//!
//! The property being tested throughout is the one PLAN.md calls a showcase:
//! a shared component's network exists in exactly one place, so changing it
//! is one diff and using it costs a reference.

use otd_core::{Component, Graph, NodeId, Project, Value};
use otd_engine::{demo, registry};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("otd-external-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn visualiser_file(path: &std::path::Path) -> NodeId {
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let comp = demo::visualiser(&reg, &mut graph, root, "vis");
    Component::from_graph(&graph, comp, &reg)
        .unwrap()
        .save(path)
        .unwrap();
    comp
}

#[test]
fn a_component_file_holds_the_network_and_the_project_holds_a_reference() {
    let reg = registry();
    let file = scratch("vis.otdc");
    visualiser_file(&file);

    let mut graph = Graph::new();
    let root = graph.root();
    let def = reg.get("containerCOMP").unwrap().clone();
    let comp = graph.create(root, &def, Some("vis1")).unwrap();
    graph
        .attach_external(comp, file.to_str().unwrap(), &reg)
        .unwrap();

    // The network arrived, and the component's API with it.
    assert!(graph.find("/vis1/ring").is_some());
    assert!(graph.node(comp).param("band").is_some());
    assert_eq!(graph.node(comp).input_labels, vec!["in1"]);

    // The project file names the component rather than repeating it.
    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    assert!(text.contains("external:"), "{text}");
    assert!(
        !text.contains("/vis1/ring"),
        "the contents should live in the .otdc, not here:\n{text}"
    );

    let back = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();
    assert!(
        back.find("/vis1/ring").is_some(),
        "loading should re-read the component file"
    );
}

#[test]
fn two_projects_can_share_one_component_file() {
    let reg = registry();
    let file = scratch("shared.otdc");
    visualiser_file(&file);

    let build = |name: &str, hue: f64| {
        let mut graph = Graph::new();
        let root = graph.root();
        let def = reg.get("containerCOMP").unwrap().clone();
        let comp = graph.create(root, &def, Some(name)).unwrap();
        graph
            .attach_external(comp, file.to_str().unwrap(), &reg)
            .unwrap();
        graph.set_param(comp, "hue", Value::Float(hue)).unwrap();
        Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap()
    };

    let a = build("vis1", 0.1);
    let b = build("vis1", 0.7);
    // Same shared definition, different settings — and neither project
    // carries the network.
    assert_ne!(a, b);
    assert!(!a.contains("glslTOP"));
    assert!(!b.contains("glslTOP"));
}

#[test]
fn editing_the_component_file_updates_every_project_that_uses_it() {
    let reg = registry();
    let file = scratch("evolving.otdc");
    visualiser_file(&file);

    let mut graph = Graph::new();
    let root = graph.root();
    let def = reg.get("containerCOMP").unwrap().clone();
    let comp = graph.create(root, &def, Some("vis1")).unwrap();
    graph
        .attach_external(comp, file.to_str().unwrap(), &reg)
        .unwrap();
    graph.set_param(comp, "hue", Value::Float(0.42)).unwrap();
    let project_text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();

    // Someone adds an operator to the shared component.
    let mut component = Component::load(&file).unwrap();
    let mut editing = Graph::new();
    let host = editing.root();
    let target = editing.create(host, &def, Some("editing")).unwrap();
    component.expand_into(&mut editing, target, &reg).unwrap();
    let blur = reg.get("blurTOP").unwrap().clone();
    editing.create(target, &blur, Some("softener")).unwrap();
    component = Component::from_graph(&editing, target, &reg).unwrap();
    component.save(&file).unwrap();

    // The unchanged project picks the change up, and keeps its own setting.
    let reloaded = Project::from_ron(&project_text)
        .unwrap()
        .to_graph(&reg)
        .unwrap();
    assert!(
        reloaded.find("/vis1/softener").is_some(),
        "the project should see the edited component"
    );
    assert_eq!(
        reloaded
            .node(reloaded.find("/vis1").unwrap())
            .param("hue")
            .unwrap()
            .value,
        Value::Float(0.42),
        "and keep the value it had dialled in"
    );
}

#[test]
fn a_missing_component_file_is_an_error_not_an_empty_component() {
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let def = reg.get("containerCOMP").unwrap().clone();
    let comp = graph.create(root, &def, Some("vis1")).unwrap();
    assert!(
        graph
            .attach_external(comp, "/definitely/not/here.otdc", &reg)
            .is_err()
    );

    // And a project referencing one refuses to load rather than opening
    // silently broken.
    let mut project = Project::from_graph(&graph, &reg, 60.0);
    project.nodes[0].external = Some("/definitely/not/here.otdc".into());
    let err = project.to_graph(&reg).unwrap_err();
    assert!(err.to_string().contains("component file"), "{err}");
}

#[test]
fn a_clone_tracks_its_master_through_a_save_and_load() {
    let reg = registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let master = demo::visualiser(&reg, &mut graph, root, "master_vis");
    let def = reg.get("containerCOMP").unwrap().clone();
    let clone = graph.create(root, &def, Some("clone_vis")).unwrap();
    graph.set_clone(clone, Some("/master_vis"));
    graph.sync_clones(&reg);

    assert!(graph.find("/clone_vis/ring").is_some());
    graph.set_param(clone, "hue", Value::Float(0.9)).unwrap();
    graph.set_param(master, "hue", Value::Float(0.1)).unwrap();

    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    assert!(text.contains("clone:"), "{text}");

    let back = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();
    assert!(
        back.find("/clone_vis/ring").is_some(),
        "the clone was re-expanded on load"
    );
    assert_eq!(
        back.node(back.find("/clone_vis").unwrap())
            .param("hue")
            .unwrap()
            .value,
        Value::Float(0.9),
        "the clone kept its own setting"
    );
}
