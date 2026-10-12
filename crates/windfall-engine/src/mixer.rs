//! Mixer tracks: summing, effects, faders, pan, routing and meters.
//!
//! The signal on a track goes: everything that plays into it, summed, then
//! its effects in order, then its fader and pan, and from there to its
//! meter, its output and its sends.

use windfall_project::MAX_MIXER_TRACKS;

use crate::plan::Plan;
use crate::ramp::Ramp;
use crate::shared::Shared;
use crate::state::{PlanState, Strip};

/// Most frames the engine processes in one go. Larger device buffers are
/// worked through in pieces of this size.
pub(crate) const MAX_BLOCK: usize = 256;

/// One stereo frame: left, then right.
pub(crate) type Frame = [f32; 2];

/// Recording sinks observing the same rendered mixer block.
pub(crate) struct CaptureSinks<'a> {
    pub disk: &'a mut [crate::recording_disk::DiskWriter],
    pub logger: &'a mut Option<Box<crate::audio_logger::LoggerTap>>,
}

/// Scratch belongs to a plan, and retires with its DSP/worker owner.
pub(crate) struct TrackScratch {
    left: Box<[f32]>,
    right: Box<[f32]>,
    dry_left: Box<[f32]>,
    dry_right: Box<[f32]>,
    key: Box<[Frame]>,
}
impl TrackScratch {
    pub fn new() -> Self {
        let side = || vec![0.0; MAX_BLOCK].into_boxed_slice();
        Self {
            left: side(),
            right: side(),
            dry_left: side(),
            dry_right: side(),
            key: vec![[0.0; 2]; MAX_BLOCK].into_boxed_slice(),
        }
    }
}
struct RenderTrack {
    buffer: *mut Frame,
    keys: *const Frame,
    chain: *mut Vec<Option<crate::rack::EffectUnit>>,
    key_delays: *mut Vec<crate::state::DelaySlot>,
    processor: *mut windfall_dsp::TrackProcessor,
    scratch: *mut TrackScratch,
    activity: *mut crate::state::Activity,
    base: u64,
    frames: usize,
    silence: f32,
}
unsafe fn render_track(context: *mut ()) {
    // SAFETY: the scoped stage grants one job exclusive access to these specific
    // track fields. Inputs are read-only for the whole stage. No broad PlanState
    // or Mixer reference is created while another lane is accessing their fields.
    let job = unsafe { &mut *context.cast::<RenderTrack>() };
    let buffer = unsafe { std::slice::from_raw_parts_mut(job.buffer, job.frames) };
    let keys = unsafe { std::slice::from_raw_parts(job.keys, job.frames) };
    let chain = unsafe { &mut *job.chain };
    let delays = unsafe { &mut *job.key_delays };
    let processor = unsafe { &mut *job.processor };
    let scratch = unsafe { &mut *job.scratch };
    let activity = unsafe { &mut *job.activity };
    let left = &mut scratch.left[..job.frames];
    let right = &mut scratch.right[..job.frames];
    for (frame, (left, right)) in buffer.iter().zip(left.iter_mut().zip(right.iter_mut())) {
        *left = frame[0];
        *right = frame[1];
    }
    for (place, unit) in chain.iter_mut().enumerate() {
        let Some(unit) = unit else {
            continue;
        };
        scratch.key[..job.frames].copy_from_slice(keys);
        if let Some(line) = &mut delays[place].line {
            line.process(&mut scratch.key[..job.frames]);
        }
        unit.process_sidechain(
            left,
            right,
            &mut scratch.dry_left,
            &mut scratch.dry_right,
            Some(&scratch.key[..job.frames]),
        );
    }
    if !chain.is_empty() {
        for (frame, (left, right)) in buffer.iter_mut().zip(left.iter().zip(right.iter())) {
            *frame = [*left, *right];
        }
        activity.hear_output(job.base, buffer, job.silence);
    }
    processor.process(left, right);
    for (frame, (left, right)) in buffer.iter_mut().zip(left.iter().zip(right.iter())) {
        *frame = [*left, *right];
    }
}

pub(crate) struct Mixer {
    waveforms: Box<[crate::waveform_meter::WaveAccumulator]>,
    /// [`MAX_BLOCK`] frames per possible mixer track, end to end. Voices add
    /// into them, then [`Mixer::mix`] turns them into post-fader signals in
    /// routing order.
    buffers: Box<[Frame]>,
    /// As many again, for what instruments and other tracks feed into a
    /// track whose own voices are delayed to line up with them. The two
    /// are summed once the voices have been through their delay.
    inbox: Box<[Frame]>,
    keys: Box<[Frame]>,
    /// [`MAX_BLOCK`] frames that go to the output as they are, past every
    /// track. Voices that left the mix to fade out add into it.
    apart: Box<[Frame]>,
    /// Finished prints bypass the mixer, with their own compensation delay.
    printed: Box<[Frame]>,
    /// One block of frames on its way through a delay.
    scratch: Box<[Frame]>,
    /// One block with the sides apart, which is how effects take it.
    left: Box<[f32]>,
    right: Box<[f32]>,
    /// The same block as it was before an effect that is fading in or out.
    dry_left: Box<[f32]>,
    dry_right: Box<[f32]>,
    /// One block of each separately routable output of the effect being
    /// run, on its way to the track it is routed to.
    aux: [Box<[Frame]>; windfall_dsp::MAX_AUX_OUTPUTS],
}

impl Mixer {
    pub fn new() -> Self {
        let frames = |count: usize| vec![[0.0; 2]; count].into_boxed_slice();
        let side = || vec![0.0; MAX_BLOCK].into_boxed_slice();
        Self {
            waveforms: (0..MAX_MIXER_TRACKS)
                .map(|_| crate::waveform_meter::WaveAccumulator::new())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            buffers: frames(MAX_MIXER_TRACKS * MAX_BLOCK),
            inbox: frames(MAX_MIXER_TRACKS * MAX_BLOCK),
            keys: frames(MAX_MIXER_TRACKS * MAX_BLOCK),
            apart: frames(MAX_BLOCK),
            printed: frames(MAX_BLOCK),
            scratch: frames(MAX_BLOCK),
            left: side(),
            right: side(),
            dry_left: side(),
            dry_right: side(),
            aux: std::array::from_fn(|_| frames(MAX_BLOCK)),
        }
    }

    /// Silences the first `frames` frames of the first `tracks` tracks and
    /// of what goes past them.
    pub fn clear(&mut self, tracks: usize, frames: usize) {
        for track in 0..tracks {
            self.track_mut(track)[..frames].fill([0.0; 2]);
            self.inbox[track * MAX_BLOCK..][..frames].fill([0.0; 2]);
            self.keys[track * MAX_BLOCK..][..frames].fill([0.0; 2]);
        }
        self.apart[..frames].fill([0.0; 2]);
        self.printed[..frames].fill([0.0; 2]);
    }

    pub fn track_mut(&mut self, track: usize) -> &mut [Frame] {
        &mut self.buffers[track * MAX_BLOCK..][..MAX_BLOCK]
    }

    /// A track's block as [`Mixer::mix`] left it: after its fader and pan.
    pub fn track(&self, track: usize) -> &[Frame] {
        &self.buffers[track * MAX_BLOCK..][..MAX_BLOCK]
    }

    /// The buffer for voices that go straight to the output.
    pub fn apart_mut(&mut self) -> &mut [Frame] {
        &mut self.apart
    }

    pub fn printed_mut(&mut self) -> &mut [Frame] {
        &mut self.printed
    }

    /// Runs the mixer over one block of `out.len()` frames starting on frame
    /// `base`, and writes the master, with what goes past it, to `out`.
    ///
    /// The instruments, already rendered, are added to their tracks first.
    /// Then the tracks are taken in the plan's routing order. Each runs its
    /// input through its effects, gets its fader and pan, is metered, and
    /// is then added to its output and sends, so by the time a track is
    /// reached everything feeding it has arrived. Wherever signals of
    /// different latency meet, the earlier ones go through their
    /// compensation delay first.
    pub fn mix(
        &mut self,
        plan: &Plan,
        state: &mut PlanState,
        shared: &Shared,
        base: u64,
        out: &mut [Frame],
        captures: CaptureSinks<'_>,
    ) {
        let CaptureSinks { disk: taps, logger } = captures;
        let frames = out.len();
        self.add_instruments(plan, state, base, frames);
        let control_tempo = state.tempo();

        for wave in plan.waves.ranges() {
            let parallel = wave.len() > 1
                && state.workers.count() > 1
                && logger.is_none()
                && plan.control_sources.is_empty()
                && plan.order[wave.clone()].iter().all(|&index| {
                    !plan.tracks[index].current
                        && !plan.tracks[index].tapped
                        && state.chains[index]
                            .iter()
                            .flatten()
                            .all(crate::rack::EffectUnit::parallel_safe)
                });
            if parallel {
                self.render_stage(plan, state, shared, wave.clone(), base, frames);
            }
            for &index in &plan.order[wave.clone()] {
                if !parallel {
                    let track = &plan.tracks[index];
                    if track.current {
                        let id = shared
                            .current_track
                            .load(std::sync::atomic::Ordering::Acquire);
                        if let Some(source) = plan
                            .track_ids
                            .get(id)
                            .filter(|&source| !plan.tracks[source].current)
                        {
                            self.scratch[..frames]
                                .copy_from_slice(&self.buffers[source * MAX_BLOCK..][..frames]);
                            self.buffers[index * MAX_BLOCK..][..frames]
                                .copy_from_slice(&self.scratch[..frames]);
                        }
                    }
                    let buffer = &mut self.buffers[index * MAX_BLOCK..][..frames];
                    if let Some(line) = &mut state.direct[index].line {
                        line.process(buffer);
                        let inbox = &self.inbox[index * MAX_BLOCK..][..frames];
                        for (frame, fed) in buffer.iter_mut().zip(inbox) {
                            frame[0] += fed[0];
                            frame[1] += fed[1];
                        }
                    }

                    state.activity[index].hear_input(base, buffer, state.silence);
                    if !state.chains[index].is_empty() {
                        let (left, right) = (&mut self.left[..frames], &mut self.right[..frames]);
                        for (frame, (left, right)) in
                            buffer.iter().zip(left.iter_mut().zip(&mut *right))
                        {
                            *left = frame[0];
                            *right = frame[1];
                        }
                        for place in 0..state.chains[index].len() {
                            let Some(unit) = &mut state.chains[index][place] else {
                                if let Some(logger) = logger.as_mut() {
                                    logger.effect(track.effects[place].id, left, right);
                                }
                                continue;
                            };
                            if let Some(effect) = track.effects.get(place) {
                                if unit.control_enabled() {
                                    state.controls.process_slot(
                                        plan,
                                        effect.id,
                                        unit.control_params(),
                                        (left, right),
                                        base,
                                        control_tempo,
                                    );
                                } else {
                                    state.controls.silence(plan, effect.id);
                                }
                            }
                            self.scratch[..frames]
                                .copy_from_slice(&self.keys[index * MAX_BLOCK..][..frames]);
                            if let Some(line) = &mut state.key_delays[index][place].line {
                                line.process(&mut self.scratch[..frames]);
                            }
                            let routed = track.routed_outputs(place);
                            if routed == 0 {
                                unit.process_sidechain(
                                    left,
                                    right,
                                    &mut self.dry_left,
                                    &mut self.dry_right,
                                    Some(&self.scratch[..frames]),
                                );
                                if let Some(logger) = logger.as_mut() {
                                    logger.effect(track.effects[place].id, left, right);
                                }
                                continue;
                            }
                            // The effect's own outputs leave the track here, before
                            // the rest of the chain and the fader. Each goes through
                            // its compensation delay into the track it is routed to,
                            // which routing order puts after this one.
                            let [first, second, third] = &mut self.aux;
                            let mut aux = windfall_dsp::AuxBlock {
                                routed,
                                outputs: [
                                    &mut first[..frames],
                                    &mut second[..frames],
                                    &mut third[..frames],
                                ],
                            };
                            unit.process_routed(
                                left,
                                right,
                                &mut self.dry_left,
                                &mut self.dry_right,
                                Some(&self.scratch[..frames]),
                                &mut aux,
                            );
                            if let Some(logger) = logger.as_mut() {
                                logger.effect(track.effects[place].id, left, right);
                            }
                            for edge in &track.edges {
                                let Some(tap) = edge.tap.filter(|tap| tap.place == place) else {
                                    continue;
                                };
                                let sent = &mut self.aux[usize::from(tap.output)][..frames];
                                if let Some(line) = &mut state.edge_delays[edge.slot].line {
                                    line.process(sent);
                                }
                                let feeds = if state.direct[edge.target].line.is_some() {
                                    &mut self.inbox
                                } else {
                                    &mut self.buffers
                                };
                                add_scaled(
                                    sent,
                                    &mut feeds[edge.target * MAX_BLOCK..][..frames],
                                    &state.edges[edge.slot],
                                    base,
                                );
                            }
                        }
                        let buffer = &mut self.buffers[index * MAX_BLOCK..][..frames];
                        for (frame, (left, right)) in buffer
                            .iter_mut()
                            .zip(self.left[..frames].iter().zip(&self.right[..frames]))
                        {
                            *frame = [*left, *right];
                        }
                        state.activity[index].hear_output(base, buffer, state.silence);
                    }
                    let buffer = &mut self.buffers[index * MAX_BLOCK..][..frames];

                    let left = &mut self.left[..frames];
                    let right = &mut self.right[..frames];
                    for (frame, (left, right)) in
                        buffer.iter().zip(left.iter_mut().zip(right.iter_mut()))
                    {
                        *left = frame[0];
                        *right = frame[1];
                    }
                    state.track_processing[index].process(left, right);
                    for (frame, (left, right)) in
                        buffer.iter_mut().zip(left.iter().zip(right.iter()))
                    {
                        *frame = [*left, *right];
                    }
                }
                let track = &plan.tracks[index];
                if !parallel {
                    state.workers.record_node(u64::from(track.id.0), shared);
                }
                let buffer = &mut self.buffers[index * MAX_BLOCK..][..frames];
                state.activity[index].hear_output(base, buffer, state.silence);
                for tap in taps.iter_mut().filter(|tap| tap.tap.track == track.id) {
                    tap.copy(
                        base,
                        state.latency,
                        state.behind[index],
                        windfall_project::MixerRecordMode::PostEffects,
                        buffer,
                    );
                }
                if let Some(logger) = logger.as_mut() {
                    logger.track(track.id, buffer);
                }
                let strip = &state.tracks[index];
                let steady =
                    strip.gain.settled(base) && strip.pan.settled(base) && strip.law.settled(base);
                let (mut left, mut right) =
                    fader_gains(strip.law, base, strip.gain.at(base), strip.pan.at(base));
                let (mut peak_left, mut peak_right) = (0.0_f32, 0.0_f32);
                for (offset, frame) in buffer.iter_mut().enumerate() {
                    if !steady {
                        let at = base + offset as u64;
                        (left, right) =
                            fader_gains(strip.law, at, strip.gain.at(at), strip.pan.at(at));
                    }
                    frame[0] *= left;
                    frame[1] *= right;
                    peak_left = peak_left.max(frame[0].abs());
                    peak_right = peak_right.max(frame[1].abs());
                }
                shared.raise_meter(index, peak_left, peak_right);
                for tap in taps.iter_mut().filter(|tap| tap.tap.track == track.id) {
                    tap.copy(
                        base,
                        state.latency,
                        state.behind[index],
                        windfall_project::MixerRecordMode::PostFader,
                        buffer,
                    );
                }

                for edge in &track.edges {
                    if edge.tap.is_some() {
                        // Already sent from its place in the chain.
                        continue;
                    }
                    let gain = &state.edges[edge.slot];
                    let delayed = state.edge_delays[edge.slot].line.as_mut();
                    if edge.sidechain {
                        let source = &self.buffers[index * MAX_BLOCK..][..frames];
                        self.scratch[..frames].copy_from_slice(source);
                        if let Some(line) = delayed {
                            line.process(&mut self.scratch[..frames]);
                        }
                        add_scaled(
                            &self.scratch[..frames],
                            &mut self.keys[edge.target * MAX_BLOCK..][..frames],
                            gain,
                            base,
                        );
                        continue;
                    }
                    let split = state.direct[edge.target].line.is_some();
                    let (source, target) = match (delayed, split) {
                        (None, false) => pair(&mut self.buffers, index, edge.target),
                        (None, true) => (
                            &self.buffers[index * MAX_BLOCK..][..MAX_BLOCK],
                            &mut self.inbox[edge.target * MAX_BLOCK..][..MAX_BLOCK],
                        ),
                        (Some(line), split) => {
                            let source = &self.buffers[index * MAX_BLOCK..][..frames];
                            let scratch = &mut self.scratch[..frames];
                            scratch.copy_from_slice(source);
                            line.process(scratch);
                            let feeds = if split {
                                &mut self.inbox
                            } else {
                                &mut self.buffers
                            };
                            (
                                &*scratch,
                                &mut feeds[edge.target * MAX_BLOCK..][..MAX_BLOCK],
                            )
                        }
                    };
                    add_scaled(&source[..frames], &mut target[..frames], gain, base);
                }
            }
        }
        out.copy_from_slice(&self.track_mut(0)[..frames]);
        if let Some(line) = &mut state.master_output.line {
            line.process(out);
        }
        let printed = &mut self.printed[..frames];
        state
            .printed_activity
            .hear_input(base, printed, state.silence);
        if let Some(line) = &mut state.printed.line {
            line.process(printed);
        }
        for (out, print) in out.iter_mut().zip(&*printed) {
            out[0] += print[0];
            out[1] += print[1];
        }
        for (out, apart) in out.iter_mut().zip(&self.apart[..frames]) {
            out[0] += apart[0];
            out[1] += apart[1];
        }
        let peaks = out.iter().fold([0.0_f32; 2], |peak, frame| {
            [peak[0].max(frame[0].abs()), peak[1].max(frame[1].abs())]
        });
        shared.raise_meter(0, peaks[0], peaks[1]);
        // Include deterministic routing/master reduction in the stage budget.
        // DSP lanes already identify their own late nodes; if only reduction
        // ran late, identify the master completion without duplicating the xrun.
        if !shared
            .processing_xrun_recorded
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            state
                .workers
                .record_node(u64::from(plan.tracks[0].id.0), shared);
        }
    }

    fn render_stage(
        &mut self,
        plan: &Plan,
        state: &mut PlanState,
        shared: &Shared,
        wave: std::ops::Range<usize>,
        base: u64,
        frames: usize,
    ) {
        for tracks in plan.order[wave].chunks(crate::parallel::LANES) {
            let mut contexts: [Option<RenderTrack>; crate::parallel::LANES] =
                std::array::from_fn(|_| None);
            let mut tasks = [crate::parallel::Task::EMPTY; crate::parallel::LANES];
            for (slot, &index) in tracks.iter().enumerate() {
                let buffer = &mut self.buffers[index * MAX_BLOCK..][..frames];
                if let Some(line) = &mut state.direct[index].line {
                    line.process(buffer);
                    for (frame, fed) in buffer
                        .iter_mut()
                        .zip(&self.inbox[index * MAX_BLOCK..][..frames])
                    {
                        frame[0] += fed[0];
                        frame[1] += fed[1];
                    }
                }
                state.activity[index].hear_input(base, buffer, state.silence);
                contexts[slot] = Some(RenderTrack {
                    buffer: buffer.as_mut_ptr(),
                    keys: self.keys[index * MAX_BLOCK..].as_ptr(),
                    chain: &mut state.chains[index],
                    key_delays: &mut state.key_delays[index],
                    processor: &mut state.track_processing[index],
                    scratch: &mut state.parallel_scratch[index],
                    activity: &mut state.activity[index],
                    base,
                    frames,
                    silence: state.silence,
                });
                // SAFETY: track indices are unique in a wave. Every pointer names
                // disjoint prepared storage, retained until run's completion barrier.
                tasks[slot] = unsafe {
                    crate::parallel::Task::new(
                        contexts[slot].as_mut().unwrap() as *mut RenderTrack as *mut (),
                        render_track,
                        u64::from(plan.tracks[index].id.0),
                    )
                };
            }
            // SAFETY: contexts, Mixer buffers and DSP seats remain stationary and
            // untouched until every worker has returned, even after the deadline.
            unsafe {
                state.workers.run(&tasks[..tracks.len()], shared);
            }
        }
    }

    /// Called after mixing, before metronome/device output gain. Master includes prints.
    pub fn publish_waveforms(
        &mut self,
        plan: &Plan,
        shared: &Shared,
        base: u64,
        rate: u32,
        out: &[Frame],
    ) {
        let epoch = shared
            .waveform_epoch
            .load(std::sync::atomic::Ordering::Acquire);
        for (index, track) in plan.tracks.iter().enumerate() {
            let frames = if index == 0 {
                out
            } else {
                &self.buffers[index * MAX_BLOCK..][..out.len()]
            };
            self.waveforms[index].capture(
                &shared.waveforms[index],
                track.id,
                epoch,
                base,
                rate,
                frames,
            );
        }
    }

    /// Adds what each instrument rendered for this block to the track it
    /// plays into, through its channel's gain and pan and its
    /// compensation delay.
    fn add_instruments(&mut self, plan: &Plan, state: &mut PlanState, base: u64, frames: usize) {
        for (index, channel) in plan.channels.iter().enumerate() {
            let Some(seat) = &mut state.instruments[index] else {
                continue;
            };
            let Some(unit) = &seat.unit else {
                continue;
            };
            state
                .controls
                .dashboard(plan, index, unit.control_params(), base);
            let (left, right) = unit.output(frames);
            let scratch = &mut self.scratch[..frames];
            apply_strip(left, right, &state.channels[index], base, scratch);
            if let Some(line) = &mut seat.delay.line {
                line.process(scratch);
            }
            let feeds = if state.direct[channel.track].line.is_some() {
                &mut self.inbox
            } else {
                &mut self.buffers
            };
            let target = &mut feeds[channel.track * MAX_BLOCK..][..frames];
            for (target, frame) in target.iter_mut().zip(&*scratch) {
                target[0] += frame[0];
                target[1] += frame[1];
            }
        }
    }
}

/// The buffers of two different tracks, the first to read and the second to
/// write.
fn pair(buffers: &mut [Frame], source: usize, target: usize) -> (&[Frame], &mut [Frame]) {
    let (low, high) = buffers.split_at_mut(source.max(target) * MAX_BLOCK);
    let low = &mut low[source.min(target) * MAX_BLOCK..][..MAX_BLOCK];
    let high = &mut high[..MAX_BLOCK];
    if source < target {
        (&*low, high)
    } else {
        (&*high, low)
    }
}

#[inline]
fn fader_gains(law: windfall_core::PanLawRamp, frame: u64, gain: f32, pan: f32) -> (f32, f32) {
    let (left, right) = law.gains_at(frame, pan);
    (gain * left, gain * right)
}

/// Writes an instrument's output to `out` at its channel's gain and pan.
/// The block starts on frame `base`.
fn apply_strip(left: &[f32], right: &[f32], strip: &Strip, base: u64, out: &mut [Frame]) {
    let steady = strip.gain.settled(base) && strip.pan.settled(base) && strip.law.settled(base);
    let mut gains = fader_gains(strip.law, base, strip.gain.at(base), strip.pan.at(base));
    let sides = left.iter().zip(right);
    for (offset, (frame, (left, right))) in out.iter_mut().zip(sides).enumerate() {
        if !steady {
            let at = base + offset as u64;
            gains = fader_gains(strip.law, at, strip.gain.at(at), strip.pan.at(at));
        }
        *frame = [left * gains.0, right * gains.1];
    }
}

/// Adds `source` times the ramp's gain to `target`.
fn add_scaled(source: &[Frame], target: &mut [Frame], gain: &Ramp, base: u64) {
    let frames = source.iter().zip(target);
    if gain.settled(base) {
        let gain = gain.at(base);
        for (source, target) in frames {
            target[0] += source[0] * gain;
            target[1] += source[1] * gain;
        }
    } else {
        for (offset, (source, target)) in frames.enumerate() {
            let gain = gain.at(base + offset as u64);
            target[0] += source[0] * gain;
            target[1] += source[1] * gain;
        }
    }
}
