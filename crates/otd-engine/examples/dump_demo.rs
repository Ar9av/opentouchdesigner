//! Write a built-in demo out as a project file, for the CLI to render.
fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().expect("demo name");
    let out = args.next().expect("output path");
    let reg = otd_engine::registry();
    let (mut graph, root) = otd_engine::demo::by_name(&name, &reg).expect("no such demo");
    graph.node_mut_quiet(root).flags.render = true;
    otd_core::Project::from_graph(&graph, &reg, 60.0)
        .save(&out)
        .unwrap();
    println!("{out}");
}
