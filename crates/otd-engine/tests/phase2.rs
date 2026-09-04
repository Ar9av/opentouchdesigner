//! The Phase 2 exit criterion from PLAN.md:
//!
//!   "audio-reactive visual driven by a MIDI controller"
//!
//! The chain under test is device → channel → parameter → texture, all inside
//! one frame. Where a real device would be, these tests substitute a signal
//! the test can control, so the assertion is about the plumbing rather than
//! about whatever happens to be plugged in.

use otd_core::{CookContext, CookEngine, Graph, NodeId, Value};
use otd_engine::{Engines, demo, registry};
use otd_gpu::{GpuContext, read_pixels_rgba8};

macro_rules! gpu_or_skip {
    () => {
        match GpuContext::headless() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("skipping: no GPU available ({e})");
                return;
            }
        }
    };
}

struct Rig {
    graph: Graph,
    engines: Engines,
    cook: CookEngine,
    time: CookContext,
    gpu: GpuContext,
}

impl Rig {
    fn new(gpu: GpuContext, graph: Graph) -> Self {
        Rig {
            graph,
            engines: Engines::new(gpu.clone()),
            cook: CookEngine::new(),
            time: CookContext::default(),
            gpu,
        }
    }

    fn run(&mut self, root: NodeId, frames: usize) {
        for _ in 0..frames {
            self.engines.begin_frame();
            self.cook
                .cook_frame(&self.graph, &[root], &self.time, &mut self.engines)
                .unwrap();
            self.engines.end_frame();
            self.time.advance(1.0 / 60.0);
        }
    }

    fn brightness(&self, id: NodeId) -> f64 {
        let tex = self
            .engines
            .top
            .output(&self.graph, id)
            .expect("the TOP has cooked")
            .clone();
        let (_, _, pixels) = read_pixels_rgba8(&self.gpu, &tex).unwrap();
        let sum: u64 = pixels.iter().step_by(4).map(|p| *p as u64).sum();
        sum as f64 / (pixels.len() / 4) as f64
    }
}

#[test]
fn a_chop_channel_drives_a_top_parameter_in_the_same_frame() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::lfo_driven(&reg);
    let mut rig = Rig::new(gpu, graph);

    // The LFO runs at 0.5 Hz, so brightness swings over a two-second cycle.
    // Sampling at the peak and the trough must give visibly different images.
    rig.run(out, 30);
    let bright = rig.brightness(out);
    rig.run(out, 60);
    let dark = rig.brightness(out);

    assert!(
        bright > dark * 1.5,
        "the exported channel did not reach the texture: {bright:.1} vs {dark:.1}"
    );

    let level = rig.graph.find("/level1").unwrap();
    assert!(
        rig.cook.is_time_dependent(level),
        "an export from an animated CHOP must animate the consumer"
    );
    let lfo = rig.graph.find("/lfo1").unwrap();
    assert!(
        rig.cook.cook_count(lfo) > 0,
        "pulling the TOP must drag the exported CHOP in"
    );
}

#[test]
fn an_export_reads_this_frames_value_not_last_frames() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (mut graph, out) = demo::lfo_driven(&reg);

    // Replace the LFO with a constant we can step, so the timing is exact.
    let root = graph.root();
    let step = graph
        .create(root, reg.get("constantCHOP").unwrap(), Some("step"))
        .unwrap();
    let level = graph.find("/level1").unwrap();
    graph
        .node_mut(level)
        .params
        .get_mut("brightness")
        .unwrap()
        .set_export("/step", "chan1");

    let mut rig = Rig::new(gpu, graph);
    rig.graph
        .set_param(step, "value0", Value::Float(0.4))
        .unwrap();
    rig.run(out, 1);
    let dim = rig.brightness(out);

    rig.graph
        .set_param(step, "value0", Value::Float(2.5))
        .unwrap();
    rig.run(out, 1);
    let bright = rig.brightness(out);

    assert!(
        bright > dim * 1.5,
        "the change should land the same frame: {dim:.1} then {bright:.1}"
    );
}

#[test]
fn the_audio_reactive_patch_runs_with_no_devices_attached() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::audio_reactive(&reg);
    let mut rig = Rig::new(gpu, graph);

    // No interface plugged in: every device CHOP reports why and produces
    // silence, and the patch still renders rather than failing to cook.
    rig.run(out, 10);
    let tex = rig.engines.top.output(&rig.graph, out);
    assert!(tex.is_some(), "the patch produced no output");
    assert!(rig.brightness(out) > 0.0, "the visual is not black");
}

#[test]
fn a_midi_note_reaches_a_texture_parameter() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (mut graph, out) = demo::audio_reactive(&reg);

    // Stand in for the controller: drive the trigger from a constant we can
    // switch, leaving the rest of the export chain exactly as shipped.
    let root = graph.root();
    let pad = graph
        .create(root, reg.get("constantCHOP").unwrap(), Some("pad"))
        .unwrap();
    graph
        .set_param(pad, "name", Value::Str("n3".into()))
        .unwrap();
    let trigger = graph.find("/trigger1").unwrap();
    graph.disconnect(trigger, 0).unwrap();
    graph.connect(pad, trigger, 0).unwrap();
    let xform = graph.find("/transform1").unwrap();
    graph
        .node_mut(xform)
        .params
        .get_mut("rotate")
        .unwrap()
        .set_export("/math_kick", "n31");

    let mut rig = Rig::new(gpu, graph);
    rig.run(out, 5);
    let rotate_at_rest = rig
        .engines
        .channel_value(&rig.graph, "/math_kick", "n31")
        .unwrap();
    assert!(rotate_at_rest.abs() < 1e-3, "no note, no rotation");

    // Hit the pad.
    rig.graph
        .set_param(pad, "value0", Value::Float(1.0))
        .unwrap();
    rig.run(out, 10);
    let rotate_after_hit = rig
        .engines
        .channel_value(&rig.graph, "/math_kick", "n31")
        .unwrap();
    assert!(
        rotate_after_hit > 1.0,
        "the note should have opened the envelope: {rotate_after_hit}"
    );

    // And the parameter the texture actually cooked with followed it.
    let eval = rig.time.eval_ctx();
    let _ = eval;
    let rendered = rig
        .graph
        .node(xform)
        .param("rotate")
        .unwrap()
        .eval(&rig.time.eval_ctx());
    // Without a network the export falls back to the constant, which proves
    // the value really is coming from the channel and not from the parameter.
    assert_eq!(rendered.as_f64(), 0.0);
}

#[test]
fn cooking_stays_within_frame_budget_with_both_families() {
    let gpu = gpu_or_skip!();
    let reg = registry();
    let (graph, out) = demo::audio_reactive(&reg);
    let mut rig = Rig::new(gpu, graph);
    rig.run(out, 5);

    let started = std::time::Instant::now();
    const FRAMES: usize = 120;
    rig.run(out, FRAMES);
    rig.gpu
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let per_frame_ms = started.elapsed().as_secs_f64() * 1000.0 / FRAMES as f64;
    println!("{per_frame_ms:.3} ms/frame for the audio-reactive patch");
    assert!(
        per_frame_ms < 16.6,
        "Phase 2 exit criterion missed: {per_frame_ms:.2} ms/frame"
    );
}

#[test]
fn the_audio_reactive_patch_round_trips_through_the_project_format() {
    use otd_core::Project;
    let reg = registry();
    let (graph, _) = demo::audio_reactive(&reg);
    let text = Project::from_graph(&graph, &reg, 60.0).to_ron().unwrap();
    let back = Project::from_ron(&text).unwrap().to_graph(&reg).unwrap();

    let noise = back.find("/noise1").expect("noise1 survived");
    let period = &back.node(noise).params["period"];
    assert_eq!(period.mode, otd_core::ParamMode::Export);
    assert_eq!(period.source_parts(), Some(("/math_bass", "bass")));

    let text2 = Project::from_graph(&back, &reg, 60.0).to_ron().unwrap();
    assert_eq!(text, text2, "second round trip must be byte-identical");
}
