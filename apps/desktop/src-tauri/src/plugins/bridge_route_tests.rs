//! Per-note pan and volume through the helper process, with the owned CLAP
//! and VST3 fixtures: a capability change replaces the helper, and the note's
//! ramp, the channel's share and the fade around the change must carry
//! across exactly as they do in process. Every expected number is a second
//! helper playing the same plugin with one neutral, centred note and no
//! project law, times the law's gain worked out here.
use super::super::runtime::ownership_tests::{clap_expression_fixtures, fixtures, transport};
use super::*;
use windfall_core::PanLaw;
use windfall_dsp::{NoteExpression, NoteInstanceId};
use windfall_engine::plugins::PluginFactory;
use windfall_plugin_host::project_expression::Carry;
use windfall_project::PluginTarget;

const LAWS: [PanLaw; 4] = [
    PanLaw::Balance,
    PanLaw::ConstantPower,
    PanLaw::Compromise,
    PanLaw::Linear,
];
/// Frames of every ramp and fade at 48 kHz.
const FADE: usize = 240;

struct Bridged {
    record: Record,
    audio: Audio,
    error: RenderError,
    offline: bool,
    /// Frames the helper round trip adds, and the plugin's own latency.
    bridge: usize,
    native: usize,
}
impl Bridged {
    fn launch(binding: &PluginBinding, offline: bool) -> Self {
        static TOKEN: AtomicU64 = AtomicU64::new(9_000);
        let helper = PathBuf::from(
            std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
                .expect("explicit prebuilt current helper"),
        );
        let stamp =
            windfall_plugin_host::paths::plugin_file_identity(Path::new(&binding.path)).unwrap();
        let launch_options = options(
            helper,
            binding,
            stamp,
            TOKEN.fetch_add(1, Ordering::Relaxed),
            1,
            48_000,
            offline,
        )
        .unwrap();
        let bridge = launch_options.config.block * 2;
        let controls = super::super::runtime::parameter_controls(binding);
        let error: RenderError = Arc::new(Mutex::new(None));
        let (record, mut audio) = launch(
            launch_options,
            binding.clone(),
            controls,
            None,
            error.clone(),
        )
        .unwrap();
        HostedEffect::transport(&mut audio, transport());
        let native = HostedEffect::latency(&audio) - bridge;
        Self {
            record,
            audio,
            error,
            offline,
            bridge,
            native,
        }
    }
    /// The same plugin playing one neutral, centred note with no project
    /// law: what the fixture itself puts out through a helper.
    fn neutral(binding: &PluginBinding, offline: bool, key: u8, velocity: f32) -> Self {
        let mut unit = Self::launch(binding, offline);
        unit.audio.note_on_instance(
            NoteInstanceId(1),
            key,
            velocity,
            0.0,
            NoteExpression::NEUTRAL,
        );
        unit
    }
    fn run(&mut self, frames: usize) -> Vec<[f32; 2]> {
        assert_eq!(frames % 64, 0);
        let mut output = Vec::with_capacity(frames);
        for _ in 0..frames / 64 {
            let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
            if self.offline {
                HostedEffect::process(&mut self.audio, &mut left, &mut right);
            } else {
                assert_eq!(
                    crate::test_alloc::allocator_calls(|| HostedEffect::process(
                        &mut self.audio,
                        &mut left,
                        &mut right
                    )),
                    0
                );
                // This is the test's device clock, not an audio callback wait.
                std::thread::sleep(Duration::from_millis(2));
            }
            output.extend(
                left.into_iter()
                    .zip(right)
                    .map(|(left, right)| [left, right]),
            );
            assert!(self.error.lock().unwrap().is_none());
            assert!(!self.record.control.status().failed);
        }
        output
    }
}

fn peak(audio: &[[f32; 2]]) -> f32 {
    audio
        .iter()
        .flat_map(|frame| frame.iter())
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
}
fn largest_step(audio: &[[f32; 2]]) -> f32 {
    audio
        .windows(2)
        .flat_map(|pair| [pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]])
        .fold(0.0_f32, |step, delta| step.max(delta.abs()))
}
/// The law's gains `elapsed` frames into the pan ramp every migration runs:
/// from half left to seven tenths right.
fn ramp_gains(law: PanLaw, elapsed: f32) -> (f32, f32) {
    let (from, to) = (law.gains(-0.5), law.gains(0.7));
    let part = elapsed.min(FADE as f32) / FADE as f32;
    (
        from.0 + (to.0 - from.0) * part,
        from.1 + (to.1 - from.1) * part,
    )
}
/// The gain on each side over `frames`, from the frames loud enough to tell,
/// and the mean position of those frames within the range.
fn estimate(
    heard: &[[f32; 2]],
    plain: &[[f32; 2]],
    presence: &[f32],
    frames: std::ops::Range<usize>,
) -> [(f32, f32); 2] {
    let loud = peak(&plain[frames.clone()]) * 0.5;
    assert!(loud > 0.0, "nothing sounds in the frames to measure");
    let first = frames.start;
    std::array::from_fn(|side| {
        let (mut sum, mut at, mut count) = (0.0_f64, 0.0_f64, 0_u32);
        for t in frames.clone() {
            if plain[t][side].abs() >= loud && presence[t] > 0.0 {
                sum += f64::from(heard[t][side]) / f64::from(plain[t][side] * presence[t]);
                at += (t - first) as f64;
                count += 1;
            }
        }
        assert!(count > 0, "nothing loud enough to measure");
        (
            (sum / f64::from(count)) as f32,
            (at / f64::from(count)) as f32,
        )
    })
}

struct Migration {
    /// Everything heard, without the frames each helper round trip adds,
    /// which differ between a live and an offline helper.
    aligned: Vec<[f32; 2]>,
    worst: f32,
    step: f32,
    own_step: f32,
    before: [(f32, f32); 2],
    after: [(f32, f32); 2],
}

/// One note, its pan ramp 64 frames in, when the owner asks for the plugin:
/// the helper fades out and holds the ramp, and a helper of `after` takes the
/// same note over. `before` and `after` are the same project object with
/// different capabilities.
fn migrate(
    before: &PluginBinding,
    after: &PluginBinding,
    law: PanLaw,
    offline: bool,
    label: &str,
) -> Migration {
    let carry = Arc::new(Mutex::new(Carry::new()));
    let id = NoteInstanceId(7);
    let mut first = Bridged::launch(before, offline);
    first.audio.share_carry(carry.clone());
    first.audio.set_note_pan_law(law);
    let mut first_plain = Bridged::neutral(before, offline, 69, 0.4);
    first
        .audio
        .note_on_instance(id, 69, 0.4, -0.5, NoteExpression::NEUTRAL);
    let mut heard = first.run(256);
    let mut plain = first_plain.run(256);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            first
                .audio
                .set_note_expression(id, 0.7, NoteExpression::NEUTRAL)
        }),
        0
    );
    heard.extend(first.run(64));
    plain.extend(first_plain.run(64));

    // The owner asks for the plugin. From this frame the helper fades its
    // output out and the ramp waits where it is.
    first
        .record
        .departure
        .requested
        .store(true, Ordering::Release);
    // Long enough for the fade to be played to its last frame and for the
    // block after it, which says so, to come back.
    let leaving = (320 + 2 * first.bridge + first.native).next_multiple_of(64);
    heard.extend(first.run(leaving));
    plain.extend(first_plain.run(leaving));
    assert!(
        first.record.departure.departed.load(Ordering::Acquire),
        "{label}: the helper reported its output faded out"
    );

    // The replacement: the same project object, another set of capabilities,
    // and the note replayed under its own identity as the engine does.
    let mut second = Bridged::launch(after, offline);
    second.audio.share_carry(carry.clone());
    second.audio.set_note_pan_law(law);
    let mut second_plain = Bridged::neutral(after, offline, 69, 0.4);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            second
                .audio
                .note_on_instance(id, 69, 0.4, 0.7, NoteExpression::NEUTRAL)
        }),
        0
    );
    let frames = (512 + second.bridge).next_multiple_of(64);
    let arrived = second.run(frames);
    let arrived_plain = second_plain.run(frames);
    assert_eq!(
        HostedInstrument::voices(&second.audio),
        1,
        "{label}: one note, not two"
    );
    let migration = judge_handover(
        law,
        label,
        [(&heard[..], &plain[..]), (&arrived[..], &arrived_plain[..])],
        [(first.bridge, first.native), (second.bridge, second.native)],
    );
    // The note ends once, in the helper that now plays it.
    second.audio.note_off_instance(id, 69);
    let tail = second.run(9600);
    assert_eq!(
        HostedInstrument::voices(&second.audio),
        0,
        "{label}: no stuck note"
    );
    assert!(
        tail[9600 - 64..].iter().all(|frame| peak(&[*frame]) < 1e-4),
        "{label}: silent after the release"
    );
    migration
}

/// What one unit and its neutral twin played.
type Played<'a> = (&'a [[f32; 2]], &'a [[f32; 2]]);

/// Every frame of a hand-over against the model. The unit that left and its
/// neutral twin played `heard` and `plain` from the note on, with the pan
/// ramp started at frame 256 and the request to leave at frame 320; the unit
/// that took the note over and its twin played `arrived` and
/// `arrived_plain` from the replayed note on. `delays` are the helper round
/// trip and the plugin's own latency of each of the two.
fn judge_handover(
    law: PanLaw,
    label: &str,
    played: [Played<'_>; 2],
    delays: [(usize, usize); 2],
) -> Migration {
    let [(heard, plain), (arrived, arrived_plain)] = played;
    let [(bridge, native), (second_bridge, second_native)] = delays;
    let mut heard = heard.to_vec();
    let mut aligned = heard[bridge..bridge + 640].to_vec();
    // Steps are measured once the note sounds: a helper's output begins
    // after its round trip, and that beginning is the note's own start.
    let sounding = bridge + native + 1;
    let first_step = largest_step(&heard[sounding + 256..]);
    let first_own_step = largest_step(&plain[sounding..]);
    // In the helper's own frames: the ramp starts at 256 and stops at 320,
    // where the fade-out starts.
    let reached = |frame: usize| frame.saturating_sub(256).min(64) as f32;
    let presence: Vec<f32> = (0..heard.len())
        .map(|t| match t.checked_sub(bridge + 320) {
            Some(k) => (1.0 - (k + 1) as f32 / FADE as f32).max(0.0),
            None => 1.0,
        })
        .collect();
    let mut model = Vec::with_capacity(heard.len());
    for t in 0..heard.len() {
        let gains = ramp_gains(law, reached(t.saturating_sub(bridge + native)));
        model.push([
            plain[t][0] * gains.0 * presence[t],
            plain[t][1] * gains.1 * presence[t],
        ]);
    }
    assert!(
        heard[bridge + 320 + FADE..]
            .iter()
            .all(|frame| *frame == [0.0; 2]),
        "{label}: silent once the fade is over"
    );
    // The last frames that still sound at half level or more.
    let before_frame = bridge + 320 + FADE / 2 - 1;
    let before_gain = estimate(&heard, plain, &presence, before_frame - 32..before_frame);

    let frames = arrived.len();
    let arrival: Vec<f32> = (0..frames)
        .map(|n| match n.checked_sub(second_bridge) {
            Some(v) => ((v + 1) as f32 / FADE as f32).min(1.0),
            None => 0.0,
        })
        .collect();
    let start = heard.len();
    for n in 0..frames {
        let gains = ramp_gains(
            law,
            64.0 + n.saturating_sub(second_bridge + second_native) as f32,
        );
        model.push([
            arrived_plain[n][0] * gains.0 * arrival[n],
            arrived_plain[n][1] * gains.1 * arrival[n],
        ]);
    }
    heard.extend(arrived);
    aligned.extend(&arrived[second_bridge..second_bridge + 512]);
    // Measured once what the replacement was sent has come out of it.
    let after_gain = estimate(
        arrived,
        arrived_plain,
        &arrival,
        second_bridge + second_native + 8..second_bridge + second_native + 40,
    );

    let mut worst = 0.0_f32;
    for (t, (got, want)) in heard.iter().zip(&model).enumerate() {
        for side in 0..2 {
            worst = worst.max((got[side] - want[side]).abs());
            assert!(
                (got[side] - want[side]).abs() < 4e-6,
                "{label} {law:?} frame {t} (replacement starts at {start}) side {side}: {} vs {}",
                got[side],
                want[side]
            );
        }
    }
    // The replacement fades in from silence, so all of it counts; the
    // neutral helper beside it starts at full level and is measured after.
    let own_step = first_own_step.max(largest_step(
        &arrived_plain[second_bridge + second_native + 1..],
    ));
    Migration {
        step: first_step.max(largest_step(arrived)),
        own_step,
        aligned,
        worst,
        before: before_gain,
        after: after_gain,
    }
}

/// What every hand-over must show, live and offline alike.
fn assert_handover(name: &str, law: PanLaw, live: &Migration, offline: &Migration) {
    // The click bound: the fixture's own largest step at full
    // level, plus what the ramp and the fade may add in one frame.
    let gains = [law.gains(-0.5), law.gains(0.7)];
    let top = gains
        .iter()
        .flat_map(|gain| [gain.0, gain.1])
        .fold(0.0_f32, f32::max);
    let loud = 0.4 * top;
    let bound = live.own_step * top + loud * (1.0 + 2.0) / FADE as f32 + 1e-5;
    assert!(
        live.step <= bound,
        "{name} {law:?}: step {} > {bound}",
        live.step
    );
    // Ramp continuity: 64 frames in before the helper left, 64 plus
    // the frames since its replacement came after, and nothing in
    // between.
    let frozen = ramp_gains(law, 64.0);
    // The estimate after starts eight frames into the replacement.
    let resumed = [
        ramp_gains(law, 64.0 + 8.0 + live.after[0].1).0,
        ramp_gains(law, 64.0 + 8.0 + live.after[1].1).1,
    ];
    for (got, want) in [
        (live.before[0].0, frozen.0),
        (live.before[1].0, frozen.1),
        (live.after[0].0, resumed[0]),
        (live.after[1].0, resumed[1]),
    ] {
        assert!(
            (got - want).abs() < 1e-3,
            "{name} {law:?}: gain {got} vs {want}"
        );
    }
    // Just after continues just before: the two differ by what the
    // ramp covers in the frames between them and by nothing else.
    let between = 8.0 + live.after[0].1;
    let slope = (ramp_gains(law, FADE as f32).0 - ramp_gains(law, 0.0).0) / FADE as f32;
    assert!(
        (live.after[0].0 - live.before[0].0 - slope * between).abs() < 1e-3,
        "{name} {law:?}: the ramp went on from {} to {}",
        live.before[0].0,
        live.after[0].0
    );
    let mut apart = 0.0_f32;
    for (live, offline) in live.aligned.iter().zip(&offline.aligned) {
        apart = apart
            .max((live[0] - offline[0]).abs())
            .max((live[1] - offline[1]).abs());
    }
    assert_eq!(live.aligned.len(), offline.aligned.len());
    assert!(
        peak(&live.aligned) > 0.05,
        "{name} {law:?}: something sounds"
    );
    assert!(
        apart <= 2e-6,
        "{name} {law:?}: live and offline differ by {apart}"
    );
    println!(
        "BRIDGED migration {name} {law:?}: worst={:.3e} step={:.5} bound={bound:.5} own_step={:.5} before=({:.5},{:.5}) frozen=({:.5},{:.5}) after=({:.5},{:.5}) live_vs_offline={apart:.1e}",
        live.worst,
        live.step,
        live.own_step,
        live.before[0].0,
        live.before[1].0,
        frozen.0,
        frozen.1,
        live.after[0].0,
        live.after[1].0,
    );
}

/// Loss and return of each capability, live and offline, for one format.
fn migrations(label: &str, pairs: &[(&str, PluginBinding, PluginBinding)]) {
    for (what, before, after) in pairs {
        for law in LAWS {
            let name = format!("{label} {what}");
            let live = migrate(before, after, law, false, &name);
            let offline = migrate(before, after, law, true, &format!("{name} offline"));
            assert_handover(&name, law, &live, &offline);
        }
    }
}

/// Plays `frames` of a unit the runtime made, at a device's pace, and
/// proves the audio side never allocates.
fn live(unit: &mut dyn HostedInstrument, frames: usize) -> Vec<[f32; 2]> {
    assert_eq!(frames % 64, 0);
    let mut output = Vec::with_capacity(frames);
    for _ in 0..frames / 64 {
        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| unit.process(&mut left, &mut right)),
            0
        );
        // This is the test's device clock, not an audio callback wait.
        std::thread::sleep(Duration::from_millis(2));
        output.extend(
            left.into_iter()
                .zip(right)
                .map(|(left, right)| [left, right]),
        );
    }
    output
}

/// `binding` with the parameters `ids` set to `value`.
fn with_values(binding: &PluginBinding, ids: &[u32], value: f32) -> PluginBinding {
    let mut binding = binding.clone();
    for param in binding
        .parameters
        .iter_mut()
        .filter(|param| ids.contains(&param.id))
    {
        param.value = value;
    }
    binding
}

/// The CLAP sine whose note dialect is a parameter: CLAP notes at 0, MIDI
/// only at 1, taken up when it is (re)activated.
fn switching_sine() -> PluginBinding {
    const ID: &str = "org.windfall.test.switching-sine";
    let (_runtime, bindings) = clap_expression_fixtures(&[false]);
    let path = bindings[0].path.clone();
    let runtime = super::super::runtime::Runtime::new().unwrap();
    runtime.approve(&[windfall_ipc::PluginEntry {
        path: path.clone(),
        id: ID.into(),
        format: "clap".into(),
        name: "Switching fixture".into(),
        vendor: "Fixture".into(),
        instrument: true,
        usable: true,
        error: None,
    }]);
    runtime
        .discover(PluginBinding {
            wrapper: Default::default(),
            target: PluginTarget::Instrument {
                channel: windfall_project::ChannelId(900),
            },
            format: "clap".into(),
            path,
            id: ID.into(),
            name: "Switching fixture".into(),
            state: vec![],
            parameters: vec![],
            sidechain_input: None,
            auxiliary_inputs: vec![],
            capabilities: Default::default(),
        })
        .unwrap()
}

/// The plugin itself changes what it offers while a note sounds, and the app
/// takes its own path: the helper's restart request is published by the
/// owner as a capture request; the session's capture re-admits the plugin,
/// which first fades the output out and leaves the note's ramp in the carry;
/// the document takes the re-admission as one edit; the next document
/// preparation asks the runtime for the unit, which launches the replacement
/// helper (`Runtime::bridge_audio`), and the note is replayed on it as the
/// engine does. The test stands in for the session worker and the engine's
/// audio thread only: it drains the requests, applies the document edit and
/// plays the units, with the note's pan ramp 64 frames in when the capture
/// starts, as `migrate` sets it up by hand. Returns what was heard, the
/// binding before and the binding the document holds after.
fn restart_through_the_app(
    before: &PluginBinding,
    switch: &[(u32, f32)],
    law: PanLaw,
    label: &str,
) -> (Migration, PluginBinding, PluginBinding) {
    use super::super::runtime::{Runtime, Update};
    use windfall_project::{Command, Document, Project};
    let helper = PathBuf::from(
        std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
            .expect("explicit prebuilt current helper"),
    );
    let runtime = Runtime::bridge_fixture(&helper);
    runtime.approve(&[windfall_ipc::PluginEntry {
        path: before.path.clone(),
        id: before.id.clone(),
        format: before.format.clone(),
        name: before.name.clone(),
        vendor: "Fixture".into(),
        instrument: true,
        usable: true,
        error: None,
    }]);
    let mut document = Document::new(Project::new("Restart through the app"));
    document
        .dispatch(
            Command::AddPluginInstrument {
                plugin: before.clone(),
            },
            None,
        )
        .unwrap();
    let channel = document.project().channels[0].id;
    let target = PluginTarget::Instrument { channel };
    let bound = |document: &Document| document.project().plugin(target).unwrap().clone();
    let before = bound(&document);
    let deadline = std::time::Instant::now() + Duration::from_secs(30);

    // The document is prepared and installed; the engine asks for its unit.
    let staged = runtime.prepare_document();
    let mut first = staged.instrument(&before, 48_000, 64).unwrap();
    staged.install_document();
    runtime.commit_parameters(document.project());
    first.transport(transport());
    first.set_note_pan_law(law);

    // The user moves the plugin's own switch: a document edit, then the
    // value on the unit that plays it.
    for &(id, value) in switch {
        document
            .dispatch(Command::SetPluginParam { target, id, value }, None)
            .unwrap();
    }
    runtime.commit_parameters(document.project());
    for &(id, value) in switch {
        first.set_param(id, value);
    }
    // The plugin, in its helper, now says it offers something else and asks
    // to be restarted; the owner publishes a capture request for it. Both
    // happen on their own threads while audio runs.
    let mut request = None;
    let departure = loop {
        assert!(
            std::time::Instant::now() < deadline,
            "{label}: the plugin never asked to be restarted"
        );
        live(first.as_mut(), 64);
        request = runtime
            .drain()
            .into_iter()
            .rfind(|update| {
                matches!(update.update, Update::Capture { target: wanted, .. } if wanted == target)
            })
            .or(request);
        if let Some((departure, true)) = runtime.selected_bridge(target)
            && request.is_some()
        {
            break departure;
        }
    };
    let request = request.unwrap();
    assert!(
        !departure.requested.load(Ordering::Acquire),
        "{label}: nothing asked the unit to leave yet"
    );

    // The note, and its pan ramp, while the restart waits for the session.
    let mut first_plain = Bridged::launch(&before, false);
    let (bridge, native) = (first_plain.bridge, first_plain.native);
    assert_eq!(first.latency(), bridge + native, "{label}");
    let id = NoteInstanceId(7);
    first.note_on_instance(id, 69, 0.4, -0.5, NoteExpression::NEUTRAL);
    first_plain
        .audio
        .note_on_instance(NoteInstanceId(1), 69, 0.4, 0.0, NoteExpression::NEUTRAL);
    let mut heard = live(first.as_mut(), 256);
    let mut plain = first_plain.run(256);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            first.set_note_expression(id, 0.7, NoteExpression::NEUTRAL)
        }),
        0
    );
    heard.extend(live(first.as_mut(), 64));
    plain.extend(first_plain.run(64));

    // The session worker takes the request on. The capture re-admits the
    // plugin and, before the helper gives it back, the unit fades out.
    let capture = {
        let runtime = runtime.clone();
        let request = request.clone();
        let desired = bound(&document).parameters;
        std::thread::spawn(move || runtime.capture_pending(request, desired))
    };
    // Audio pauses only until the request to leave is there, so the fade
    // starts at frame 320 of the note as in `migrate`. A stopped stream
    // would not be waited for longer than 40 ms.
    while !departure.requested.load(Ordering::Acquire) {
        assert!(
            std::time::Instant::now() < deadline,
            "{label}: the capture never asked the unit to leave"
        );
        assert!(!capture.is_finished(), "{label}: the capture ended first");
        std::thread::sleep(Duration::from_micros(100));
    }
    let leaving = (320 + 2 * bridge + native).next_multiple_of(64);
    heard.extend(live(first.as_mut(), leaving));
    plain.extend(first_plain.run(leaving));
    while !capture.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "{label}: the capture never ended"
        );
        heard.extend(live(first.as_mut(), 64));
        plain.extend(first_plain.run(64));
    }
    let captured = capture
        .join()
        .unwrap()
        .unwrap()
        .expect("a capture of the playing unit");
    assert!(
        departure.departed.load(Ordering::Acquire),
        "{label}: the output faded out before the plugin was given back"
    );
    assert!(
        captured.restart,
        "{label}: a re-admission replaces the unit"
    );
    let metadata = captured
        .readmission
        .clone()
        .expect("the capability re-admission");

    // The session applies it as one document edit and acknowledges it.
    document
        .dispatch(
            Command::ReadmitPlugin {
                target,
                state: captured.bytes.clone(),
                parameters: metadata.parameters.clone(),
                auxiliary_inputs: metadata.auxiliary_inputs.clone(),
                outputs: metadata.outputs.clone(),
            },
            None,
        )
        .unwrap();
    let _ = runtime.acknowledge_committed_capture(&request, &captured);
    let after = bound(&document);
    for &(id, value) in switch {
        let saved = after
            .parameters
            .iter()
            .find(|param| param.id == id)
            .map(|param| param.value);
        assert!(
            saved.is_some_and(|saved| (saved - value).abs() < 1e-6),
            "{label}: parameter {id} is saved as {saved:?}"
        );
    }

    // The next document is prepared while the old unit plays on, silent
    // and holding the ramp however long that takes.
    let staged = runtime.prepare_document();
    let preparing = {
        let (staged, after) = (staged.clone(), after.clone());
        std::thread::spawn(move || staged.instrument(&after, 48_000, 64))
    };
    while !preparing.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "{label}: the replacement never came"
        );
        heard.extend(live(first.as_mut(), 64));
        plain.extend(first_plain.run(64));
    }
    let mut second = preparing.join().unwrap().unwrap();
    staged.install_document();
    runtime.commit_parameters(document.project());
    second.transport(transport());
    second.set_note_pan_law(law);
    // The engine plays the sounding note again on the unit that replaces
    // the old one, under the same identity and with its current pan.
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            second.note_on_instance(id, 69, 0.4, 0.7, NoteExpression::NEUTRAL)
        }),
        0
    );
    drop(first);
    let mut second_plain = Bridged::neutral(&after, false, 69, 0.4);
    let (second_bridge, second_native) = (second_plain.bridge, second_plain.native);
    assert_eq!(second.latency(), second_bridge + second_native, "{label}");
    let frames = (512 + second_bridge).next_multiple_of(64);
    let arrived = live(second.as_mut(), frames);
    let arrived_plain = second_plain.run(frames);
    assert_eq!(second.voices(), 1, "{label}: one note, not two");
    let migration = judge_handover(
        law,
        label,
        [(&heard[..], &plain[..]), (&arrived[..], &arrived_plain[..])],
        [(bridge, native), (second_bridge, second_native)],
    );
    // The note ends once, in the helper that now plays it.
    second.note_off_instance(id, 69);
    let tail = live(second.as_mut(), 9600);
    assert_eq!(second.voices(), 0, "{label}: no stuck note");
    assert!(
        tail[9600 - 64..].iter().all(|frame| peak(&[*frame]) < 1e-4),
        "{label}: silent after the release"
    );
    (migration, before, after)
}

/// Each switch through the app, under each law, against the same hand-over
/// with offline helpers set up by hand.
fn restarts_through_the_app(cases: &[(String, PluginBinding, Vec<(u32, f32)>)]) {
    for (what, before, switch) in cases {
        for law in LAWS {
            let name = format!("{what} through the app");
            let (live, before, after) = restart_through_the_app(before, switch, law, &name);
            let offline = migrate(&before, &after, law, true, &format!("{name} offline"));
            assert_handover(&name, law, &live, &offline);
        }
    }
}

#[test]
fn a_vst3_plugin_that_restarts_itself_mid_note_hands_the_ramp_over_on_the_apps_own_path() {
    // Class 29 offers pan and volume per note. Parameter 8 withholds pan
    // and 9 volume from its next activation, and while it runs the plugin
    // itself announces the change; the helper does not follow it live, so
    // it asks for a restart.
    let (_runtime, bindings) = fixtures(&[29]);
    let offers = bindings[0].clone();
    let mut cases = Vec::new();
    for (what, ids) in [
        ("pan", &[8][..]),
        ("volume", &[9][..]),
        ("both", &[8, 9][..]),
    ] {
        cases.push((
            format!("VST3 {what} lost"),
            offers.clone(),
            ids.iter().map(|id| (*id, 1.0)).collect(),
        ));
        cases.push((
            format!("VST3 {what} returns"),
            with_values(&offers, ids, 1.0),
            ids.iter().map(|id| (*id, 0.0)).collect(),
        ));
    }
    restarts_through_the_app(&cases);
}

#[test]
fn a_clap_plugin_that_restarts_itself_mid_note_hands_the_ramp_over_on_the_apps_own_path() {
    // The switching sine takes CLAP notes, so note instances, pan and
    // volume per note, until its note dialect parameter says MIDI only; it
    // asks for a restart itself when that changes while it runs.
    let sine = switching_sine();
    restarts_through_the_app(&[
        (
            "CLAP note instances lost".into(),
            sine.clone(),
            vec![(2, 1.0)],
        ),
        (
            "CLAP note instances return".into(),
            with_values(&sine, &[2], 1.0),
            vec![(2, 0.0)],
        ),
    ]);
}

#[test]
fn bridged_vst3_capability_loss_and_return_mid_note_keeps_the_ramp_and_never_clicks() {
    // 26: pan and volume per note. 23: pan only. 27: volume only. 2: neither.
    let (_runtime, bindings) = fixtures(&[26, 23, 27, 2]);
    let [both, pan_only, volume_only, neither] = [0, 1, 2, 3].map(|i| bindings[i].clone());
    migrations(
        "VST3",
        &[
            ("volume lost", both.clone(), pan_only.clone()),
            ("volume returns", pan_only, both.clone()),
            ("pan lost", both.clone(), volume_only.clone()),
            ("pan returns", volume_only, both.clone()),
            ("both lost", both.clone(), neither.clone()),
            ("both return", neither, both),
        ],
    );
}

#[test]
fn bridged_clap_note_instance_loss_and_return_mid_note_keeps_the_ramp_and_never_clicks() {
    // The CLAP sine takes note expressions; its twin takes MIDI only, which
    // is how a CLAP plugin loses note instances, pan and volume together.
    let (_runtime, bindings) = clap_expression_fixtures(&[false, true]);
    let (native, midi) = (bindings[0].clone(), bindings[1].clone());
    migrations(
        "CLAP",
        &[
            ("note instances lost", native.clone(), midi.clone()),
            ("note instances return", midi, native),
        ],
    );
}

#[test]
fn a_bridged_plugin_without_note_instances_gets_the_channel_stage_like_in_process() {
    // The CLAP twin takes MIDI only: it is played by key and channel, and
    // the channel applies its notes' pan and volume after it. The helper
    // must do what the in-process owner does, sample for sample: law
    // changes, per-note moves, a note gain, a newer note and its end.
    let mut worst = 0.0_f32;
    let mut loudest = 0.0_f32;
    for offline in [false, true] {
        for law in LAWS {
            let (runtime, bindings) = clap_expression_fixtures(&[true]);
            let binding = &bindings[0];
            let mut direct = runtime.instrument(binding, 48_000, 64).unwrap();
            direct.transport(transport());
            let mut bridged = Bridged::launch(binding, offline);
            assert!(!HostedInstrument::supports_note_instances(&bridged.audio));
            let delay = HostedEffect::latency(&bridged.audio) - direct.latency();
            let on = |channel| NoteExpression {
                color_group: Some(channel),
                ..NoteExpression::NEUTRAL
            };
            for unit in [
                &mut *direct,
                &mut bridged.audio as &mut dyn HostedInstrument,
            ] {
                unit.set_note_pan_law(law);
                unit.note_on_instance(NoteInstanceId(1), 69, 0.4, 0.65, on(3));
                unit.note_on_instance(NoteInstanceId(2), 76, 0.2, -0.4, on(5));
            }
            let mut direct_pcm = Vec::new();
            let mut bridge_pcm = Vec::new();
            let total = (3584 + delay).next_multiple_of(64);
            for frame in (0..total).step_by(64) {
                for unit in [
                    &mut *direct,
                    &mut bridged.audio as &mut dyn HostedInstrument,
                ] {
                    match frame {
                        1024 => unit.set_note_pan_law(PanLaw::Balance),
                        1536 => {
                            unit.set_note_expression(NoteInstanceId(1), -0.3, on(3));
                            unit.set_note_gain(NoteInstanceId(2), 0.6);
                        }
                        // A newer note leads the channel's share...
                        2048 => unit.note_on_instance(NoteInstanceId(3), 81, 0.3, 0.9, on(7)),
                        // ...and hands it back when it ends.
                        2560 => unit.note_off_on_channel(NoteInstanceId(3), 81, 7),
                        _ => {}
                    }
                }
                let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
                assert_eq!(
                    crate::test_alloc::allocator_calls(|| direct.process(&mut left, &mut right)),
                    0
                );
                direct_pcm.extend(
                    left.into_iter()
                        .zip(right)
                        .map(|(left, right)| [left, right]),
                );
                bridge_pcm.extend(bridged.run(64));
            }
            loudest = loudest.max(peak(&direct_pcm));
            // The channel's share is heard: left and right differ.
            assert!(
                direct_pcm[512..1024]
                    .iter()
                    .any(|frame| (frame[0] - frame[1]).abs() > 0.01),
                "{law:?}: the channel pans the keyed notes"
            );
            for frame in 0..3584 {
                for side in 0..2 {
                    let apart = (bridge_pcm[frame + delay][side] - direct_pcm[frame][side]).abs();
                    worst = worst.max(apart);
                    assert!(
                        apart < 2e-6,
                        "offline={offline} {law:?} frame={frame} side={side}: {} vs {}",
                        bridge_pcm[frame + delay][side],
                        direct_pcm[frame][side]
                    );
                }
            }
            // The notes end once and nothing is left sounding.
            for unit in [
                &mut *direct,
                &mut bridged.audio as &mut dyn HostedInstrument,
            ] {
                unit.note_off_on_channel(NoteInstanceId(1), 69, 3);
                unit.note_off_on_channel(NoteInstanceId(2), 76, 5);
            }
            let tail = bridged.run(9600);
            assert_eq!(HostedInstrument::voices(&bridged.audio), 0);
            assert!(tail[9600 - 64..].iter().all(|frame| peak(&[*frame]) < 1e-4));
        }
    }
    assert!(loudest > 0.05);
    println!("BRIDGED keyed plugin against in-process, four laws: worst {worst:.3e}");
}

#[test]
fn leaving_waits_for_the_fade_while_audio_runs_and_gives_up_when_it_does_not() {
    let (_runtime, bindings) = fixtures(&[26]);
    let mut unit = Bridged::launch(&bindings[0], false);
    unit.audio.set_note_pan_law(PanLaw::ConstantPower);
    unit.audio
        .note_on_instance(NoteInstanceId(3), 69, 0.4, 0.2, NoteExpression::NEUTRAL);
    unit.run(512);
    // No audio runs: the owner is not kept waiting for a fade nobody plays.
    let departure = unit.record.departure.clone();
    let started = std::time::Instant::now();
    assert!(!departure.leave(Duration::from_secs(5)));
    assert!(started.elapsed() < Duration::from_millis(500));
    departure.stay();
    let resumed = unit.run(1024);
    assert!(peak(&resumed[512..]) > 0.05, "it plays on after staying");
    // Audio runs: the owner returns once the faded output was played.
    let owner = {
        let departure = departure.clone();
        std::thread::spawn(move || departure.leave(Duration::from_secs(5)))
    };
    let mut silent = false;
    for _ in 0..200 {
        let block = unit.run(64);
        silent = block.iter().all(|frame| *frame == [0.0; 2]);
        if owner.is_finished() {
            break;
        }
    }
    assert!(owner.join().unwrap(), "left once the output had faded out");
    assert!(silent || unit.run(64).iter().all(|frame| *frame == [0.0; 2]));
}

#[test]
fn a_bridged_state_capture_during_a_sounding_note_leaves_the_audio_untouched() {
    // Saving while music plays does not replace the helper, so nothing
    // about its sound may change: no fade, no gap, no restarted note and no
    // restarted ramp. Each run is compared sample for sample with the same
    // note played without a capture, live and offline.
    type Fixture = fn() -> (Arc<super::super::runtime::Runtime>, Vec<PluginBinding>);
    let kinds: [(&str, Fixture); 2] = [
        ("VST3", || fixtures(&[26])),
        ("CLAP", || clap_expression_fixtures(&[false])),
    ];
    for (label, fixture) in kinds {
        for offline in [false, true] {
            let run = |capture: bool| {
                let id = NoteInstanceId(11);
                let (_runtime, bindings) = fixture();
                let mut unit = Bridged::launch(&bindings[0], offline);
                unit.audio.set_note_pan_law(PanLaw::ConstantPower);
                unit.audio
                    .note_on_instance(id, 69, 0.4, -0.5, NoteExpression::NEUTRAL);
                let mut heard = unit.run(448);
                unit.audio
                    .set_note_expression(id, 0.7, NoteExpression::NEUTRAL);
                heard.extend(unit.run(64));
                if capture {
                    let (state, _) = unit.record.capture(&bindings[0].parameters).unwrap();
                    assert!(!state.is_empty());
                }
                heard.extend(unit.run(3584 + unit.bridge));
                assert!(peak(&heard) > 0.05);
                heard
            };
            let (plain, captured) = (run(false), run(true));
            let first = plain
                .iter()
                .zip(&captured)
                .position(|(plain, captured)| plain != captured);
            assert_eq!(
                first, None,
                "{label} offline={offline}: the capture changed the audio from frame {first:?}"
            );
            println!(
                "BRIDGED capture {label} offline={offline}: {} frames identical with and without a state capture",
                plain.len()
            );
        }
    }
}
