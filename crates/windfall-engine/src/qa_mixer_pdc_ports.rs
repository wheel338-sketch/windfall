//! Delay compensation at the hardware ports (`win-mixer-pdc`).
//!
//! A track routed to its own pair of device channels leaves the mixer
//! before the master, so nothing downstream lines it up. The processor
//! delays each such path to the latest one, and a manual correction counts
//! there as it does inside the mix. These cases read the multichannel
//! block the device callback is given, which only the crate can see.

use windfall_core::{AudioBuffer, TICKS_PER_STEP};
use windfall_dsp::LimiterParams;
use windfall_project::{
    Channel, ChannelId, ChannelSource, DEFAULT_KEY, EffectId, EffectParams, EffectSlot,
    ExternalOutputRoute, Lane, MixerTrack, Note, NoteId, Project, SampleAsset, SampleId,
    SamplePath, SamplerSettings, TrackId,
};

use crate::{Processor, SamplePool};

const RATE: u32 = 48_000;
const CHANNELS: usize = 4;

struct Rig {
    project: Project,
    pool: SamplePool,
}

impl Rig {
    fn new() -> Self {
        Self {
            project: Project::new("ports"),
            pool: SamplePool::new(),
        }
    }

    fn id(&mut self) -> u32 {
        self.project.next_id += 1;
        self.project.next_id - 1
    }

    /// A track playing an impulse `late` frames into its sample on the
    /// first step, routed to the master.
    fn track(&mut self, late: usize) -> TrackId {
        let track = TrackId(self.id());
        self.project.mixer.tracks.push(MixerTrack {
            dock: windfall_project::MixerDock::default(),
            external_output: None,
            processing: windfall_dsp::TrackParams::default(),
            current: false,
            latency_offset_ms: 0.0,
            id: track,
            name: String::new(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            output: Some(TrackId::MASTER),
            sidechains: Vec::new(),
            sends: Vec::new(),
            effects: Vec::new(),
            recording: None,
        });
        let sample = SampleId(self.id());
        let mut data = vec![0.0; late + 1];
        data[late] = 1.0;
        self.pool
            .insert(sample, AudioBuffer::from_interleaved(RATE, 1, data));
        self.project.samples.push(SampleAsset {
            id: sample,
            name: String::new(),
            path: SamplePath::Project(format!("{}.wav", sample.0)),
        });
        let channel = ChannelId(self.id());
        self.project.channels.push(Channel {
            id: channel,
            name: String::new(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            group: String::new(),
            voice: Default::default(),
            timing: windfall_project::ChannelTiming::default(),
            expression_map: Default::default(),
            mixer_track: track,
            source: ChannelSource::Sampler(SamplerSettings {
                sample: Some(sample),
                ..SamplerSettings::default()
            }),
        });
        let note = NoteId(self.id());
        let pattern = &mut self.project.patterns[0];
        pattern.lanes.push(Lane {
            channel,
            notes: vec![Note {
                id: note,
                start: 0,
                length: TICKS_PER_STEP,
                key: DEFAULT_KEY,
                velocity: 1.0,
                pan: 0.0,
                expression: Default::default(),
            }],
        });
        pattern.lanes.sort_by_key(|lane| lane.channel);
        track
    }

    fn track_mut(&mut self, id: TrackId) -> &mut MixerTrack {
        let tracks = self.project.mixer.tracks.iter_mut();
        tracks
            .into_iter()
            .find(|track| track.id == id)
            .expect("the track exists")
    }

    /// A limiter that does nothing but put its output out 5 ms, 240 frames,
    /// late.
    fn limiter(&mut self, track: TrackId) {
        let id = EffectId(self.id());
        self.track_mut(track).effects.push(EffectSlot {
            id,
            enabled: true,
            mix: 1.0,
            params: EffectParams::Limiter(LimiterParams {
                ceiling_db: 0.0,
                input_gain_db: 0.0,
                release_ms: 100.0,
                lookahead_ms: 5.0,
            }),
        });
    }

    /// The frame of the one impulse on each device channel, or `None` for
    /// a silent channel, over `frames` frames played `block` at a time.
    fn port_marks(&self, frames: usize, block: usize) -> [Option<usize>; CHANNELS] {
        let (mut processor, controller) = Processor::new(RATE);
        processor.set_output_channels(CHANNELS);
        controller.set_project(&self.project, &self.pool);
        controller.play();
        let mut marks = [None; CHANNELS];
        let mut stereo = vec![0.0_f32; 2 * block];
        let mut done = 0;
        while done < frames {
            processor.process(&mut stereo);
            let ports = processor.hardware_samples();
            assert_eq!(ports.len(), block * CHANNELS);
            for (offset, frame) in ports.chunks_exact(CHANNELS).enumerate() {
                for (channel, value) in frame.iter().enumerate() {
                    if value.abs() > 1e-6 {
                        assert!(
                            marks[channel].is_none(),
                            "channel {channel} has more than one impulse"
                        );
                        assert!((value - 1.0).abs() < 1e-5, "channel {channel}: {value}");
                        marks[channel] = Some(done + offset);
                    }
                }
            }
            done += block;
        }
        marks
    }
}

fn pair(left: u16, exclusive: bool) -> Option<ExternalOutputRoute> {
    Some(ExternalOutputRoute {
        left,
        right: Some(left + 1),
        exclusive,
    })
}

#[test]
fn qa_an_external_pair_is_delayed_to_meet_a_latent_master_path() {
    let mut rig = Rig::new();
    let slow = rig.track(0);
    let out = rig.track(0);
    rig.limiter(slow);
    rig.track_mut(out).external_output = pair(2, true);
    // The latent track reaches the master pair 240 frames late; the track
    // with its own pair has no latency and waits for it.
    for block in [1, 64, 127, 256] {
        assert_eq!(
            rig.port_marks(2_048, block),
            [Some(240), Some(240), Some(240), Some(240)],
            "block {block}"
        );
    }
}

#[test]
fn qa_a_latent_external_pair_holds_the_master_back() {
    let mut rig = Rig::new();
    let plain = rig.track(0);
    let out = rig.track(0);
    rig.limiter(out);
    rig.track_mut(out).external_output = pair(2, true);
    let _ = plain;
    assert_eq!(
        rig.port_marks(2_048, 127),
        [Some(240), Some(240), Some(240), Some(240)]
    );
}

#[test]
fn qa_manual_correction_counts_at_the_external_ports() {
    // The track on the external pair is 100 frames late without reporting
    // it. Corrected, the master pair waits those 100 frames.
    let mut rig = Rig::new();
    let _plain = rig.track(0);
    let out = rig.track(100);
    rig.track_mut(out).external_output = pair(2, true);
    assert_eq!(
        rig.port_marks(2_048, 127),
        [Some(0), Some(0), Some(100), Some(100)],
        "uncorrected"
    );
    rig.track_mut(out).latency_offset_ms = 100.0 * 1_000.0 / f64::from(RATE);
    for block in [1, 127, 256] {
        assert_eq!(
            rig.port_marks(2_048, block),
            [Some(100), Some(100), Some(100), Some(100)],
            "corrected, block {block}"
        );
    }

    // A correction on a track inside the mix moves the external pair too.
    let mut rig = Rig::new();
    let late = rig.track(100);
    let out = rig.track(0);
    rig.track_mut(late).latency_offset_ms = 100.0 * 1_000.0 / f64::from(RATE);
    rig.track_mut(out).external_output = pair(2, true);
    assert_eq!(
        rig.port_marks(2_048, 127),
        [Some(100), Some(100), Some(100), Some(100)]
    );
}
