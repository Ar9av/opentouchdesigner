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

// ------------------------------------------------------------- animation

/// Run to roughly `seconds` of timeline at 60 fps.
fn run_to(p: &mut Patch, id: NodeId, seconds: f64) {
    p.run(id, (seconds * 60.0).round() as usize);
}

#[test]
fn an_animation_chop_follows_its_keys() {
    let mut p = Patch::new();
    let a = p.add("animationCHOP", "anim1");
    p.set(
        a,
        "keys",
        Value::Str("tx 0 0 linear\ntx 2 10 linear\nty 0 5 constant\n".into()),
    );

    // Two named channels, in name order, each reading its own curve.
    run_to(&mut p, a, 1.0);
    assert_eq!(p.data(a).names(), vec!["tx", "ty"]);
    assert!((p.value(a, "tx") - 5.0).abs() < 0.2, "{}", p.value(a, "tx"));
    assert_eq!(p.value(a, "ty"), 5.0);

    run_to(&mut p, a, 1.0);
    assert!(
        (p.value(a, "tx") - 10.0).abs() < 0.2,
        "{}",
        p.value(a, "tx")
    );
}

#[test]
fn an_animation_chop_can_loop_its_keyed_span() {
    let mut p = Patch::new();
    let a = p.add("animationCHOP", "anim1");
    p.set(a, "keys", Value::Str("a 0 0 linear\na 2 2 linear\n".into()));

    // The default holds after the last key, which is what an unlooped cue
    // should do when it finishes.
    run_to(&mut p, a, 5.0);
    assert_eq!(p.value(a, "a"), 2.0);

    // Looping wraps into the keyed span instead: five seconds into a
    // two-second curve is one second in.
    p.set(a, "play", Value::Str("loop".into()));
    p.run(a, 1);
    assert!(
        (p.value(a, "a") - 1.0).abs() < 0.1,
        "expected the loop to wrap, got {}",
        p.value(a, "a")
    );
}

#[test]
fn an_animation_chop_with_no_keys_still_produces_a_channel() {
    let mut p = Patch::new();
    let a = p.add("animationCHOP", "anim1");
    p.set(a, "keys", Value::Str(String::new()));
    p.run(a, 1);
    // Downstream operators should not have to special-case an empty CHOP just
    // because nobody has keyed anything yet.
    assert_eq!(p.data(a).names(), vec!["chan1"]);
    assert_eq!(p.value(a, "chan1"), 0.0);
}

#[test]
fn animation_speed_and_offset_shift_the_curve_in_time() {
    let mut p = Patch::new();
    let a = p.add("animationCHOP", "anim1");
    p.set(
        a,
        "keys",
        Value::Str("a 0 0 linear\na 10 10 linear\n".into()),
    );

    p.set(a, "speed", Value::Float(2.0));
    run_to(&mut p, a, 2.0);
    assert!((p.value(a, "a") - 4.0).abs() < 0.2, "{}", p.value(a, "a"));

    p.set(a, "speed", Value::Float(1.0));
    p.set(a, "offset", Value::Float(3.0));
    p.run(a, 1);
    assert!((p.value(a, "a") - 5.0).abs() < 0.2, "{}", p.value(a, "a"));
}

#[test]
fn a_keyframed_curve_drives_a_parameter_through_export() {
    let mut p = Patch::new();
    let a = p.add("animationCHOP", "anim1");
    p.set(
        a,
        "keys",
        Value::Str("gain 0 0 linear\ngain 2 1 linear\n".into()),
    );
    let m = p.add("mathCHOP", "math1");
    p.graph.connect(a, m, 0).unwrap();

    // The point of making keyframes a CHOP: everything downstream — filters,
    // maths, Export to a parameter — works on them with no new mechanism.
    p.set(m, "gain", Value::Float(100.0));
    run_to(&mut p, m, 1.0);
    assert!(
        (p.value(m, "gain") - 50.0).abs() < 2.0,
        "{}",
        p.value(m, "gain")
    );
}

#[test]
fn an_audio_file_follows_the_timeline() {
    // A one-second WAV whose sample value *is* its time in seconds, so the
    // test can ask "what time is the player reading?" by looking at a sample.
    const RATE: u32 = 1000;
    let mut data = Vec::new();
    for i in 0..RATE {
        data.extend_from_slice(&(i as f32 / RATE as f32).to_le_bytes());
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&32u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);

    let dir = std::env::temp_dir().join(format!("otd-audiofile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ramp.wav"), &wav).unwrap();

    let mut p = Patch::new();
    // A relative path, resolved against the project's directory — the same
    // rule external components follow, and what makes a bundle portable.
    p.graph.set_base_dir(Some(dir.clone()));
    let f = p.add("audiofileinCHOP", "file1");
    p.set(f, "file", Value::Str("ramp.wav".into()));

    // Half a second in, the player must be reading the half-second sample:
    // playback is a function of the timeline, not of a private play head.
    p.run(f, 30);
    assert!(
        p.host.engine.status("/file1").is_none(),
        "{:?}",
        p.host.engine.status("/file1")
    );
    assert!(
        (p.value(f, "chan1") - 0.5).abs() < 0.02,
        "at t=0.5 expected ~0.5, got {}",
        p.value(f, "chan1")
    );

    // Looping: at t=1.5 the file has wrapped and reads ~0.5 again.
    p.run(f, 60);
    assert!(
        (p.value(f, "chan1") - 0.5).abs() < 0.02,
        "looped read at t=1.5 gave {}",
        p.value(f, "chan1")
    );

    // `once` instead holds silence after the end.
    p.set(f, "play", Value::Str("once".into()));
    p.run(f, 1);
    assert_eq!(p.value(f, "chan1"), 0.0);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_missing_audio_file_is_reported_not_fatal() {
    let mut p = Patch::new();
    let f = p.add("audiofileinCHOP", "file1");
    p.set(f, "file", Value::Str("no-such-file.wav".into()));
    p.run(f, 2);
    assert!(p.host.engine.status("/file1").is_some());
    assert_eq!(p.value(f, "chan1"), 0.0);
}

#[test]
fn a_missing_audio_output_passes_its_input_through() {
    // Same rule as every output op: losing the device mid-show must not
    // stop the render, and the channels keep flowing to whatever is next.
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "const1");
    let out = p.add("audiodeviceoutCHOP", "audioout1");
    p.graph.connect(c, out, 0).unwrap();
    p.set(c, "value0", Value::Float(0.75));
    p.set(out, "device", Value::Str("no such output device".into()));
    p.run(out, 2);
    assert!(p.host.engine.status("/audioout1").is_some());
    assert_eq!(p.value(out, "chan1"), 0.75);
}

// ------------------------------------------------------------ new operators

#[test]
fn limit_clamps_wraps_folds_and_quantises() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "src");
    let l = p.add("limitCHOP", "limit1");
    p.graph.connect(c, l, 0).unwrap();
    p.set(c, "value0", Value::Float(1.75));

    p.run(l, 1);
    assert_eq!(p.value(l, "chan1"), 1.0, "clamp");

    p.set(l, "type", Value::Str("loop".into()));
    p.run(l, 1);
    assert!((p.value(l, "chan1") - 0.75).abs() < 1e-5, "loop wraps");

    p.set(l, "type", Value::Str("zigzag".into()));
    p.run(l, 1);
    // 1.75 folds back down: past the top at 1.0, 0.75 further, so 0.25.
    assert!((p.value(l, "chan1") - 0.25).abs() < 1e-5, "zigzag folds");

    p.set(l, "type", Value::Str("quantise".into()));
    p.set(l, "step", Value::Float(0.5));
    p.run(l, 1);
    assert_eq!(p.value(l, "chan1"), 2.0, "quantise snaps to the step");
}

#[test]
fn slope_is_the_inverse_of_speed() {
    // Integrating then differentiating should give back what went in, which
    // is a stronger statement than either operator's own arithmetic.
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "src");
    let speed = p.add("speedCHOP", "integrate");
    let slope = p.add("slopeCHOP", "differentiate");
    p.graph.connect(c, speed, 0).unwrap();
    p.graph.connect(speed, slope, 0).unwrap();
    p.set(c, "value0", Value::Float(2.0));

    p.run(slope, 10);
    assert!(
        (p.value(slope, "chan1") - 2.0).abs() < 0.05,
        "got {}",
        p.value(slope, "chan1")
    );
}

#[test]
fn hold_freezes_until_the_trigger_fires() {
    let mut p = Patch::new();
    let src = p.add("constantCHOP", "src");
    let trig = p.add("constantCHOP", "trig");
    let hold = p.add("holdCHOP", "hold1");
    p.graph.connect(src, hold, 0).unwrap();
    p.graph.connect(trig, hold, 1).unwrap();

    p.set(src, "value0", Value::Float(0.25));
    p.set(trig, "value0", Value::Float(1.0));
    p.run(hold, 2);
    assert_eq!(p.value(hold, "chan1"), 0.25, "the rising edge samples");

    // The source moves but the trigger stays high: no new edge, no new value.
    p.set(src, "value0", Value::Float(0.9));
    p.run(hold, 2);
    assert_eq!(p.value(hold, "chan1"), 0.25, "held across a static trigger");

    // Drop and raise it again.
    p.set(trig, "value0", Value::Float(0.0));
    p.run(hold, 1);
    p.set(trig, "value0", Value::Float(1.0));
    p.run(hold, 1);
    assert_eq!(p.value(hold, "chan1"), 0.9, "a new edge samples again");
}

#[test]
fn shuffle_transposes_channels_into_samples() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "src");
    let s = p.add("shuffleCHOP", "shuffle1");
    p.graph.connect(c, s, 0).unwrap();
    p.set(c, "channels", Value::Int(3));
    p.set(c, "value0", Value::Float(0.1));
    p.set(c, "value1", Value::Float(0.2));
    p.set(c, "value2", Value::Float(0.3));
    p.run(s, 1);

    // Three channels of one sample become one channel of three.
    assert_eq!(p.data(s).num_channels(), 1);
    assert_eq!(p.data(s).nth(0).unwrap().samples.len(), 3);
    let got = &p.data(s).nth(0).unwrap().samples;
    assert!((got[0] - 0.1).abs() < 1e-6 && (got[2] - 0.3).abs() < 1e-6);
}

#[test]
fn rename_numbers_the_channels_it_matches() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "src");
    let r = p.add("renameCHOP", "rename1");
    p.graph.connect(c, r, 0).unwrap();
    p.set(c, "channels", Value::Int(3));
    p.set(r, "to", Value::Str("led[1-]".into()));
    p.run(r, 1);

    assert_eq!(p.data(r).names(), vec!["led1", "led2", "led3"]);
}

#[test]
fn analyze_reduces_a_waveform_to_one_number() {
    let mut p = Patch::new();
    let pat = p.add("patternCHOP", "wave");
    let a = p.add("analyzeCHOP", "peak");
    p.graph.connect(pat, a, 0).unwrap();
    p.set(pat, "length", Value::Int(100));
    p.set(a, "function", Value::Str("maximum".into()));
    p.run(a, 1);

    assert_eq!(p.data(a).num_samples(), 1);
    let peak = p.value(a, "chan1");
    let raw = p.data(pat).nth(0).unwrap().max();
    assert!((peak - raw).abs() < 1e-6, "{peak} vs {raw}");
}

#[test]
fn an_equal_power_cross_does_not_dip_in_the_middle() {
    // The reason the curve exists: at the halfway point a linear blend of two
    // full-scale signals reads 1.0 where each input read 1.0, but the *power*
    // has dropped. Equal power holds it up at ~0.707 each, summing to ~1.41.
    let mut p = Patch::new();
    let a = p.add("constantCHOP", "a");
    let b = p.add("constantCHOP", "b");
    let x = p.add("crossCHOP", "cross1");
    p.graph.connect(a, x, 0).unwrap();
    p.graph.connect(b, x, 1).unwrap();
    p.set(a, "value0", Value::Float(1.0));
    p.set(b, "value0", Value::Float(1.0));

    p.run(x, 1);
    assert!(
        (p.value(x, "chan1") - 1.0).abs() < 1e-5,
        "linear at the middle"
    );

    p.set(x, "curve", Value::Str("equalpower".into()));
    p.run(x, 1);
    let v = p.value(x, "chan1");
    assert!((v - 1.414).abs() < 0.01, "equal power held at {v}");
}

#[test]
fn resample_changes_the_length_and_keeps_the_shape() {
    let mut p = Patch::new();
    let pat = p.add("patternCHOP", "wave");
    let r = p.add("resampleCHOP", "resample1");
    p.graph.connect(pat, r, 0).unwrap();
    p.set(pat, "length", Value::Int(100));
    p.set(r, "length", Value::Int(10));
    p.run(r, 1);

    assert_eq!(p.data(r).num_samples(), 10);
    // The endpoints are the endpoints — a resample that dropped or shifted
    // them would be a crop, not a resample.
    let src = &p.data(pat).nth(0).unwrap().samples;
    let out = &p.data(r).nth(0).unwrap().samples;
    assert!((out[0] - src[0]).abs() < 1e-5);
    assert!((out[9] - src[99]).abs() < 1e-5);
}

#[test]
fn the_beat_chop_is_a_function_of_time_not_a_counter() {
    // Jumping the clock forward must land on the same phase a run of frames
    // would have reached. A counter would be permanently behind.
    let mut p = Patch::new();
    let b = p.add("beatCHOP", "beat1");
    p.set(b, "tempo", Value::Float(120.0)); // Two beats a second.

    // 0.25 s in: half a beat.
    p.time.advance(0.25);
    p.run(b, 1);
    let after_jump = p.value(b, "ramp");
    assert!(
        (after_jump - 0.5).abs() < 0.05,
        "half a beat should read ~0.5, got {after_jump}"
    );

    let count = p.value(b, "count");
    assert_eq!(count, 0.0, "still in the first beat");
}

#[test]
fn delay_plays_a_channel_back_later() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "src");
    let d = p.add("delayCHOP", "delay1");
    p.graph.connect(c, d, 0).unwrap();
    // 5 frames at the control rate.
    p.set(d, "delay", Value::Float(5.0 / 60.0));

    p.set(c, "value0", Value::Float(1.0));
    p.run(d, 1);
    assert_eq!(p.value(d, "chan1"), 0.0, "nothing has arrived yet");

    p.run(d, 5);
    assert_eq!(p.value(d, "chan1"), 1.0, "the value arrives five frames on");
}

#[test]
fn an_expression_chop_sees_the_sample_and_its_position() {
    let mut p = Patch::new();
    let pat = p.add("patternCHOP", "wave");
    let e = p.add("expressionCHOP", "expr1");
    p.graph.connect(pat, e, 0).unwrap();
    p.set(pat, "length", Value::Int(10));
    p.set(pat, "type", Value::Str("ramp".into()));

    // `i` is the sample index and `n` the count, so this rebuilds the ramp
    // from scratch and must match a straight 0..1 sweep.
    p.set(e, "expr", Value::Str("i / (n - 1)".into()));
    p.run(e, 1);
    let out = &p.data(e).nth(0).unwrap().samples;
    assert_eq!(out.len(), 10);
    assert!((out[0] - 0.0).abs() < 1e-5);
    assert!((out[9] - 1.0).abs() < 1e-5);

    // `v` is the incoming sample.
    p.set(e, "expr", Value::Str("v * 2 + 1".into()));
    p.run(e, 1);
    let src = p.data(pat).nth(0).unwrap().samples.clone();
    let out = &p.data(e).nth(0).unwrap().samples;
    for (i, v) in src.iter().enumerate() {
        assert!((out[i] - (v * 2.0 + 1.0)).abs() < 1e-4, "sample {i}");
    }
}

#[test]
fn a_half_typed_expression_holds_the_input_rather_than_emptying_it() {
    // Typing `v *` on the way to `v * 2` must not black out a running patch,
    // which is the same promise the GLSL TOP makes for a shader mid-edit.
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "src");
    let e = p.add("expressionCHOP", "expr1");
    p.graph.connect(c, e, 0).unwrap();
    p.set(c, "value0", Value::Float(0.75));
    p.set(e, "expr", Value::Str("v *".into()));
    p.run(e, 1);

    assert_eq!(p.value(e, "chan1"), 0.75);
}

#[test]
fn an_expression_can_reach_the_clock() {
    let mut p = Patch::new();
    let c = p.add("constantCHOP", "src");
    let e = p.add("expressionCHOP", "expr1");
    p.graph.connect(c, e, 0).unwrap();
    p.set(e, "expr", Value::Str("frame".into()));

    p.run(e, 5);
    let first = p.value(e, "chan1");
    p.run(e, 5);
    assert!(
        p.value(e, "chan1") > first,
        "the frame number should be advancing"
    );
}
