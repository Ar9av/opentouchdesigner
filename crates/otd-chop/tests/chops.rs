//! Behaviour tests for the CHOP family.

use otd_chop::ops;
use otd_chop::{ChopData, ChopHost};
use otd_core::{CookContext, CookEngine, Graph, NodeId, OpRegistry, Value};

struct Patch {
    graph: Graph,
    reg: OpRegistry,
    host: ChopHost,
    cook: CookEngine,
    time: CookContext,
}

impl Patch {
    fn new() -> Self {
        Patch {
            graph: Graph::new(),
            reg: ops::registry(),
            host: ChopHost::new(),
            cook: CookEngine::new(),
            time: CookContext::default(),
        }
    }

    fn add(&mut self, op: &str, name: &str) -> NodeId {
        let root = self.graph.root();
        let def = self
            .reg
            .get(op)
            .unwrap_or_else(|| panic!("no operator `{op}`"))
            .clone();
        self.graph.create(root, &def, Some(name)).unwrap()
    }

    fn set(&mut self, id: NodeId, key: &str, value: Value) {
        self.graph.set_param(id, key, value).unwrap();
    }

    fn run(&mut self, root: NodeId, frames: usize) {
        for _ in 0..frames {
            self.cook
                .cook_frame(&self.graph, &[root], &self.time, &mut self.host)
                .unwrap();
            self.time.advance(1.0 / 60.0);
        }
    }

    fn data(&self, id: NodeId) -> &ChopData {
        self.host.data(id).expect("node has cooked")
    }

    fn value(&self, id: NodeId, channel: &str) -> f32 {
        self.data(id)
            .value(channel)
            .unwrap_or_else(|| panic!("no channel `{channel}`; have {:?}", self.data(id).names()))
    }
}

#[test]
fn a_constant_chop_produces_named_channels() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "const1");
    p.set(c, "channels", Value::Int(3));
    p.set(c, "value0", Value::Float(0.25));
    p.set(c, "value2", Value::Float(-1.0));
    p.run(c, 1);

    assert_eq!(p.data(c).names(), vec!["chan1", "chan2", "chan3"]);
    assert_eq!(p.value(c, "chan1"), 0.25);
    assert_eq!(p.value(c, "chan3"), -1.0);
}

#[test]
fn an_lfo_traces_the_waveform_it_is_asked_for() {
    let mut p = Patch::new();
    let l = p.add("lfoCHOP", "lfo1");
    p.set(l, "frequency", Value::Float(1.0));
    // One cycle per second at 60 fps: a quarter turn is 15 frames.
    p.run(l, 1);
    assert!(p.value(l, "lfo").abs() < 0.2, "starts near zero");
    p.run(l, 14);
    assert!(p.value(l, "lfo") > 0.9, "quarter cycle is near the peak");
    p.run(l, 15);
    assert!(p.value(l, "lfo").abs() < 0.2, "half cycle is back to zero");
    p.run(l, 15);
    assert!(
        p.value(l, "lfo") < -0.9,
        "three quarters is near the trough"
    );
}

#[test]
fn time_slicing_follows_real_elapsed_time() {
    let mut p = Patch::new();
    let l = p.add("lfoCHOP", "lfo1");
    p.set(l, "rate", Value::Float(600.0));

    p.time.dt = 1.0 / 60.0;
    p.run(l, 1);
    assert_eq!(p.data(l).num_samples(), 10, "600 Hz over 1/60 s");

    // A frame that took four times as long carries four times the samples,
    // so the waveform does not slow down when the renderer does.
    p.time.dt = 4.0 / 60.0;
    p.cook
        .cook_frame(&p.graph, &[l], &p.time, &mut p.host)
        .unwrap();
    assert_eq!(p.data(l).num_samples(), 40);
}

#[test]
fn a_lag_chop_approaches_its_input_instead_of_jumping() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "const1");
    let lag = p.add("lagCHOP", "lag1");
    p.graph.connect(c, lag, 0).unwrap();
    p.set(c, "value0", Value::Float(1.0));
    p.set(lag, "lagup", Value::Float(0.5));

    p.run(lag, 1);
    let first = p.value(lag, "chan1");
    assert!(first > 0.0 && first < 0.2, "first step is partial: {first}");
    p.run(lag, 120);
    assert!(
        (p.value(lag, "chan1") - 1.0).abs() < 0.05,
        "settles at the target"
    );
}

#[test]
fn a_speed_chop_integrates_its_input() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "const1");
    let speed = p.add("speedCHOP", "speed1");
    p.graph.connect(c, speed, 0).unwrap();
    p.set(c, "value0", Value::Float(2.0));

    // Two units per second for one second.
    p.run(speed, 60);
    let v = p.value(speed, "chan1");
    assert!((v - 2.0).abs() < 0.05, "expected ~2.0, got {v}");
}

#[test]
fn a_count_chop_counts_rising_edges() {
    let mut p = Patch::new();
    let lfo = p.add("lfoCHOP", "lfo1");
    let count = p.add("countCHOP", "count1");
    p.graph.connect(lfo, count, 0).unwrap();
    p.set(lfo, "type", Value::Str("square".into()));
    p.set(lfo, "frequency", Value::Float(2.0));
    p.set(count, "threshold", Value::Float(0.0));

    // Two cycles per second for two seconds.
    p.run(count, 120);
    let n = p.value(count, "lfo");
    assert!((3.0..=5.0).contains(&n), "expected about 4 edges, got {n}");
}

#[test]
fn a_trigger_chop_rises_then_releases() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "gate");
    let trig = p.add("triggerCHOP", "trigger1");
    p.graph.connect(c, trig, 0).unwrap();
    p.set(trig, "attack", Value::Float(0.1));
    p.set(trig, "sustain", Value::Float(1.0));
    p.set(trig, "release", Value::Float(0.1));

    p.run(trig, 2);
    assert_eq!(p.value(trig, "chan1"), 0.0, "idle before the gate opens");

    p.set(c, "value0", Value::Float(1.0));
    p.run(trig, 12);
    assert!(p.value(trig, "chan1") > 0.9, "attack reaches full level");

    p.set(c, "value0", Value::Float(0.0));
    p.run(trig, 12);
    assert!(p.value(trig, "chan1") < 0.1, "release returns to zero");
}

#[test]
fn a_math_chop_broadcasts_a_single_channel_across_many() {
    let mut p = Patch::new();
    let many = p.add("constantCHOP", "many");
    let one = p.add("constantCHOP", "one");
    let math = p.add("mathCHOP", "math1");
    p.set(many, "channels", Value::Int(3));
    for (i, v) in [1.0, 2.0, 3.0].iter().enumerate() {
        p.set(many, &format!("value{i}"), Value::Float(*v));
    }
    p.set(one, "value0", Value::Float(10.0));
    p.set(one, "name", Value::Str("gain".into()));
    p.graph.connect(many, math, 0).unwrap();
    p.graph.connect(one, math, 1).unwrap();
    p.set(math, "combine", Value::Str("multiply".into()));

    p.run(math, 1);
    assert_eq!(p.data(math).num_channels(), 3);
    assert_eq!(p.value(math, "chan1"), 10.0);
    assert_eq!(p.value(math, "chan3"), 30.0);
}

#[test]
fn a_select_chop_matches_names_with_wildcards() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "const1");
    let sel = p.add("selectCHOP", "select1");
    p.set(c, "channels", Value::Int(4));
    p.set(c, "name", Value::Str("knob".into()));
    p.graph.connect(c, sel, 0).unwrap();
    p.set(sel, "channels", Value::Str("knob1 knob4".into()));
    p.run(sel, 1);
    assert_eq!(p.data(sel).names(), vec!["knob1", "knob4"]);

    p.set(sel, "channels", Value::Str("knob*".into()));
    p.set(sel, "rename", Value::Str("a b".into()));
    p.run(sel, 1);
    assert_eq!(p.data(sel).names(), vec!["a", "b", "knob3", "knob4"]);
}

#[test]
fn a_merge_chop_disambiguates_duplicate_names() {
    let mut p = Patch::new();
    let a = p.add("constantCHOP", "a");
    let b = p.add("constantCHOP", "b");
    let merge = p.add("mergeCHOP", "merge1");
    p.graph.connect(a, merge, 0).unwrap();
    p.graph.connect(b, merge, 1).unwrap();
    p.set(a, "value0", Value::Float(1.0));
    p.set(b, "value0", Value::Float(2.0));
    p.run(merge, 1);

    // Both inputs call their channel `chan1`; an ambiguous name would make
    // an export unresolvable.
    assert_eq!(p.data(merge).names(), vec!["chan1", "chan11"]);
    assert_eq!(p.value(merge, "chan1"), 1.0);
    assert_eq!(p.value(merge, "chan11"), 2.0);
}

#[test]
fn a_timer_chop_cycles() {
    let mut p = Patch::new();
    let t = p.add("timerCHOP", "timer1");
    p.set(t, "length", Value::Float(0.5));
    p.run(t, 30);
    assert!(p.value(t, "cycles") >= 1.0, "half-second timer cycled");
    assert!(p.value(t, "timer_fraction") < 0.2, "and wrapped round");
}

#[test]
fn a_parameter_exports_from_a_chop_channel() {
    let mut p = Patch::new();
    let lfo = p.add("lfoCHOP", "lfo1");
    let driven = p.add("constantCHOP", "driven");
    p.set(lfo, "frequency", Value::Float(1.0));

    // `driven.value0` follows `/lfo1:lfo` rather than its own constant.
    p.graph
        .node_mut(driven)
        .params
        .get_mut("value0")
        .unwrap()
        .set_export("/lfo1", "lfo");

    // Pulling only `driven` must drag the LFO in as a dependency.
    p.run(driven, 15);
    assert!(
        p.cook.cook_count(lfo) > 0,
        "the export source must have cooked"
    );
    let exported = p.value(driven, "chan1");
    let source = p.value(lfo, "lfo");
    assert!(
        (exported - source).abs() < 1e-5,
        "exported {exported} should track the channel {source}"
    );
    assert!(
        p.cook.is_time_dependent(driven),
        "an export from an animated CHOP animates the consumer"
    );
}

#[test]
fn a_dangling_export_leaves_the_constant_in_place() {
    let mut p = Patch::new();
    let driven = p.add("constantCHOP", "driven");
    p.set(driven, "value0", Value::Float(0.5));
    p.graph
        .node_mut(driven)
        .params
        .get_mut("value0")
        .unwrap()
        .set_export("/nonexistent", "chan1");
    p.run(driven, 1);
    assert_eq!(p.value(driven, "chan1"), 0.5);
}

#[test]
fn a_bind_reads_another_operators_parameter() {
    let mut p = Patch::new();
    let source = p.add("constantCHOP", "source");
    let follower = p.add("constantCHOP", "follower");
    p.set(source, "value0", Value::Float(7.0));
    p.graph
        .node_mut(follower)
        .params
        .get_mut("value0")
        .unwrap()
        .set_bind("/source", "value0");

    p.run(follower, 1);
    assert_eq!(p.value(follower, "chan1"), 7.0);

    p.set(source, "value0", Value::Float(9.0));
    p.run(follower, 1);
    assert_eq!(p.value(follower, "chan1"), 9.0);
}

#[test]
fn osc_arrives_as_channels() {
    use std::net::UdpSocket;

    let mut p = Patch::new();
    let osc = p.add("oscinCHOP", "oscin1");
    // A high port, to avoid colliding with anything a developer is running.
    const PORT: i64 = 38473;
    p.set(osc, "port", Value::Int(PORT));
    p.run(osc, 1);

    if p.host.engine.status("/oscin1").is_some() {
        eprintln!("skipping: could not bind UDP port {PORT}");
        return;
    }

    let socket = UdpSocket::bind("127.0.0.1:0").expect("bind sender");
    let message = rosc::OscPacket::Message(rosc::OscMessage {
        addr: "/fader/1".into(),
        args: vec![rosc::OscType::Float(0.625)],
    });
    let bytes = rosc::encoder::encode(&message).unwrap();
    socket
        .send_to(&bytes, format!("127.0.0.1:{PORT}"))
        .expect("send");

    // The listener is on its own thread; give it a moment, then cook.
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        p.run(osc, 1);
        if p.data(osc).value("fader_1").is_some() {
            break;
        }
    }
    assert_eq!(
        p.data(osc).value("fader_1"),
        Some(0.625),
        "OSC address became a channel; have {:?}",
        p.data(osc).names()
    );
}

#[test]
fn the_audio_spectrum_finds_the_frequency_it_is_given() {
    let mut p = Patch::new();
    let tone = p.add("lfoCHOP", "tone");
    let spectrum = p.add("audiospectrumCHOP", "spectrum1");
    p.graph.connect(tone, spectrum, 0).unwrap();

    // A 4 kHz tone sampled at 48 kHz, into an 8-band spectrum.
    p.set(tone, "rate", Value::Float(48000.0));
    p.set(tone, "frequency", Value::Float(4000.0));
    p.set(spectrum, "size", Value::Str("1024".into()));
    p.set(spectrum, "bands", Value::Int(8));
    p.set(spectrum, "gain", Value::Float(1.0));

    p.run(spectrum, 10);
    let bands: Vec<f32> = (1..=8)
        .map(|i| p.value(spectrum, &format!("band{i}")))
        .collect();
    let loudest = bands
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, _)| i)
        .unwrap();

    // Bands are cubically spaced, so 4 kHz of 24 kHz (a sixth of the range)
    // lands in the upper half but not the top band.
    assert!(
        (4..=7).contains(&loudest),
        "4 kHz landed in band {} of {bands:?}",
        loudest + 1
    );
    assert!(
        bands[loudest] > bands[0] * 4.0,
        "peak is distinct: {bands:?}"
    );
}

#[test]
fn a_missing_audio_device_is_reported_not_fatal() {
    let mut p = Patch::new();
    let audio = p.add("audiodeviceinCHOP", "audio1");
    p.set(audio, "device", Value::Str("no such device at all".into()));
    p.run(audio, 2);

    assert!(
        p.host.engine.status("/audio1").is_some(),
        "the node should carry an explanation"
    );
    // And still produce a channel, so downstream operators keep working.
    assert_eq!(p.value(audio, "chan1"), 0.0);
}

#[test]
fn dmx_goes_out_over_the_wire() {
    use std::net::UdpSocket;

    // Receive on the Art-Net port ourselves. If something else on this
    // machine already has it, there is nothing to prove here.
    let Ok(listener) = UdpSocket::bind(("127.0.0.1", otd_chop::dmx::ARTNET_PORT)) else {
        eprintln!("skipping: Art-Net port already in use");
        return;
    };
    listener
        .set_read_timeout(Some(std::time::Duration::from_millis(500)))
        .unwrap();

    let mut p = Patch::new();
    let levels = p.add("constantCHOP", "levels");
    let dmx = p.add("dmxoutCHOP", "dmxout1");
    p.graph.connect(levels, dmx, 0).unwrap();
    p.set(levels, "channels", Value::Int(3));
    p.set(levels, "value0", Value::Float(1.0));
    p.set(levels, "value1", Value::Float(0.5));
    p.set(levels, "value2", Value::Float(0.0));
    p.set(dmx, "address", Value::Str("127.0.0.1".into()));
    p.set(dmx, "universe", Value::Int(2));
    p.run(dmx, 1);

    assert!(
        p.host.engine.status("/dmxout1").is_none(),
        "{:?}",
        p.host.engine.status("/dmxout1")
    );

    let mut buf = [0u8; 1024];
    let (len, _) = listener
        .recv_from(&mut buf)
        .expect("a packet should arrive");
    assert!(len > 20);
    assert_eq!(&buf[0..8], b"Art-Net\0");
    assert_eq!(&buf[14..16], &[2, 0], "universe 2");
    // Levels are 0..1 in the network and 0..255 on the wire.
    assert_eq!(buf[18], 255);
    assert_eq!(buf[19], 128);
    assert_eq!(buf[20], 0);

    // A DMX Out passes its input through, so it can sit mid-chain.
    assert_eq!(p.data(dmx).num_channels(), 3);
}

#[test]
fn a_bad_dmx_address_is_reported_not_fatal() {
    let mut p = Patch::new();
    let levels = p.add("constantCHOP", "levels");
    let dmx = p.add("dmxoutCHOP", "dmxout1");
    p.graph.connect(levels, dmx, 0).unwrap();
    p.set(dmx, "address", Value::Str("not a host".into()));
    p.run(dmx, 1);
    assert!(p.host.engine.status("/dmxout1").is_some());
}

#[test]
fn every_chop_cooks_without_panicking() {
    let mut p = Patch::new();
    for spec in ops::all() {
        // Devices and sockets are exercised by their own tests; opening one
        // per operator here would fight for the same hardware.
        if spec.def.type_name.contains("audiodevice")
            || spec.def.type_name.contains("osc")
            || spec.def.type_name.contains("midi")
            // A DMX Out broadcasts by default; leave the network alone.
            || spec.def.type_name.contains("dmx")
        {
            continue;
        }
        let id = p.add(spec.def.type_name, spec.def.type_name);
        p.run(id, 2);
        assert!(
            p.host.data(id).is_some(),
            "{} produced nothing",
            spec.def.type_name
        );
    }
}
