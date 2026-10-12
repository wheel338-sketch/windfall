//! Audio and automation pads reuse the normal control-side clip preparation.
use crate::{
    SamplePool,
    automation::Lane,
    plan::{Plan, PlanAudioClip},
};
use windfall_project::{PatternId, Project};
#[derive(Debug, Clone)]
pub(crate) struct PreparedClip {
    pub id: PatternId,
    audio: Option<PlanAudioClip>,
    lanes: std::sync::Arc<[Lane]>,
}
pub(crate) fn compile(project: &Project, pool: &SamplePool, plan: &Plan) -> Vec<PreparedClip> {
    let mut clips = Vec::new();
    for pad in &project.settings.stage_media.pads {
        let Some(source) = pad
            .clip
            .and_then(|id| project.playlist.clips.iter().find(|clip| clip.id == id))
        else {
            continue;
        };
        let id = PatternId(source.id.0);
        if clips.iter().any(|clip: &PreparedClip| clip.id == id) {
            continue;
        }
        let mut playlist = project.playlist.clone();
        let mut source = source.clone();
        source.start = 0;
        source.muted = false;
        for track in &mut playlist.tracks {
            track.muted = false;
            track.solo = false;
        }
        playlist.clips = vec![source];
        clips.push(PreparedClip {
            id,
            audio: crate::plan::compile_audio_clips(
                &playlist,
                pool,
                &plan.track_ids,
                plan.tempo_bpm,
            )
            .into_iter()
            .next(),
            lanes: crate::automation::compile(project, &playlist, plan)
                .into_iter()
                .filter(|lane| lane.value_at(0.0).is_some())
                .collect::<Vec<_>>()
                .into(),
        });
    }
    clips
}
impl PreparedClip {
    pub fn has_tempo(&self) -> bool {
        self.lanes.iter().any(|lane| lane.is_tempo())
    }
    pub fn has_audio(&self) -> bool {
        self.audio.is_some()
    }
    pub fn has_automation(&self) -> bool {
        !self.lanes.is_empty()
    }
    pub fn tempo(&self, phase: f64) -> Option<f64> {
        self.lanes
            .iter()
            .find(|lane| lane.is_tempo())
            .and_then(|lane| lane.real_in(crate::automation::Place::Song(phase)))
            .map(f64::from)
    }
    pub fn automate(
        &self,
        plan: &Plan,
        state: &mut crate::state::PlanState,
        phase: Option<f64>,
        now: u64,
        frames: u64,
    ) {
        for lane in self.lanes.iter() {
            lane.performance_value(plan, state, phase, now, frames);
        }
    }
    pub fn render(
        &self,
        plan: &Plan,
        mixer: &mut crate::mixer::Mixer,
        position: PadPosition,
        frame: usize,
        rate: u32,
    ) {
        let Some(clip) = &self.audio else {
            return;
        };
        let PadPosition {
            phase,
            origin,
            seconds,
            level,
            ..
        } = position;
        let position = clip.skip + seconds * clip.speed * f64::from(clip.sample.sample_rate());
        if position >= clip.sample.frames() as f64 {
            return;
        }
        let region = crate::voice::Region {
            first: 0,
            frames: clip.sample.frames(),
            reverse: clip.reverse,
        };
        let (a, b) = region.read(&clip.sample, position);
        let mut gain = clip.gain * level;
        let length = f64::from(clip.end);
        if clip.fade_in > 0 {
            gain *= crate::clips::equal_power(phase / f64::from(clip.fade_in));
        }
        if clip.fade_out > 0 {
            gain *= crate::clips::equal_power((length - phase) / f64::from(clip.fade_out));
        }
        // Same cut-edge fade as ordinary timeline clips. Source/frame position
        // is derived from the musical clock, independent of process partitions.
        let left = ((clip.sample.frames() as f64 - position)
            / (clip.speed * f64::from(clip.sample.sample_rate())))
        .min(
            (plan.warp(origin + length) - plan.warp(origin + phase)) * 60.0
                / (plan.tempo_bpm * f64::from(windfall_core::PPQ)),
        );
        let fade = (0.003 * f64::from(rate)).round().max(1.0);
        gain *= (left * f64::from(rate) / fade).clamp(0.0, 1.0) as f32;
        if clip.skip > 0.0 || clip.reverse {
            gain *= (seconds * f64::from(rate) / fade).clamp(0.0, 1.0) as f32;
        }
        let (l, r) = plan.pan_law.gains(clip.pan);
        let out = if clip.direct_output {
            &mut mixer.printed_mut()[frame]
        } else {
            &mut mixer.track_mut(clip.track)[frame]
        };
        out[0] += a * gain * l;
        out[1] += b * gain * r;
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PadPosition {
    id: PatternId,
    origin: f64,
    phase: f64,
    seconds: f64,
    level: f32,
}
#[derive(Clone, Copy, Default)]
pub(crate) struct AudioPad {
    current: Option<PadPosition>,
    fading: Option<PadPosition>,
    release: u32,
}
impl AudioPad {
    pub fn idle(&self) -> bool {
        self.current.is_none() && self.fading.is_none()
    }
    pub fn frame(
        &mut self,
        plan: &Plan,
        mixer: &mut crate::mixer::Mixer,
        active: Option<(PatternId, f64, f64)>,
        frame: usize,
        rate: u32,
    ) {
        let active = active.filter(|(id, _, _)| {
            plan.performance_clip_ids
                .get(id.0)
                .is_some_and(|index| plan.performance_clips[index].has_audio())
        });
        let same = match (self.current, active) {
            (None, None) => true,
            (Some(old), Some((id, _, origin))) => old.id == id && old.origin == origin,
            _ => false,
        };
        if !same {
            self.fading = self.current.take();
            self.release = (0.003 * f64::from(rate)).round().max(1.0) as u32;
            self.current = active.map(|(id, phase, origin)| PadPosition {
                id,
                phase,
                origin,
                seconds: 0.0,
                level: 1.0,
            });
        }
        if let Some(current) = &mut self.current {
            if let Some((_, phase, _)) = active {
                current.phase = phase;
            }
            if let Some(index) = plan.performance_clip_ids.get(current.id.0) {
                plan.performance_clips[index].render(plan, mixer, *current, frame, rate);
            }
            current.seconds += 1.0 / f64::from(rate);
        }
        if let Some(fading) = &mut self.fading {
            fading.level = self.release as f32 / (0.003 * rate as f32).round().max(1.0);
            if let Some(index) = plan.performance_clip_ids.get(fading.id.0) {
                plan.performance_clips[index].render(plan, mixer, *fading, frame, rate);
            }
            fading.seconds += 1.0 / f64::from(rate);
            self.release = self.release.saturating_sub(1);
            if self.release == 0 {
                self.fading = None;
            }
        }
    }
}
