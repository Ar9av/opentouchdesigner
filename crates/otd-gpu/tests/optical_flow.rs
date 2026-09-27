use otd_core::{CookContext, CookEngine, Graph, Value};
use otd_gpu::{GpuContext, TopEngine, ops, read_pixels_rgba8};

fn flow(shift: [f64; 2], flat: bool) -> [f64; 3] {
    let gpu = GpuContext::headless().expect("optical flow tests need a GPU");
    let reg = ops::registry();
    let mut graph = Graph::new();
    let root = graph.root();
    let make = |graph: &mut Graph, shift: [f64; 2]| {
        let id = graph.create(root, reg.get("glslTOP").unwrap(), None).unwrap();
        graph.set_param(id, "language", Value::Str("glsl".into())).unwrap();
        graph.set_param(id, "resw", Value::Int(64)).unwrap();
        graph.set_param(id, "resh", Value::Int(64)).unwrap();
        let source = if flat {
            "void mainImage(out vec4 c, in vec2 p) { c=vec4(0.4,0.4,0.4,1.0); }".into()
        } else {
            format!("void mainImage(out vec4 c, in vec2 p) {{
                vec2 q=vec2(p.x,64.0-p.y)-vec2({},{});
                float v=0.5+0.2*sin(q.x*0.35)+0.2*cos(q.y*0.31);
                c=vec4(v,v,v,1.0);
            }}", shift[0], shift[1])
        };
        graph.set_param(id, "source", Value::Str(source)).unwrap();
        id
    };
    let current = make(&mut graph, shift);
    let previous = make(&mut graph, [0.0, 0.0]);
    let flow = graph.create(root, reg.get("opticalflowTOP").unwrap(), None).unwrap();
    graph.set_param(flow, "gain", Value::Float(8.0)).unwrap();
    graph.connect(current, flow, 0).unwrap();
    graph.connect(previous, flow, 1).unwrap();
    let mut engine = TopEngine::new(gpu.clone());
    let mut cook = CookEngine::new();
    engine.begin_frame();
    cook.pull(&graph, flow, &CookContext::default(), &mut engine).unwrap();
    engine.end_frame();
    let (w, _, pixels) = read_pixels_rgba8(&gpu, engine.output(&graph, flow).unwrap()).unwrap();
    let mut sum = [0.0; 3];
    for y in 8..56 {
        for x in 8..56 {
            let index = ((y * w + x) * 4) as usize;
            for c in 0..3 { sum[c] += pixels[index + c] as f64 / 255.0; }
        }
    }
    sum.map(|s| s / (48.0 * 48.0))
}

#[test]
fn identical_and_flat_frames_have_neutral_motion() {
    for flat in [false, true] {
        let v = flow([0.0, 0.0], flat);
        assert!((v[0] - 0.5).abs() < 0.005 && (v[1] - 0.5).abs() < 0.005, "{v:?}");
        assert!(v[2] < 0.005, "{v:?}");
    }
}

#[test]
fn translated_texture_recovers_direction_and_subpixel_magnitude() {
    for shift in [[0.5, 0.0], [-0.5, 0.0], [0.0, 0.5], [0.0, -0.5]] {
        let v = flow(shift, false);
        for axis in 0..2 {
            let pixels = (v[axis] - 0.5) * 64.0 / 8.0;
            assert!((pixels - shift[axis]).abs() < 0.09, "{shift:?}: {v:?} -> {pixels}");
        }
    }
}
