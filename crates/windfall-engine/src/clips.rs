//! Audio clips: playlist clips that play a sample straight onto the
//! timeline, in slots allocated once.
//!
//! A clip starts on the frame of its start tick and then runs at its own
//! speed, whatever the tempo does. It plays into its mixer track where the
//! sampler voices do, so it goes through the same compensation delay, the
//! track's effects, its fader and its sends.
//!
//! # Level
//!
//! What a clip puts out is its audio times its gain and pan, its two
//! fades, and a short fade of the engine's own wherever its audio is cut.
//! The fades of the clip are equal-power curves between two ticks, shaped
//! over the time between them, so they follow the tempo like the clip's
//! edges do, a tempo that automation moves included. The engine's own is
//! a straight line of [`DECLICK_SECONDS`]: down into the frame the clip
//! ends on, whether that is the end of the clip, the end of the audio, or
//! the transport stopping or moving away, and up from the frame it starts
//! sounding on when that is not the first frame of its audio.
//!
//! A clip that starts on the first frame of its audio plays it as it is,
//! the way a sampler plays a one-shot: a drum loop that begins on a hit
//! keeps the hit. A start anywhere else is a cut and is faded: a clip
//! with an offset, a reversed clip, which starts on the file's last
//! frame, and playback that begins or is moved inside a clip.
//!
//! # Limits
//!
//! [`MAX_AUDIO_CLIPS`] clips sound at once. One more is not started, and
//! stays silent for its whole length. Clips get their slots as they
//! start, and clips that start together in the order of their ids, so the
//! ones left out are the ones that start last and, of those that start on
//! one tick, the ones with the highest ids. Playback that begins in the
//! middle of a song hands the slots to the clips under the playhead in
//! that same order. Clips that are fading out after a stop or a seek do
//! not count: they finish in spare slots.
//!
//! A clip that is left out is not forgotten: the player counts the clips
//! that would be sounding now had there been room, for the UI to warn
//! with, and every start it has refused, for an export to report.

use rtrb::Producer;
use windfall_core::{AudioBuffer, PanLaw};
use windfall_project::{ClipId, TrackId};

use crate::message::{Garbage, retire};
use crate::mixer::Mixer;
use crate::plan::{Plan, PlanAudioClip, same_audio};
use crate::ramp::Ramp;
use crate::sequencer::Clock;
use crate::state::Heard;
use crate::voice::{Region, Stretch};

/// Most audio clips that sound at once.
pub const MAX_AUDIO_CLIPS: usize = 128;

/// Extra slots for clips that are fading out, so that stopping and starting
/// again at once finds room for what starts.
const FADING_SLOTS: usize = MAX_AUDIO_CLIPS;

/// Most clips left out for lack of a slot that are kept track of at once.
/// A song that leaves out more reports this many.
const MAX_MISSED: usize = 1024;

/// Length of the fade that keeps a clip from clicking where it starts and
/// stops.
const DECLICK_SECONDS: f64 = 0.003;

/// A clip that found no slot, for as long as it would have sounded.
#[derive(Clone, Copy)]
struct Missed {
    /// Clock tick of the clip's end.
    ends: f64,
    /// The frame on which its audio would have run out.
    runs_out: u64,
}

/// One sounding clip.
struct Slot {
    active: bool,
    /// Held for as long as the slot may read it. A stopped slot can still
    /// hold its audio for a moment when there was no room to hand it back.
    sample: Option<AudioBuffer>,
    clip: ClipId,
    /// What the clip was when it started. A clip that differs in any of
    /// these is not the same sound any more.
    start: u32,
    offset: u32,
    pitch: f32,
    /// The mixer track the clip plays into: its index in the current plan
    /// and its id.
    track: usize,
    track_id: TrackId,
    direct_output: bool,
    region: Region,
    /// Frames into the region, in playing order.
    position: f64,
    /// Sample frames to advance per output frame.
    step: f64,
    gain: Ramp,
    pan: Ramp,
    /// The panning law of the plan the slot plays under.
    law: windfall_core::PanLawRamp,
    /// Frame the slot started sounding on.
    began: u64,
    /// The slot started in the middle of its audio, which is a cut, and
    /// fades in over the engine's own fade.
    fades_in: bool,
    /// Clock tick of the start of the pass of the song the clip is part
    /// of. The four ticks below are counted from it.
    origin: f64,
    /// Clock ticks of the clip's start and end, of the end of its fade in
    /// and of the start of its fade out.
    starts: f64,
    ends: f64,
    faded_in: f64,
    fades_out: f64,
    /// Frames left of the fade that ends the clip early. Zero while it is
    /// not being ended.
    release: u32,
}

impl Slot {
    fn idle() -> Self {
        Self {
            active: false,
            sample: None,
            clip: ClipId(0),
            start: 0,
            offset: 0,
            pitch: 0.0,
            track: 0,
            track_id: TrackId::MASTER,
            direct_output: false,
            region: Region {
                first: 0,
                frames: 0,
                reverse: false,
            },
            position: 0.0,
            step: 1.0,
            gain: Ramp::at_rest(0.0),
            pan: Ramp::at_rest(0.0),
            law: windfall_core::PanLawRamp::at_rest(PanLaw::Balance),
            began: 0,
            fades_in: false,
            origin: 0.0,
            starts: 0.0,
            ends: 0.0,
            faded_in: 0.0,
            fades_out: 0.0,
            release: 0,
        }
    }

    /// Whether the slot plays `clip` and can go on playing it as it is.
    fn plays(&self, clip: &PlanAudioClip) -> bool {
        self.active
            && self.release == 0
            && self.clip == clip.id
            && self
                .sample
                .as_ref()
                .is_some_and(|sample| same_audio(sample, &clip.sample))
            && self.start == clip.start
            && self.offset == clip.offset
            && self.region.reverse == clip.reverse
            && self.pitch == clip.pitch
            && self.track_id == clip.track_id
            && self.direct_output == clip.direct_output
    }

    /// Takes the ticks of the clip's edges and fades from the plan, as its
    /// clock counts them.
    fn place(&mut self, clip: &PlanAudioClip, plan: &Plan) {
        let (start, end) = (f64::from(clip.start), f64::from(clip.end));
        let at = |tick: f64| self.origin + plan.warp(tick);
        self.starts = at(start);
        self.ends = at(end);
        self.faded_in = at((start + f64::from(clip.fade_in)).min(end));
        self.fades_out = at((end - f64::from(clip.fade_out)).max(start));
    }

    /// Adds the slot's next frames to `out`, a stretch of its mixer
    /// track's buffer that begins on frame `first`. Returns true when the
    /// clip has ended.
    ///
    /// Every piece of state moves one frame at a time and every level is
    /// worked out from the frame's own number, so cutting the output at
    /// different places gives the same samples.
    fn render(&mut self, out: &mut [[f32; 2]], first: u64, clock: Clock, declick: u32) -> bool {
        let Some(sample) = &self.sample else {
            return true;
        };
        let end_frame = clock.frame_of(self.ends);
        let faded_in_frame = clock.frame_of(self.faded_in);
        let fades_out_frame = clock.frame_of(self.fades_out);
        let fade_in_ticks = self.faded_in - self.starts;
        let fade_out_ticks = self.ends - self.fades_out;
        let steady = self.gain.settled(first) && self.pan.settled(first) && self.law.settled(first);
        let (mut left, mut right) = sides(self.law, first, self.gain.at(first), self.pan.at(first));
        let declick_frames = declick as f32;
        let frames = self.region.frames as f64;

        for (offset, out) in out.iter_mut().enumerate() {
            let frame = first + offset as u64;
            if self.release == 0 && frame >= end_frame {
                return true;
            }
            if !steady {
                (left, right) = sides(self.law, frame, self.gain.at(frame), self.pan.at(frame));
            }

            let mut level = 1.0_f32;
            let since = frame - self.began;
            if self.fades_in && since < u64::from(declick) {
                level *= (since + 1) as f32 / declick_frames;
            }
            if self.release > 0 {
                level *= self.release as f32 / declick_frames;
            } else {
                // Frames left before the clip's end, and before the audio
                // runs out, this one included.
                let to_end = end_frame - frame;
                let to_last = ((frames - self.position) / self.step).ceil();
                let left_over = (to_end as f64).min(to_last);
                if left_over < f64::from(declick) {
                    level *= left_over as f32 / declick_frames;
                }
            }
            if frame < faded_in_frame || frame >= fades_out_frame {
                let tick = clock.tick_at(frame);
                if frame < faded_in_frame && fade_in_ticks > 0.0 {
                    level *= equal_power((tick - self.starts) / fade_in_ticks);
                }
                if frame >= fades_out_frame && fade_out_ticks > 0.0 {
                    level *= equal_power((self.ends - tick) / fade_out_ticks);
                }
            }

            let (sample_left, sample_right) = self.region.read(sample, self.position);
            out[0] += sample_left * level * left;
            out[1] += sample_right * level * right;

            self.position += self.step;
            if self.release > 0 {
                self.release -= 1;
                if self.release == 0 {
                    return true;
                }
            }
            if self.position >= frames {
                return true;
            }
        }
        false
    }
}

#[inline]
fn sides(law: windfall_core::PanLawRamp, frame: u64, gain: f32, pan: f32) -> (f32, f32) {
    let (left, right) = law.gains_at(frame, pan);
    (gain * left, gain * right)
}

/// The level of an equal-power fade `part` of the way from silence to full
/// level: a quarter of a sine wave.
#[inline]
pub(crate) fn equal_power(part: f64) -> f32 {
    (part.clamp(0.0, 1.0) * std::f64::consts::FRAC_PI_2).sin() as f32
}

pub(crate) struct ClipPlayer {
    slots: Box<[Slot]>,
    /// The clips that would be sounding had there been a slot for them.
    /// Only the first `missing` entries count.
    missed: Box<[Missed]>,
    missing: usize,
    /// Starts refused for lack of a slot since the player was made.
    refused: u32,
    sample_rate: f64,
    /// Length of the engine's own fade in frames.
    declick: u32,
}

impl ClipPlayer {
    pub fn new(sample_rate: u32) -> Self {
        let sample_rate = f64::from(sample_rate);
        Self {
            slots: (0..MAX_AUDIO_CLIPS + FADING_SLOTS)
                .map(|_| Slot::idle())
                .collect(),
            missed: vec![
                Missed {
                    ends: 0.0,
                    runs_out: 0,
                };
                MAX_MISSED
            ]
            .into_boxed_slice(),
            missing: 0,
            refused: 0,
            sample_rate,
            declick: ((DECLICK_SECONDS * sample_rate).round() as u32).max(1),
        }
    }

    /// Clips sounding right now, ones that are fading out included.
    pub fn active(&self) -> usize {
        self.slots.iter().filter(|slot| slot.active).count()
    }

    /// Clips playing right now. The ones that are fading out after a stop,
    /// a seek or an edit are not counted: at most [`MAX_AUDIO_CLIPS`].
    pub fn playing(&self) -> usize {
        let playing = |slot: &&Slot| slot.active && slot.release == 0;
        self.slots.iter().filter(playing).count()
    }

    /// Clips that would be sounding on frame `now` and are not, because
    /// every slot was taken when they were to start. `clock` says on which
    /// frame each of them ends.
    pub fn missing(&mut self, clock: Clock, now: u64) -> usize {
        let mut index = 0;
        while index < self.missing {
            let missed = self.missed[index];
            if missed.runs_out <= now || clock.frame_of(missed.ends) <= now {
                self.missing -= 1;
                self.missed[index] = self.missed[self.missing];
            } else {
                index += 1;
            }
        }
        self.missing
    }

    /// Starts refused for lack of a slot since the player was made. In a
    /// render, which plays the song once from the top, that is the number
    /// of clips left out of it.
    pub fn refused(&self) -> u32 {
        self.refused
    }

    /// The mixer track each sounding clip plays into, in the plan the
    /// slots are bound to.
    pub fn destinations(&self) -> impl Iterator<Item = Heard> {
        let sounding = self.slots.iter().filter(|slot| slot.active);
        sounding.map(|slot| {
            if slot.direct_output {
                Heard::Printed
            } else {
                Heard::Track(slot.track)
            }
        })
    }

    /// Hands back audio that stopped slots could not return earlier.
    pub fn sweep(&mut self, plan: &Plan, garbage: &mut Producer<Garbage>) {
        for index in 0..self.slots.len() {
            if !self.slots[index].active && self.slots[index].sample.is_some() {
                self.end(index, plan, garbage);
            }
        }
    }

    /// Starts the audio clip at `index` of the plan on frame `now`, `into`
    /// output frames into what it plays. `origin` is the clock tick of the
    /// start of the pass of the song it is part of.
    ///
    /// Nothing starts when the clip has no audio left at that point, or
    /// when [`MAX_AUDIO_CLIPS`] clips are playing already. A clip left out
    /// for that is counted.
    pub fn start(
        &mut self,
        plan: &Plan,
        garbage: &mut Producer<Garbage>,
        index: usize,
        origin: f64,
        into: f64,
        now: u64,
    ) {
        let clip = &plan.audio_clips[index];
        let step = clip.speed * f64::from(clip.sample.sample_rate()) / self.sample_rate;
        let position = clip.skip + into.max(0.0) * step;
        let frames = clip.sample.frames();
        if position >= frames as f64 {
            return;
        }
        let room = self.playing() < MAX_AUDIO_CLIPS;
        let Some(slot) = room.then(|| self.free_slot(plan, garbage)).flatten() else {
            self.refused = self.refused.saturating_add(1);
            if self.missing < self.missed.len() {
                let left = ((frames as f64 - position) / step).ceil();
                self.missed[self.missing] = Missed {
                    ends: origin + plan.warp(f64::from(clip.end)),
                    runs_out: now.saturating_add(left as u64),
                };
                self.missing += 1;
            }
            return;
        };
        let mut started = Slot {
            active: true,
            sample: Some(clip.sample.clone()),
            clip: clip.id,
            start: clip.start,
            offset: clip.offset,
            pitch: clip.pitch,
            track: clip.track,
            track_id: clip.track_id,
            direct_output: clip.direct_output,
            region: Region {
                first: 0,
                frames,
                reverse: clip.reverse,
            },
            position,
            step,
            gain: Ramp::at_rest(clip.gain),
            pan: Ramp::at_rest(clip.pan),
            law: windfall_core::PanLawRamp::at_rest(plan.pan_law),
            began: now,
            // The first frame of the audio is where the audio itself
            // begins. Any other frame, the last one of a reversed clip
            // included, is somewhere inside a sound.
            fades_in: position > 0.0 || clip.reverse,
            origin,
            ..Slot::idle()
        };
        started.place(clip, plan);
        self.slots[slot] = started;
    }

    /// Starts every clip that tick `tick` of the song falls inside of and
    /// that is not sounding, part way through, on frame `now`: playback
    /// has just begun there, or a clip has just appeared there. A clip
    /// that begins exactly on the tick is left to the sequencer.
    pub fn chase(
        &mut self,
        plan: &Plan,
        garbage: &mut Producer<Garbage>,
        clock: Clock,
        origin: f64,
        tick: f64,
        now: u64,
    ) {
        let first = plan
            .audio_clips
            .partition_point(|clip| f64::from(clip.reach) <= tick);
        for (index, clip) in plan.audio_clips.iter().enumerate().skip(first) {
            let start = f64::from(clip.start);
            if start >= tick {
                break;
            }
            if f64::from(clip.end) <= tick || self.slots.iter().any(|slot| slot.plays(clip)) {
                continue;
            }
            let into = clock.frames_between(plan.warp(start), plan.warp(tick));
            self.start(plan, garbage, index, origin, into, now);
        }
    }

    /// Fades out every clip, as when the transport stops or moves away.
    /// Nothing is missing from a song that is not where it was.
    pub fn release_all(&mut self) {
        for slot in self.slots.iter_mut().filter(|slot| slot.active) {
            if slot.release == 0 {
                slot.release = self.declick;
            }
        }
        self.missing = 0;
    }

    /// Gets the slots ready for a new plan that takes over on frame `now`.
    ///
    /// A clip the new plan still has, playing the same audio from the same
    /// place into the same track, carries on: its level and pan glide to
    /// their new values over `glide` frames, and its edges and fades move
    /// to where the plan now puts them. Any other clip fades out; if the
    /// plan has another form of it under the playhead,
    /// [`ClipPlayer::chase`] starts that one. It finds the clips that are
    /// left out under the new plan as well, so the count of them starts
    /// over here.
    pub fn rebind(&mut self, plan: &Plan, now: u64, glide: u64) {
        self.missing = 0;
        for slot in self.slots.iter_mut().filter(|slot| slot.active) {
            let found = plan.audio_clip_ids.get(slot.clip.0);
            let clip = found.map(|index| &plan.audio_clips[index]);
            match clip.filter(|clip| slot.plays(clip)) {
                Some(clip) => {
                    slot.track = clip.track;
                    slot.gain.retarget(clip.gain, now, glide);
                    slot.pan.retarget(clip.pan, now, glide);
                    slot.law.retarget(plan.pan_law, now, glide);
                    slot.place(clip, plan);
                }
                None => {
                    if slot.release == 0 {
                        slot.release = self.declick;
                    }
                    // What is fading out finishes on its track, or on the
                    // master if the track is gone.
                    slot.track = plan.track_ids.get(slot.track_id.0).unwrap_or(0);
                }
            }
        }
    }

    /// Adds every sounding clip's output to its mixer track over
    /// `stretch`. `clock` says on which frame each clip's ticks fall.
    pub fn render(
        &mut self,
        plan: &Plan,
        clock: Clock,
        garbage: &mut Producer<Garbage>,
        mixer: &mut Mixer,
        stretch: Stretch,
    ) {
        let Stretch { base, frames } = stretch;
        let first = base + frames.start as u64;
        for index in 0..self.slots.len() {
            let slot = &mut self.slots[index];
            if !slot.active {
                continue;
            }
            let buffer = if slot.direct_output {
                mixer.printed_mut()
            } else {
                mixer.track_mut(slot.track)
            };
            let out = &mut buffer[frames.clone()];
            if slot.render(out, first, clock, self.declick) {
                self.end(index, plan, garbage);
            }
        }
    }

    /// A slot a clip can start in. When every slot is busy the spare ones
    /// are full of clips fading out, and the one nearest silence is ended.
    fn free_slot(&mut self, plan: &Plan, garbage: &mut Producer<Garbage>) -> Option<usize> {
        let free = |slot: &Slot| !slot.active && slot.sample.is_none();
        if let Some(index) = self.slots.iter().position(free) {
            return Some(index);
        }
        let index = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.active && slot.release > 0)
            .min_by_key(|(_, slot)| slot.release)
            .map(|(index, _)| index)?;
        self.end(index, plan, garbage);
        free(&self.slots[index]).then_some(index)
    }

    /// Stops a slot and lets go of its audio without freeing memory here.
    fn end(&mut self, index: usize, plan: &Plan, garbage: &mut Producer<Garbage>) {
        let slot = &mut self.slots[index];
        slot.active = false;
        let Some(sample) = slot.sample.take() else {
            return;
        };
        if plan.holds(&sample) {
            // The plan keeps the audio alive, so this drop frees nothing.
            drop(sample);
        } else if garbage.is_full() {
            slot.sample = Some(sample);
        } else {
            retire(garbage, Garbage::Sample(sample));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_equal_power_fade_keeps_the_power_of_two_clips_that_cross() {
        assert_eq!(equal_power(0.0), 0.0);
        assert_eq!(equal_power(1.0), 1.0);
        assert_eq!(equal_power(-3.0), 0.0);
        assert_eq!(equal_power(7.0), 1.0);
        for step in 0..=20 {
            let part = f64::from(step) / 20.0;
            let (up, down) = (equal_power(part), equal_power(1.0 - part));
            assert!((up * up + down * down - 1.0).abs() < 1e-6, "{part}");
        }
        // Half way is 3 dB down, not 6.
        assert!((equal_power(0.5) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    }
}
