//! Offline rendering: the realtime audio path run as fast as it will go.

use windfall_core::{AudioBuffer, db_to_gain, samples_per_tick};
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{PatternId, Project};

use crate::controller::Controller;
use crate::plan::compile_render as compile;
use crate::pool::SamplePool;

/// What [`render`] produces.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    /// Linear, once-only song interval. Navigation markers are not rendered.
    pub region: Option<windfall_project::TickRange>,
    pub sample_rate: u32,
    /// Pattern mode renders one pattern `pattern_loops` times. Song mode
    /// renders the playlist from the start to the end of its last clip.
    pub mode: PlayMode,
    /// The pattern to render in pattern mode. `None`, or an id the project
    /// does not have, means the project's first pattern.
    pub pattern: Option<PatternId>,
    pub pattern_loops: u32,
    /// Extra time rendered after the end, so notes that are still sounding
    /// ring out instead of being cut off. With `auto_tail` it is the
    /// longest the tail may get.
    pub tail_secs: f32,
    /// Ends the tail as soon as everything has rung out, instead of always
    /// rendering all of `tail_secs`: once no note, instrument or effect can
    /// sound any more, the audio ends right after its last frame louder
    /// than [`TAIL_SILENCE_DB`].
    pub auto_tail: bool,
    /// Frames processed per step. It sets how often `progress` is called
    /// and has no effect on the audio.
    pub block_frames: usize,
}

/// The level below which an automatic tail counts as over, in decibels
/// relative to full scale.
pub const TAIL_SILENCE_DB: f32 = -90.0;

/// How long the output has to stay that quiet, once nothing can sound any
/// more, before an automatic tail is taken to be over.
pub(crate) const TAIL_HOLD_SECONDS: f64 = 0.1;

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            region: None,
            sample_rate: 48_000,
            mode: PlayMode::Pattern,
            pattern: None,
            pattern_loops: 1,
            tail_secs: 0.0,
            auto_tail: false,
            block_frames: 1024,
        }
    }
}

/// What [`render_reporting`] returns: the audio, and what the listener
/// should be told about it.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub timeline_error: Option<String>,
    pub audio: AudioBuffer,
    /// Audio clips of the playlist that are not in the audio, because
    /// [`MAX_AUDIO_CLIPS`](crate::MAX_AUDIO_CLIPS) were playing already
    /// when each was to start. They are the ones that start last and, of
    /// those that start on one tick, the ones with the highest ids.
    pub dropped_clips: u32,
    pub sampler_error: Option<crate::sampler_processing::SamplerPreparationError>,
    /// Authoritative plugin failure; audio is empty even after a valid prefix.
    pub plugin_error: Option<String>,
}

/// Renders a project to stereo audio. See [`render_reporting`], which this
/// is without the report.
pub fn render(
    project: &Project,
    pool: &SamplePool,
    options: &RenderOptions,
    progress: &mut dyn FnMut(f32) -> bool,
) -> AudioBuffer {
    render_reporting(project, pool, options, progress).audio
}

/// Renders a project to stereo audio with the same
/// [`Processor`](crate::Processor) that plays it live, so the result is what
/// playback sounds like.
///
/// The same input always gives bit-identical output. `progress` is called
/// after every block with the fraction done, 0 to 1; returning `false` from
/// it cancels the render, and the audio rendered so far is returned. With
/// `auto_tail` the fraction is of the longest the render can get, so it
/// may end well short of 1.
///
/// Instruments and effects can put their output out late, a limiter by its
/// look-ahead for one. The engine lines every path up with the slowest and
/// the render leaves that much off its front, so a note on the first tick
/// is heard on the first frame, as it would be with no latency anywhere.
///
/// Automation stays where the song's end left it for the whole of the
/// tail: nobody is there to play on, so nothing returns to its stored
/// value.
pub fn render_reporting(
    project: &Project,
    pool: &SamplePool,
    options: &RenderOptions,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Rendered {
    let sample_rate = options.sample_rate.max(1);
    if let Err(error) = options.check_region() {
        return Rendered {
            audio: AudioBuffer::from_interleaved(sample_rate, 2, Vec::new()),
            dropped_clips: 0,
            sampler_error: None,
            timeline_error: Some(error),
            plugin_error: None,
        };
    }
    let prepared_pool = match pool.prepare_samplers(project, &mut || true, &mut |_, _, _| {}) {
        Ok(pool) => pool,
        Err(error) => {
            return Rendered {
                timeline_error: None,
                audio: AudioBuffer::from_interleaved(sample_rate, 2, Vec::new()),
                dropped_clips: 0,
                sampler_error: Some(error),
                plugin_error: None,
            };
        }
    };
    let plan = compile(project, &prepared_pool);
    let provider = plan.plugin_factory.clone();
    let plugin_error = || provider.as_ref().and_then(|factory| factory.render_error());
    let pattern = options
        .pattern
        .filter(|id| plan.pattern_ids.get(id.0).is_some())
        .or(plan.patterns.first().map(|pattern| pattern.id));
    // The length in ticks of the stored tempo. A song whose tempo is
    // automated takes as long as its tempo map says.
    let (passes, ticks) = match options.mode {
        PlayMode::Pattern => {
            let length = pattern
                .and_then(|id| plan.pattern_ids.get(id.0))
                .map_or(0, |index| plan.patterns[index].length);
            let ticks = u64::from(length) * u64::from(options.pattern_loops);
            (options.pattern_loops, ticks as f64)
        }
        PlayMode::Song => (1, plan.warp(f64::from(plan.song_end))),
    };
    let frames_per_tick = samples_per_tick(plan.tempo_bpm, f64::from(sample_rate));
    let region = options.region.filter(|_| options.mode == PlayMode::Song);
    let body_frames = region.map_or((ticks * frames_per_tick).ceil() as usize, |range| {
        let (first, last) = crate::timeline::region_frames(&plan, range, sample_rate);
        (last - first) as usize
    });
    let tail_frames = if options.tail_secs.is_finite() {
        (f64::from(options.tail_secs.max(0.0)) * f64::from(sample_rate)).round() as usize
    } else {
        0
    };
    // The processor is made after the controller has the plan, so it starts
    // out on the project instead of changing over to it.
    let controller = Controller::new();
    controller.set_plan(plan);
    let mut processor = match controller.try_attach(
        sample_rate,
        crate::project_preparation::AttachmentMode::Independent,
    ) {
        Ok((processor, _, retirement)) => {
            drop(retirement);
            processor
        }
        Err(error) => {
            return Rendered {
                audio: AudioBuffer::from_interleaved(sample_rate, 2, Vec::new()),
                dropped_clips: 0,
                sampler_error: None,
                plugin_error: Some(error.to_string()),
                timeline_error: None,
            };
        }
    };
    controller.set_transport(TransportPatch {
        mode: Some(options.mode),
        pattern,
        loop_song: Some(false),
        ..Default::default()
    });
    if let Some(range) = region {
        controller
            .set_timeline_region(Some(range))
            .expect("region checked before preparation");
        controller.seek(f64::from(range.start));
    }
    if ticks > 0.0 || region.is_some() {
        controller.play_passes(passes);
    }

    // Everything comes out of the processor this many frames late.
    if let Some(error) = plugin_error() {
        return Rendered {
            audio: AudioBuffer::from_interleaved(sample_rate, 2, Vec::new()),
            dropped_clips: 0,
            sampler_error: None,
            plugin_error: Some(error),
            timeline_error: None,
        };
    }
    let latency = controller.latency_frames() as usize;
    let most = latency + body_frames + tail_frames;
    let body_end = latency + body_frames;
    let hold = (TAIL_HOLD_SECONDS * f64::from(sample_rate)).round() as usize;
    let silence = db_to_gain(TAIL_SILENCE_DB);

    // Once nothing can sound any more the output only dies away, so quiet
    // for this long is quiet for good.
    let rung_out = |processor: &crate::Processor, done: usize, loud_end: usize| {
        done >= body_end && done >= loud_end + hold && processor.settled()
    };

    let block = options.block_frames.max(1);
    let certain = if options.auto_tail { body_end } else { most };
    let mut data = Vec::with_capacity(certain * 2);
    let mut done = 0;
    // One past the last frame that was not silent.
    let mut loud_end = 0;
    let mut cancelled = false;
    while done < most {
        let frames = block.min(most - done);
        data.resize((done + frames) * 2, 0.0);
        let fresh = &mut data[done * 2..];
        processor.process(fresh);
        if plugin_error().is_some() {
            break;
        }
        let (fresh, _) = fresh.as_chunks::<2>();
        let loud = |frame: &[f32; 2]| frame[0].abs() >= silence || frame[1].abs() >= silence;
        if let Some(last) = fresh.iter().rposition(loud) {
            loud_end = done + last + 1;
        }
        done += frames;
        if !progress(done as f32 / most as f32) {
            cancelled = true;
            break;
        }
        if options.auto_tail && rung_out(&processor, done, loud_end) {
            break;
        }
    }
    if options.auto_tail && !cancelled && rung_out(&processor, done, loud_end) {
        // The same length whatever the block size: it only depends on
        // where the sound ends. A tail that was still sounding when it ran
        // out of time is kept whole.
        data.truncate(loud_end.max(body_end) * 2);
    }
    let plugin_error = plugin_error();
    if plugin_error.is_some() {
        data.clear();
    }
    data.drain(..(latency * 2).min(data.len()));
    Rendered {
        timeline_error: None,
        audio: AudioBuffer::from_interleaved(sample_rate, 2, data),
        dropped_clips: processor.clips_left_out(),
        sampler_error: None,
        plugin_error,
    }
}

impl RenderOptions {
    pub fn check_region(&self) -> Result<(), String> {
        if let Some(range) = self.region {
            if self.mode != PlayMode::Song {
                return Err("a timeline export region requires song mode".to_owned());
            }
            range.check()?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum RenderError {
    Timeline(String),
    Sampler(crate::sampler_processing::SamplerPreparationError),
    Plugin(String),
}
impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeline(error) => f.write_str(error),
            Self::Sampler(error) => error.fmt(f),
            Self::Plugin(error) => f.write_str(error),
        }
    }
}
impl std::error::Error for RenderError {}
impl From<crate::sampler_processing::SamplerPreparationError> for RenderError {
    fn from(error: crate::sampler_processing::SamplerPreparationError) -> Self {
        Self::Sampler(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_pattern_renders_silence_of_the_right_length() {
        let project = Project::new("empty");
        let options = RenderOptions {
            pattern_loops: 2,
            tail_secs: 0.5,
            ..RenderOptions::default()
        };
        let mut calls = 0;
        let audio = render(&project, &SamplePool::new(), &options, &mut |fraction| {
            calls += 1;
            assert!(fraction > 0.0 && fraction <= 1.0);
            true
        });
        // Two bars at 120 bpm are four seconds, and the tail adds half of one.
        assert_eq!(audio.frames(), 192_000 + 24_000);
        assert_eq!(audio.channels(), 2);
        assert!(audio.samples().iter().all(|sample| *sample == 0.0));
        assert_eq!(calls, (192_000 + 24_000_usize).div_ceil(1024));
    }

    #[test]
    fn returning_false_from_progress_cancels() {
        let project = Project::new("empty");
        let options = RenderOptions {
            block_frames: 100,
            ..RenderOptions::default()
        };
        let audio = render(&project, &SamplePool::new(), &options, &mut |_| false);
        assert_eq!(audio.frames(), 100);
    }
}

#[cfg(test)]
mod plugin_error_tests {
    use super::*;
    use crate::plugins::{HostedEffect, HostedInstrument, PluginFactory};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use windfall_project::{Command, Document, EffectId, PluginBinding, PluginTarget, TrackId};
    #[derive(Debug)]
    struct Factory {
        error: Arc<AtomicBool>,
        enabled: Arc<AtomicBool>,
        private: bool,
    }
    struct Effect {
        error: Arc<AtomicBool>,
        enabled: Arc<AtomicBool>,
        frames: usize,
    }
    impl HostedEffect for Effect {
        fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
            self.frames += left.len();
            left.fill(0.25);
            right.fill(0.25);
            if self.frames >= 16_001 && self.enabled.load(Ordering::Acquire) {
                self.error.store(true, Ordering::Release);
                left.fill(0.0);
                right.fill(0.0);
            }
        }
        fn set_param(&mut self, _: u32, _: f32) {}
        fn set_tempo(&mut self, _: f32) {}
        fn latency(&self) -> usize {
            0
        }
        fn tail(&self) -> usize {
            0
        }
    }
    impl PluginFactory for Factory {
        fn render_factory(&self) -> Option<Arc<dyn PluginFactory>> {
            (!self.private).then(|| {
                Arc::new(Self {
                    error: Arc::new(AtomicBool::new(false)),
                    enabled: self.enabled.clone(),
                    private: true,
                }) as Arc<dyn PluginFactory>
            })
        }
        fn render_error(&self) -> Option<String> {
            self.error
                .load(Ordering::Acquire)
                .then(|| "final plugin block failed".into())
        }
        fn effect(
            &self,
            _: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedEffect>, String> {
            Ok(Box::new(Effect {
                error: self.error.clone(),
                enabled: self.enabled.clone(),
                frames: 0,
            }))
        }
        fn instrument(
            &self,
            _: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedInstrument>, String> {
            Err("effect only".into())
        }
    }
    fn fixture() -> (Project, SamplePool, Arc<Factory>, RenderOptions) {
        let mut document = Document::new(Project::new("plugin failure"));
        document
            .dispatch(
                Command::AddPluginEffect {
                    track: TrackId::MASTER,
                    plugin: PluginBinding {
                        target: PluginTarget::Effect {
                            effect: EffectId(0),
                        },
                        format: "clap".into(),
                        path: "fault.clap".into(),
                        id: "fault".into(),
                        name: "Fault".into(),
                        state: Vec::new(),
                        parameters: Vec::new(),
                        sidechain_input: None,
                        auxiliary_inputs: Vec::new(),
                    },
                },
                None,
            )
            .unwrap();
        let factory = Arc::new(Factory {
            error: Arc::new(AtomicBool::new(false)),
            enabled: Arc::new(AtomicBool::new(true)),
            private: false,
        });
        let mut pool = SamplePool::new();
        pool.set_plugin_factory(factory.clone());
        (
            document.project().clone(),
            pool,
            factory,
            RenderOptions {
                sample_rate: 8_000,
                block_frames: 64,
                ..Default::default()
            },
        )
    }
    #[test]
    fn failed_final_plugin_block_discards_collected_audio_and_new_render_recovers() {
        let (project, pool, factory, options) = fixture();
        let failed = render_reporting(&project, &pool, &options, &mut |_| true);
        assert!(
            failed
                .plugin_error
                .as_deref()
                .unwrap()
                .contains("final plugin")
        );
        assert_eq!(failed.audio.frames(), 0);
        assert!(failed.sampler_error.is_none());
        assert!(
            factory.render_error().is_none(),
            "live provider is unchanged"
        );
        assert_eq!(render(&project, &pool, &options, &mut |_| true).frames(), 0);
        factory.enabled.store(false, Ordering::Release);
        let healthy = render_reporting(&project, &pool, &options, &mut |_| true);
        assert_eq!(healthy.plugin_error, None);
        // The existing tick-to-frame ceiling produces 16,001 frames at 8 kHz.
        // The fault is on the final one-frame block, after 16,000 valid frames.
        assert_eq!(healthy.audio.frames(), 16_001);
        assert!(healthy.audio.samples().iter().any(|sample| *sample > 0.1));
    }
    #[test]
    fn failed_final_plugin_block_never_reports_complete_stream_or_stems() {
        let (project, pool, _, options) = fixture();
        let mut prefix = Vec::new();
        let result = crate::render_streaming_checked(
            &project,
            &pool,
            &options,
            &mut |block| {
                prefix.extend_from_slice(block);
                true
            },
            &mut |_| true,
        );
        assert!(matches!(result, Err(crate::RenderError::Plugin(_))));
        assert!(!prefix.is_empty() && prefix.iter().any(|sample| *sample > 0.1));
        let convenience =
            crate::render_streaming(&project, &pool, &options, &mut |_| true, &mut |_| true);
        assert!(!convenience.completed && convenience.plugin_error.is_some());
        for mode in [crate::StemMode::TrackOutputs, crate::StemMode::ToMaster] {
            let result = crate::render_stems(
                &project,
                &pool,
                &options,
                &crate::StemOptions {
                    mode,
                    tracks: Some(Vec::new()),
                    include_mix: true,
                    numbered: false,
                },
                &mut |_, _| true,
                &mut |_| true,
            );
            assert!(matches!(result, Err(crate::StemError::Plugin(_))));
        }
    }
    #[test]
    fn plugin_error_wins_over_simultaneous_final_progress_cancellation() {
        let (project, pool, factory, options) = fixture();
        factory.enabled.store(false, Ordering::Release);
        let private = pool.prepare_plugin_render();
        // Exact same private provider, failure raised at the final progress edge.
        let error_provider = private.plugin_factory.as_ref().unwrap().clone();
        let fresh = Arc::new(Factory {
            error: Arc::new(AtomicBool::new(false)),
            enabled: factory.enabled.clone(),
            private: true,
        });
        let mut private = private;
        private.set_plugin_factory(fresh.clone());
        let streamed = crate::render_streaming_checked(
            &project,
            &private,
            &options,
            &mut |_| true,
            &mut |fraction| {
                if fraction >= 1.0 {
                    fresh.error.store(true, Ordering::Release);
                    false
                } else {
                    true
                }
            },
        );
        assert!(matches!(streamed, Err(crate::RenderError::Plugin(_))));
        assert!(error_provider.render_error().is_none());
    }
}
