//! Shared helpers: building small projects and running a processor.

use windfall_core::{AudioBuffer, TICKS_PER_STEP};
use windfall_dsp::{EnvelopeParams, LimiterParams, ParamSet, SynthParams, Waveform};
use windfall_engine::{Controller, Processor, SamplePool};
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{Automation, AutomationId, AutomationPoint, AutomationTarget};
use windfall_project::{
    Channel, ChannelId, ChannelSource, Clip, ClipContent, ClipId, DEFAULT_KEY, EffectId,
    EffectParams, EffectSlot, InstrumentParams, Lane, MixerTrack, Note, NoteId, Pattern, PatternId,
    PlaylistTrack, PlaylistTrackId, Project, SampleAsset, SampleId, SamplePath, SamplerSettings,
    TrackId,
};

/// A project under construction together with its decoded samples.
///
/// Everything starts at unity: channel volume, track volume and velocity
/// are 1 and pans are centered, so a sample comes out of the master exactly
/// as it went in unless a test changes something.
pub struct Rig {
    pub project: Project,
    pub pool: SamplePool,
}

impl Rig {
    pub fn new() -> Self {
        Self {
            project: Project::new("test"),
            pool: SamplePool::new(),
        }
    }

    fn id(&mut self) -> u32 {
        self.project.next_id += 1;
        self.project.next_id - 1
    }

    pub fn first_pattern(&self) -> PatternId {
        self.project.patterns[0].id
    }

    pub fn sample(&mut self, audio: AudioBuffer) -> SampleId {
        let id = SampleId(self.id());
        self.pool.insert(id, audio);
        self.project.samples.push(SampleAsset {
            id,
            name: format!("sample {}", id.0),
            path: SamplePath::Project(format!("{}.wav", id.0)),
        });
        id
    }

    /// Adds a mixer track that plays into the master.
    pub fn track(&mut self) -> TrackId {
        let id = TrackId(self.id());
        self.project.mixer.tracks.push(MixerTrack {
            dock: windfall_project::MixerDock::default(),
            external_output: None,
            processing: windfall_dsp::TrackParams::default(),
            current: false,
            latency_offset_ms: 0.0,
            id,
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
        id
    }

    /// Adds a channel that plays `audio` into `track`.
    pub fn channel_on(&mut self, audio: AudioBuffer, track: TrackId) -> ChannelId {
        let sample = self.sample(audio);
        let id = ChannelId(self.id());
        self.project.channels.push(Channel {
            sample_playback: None,
            id,
            name: String::new(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            group: String::new(),
            voice: Default::default(),
            expression_map: Default::default(),
            sample_instrument: None,
            timing: windfall_project::ChannelTiming::default(),
            mixer_track: track,
            source: ChannelSource::Sampler(SamplerSettings {
                sample: Some(sample),
                ..SamplerSettings::default()
            }),
        });
        id
    }

    /// Adds a channel that plays `audio` straight into the master.
    pub fn channel(&mut self, audio: AudioBuffer) -> ChannelId {
        self.channel_on(audio, TrackId::MASTER)
    }

    pub fn channel_mut(&mut self, id: ChannelId) -> &mut Channel {
        self.project
            .channels
            .iter_mut()
            .find(|channel| channel.id == id)
            .expect("the channel exists")
    }

    pub fn sampler_mut(&mut self, id: ChannelId) -> &mut SamplerSettings {
        let ChannelSource::Sampler(settings) = &mut self.channel_mut(id).source else {
            panic!("the channel is not a sampler");
        };
        settings
    }

    /// Puts an effect at the end of a track's chain, switched on and at
    /// full mix.
    pub fn effect(&mut self, track: TrackId, params: EffectParams) -> EffectId {
        let id = EffectId(self.id());
        self.track_mut(track).effects.push(EffectSlot {
            impulse: None,
            id,
            enabled: true,
            mix: 1.0,
            params,
        });
        id
    }

    pub fn effect_mut(&mut self, id: EffectId) -> &mut EffectSlot {
        let tracks = self.project.mixer.tracks.iter_mut();
        tracks
            .flat_map(|track| &mut track.effects)
            .find(|effect| effect.id == id)
            .expect("the effect exists")
    }

    /// Takes an effect out of the project and returns it.
    pub fn remove_effect(&mut self, id: EffectId) -> EffectSlot {
        for track in &mut self.project.mixer.tracks {
            if let Some(index) = track.effects.iter().position(|effect| effect.id == id) {
                return track.effects.remove(index);
            }
        }
        panic!("the effect exists");
    }

    /// Adds a channel that plays the synth with these settings into `track`.
    pub fn synth_on(&mut self, params: SynthParams, track: TrackId) -> ChannelId {
        let id = ChannelId(self.id());
        self.project.channels.push(Channel {
            sample_playback: None,
            id,
            name: String::new(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            group: String::new(),
            voice: Default::default(),
            expression_map: Default::default(),
            sample_instrument: None,
            timing: windfall_project::ChannelTiming::default(),
            mixer_track: track,
            source: ChannelSource::Instrument {
                asset: None,
                params: InstrumentParams::SubtractiveSynth(params),
            },
        });
        id
    }

    pub fn synth_mut(&mut self, id: ChannelId) -> &mut SynthParams {
        let ChannelSource::Instrument { params, .. } = &mut self.channel_mut(id).source else {
            panic!("the channel is not an instrument");
        };
        match params {
            InstrumentParams::SubtractiveSynth(synth) => synth,
            _ => panic!("expected the subtractive synth"),
        }
    }

    pub fn track_mut(&mut self, id: TrackId) -> &mut MixerTrack {
        self.project
            .mixer
            .tracks
            .iter_mut()
            .find(|track| track.id == id)
            .expect("the track exists")
    }

    pub fn pattern_mut(&mut self, id: PatternId) -> &mut Pattern {
        self.project
            .patterns
            .iter_mut()
            .find(|pattern| pattern.id == id)
            .expect("the pattern exists")
    }

    pub fn pattern(&mut self, length_steps: u32) -> PatternId {
        let id = PatternId(self.id());
        self.project.patterns.push(Pattern {
            note_curves: Vec::new(),
            events: Vec::new(),
            time_signature: None,
            timeline: Default::default(),
            id,
            name: String::new(),
            color: 0,
            length_steps,
            lanes: Vec::new(),
        });
        id
    }

    /// Adds a note to a pattern and returns it for further changes. The
    /// note has the default key, full velocity and no pan.
    pub fn note_in(
        &mut self,
        pattern: PatternId,
        channel: ChannelId,
        start: u32,
        length: u32,
    ) -> &mut Note {
        let id = NoteId(self.id());
        let pattern = self.pattern_mut(pattern);
        let lane = match pattern
            .lanes
            .iter()
            .position(|lane| lane.channel == channel)
        {
            Some(index) => index,
            None => {
                pattern.lanes.push(Lane {
                    channel,
                    notes: Vec::new(),
                });
                pattern.lanes.sort_by_key(|lane| lane.channel);
                pattern
                    .lanes
                    .iter()
                    .position(|lane| lane.channel == channel)
                    .expect("the lane was just added")
            }
        };
        let notes = &mut pattern.lanes[lane].notes;
        notes.push(Note {
            id,
            start,
            length,
            key: DEFAULT_KEY,
            velocity: 1.0,
            pan: 0.0,
            expression: Default::default(),
        });
        notes.sort_by_key(Note::sort_key);
        notes
            .iter_mut()
            .find(|note| note.id == id)
            .expect("the note was just added")
    }

    /// Adds a note to the first pattern.
    pub fn note(&mut self, channel: ChannelId, start: u32, length: u32) -> &mut Note {
        self.note_in(self.first_pattern(), channel, start, length)
    }

    /// Lights steps of the first pattern's step sequencer.
    pub fn steps(&mut self, channel: ChannelId, steps: &[u32]) {
        for step in steps {
            self.note(channel, step * TICKS_PER_STEP, TICKS_PER_STEP);
        }
    }

    pub fn playlist_track(&mut self) -> PlaylistTrackId {
        let id = PlaylistTrackId(self.id());
        self.project.playlist.tracks.push(PlaylistTrack {
            id,
            name: String::new(),
            muted: false,
            solo: false,
            color: 0,
            height: 0,
        });
        id
    }

    pub fn clip(
        &mut self,
        track: PlaylistTrackId,
        pattern: PatternId,
        start: u32,
        length: u32,
    ) -> &mut Clip {
        let id = ClipId(self.id());
        let clips = &mut self.project.playlist.clips;
        clips.push(Clip {
            id,
            track,
            start,
            length,
            offset: 0,
            muted: false,
            content: ClipContent::Pattern { pattern },
        });
        clips.sort_by_key(Clip::sort_key);
        clips
            .iter_mut()
            .find(|clip| clip.id == id)
            .expect("the clip was just added")
    }

    /// Puts an audio clip that plays `audio` into `mixer_track` on the
    /// playlist, at unity, with no fades and no offset.
    pub fn audio_clip(
        &mut self,
        track: PlaylistTrackId,
        audio: AudioBuffer,
        mixer_track: TrackId,
        start: u32,
        length: u32,
    ) -> ClipId {
        let sample = self.sample(audio);
        let id = ClipId(self.id());
        let clips = &mut self.project.playlist.clips;
        clips.push(Clip {
            id,
            track,
            start,
            length,
            offset: 0,
            muted: false,
            content: ClipContent::Audio {
                sample,
                mixer_track,
                output: Default::default(),
                normalize: false,
                gain: 1.0,
                pan: 0.0,
                fade_in: 0,
                fade_out: 0,
                reverse: false,
                pitch: 0.0,

                stretch: Default::default(),
            },
        });
        clips.sort_by_key(Clip::sort_key);
        id
    }

    pub fn clip_mut(&mut self, id: ClipId) -> &mut Clip {
        let mut clips = self.project.playlist.clips.iter_mut();
        clips.find(|clip| clip.id == id).expect("the clip exists")
    }

    /// Changes what is particular to an audio clip.
    pub fn audio_mut(&mut self, id: ClipId, change: impl FnOnce(&mut AudioSettings)) {
        let ClipContent::Audio {
            sample,
            mixer_track,
            normalize,
            gain,
            pan,
            fade_in,
            fade_out,
            reverse,
            pitch,
            stretch,
            output,
        } = self.clip_mut(id).content
        else {
            panic!("the clip is not an audio clip");
        };
        let mut settings = AudioSettings {
            mixer_track,
            gain,
            pan,
            fade_in,
            fade_out,
            reverse,
            pitch,
        };
        change(&mut settings);
        self.clip_mut(id).content = ClipContent::Audio {
            sample,
            mixer_track: settings.mixer_track,
            normalize,
            gain: settings.gain,
            pan: settings.pan,
            fade_in: settings.fade_in,
            fade_out: settings.fade_out,
            reverse: settings.reverse,
            pitch: settings.pitch,
            stretch,
            output,
        };
    }

    /// Adds an automation of `target` whose curve runs in straight lines
    /// through `points`, given as a tick and a value from 0 to 1.
    pub fn automation(&mut self, target: AutomationTarget, points: &[(u32, f32)]) -> AutomationId {
        let id = AutomationId(self.id());
        self.project.automations.push(Automation {
            id,
            name: String::new(),
            color: 0,
            target,
            points: points
                .iter()
                .map(|&(tick, value)| AutomationPoint {
                    tick,
                    value,
                    curve: 0.0,
                    hold: false,
                })
                .collect(),
        });
        id
    }

    pub fn automation_mut(&mut self, id: AutomationId) -> &mut Automation {
        let mut automations = self.project.automations.iter_mut();
        automations
            .find(|automation| automation.id == id)
            .expect("the automation exists")
    }

    /// Puts an automation on the playlist, from the start of its curve.
    pub fn automation_clip(
        &mut self,
        track: PlaylistTrackId,
        automation: AutomationId,
        start: u32,
        length: u32,
    ) -> ClipId {
        let id = ClipId(self.id());
        let clips = &mut self.project.playlist.clips;
        clips.push(Clip {
            id,
            track,
            start,
            length,
            offset: 0,
            muted: false,
            content: ClipContent::Automation { automation },
        });
        clips.sort_by_key(Clip::sort_key);
        id
    }

    /// A processor with this project loaded, set to play the playlist,
    /// not yet playing.
    pub fn song_processor(&self, sample_rate: u32) -> (Processor, Controller) {
        let (processor, controller) = self.processor(sample_rate);
        controller.set_transport(TransportPatch {
            mode: Some(PlayMode::Song),
            ..TransportPatch::default()
        });
        (processor, controller)
    }

    /// Plays the playlist from the top for `frames` frames, `block` frames
    /// per call, and returns the interleaved stereo output.
    pub fn play_song(&self, sample_rate: u32, frames: usize, block: usize) -> Vec<f32> {
        let (mut processor, controller) = self.song_processor(sample_rate);
        controller.play();
        run(&mut processor, frames, block)
    }

    /// A processor with this project loaded, not yet playing.
    pub fn processor(&self, sample_rate: u32) -> (Processor, Controller) {
        let (processor, controller) = Processor::new(sample_rate);
        controller.set_project(&self.project, &self.pool);
        (processor, controller)
    }

    /// Plays the project from the top for `frames` frames, `block` frames
    /// per call, and returns the interleaved stereo output.
    pub fn play(&self, sample_rate: u32, frames: usize, block: usize) -> Vec<f32> {
        let (mut processor, controller) = self.processor(sample_rate);
        controller.play();
        run(&mut processor, frames, block)
    }
}

/// The settings of an audio clip, for [`Rig::audio_mut`].
pub struct AudioSettings {
    pub mixer_track: TrackId,
    pub gain: f32,
    pub pan: f32,
    pub fade_in: u32,
    pub fade_out: u32,
    pub reverse: bool,
    pub pitch: f32,
}

/// A mono buffer in which every frame has a value of its own, small enough
/// that a mix of a few of them stays exact: frame `n` holds `counted(n)`.
pub fn counting(sample_rate: u32, frames: usize) -> AudioBuffer {
    let data = (0..frames).map(counted).collect();
    AudioBuffer::from_interleaved(sample_rate, 1, data)
}

/// The value of frame `frame` of a [`counting`] buffer.
pub fn counted(frame: usize) -> f32 {
    (frame + 1) as f32 / (1 << 20) as f32
}

/// Processes `frames` frames, `block` frames per call.
pub fn run(processor: &mut Processor, frames: usize, block: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * 2];
    for chunk in out.chunks_mut(block * 2) {
        processor.process(chunk);
    }
    out
}

/// Puts an automation of every kind of target on the playlist of a project
/// that has at least three mixer tracks besides the master, the first of
/// which sends to the third, a sampler as its first channel, an instrument
/// on `synth`, and two effects on the third track. The curves have bends,
/// holds and jumps in them, the clips overlap and start part way into
/// their curves, and the tempo moves all the way through.
pub fn automate_everything(rig: &mut Rig, synth: ChannelId) {
    let (first, third) = (
        rig.project.mixer.tracks[1].id,
        rig.project.mixer.tracks[3].id,
    );
    let sampler = rig.project.channels[0].id;
    let effects: Vec<EffectId> = rig.track_mut(third).effects.iter().map(|e| e.id).collect();
    let (above, below) = (rig.playlist_track(), rig.playlist_track());

    let ride = rig.automation(
        AutomationTarget::TrackVolume { track: first },
        &[
            (0, 0.7),
            (400, 0.3),
            (400, 0.9),
            (1_300, 0.5),
            (2_000, 0.75),
        ],
    );
    rig.automation_mut(ride).points[0].curve = 0.6;
    rig.automation_mut(ride).points[3].hold = true;
    rig.automation_clip(above, ride, 37, 2_400);
    let dip = rig.automation(AutomationTarget::TrackVolume { track: first }, &[(0, 0.4)]);
    rig.automation_clip(below, dip, 0, 700);

    let pan = rig.automation(
        AutomationTarget::ChannelPan { channel: sampler },
        &[(0, 0.0), (900, 1.0), (1_800, 0.2)],
    );
    rig.automation_mut(pan).points[1].curve = -0.8;
    rig.automation_clip(above, pan, 2_450, 400);
    rig.automation_clip(below, pan, 701, 1_500);
    let quiet = rig.automation(
        AutomationTarget::ChannelVolume { channel: synth },
        &[(0, 0.8), (1_000, 0.45), (2_900, 0.7)],
    );
    rig.automation_clip(below, quiet, 2_201, 700);
    let throw = rig.automation(
        AutomationTarget::SendGain {
            track: first,
            target: third,
        },
        &[(0, 0.1), (600, 0.8), (601, 0.0), (1_500, 0.6)],
    );
    let clip = rig.automation_clip(above, throw, 2_850, 70);
    rig.clip_mut(clip).offset = 550;
    let master = rig.automation(
        AutomationTarget::TrackPan {
            track: TrackId::MASTER,
        },
        &[(0, 0.5), (2_920, 0.35)],
    );
    let lane = rig.playlist_track();
    rig.automation_clip(lane, master, 0, 2_920);

    // The first setting of each effect and the mix of the second.
    for (index, effect) in effects.iter().enumerate() {
        let target = AutomationTarget::EffectParam {
            track: third,
            effect: *effect,
            param: 1 + index as u32,
        };
        let sweep = rig.automation(target, &[(0, 0.2), (1_000, 0.9), (2_500, 0.4)]);
        rig.automation_mut(sweep).points[0].curve = 1.0;
        rig.automation_clip(lane, sweep, 100 + 50 * index as u32, 2_000);
    }
    let mix = rig.automation(
        AutomationTarget::EffectMix {
            track: third,
            effect: effects[1],
        },
        &[(0, 1.0), (700, 0.3), (1_400, 0.8)],
    );
    let other = rig.playlist_track();
    rig.automation_clip(other, mix, 300, 1_400);
    let cutoff = SynthParams::index_of("filter.cutoffHz").expect("the synth has a cutoff");
    let filter = rig.automation(
        AutomationTarget::InstrumentParam {
            channel: synth,
            param: cutoff as u32,
        },
        &[(0, 0.5), (800, 0.95), (2_000, 0.6)],
    );
    rig.automation_clip(other, filter, 1_701, 1_200);
    rig.automation_clip(other, filter, 0, 300);

    // The tempo: a bent climb, a jump, a hold, and a second clip that
    // takes over from a lower track for a while.
    let bpm = |bpm: f32| (bpm - 10.0) / 512.0;
    let tempo = rig.automation(
        AutomationTarget::Tempo,
        &[
            (0, bpm(150.0)),
            (900, bpm(260.0)),
            (900, bpm(120.0)),
            (1_500, bpm(120.0)),
            (2_400, bpm(310.0)),
        ],
    );
    rig.automation_mut(tempo).points[0].curve = -0.5;
    rig.automation_mut(tempo).points[3].hold = true;
    let clip = rig.automation_clip(other, tempo, 211, 2_100);
    rig.clip_mut(clip).offset = 130;
    let rush = rig.automation(
        AutomationTarget::Tempo,
        &[(0, bpm(90.0)), (300, bpm(400.0))],
    );
    let last = rig.playlist_track();
    rig.automation_clip(last, rush, 1_900, 700);
}

/// A limiter with nothing to do to a signal that stays within full scale:
/// its ceiling is at 0 dB and it adds no gain. All it does is put its
/// output out `lookahead_ms` late.
pub fn idle_limiter(lookahead_ms: f32) -> EffectParams {
    EffectParams::Limiter(LimiterParams {
        ceiling_db: 0.0,
        input_gain_db: 0.0,
        release_ms: 100.0,
        lookahead_ms,
        ..LimiterParams::default()
    })
}

/// A limiter that holds its output to `ceiling_db` and looks 5 ms, 240
/// frames at 48 kHz, ahead.
pub fn limiter(ceiling_db: f32) -> EffectParams {
    EffectParams::Limiter(LimiterParams {
        ceiling_db,
        input_gain_db: 0.0,
        release_ms: 100.0,
        lookahead_ms: 5.0,
        ..LimiterParams::default()
    })
}

/// A plain synth sound: one sine at half scale that is at full level a
/// millisecond after its note starts and gone a millisecond after it ends,
/// whatever the velocity.
pub fn plain_synth() -> SynthParams {
    let mut params = SynthParams {
        amp_envelope: EnvelopeParams {
            attack_ms: 1.0,
            decay_ms: 1.0,
            sustain: 1.0,
            release_ms: 1.0,
        },
        amp_velocity: 0.0,
        gain: 0.5,
        ..SynthParams::default()
    };
    params.oscillators[0].waveform = Waveform::Sine;
    params
}

/// A single full-scale mono sample. Played at its own rate it marks exactly
/// one output frame.
pub fn impulse(sample_rate: u32) -> AudioBuffer {
    AudioBuffer::from_interleaved(sample_rate, 1, vec![1.0])
}

/// A mono buffer holding one constant value.
pub fn level(sample_rate: u32, value: f32, seconds: f64) -> AudioBuffer {
    let frames = (seconds * f64::from(sample_rate)).round() as usize;
    AudioBuffer::from_interleaved(sample_rate, 1, vec![value; frames])
}

/// The same audio upside down.
pub fn inverted(audio: &AudioBuffer) -> AudioBuffer {
    let data = audio.samples().iter().map(|sample| -sample).collect();
    AudioBuffer::from_interleaved(audio.sample_rate(), audio.channels(), data)
}

/// The square root of the mean of the squares: how loud a stretch of
/// signal is.
pub fn rms(signal: &[f32]) -> f32 {
    let sum: f64 = signal.iter().map(|sample| f64::from(*sample).powi(2)).sum();
    (sum / signal.len().max(1) as f64).sqrt() as f32
}

/// A mono sine wave at half scale.
pub fn sine(sample_rate: u32, frequency: f64, seconds: f64) -> AudioBuffer {
    let frames = (seconds * f64::from(sample_rate)).round() as usize;
    let data = (0..frames)
        .map(|frame| {
            let phase = frame as f64 * frequency / f64::from(sample_rate);
            ((phase * std::f64::consts::TAU).sin() * 0.5) as f32
        })
        .collect();
    AudioBuffer::from_interleaved(sample_rate, 1, data)
}

/// The frames of an interleaved stereo buffer where either side is not zero.
pub fn sounding_frames(audio: &[f32]) -> Vec<usize> {
    frames(audio)
        .iter()
        .enumerate()
        .filter(|(_, frame)| frame[0] != 0.0 || frame[1] != 0.0)
        .map(|(index, _)| index)
        .collect()
}

/// The left side of an interleaved stereo buffer.
pub fn left(audio: &[f32]) -> Vec<f32> {
    frames(audio).iter().map(|frame| frame[0]).collect()
}

/// The right side of an interleaved stereo buffer.
pub fn right(audio: &[f32]) -> Vec<f32> {
    frames(audio).iter().map(|frame| frame[1]).collect()
}

/// An interleaved stereo buffer as left and right pairs.
fn frames(audio: &[f32]) -> &[[f32; 2]] {
    audio.as_chunks().0
}

/// How much of an envelope's decay or release is still to come `elapsed`
/// frames into its `frames`: 1 on the first frame and 0 on the frame after
/// the last. It is an exponential that would be 60 dB down when the time is
/// up, lowered by that last thousandth and scaled back to full height, so
/// it falls at a steady rate in decibels and still arrives.
pub fn fall(elapsed: usize, frames: usize) -> f32 {
    let floor = 0.001_f64 / 1.001;
    (1.001 * floor.powf(elapsed as f64 / frames as f64) - 0.001) as f32
}

/// The largest jump between neighbouring samples.
pub fn largest_step(signal: &[f32]) -> f32 {
    signal
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max)
}

pub fn peak(signal: &[f32]) -> f32 {
    signal
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// The frame a note at `tick` must land on: the first frame at or after the
/// tick's time, worked out in whole numbers. The tempo is `bpm_times_100`
/// hundredths of a beat per minute.
pub fn frame_of_tick(tick: u64, bpm_times_100: u64, sample_rate: u32) -> usize {
    // frames = tick * sample_rate * 60 / (bpm * 960)
    let numerator = u128::from(tick) * u128::from(sample_rate) * 60 * 100;
    let denominator = u128::from(bpm_times_100) * 960;
    numerator.div_ceil(denominator) as usize
}

/// Frequency of a signal in hertz, from the average spacing of its upward
/// zero crossings.
pub fn frequency(signal: &[f32], sample_rate: u32) -> f64 {
    let crossings: Vec<f64> = signal
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(index, pair)| {
            // Where the line between the two samples crosses zero.
            index as f64 + f64::from(-pair[0]) / f64::from(pair[1] - pair[0])
        })
        .collect();
    assert!(crossings.len() > 2, "the signal does not oscillate");
    let cycles = (crossings.len() - 1) as f64;
    let span = crossings[crossings.len() - 1] - crossings[0];
    cycles * f64::from(sample_rate) / span
}
