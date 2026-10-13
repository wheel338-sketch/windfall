//! Per-note pan and volume routes through the production desktop owner, with
//! the owned CLAP and VST3 fixtures. A hosted plugin returns one summed
//! output: a property it offers per note is applied by the plugin, one it
//! does not offer is applied by the channel after the plugin. Every expected
//! number here is the fixture's own output for a neutral, centred note times
//! the project pan law's gain, worked out in the test.
use super::ownership_tests::{clap_expression_fixtures, expression_audio, fixtures, transport};
use super::*;
use windfall_core::PanLaw;
use windfall_dsp::{NoteExpression, NoteInstanceId};
use windfall_plugin_host::project_expression::Routes;

const LAWS: [PanLaw; 4] = [
    PanLaw::Balance,
    PanLaw::ConstantPower,
    PanLaw::Compromise,
    PanLaw::Linear,
];
/// Frames of every ramp and fade at 48 kHz.
const FADE: usize = 240;

/// The same plugin playing one neutral, centred note with no project law:
/// what the fixture itself puts out. Its factory is returned to keep it alive
/// and is not shared, so it never sees another unit's notes.
fn neutral(
    runtime: &Arc<Runtime>,
    binding: &PluginBinding,
    key: u8,
    velocity: f32,
) -> (Arc<dyn PluginFactory>, Box<dyn HostedInstrument>) {
    let factory = runtime.render_factory().unwrap();
    let mut unit = factory.instrument(binding, 48_000, 64).unwrap();
    unit.transport(transport());
    unit.note_on_instance(
        NoteInstanceId(1),
        key,
        velocity,
        0.0,
        NoteExpression::NEUTRAL,
    );
    (factory, unit)
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

#[test]
fn all_four_native_and_channel_routes_apply_the_project_law_exactly_once() {
    // Fixture class, and whether it offers pan and volume per note.
    let mut worst = 0.0_f32;
    for (class, pan, volume) in [
        (26, true, true),
        (23, true, false),
        (27, false, true),
        (2, false, false),
    ] {
        for law in LAWS {
            let (runtime, bindings) = fixtures(&[class]);
            let binding = &bindings[0];
            let mut unit = runtime.instrument(binding, 48_000, 64).unwrap();
            unit.transport(transport());
            let capabilities = runtime.note_capabilities(binding.target).unwrap();
            assert_eq!(
                capabilities.routes(0),
                Routes { pan, volume },
                "class {class}"
            );
            // Whatever the routes, the law is available on this channel.
            capabilities.require_pan_law(0).unwrap();
            capabilities.require_expression(0, true, true).unwrap();
            let (_factory, mut centre) = neutral(&runtime, binding, 69, 0.4);
            unit.set_note_pan_law(law);
            unit.note_on_instance(NoteInstanceId(1), 69, 0.4, 0.65, NoteExpression::NEUTRAL);
            unit.set_note_gain(NoteInstanceId(1), 0.5);
            // The gain takes one ramp to arrive.
            expression_audio(unit.as_mut(), 512);
            expression_audio(centre.as_mut(), 512);
            let got = expression_audio(unit.as_mut(), 512);
            let plain = expression_audio(centre.as_mut(), 512);
            assert!(peak(&plain) > 0.01, "class {class}: the fixture sounds");
            let gains = law.gains(0.65);
            for (i, (got, plain)) in got.iter().zip(&plain).enumerate() {
                for (side, gain) in [(0, gains.0), (1, gains.1)] {
                    let want = plain[side] * gain * 0.5;
                    worst = worst.max((got[side] - want).abs());
                    assert!(
                        (got[side] - want).abs() < 2e-6,
                        "class {class} {law:?} side {side} frame {i}: {} vs {want}",
                        got[side]
                    );
                }
            }
            assert_eq!(unit.voices(), 1);
        }
    }
    println!("ROUTE four combinations in process: worst difference {worst:.3e}");
}

/// The law's gains `elapsed` frames into the pan ramp every migration test
/// runs: from half left to seven tenths right.
fn ramp_gains(law: PanLaw, elapsed: usize) -> (f32, f32) {
    ramp_gains_at(law, elapsed as f32)
}
fn ramp_gains_at(law: PanLaw, elapsed: f32) -> (f32, f32) {
    let (from, to) = (law.gains(-0.5), law.gains(0.7));
    let part = elapsed.min(FADE as f32) / FADE as f32;
    (
        from.0 + (to.0 - from.0) * part,
        from.1 + (to.1 - from.1) * part,
    )
}

/// Asks for the plugin of the instance `token` the way a restart does, and
/// deactivates it once the audio side has handed it over.
fn take_plugin(runtime: &Arc<Runtime>, token: u64) -> (mpsc::Receiver<()>, mpsc::Receiver<()>) {
    let (asked, asked_rx) = mpsc::channel();
    let (gone, gone_rx) = mpsc::channel();
    runtime
        .jobs
        .send(Box::new(move |owner| {
            let record = owner.instances.get_mut(&token).unwrap();
            record.ownership.request();
            asked.send(()).unwrap();
            let adapter = loop {
                if let Some(adapter) = record.ownership.take_returned() {
                    break adapter;
                }
                std::thread::sleep(Duration::from_millis(1));
            };
            record.release(adapter).unwrap();
            gone.send(()).unwrap();
        }))
        .unwrap();
    (asked_rx, gone_rx)
}

struct Migration {
    /// Everything heard: the ramp's first 64 frames, the departure and the
    /// first 512 frames of the replacement.
    heard: Vec<[f32; 2]>,
    worst: f32,
    step: f32,
    own_step: f32,
    /// The gain measured on each side just before the plugin left and just
    /// after its replacement arrived, with the mean frame each was taken at.
    before: [(f32, f32); 2],
    after: [(f32, f32); 2],
}

/// One note, its pan ramp 64 frames in, when the plugin is taken away and a
/// unit of `after` takes over the same note. `before` and `after` are the
/// same project object with different capabilities.
fn migrate(
    runtime: &Arc<Runtime>,
    factory: &dyn PluginFactory,
    before: &PluginBinding,
    after: &PluginBinding,
    law: PanLaw,
    label: &str,
) -> Migration {
    let id = NoteInstanceId(7);
    let mut first = factory.instrument(before, 48_000, 64).unwrap();
    let token = runtime
        .call(|owner| Ok(*owner.instances.keys().max().unwrap()))
        .unwrap();
    first.transport(transport());
    first.set_note_pan_law(law);
    let (_first_factory, mut first_plain) = neutral(runtime, before, 69, 0.4);
    first.note_on_instance(id, 69, 0.4, -0.5, NoteExpression::NEUTRAL);
    let mut model = Vec::new();
    let mut heard = expression_audio(first.as_mut(), 256);
    let mut plain = expression_audio(first_plain.as_mut(), 256);
    // What the ramp has reached on each frame of the first unit, and how
    // much of the output is let through.
    let mut reached = vec![0_usize; 256];
    let mut presence = vec![1.0_f32; 256];
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            first.set_note_expression(id, 0.7, NoteExpression::NEUTRAL)
        }),
        0
    );
    heard.extend(expression_audio(first.as_mut(), 64));
    plain.extend(expression_audio(first_plain.as_mut(), 64));
    reached.extend(0..64);
    presence.extend([1.0; 64]);

    // The plugin restarts. The ramp waits where it is while the output
    // fades out, and the plugin leaves at a block boundary after that.
    let (asked, gone) = take_plugin(runtime, token);
    asked.recv_timeout(Duration::from_secs(5)).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut departure = Vec::new();
    loop {
        assert_eq!(
            crate::test_alloc::allocator_calls(|| first.control_boundary()),
            0
        );
        departure.extend(expression_audio(first.as_mut(), 64));
        if gone.try_recv().is_ok() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{label}: not returned"
        );
        if departure.len() > 320 {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    first.control_boundary();
    departure.extend(expression_audio(first.as_mut(), 64));
    assert!(departure.len() >= 320, "{label}: {}", departure.len());
    assert!(
        departure[FADE..].iter().all(|frame| *frame == [0.0; 2]),
        "{label}: silent once the fade is over"
    );
    departure.truncate(320);
    heard.extend(&departure);
    plain.extend(expression_audio(first_plain.as_mut(), 320));
    reached.extend([64; 320]);
    presence.extend((0..320).map(|k| (1.0 - (k + 1) as f32 / FADE as f32).max(0.0)));
    let latency = first.latency();
    for t in 0..heard.len() {
        let gains = ramp_gains(law, reached[t.saturating_sub(latency)]);
        model.push([
            plain[t][0] * gains.0 * presence[t],
            plain[t][1] * gains.1 * presence[t],
        ]);
    }
    // The last frames that still sound at half level or more.
    let before_frame = 320 + FADE / 2 - 1;
    let before_gain = estimate(&heard, &plain, &presence, before_frame - 32..before_frame);

    // The replacement: the same project object, another set of capabilities,
    // and the note replayed under its own identity as the engine does.
    let mut second = factory.instrument(after, 48_000, 64).unwrap();
    second.transport(transport());
    second.set_note_pan_law(law);
    let (_second_factory, mut second_plain) = neutral(runtime, after, 69, 0.4);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            second.note_on_instance(id, 69, 0.4, 0.7, NoteExpression::NEUTRAL)
        }),
        0
    );
    let arrived = expression_audio(second.as_mut(), 512);
    let arrived_plain = expression_audio(second_plain.as_mut(), 512);
    let latency = second.latency();
    let arrival: Vec<f32> = (0..512)
        .map(|n| ((n + 1) as f32 / FADE as f32).min(1.0))
        .collect();
    let start = heard.len();
    for n in 0..512_usize {
        let gains = ramp_gains(law, 64 + n.saturating_sub(latency));
        model.push([
            arrived_plain[n][0] * gains.0 * arrival[n],
            arrived_plain[n][1] * gains.1 * arrival[n],
        ]);
    }
    heard.extend(&arrived);
    // Measured once what the replacement was sent has come out of it.
    let after_gain = estimate(
        &arrived,
        &arrived_plain,
        &arrival,
        latency + 8..latency + 40,
    );
    assert_eq!(second.voices(), 1, "{label}: one note, not two");

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
    // The note ends once, in the unit that now plays it.
    second.note_off_instance(id, 69);
    let tail = expression_audio(second.as_mut(), 9600);
    assert_eq!(second.voices(), 0, "{label}: no stuck note");
    assert!(
        tail[9600 - 64..].iter().all(|frame| peak(&[*frame]) < 1e-4),
        "{label}: silent after the release"
    );
    let own_step = largest_step(&plain).max(largest_step(&arrived_plain));
    Migration {
        step: largest_step(&heard[256..]),
        own_step,
        heard,
        worst,
        before: before_gain,
        after: after_gain,
    }
}

/// The gain on each side over `frames`, from the frames loud enough to
/// tell, and the mean position of those frames within the range.
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

/// Loss and return of each capability, live and offline, for one format.
fn migrations(label: &str, runtime: &Arc<Runtime>, pairs: &[(&str, PluginBinding, PluginBinding)]) {
    for (what, before, after) in pairs {
        for law in LAWS {
            let name = format!("{label} {what}");
            let live = migrate(runtime, runtime.as_ref(), before, after, law, &name);
            let offline_factory = runtime.render_factory().unwrap();
            let offline = migrate(
                runtime,
                offline_factory.as_ref(),
                before,
                after,
                law,
                &format!("{name} offline"),
            );
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
            // Ramp continuity: 64 frames in before the plugin left, 64 plus
            // the frames since it came back after, and nothing in between.
            let frozen = ramp_gains(law, 64);
            // The estimate after starts eight frames into the replacement.
            let resumed = [
                ramp_gains_at(law, 64.0 + 8.0 + live.after[0].1).0,
                ramp_gains_at(law, 64.0 + 8.0 + live.after[1].1).1,
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
            let slope = (ramp_gains(law, FADE).0 - ramp_gains(law, 0).0) / FADE as f32;
            assert!(
                (live.after[0].0 - live.before[0].0 - slope * between).abs() < 1e-3,
                "{name} {law:?}: the ramp went on from {} to {}",
                live.before[0].0,
                live.after[0].0
            );
            let mut apart = 0.0_f32;
            for (live, offline) in live.heard.iter().zip(&offline.heard) {
                apart = apart
                    .max((live[0] - offline[0]).abs())
                    .max((live[1] - offline[1]).abs());
            }
            assert_eq!(live.heard.len(), offline.heard.len());
            assert!(
                apart <= 2e-6,
                "{name} {law:?}: live and offline differ by {apart}"
            );
            println!(
                "ROUTE migration {name} {law:?}: worst={:.3e} step={:.5} bound={bound:.5} own_step={:.5} before=({:.5},{:.5}) frozen=({:.5},{:.5}) after=({:.5},{:.5}) live_vs_offline={apart:.1e}",
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
    }
}

#[test]
fn vst3_capability_loss_and_return_mid_note_keeps_the_ramp_and_never_clicks() {
    // 26: pan and volume per note. 23: pan only. 27: volume only. 2: neither.
    let (runtime, bindings) = fixtures(&[26, 23, 27, 2]);
    let [both, pan_only, volume_only, neither] = [0, 1, 2, 3].map(|i| bindings[i].clone());
    migrations(
        "VST3",
        &runtime,
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
fn clap_note_instance_loss_and_return_mid_note_keeps_the_ramp_and_never_clicks() {
    // The CLAP sine takes note expressions; its twin takes MIDI only, which
    // is how a CLAP plugin loses note instances, pan and volume together.
    let (runtime, bindings) = clap_expression_fixtures(&[false, true]);
    let (native, midi) = (bindings[0].clone(), bindings[1].clone());
    migrations(
        "CLAP",
        &runtime,
        &[
            ("note instances lost", native.clone(), midi.clone()),
            ("note instances return", midi, native),
        ],
    );
}

/// The mark of the fourth note below: velocity .5 with the probe's neutral
/// expression and brightness, `1 + .5 / 4 + .5 / 8`.
const UNDECLARED: f64 = 0.5 * 1.1875;

/// One note of the VST3 expression probe as the test follows it. The probe's
/// output is a constant per note: volume times velocity times
/// `1 + tuning / 12 + expression / 4 + brightness / 8`, panned `(1 - p, p)`.
#[derive(Clone, Copy)]
struct Gesture {
    velocity: f64,
    pan: Slide,
    tuning: Slide,
    expression: Slide,
    brightness: Slide,
}
/// A control on its way from one value to another over the fade.
#[derive(Clone, Copy)]
struct Slide {
    from: f64,
    to: f64,
    since: usize,
}
impl Slide {
    fn at(value: f64) -> Self {
        Self {
            from: value,
            to: value,
            since: 0,
        }
    }
    fn value(self, frame: usize) -> f64 {
        let part = (frame.saturating_sub(self.since)).min(FADE) as f64 / FADE as f64;
        self.from + (self.to - self.from) * part
    }
    fn move_to(&mut self, to: f64, frame: usize) {
        self.from = self.value(frame);
        self.to = to;
        self.since = frame;
    }
}
impl Gesture {
    fn mark(self, frame: usize) -> f64 {
        self.velocity
            * (1.0
                + self.tuning.value(frame) / 12.0
                + self.expression.value(frame) / 4.0
                + self.brightness.value(frame) / 8.0)
    }
    /// The law's gains: the pan ramp is linear in gain, not in position.
    fn gains(self, law: PanLaw, frame: usize) -> (f64, f64) {
        let (from, to) = (
            law.gains(self.pan.from as f32),
            law.gains(self.pan.to as f32),
        );
        let part = (frame.saturating_sub(self.pan.since)).min(FADE) as f64 / FADE as f64;
        (
            f64::from(from.0) + f64::from(to.0 - from.0) * part,
            f64::from(from.1) + f64::from(to.1 - from.1) * part,
        )
    }
}

#[test]
fn per_note_channel_gestures_stay_independent_and_an_undeclared_channel_moves_them_to_the_channel()
{
    // The VST3 probe declares pan and volume on MIDI channels 1 to 15 and
    // nothing on channel 16: per-note channels, as an MPE instrument has.
    let law = PanLaw::ConstantPower;
    let (runtime, bindings) = fixtures(&[26]);
    let binding = &bindings[0];
    let mut unit = runtime.instrument(binding, 48_000, 64).unwrap();
    unit.transport(transport());
    unit.set_note_pan_law(law);
    let on = |channel: u8| NoteExpression {
        color_group: Some(channel),
        ..NoteExpression::NEUTRAL
    };
    let ids = [NoteInstanceId(11), NoteInstanceId(12), NoteInstanceId(13)];
    let mut notes = [
        (60_u8, 1_u8, 0.5_f64, -0.6_f64),
        (64, 2, 0.4, 0.0),
        (67, 3, 0.3, 0.6),
    ]
    .map(|(_, _, velocity, pan)| Gesture {
        velocity,
        pan: Slide::at(pan),
        tuning: Slide::at(0.0),
        expression: Slide::at(0.5),
        brightness: Slide::at(0.5),
    });
    for (index, (key, channel, velocity, pan)) in [
        (60_u8, 1_u8, 0.5_f32, -0.6_f32),
        (64, 2, 0.4, 0.0),
        (67, 3, 0.3, 0.6),
    ]
    .into_iter()
    .enumerate()
    {
        unit.note_on_instance(ids[index], key, velocity, pan, on(channel));
    }
    let capabilities = runtime.note_capabilities(binding.target).unwrap();
    assert_eq!(capabilities.routes(3), Routes::NATIVE);
    assert_eq!(capabilities.routes(15), Routes::CHANNEL);

    let mut heard = expression_audio(unit.as_mut(), 256);
    // Three gestures at once, one per note and none shared: a bend on the
    // first, a pan sweep on the second, both timbre controls on the third.
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            unit.set_note_pitch(ids[0], 60.5);
            unit.set_note_expression(ids[1], 0.8, on(2));
            unit.set_note_expression(
                ids[2],
                0.6,
                NoteExpression {
                    modulation_x: 0.9,
                    modulation_y: 0.2,
                    ..on(3)
                },
            );
        }),
        0
    );
    notes[0].tuning.move_to(0.5, 256);
    notes[1].pan.move_to(0.8, 256);
    // The default map sends modulation X to brightness and Y to expression.
    notes[2].brightness.move_to(0.9, 256);
    notes[2].expression.move_to(0.2, 256);
    heard.extend(expression_audio(unit.as_mut(), 320));
    let mut worst = 0.0_f64;
    for (frame, got) in heard.iter().enumerate() {
        let mut want = (0.0, 0.0);
        for note in notes {
            let gains = note.gains(law, frame);
            want.0 += note.mark(frame) * gains.0 / 2.0;
            want.1 += note.mark(frame) * gains.1 / 2.0;
        }
        for (got, want) in [(got[0], want.0), (got[1], want.1)] {
            worst = worst.max((f64::from(got) - want).abs());
            assert!(
                (f64::from(got) - want).abs() < 4e-6,
                "frame {frame}: {got} vs {want}"
            );
        }
    }
    println!("ROUTE per-channel gestures: worst difference {worst:.3e}");
    let settled = *heard.last().unwrap();

    // A fourth note on the channel that declares nothing. One plugin has one
    // output, so pan and volume move to the channel for every note, across
    // the fade: the plugin is sent each note's values on their way to
    // neutral while the channel's gains travel to the newest note's.
    let fourth = NoteInstanceId(14);
    assert_eq!(
        crate::test_alloc::allocator_calls(|| {
            unit.note_on_instance(fourth, 72, 0.5, -1.0, on(15))
        }),
        0
    );
    let moved = expression_audio(unit.as_mut(), 512);
    let frame = 576 + 512;
    let newest = law.gains(-1.0);
    let mut worst = 0.0_f64;
    for (n, got) in moved.iter().enumerate() {
        // Weight 1 is all in the plugin, 0 all on the channel.
        let weight = 1.0 - n.min(FADE) as f64 / FADE as f64;
        let slew = (n + 1).min(FADE) as f64 / FADE as f64;
        let stage = (
            1.0 + (f64::from(newest.0) - 1.0) * slew,
            1.0 + (f64::from(newest.1) - 1.0) * slew,
        );
        let mut plugin = (UNDECLARED * 0.5, UNDECLARED * 0.5);
        for note in notes {
            let gains = note.gains(law, 576 + n);
            let sum = gains.0 + gains.1;
            let pan = 0.5 + weight * (gains.1 / sum - 0.5);
            let volume = 1.0 + weight * (sum / 2.0 - 1.0);
            plugin.0 += note.mark(576 + n) * volume * (1.0 - pan);
            plugin.1 += note.mark(576 + n) * volume * pan;
        }
        for (got, want) in [(got[0], plugin.0 * stage.0), (got[1], plugin.1 * stage.1)] {
            worst = worst.max((f64::from(got) - want).abs());
            assert!(
                (f64::from(got) - want).abs() < 4e-6,
                "moving to the channel, frame {n}: {got} vs {want}"
            );
        }
    }
    // The newest note starts at once in this fixture, as any note does.
    // Apart from its own onset nothing steps: take its part out, at the
    // gains the channel had on each frame, and look at what is left.
    let mut rest = vec![settled];
    for (n, got) in moved.iter().enumerate() {
        let slew = (n + 1).min(FADE) as f32 / FADE as f32;
        let own = (UNDECLARED * 0.5) as f32;
        rest.push([
            got[0] - own * (1.0 + (newest.0 - 1.0) * slew),
            got[1] - own * (1.0 + (newest.1 - 1.0) * slew),
        ]);
    }
    let step = largest_step(&rest);
    println!("ROUTE undeclared channel: worst difference {worst:.3e} largest step {step:.5}");
    assert!(step < 0.01, "step {step}");
    // Settled: every note at the newest note's pan, each still with its own
    // bend and timbre.
    let last = moved.last().unwrap();
    let total: f64 = notes.iter().map(|note| note.mark(frame)).sum::<f64>() + UNDECLARED;
    assert!((f64::from(last[0]) - total * 0.5 * f64::from(newest.0)).abs() < 4e-6);
    assert!((f64::from(last[1]) - total * 0.5 * f64::from(newest.1)).abs() < 4e-6);

    // The undeclared note ends: per-note pan and volume go back to the plugin
    // for the three that remain, and the sound returns to what it was.
    unit.note_off_instance(fourth, 72);
    let back = expression_audio(unit.as_mut(), 512);
    let step = largest_step(&[&[*last][..], &back[..]].concat());
    // The undeclared note itself stops at once in this fixture.
    let step = step.max(largest_step(&back));
    let own = (UNDECLARED * 0.5) as f32 * newest.0.max(newest.1);
    assert!(step < own + 0.02, "step {step}");
    let last = back.last().unwrap();
    assert!((last[0] - settled[0]).abs() < 4e-6 && (last[1] - settled[1]).abs() < 4e-6);
    assert_eq!(unit.voices(), 3);
    for id in ids {
        unit.note_off_instance(id, 0);
    }
    expression_audio(unit.as_mut(), 64);
    assert_eq!(unit.voices(), 0);
}
