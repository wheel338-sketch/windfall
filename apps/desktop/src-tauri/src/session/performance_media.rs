use super::Session;
use windfall_engine::performance::LaunchRequest;
use windfall_ipc::PerformanceState;
use windfall_project::LaunchQuantization;

impl Session {
    pub fn performance_state(&self) -> PerformanceState {
        self.controller().performance_state()
    }

    pub fn performance_pad(
        &self,
        track: u8,
        scene: Option<u8>,
        generation: u64,
        revision: u64,
    ) -> Result<(), String> {
        let _recording = self.recording_idle()?;
        let state = self.state();
        if state.generation != generation || state.document.revision() != revision {
            return Err("The project changed before the performance launch".into());
        }
        let media = &state.document.project().settings.stage_media;
        let pattern = match scene {
            Some(scene) => Some(
                media
                    .pads
                    .iter()
                    .find(|pad| pad.track == track && pad.scene == scene)
                    .ok_or("Assign a pattern to this pad first")?
                    .pattern,
            ),
            None => None,
        };
        self.controller()
            .performance_launch(LaunchRequest {
                track,
                pattern,
                quantization: media.quantization,
            })
            .map_err(str::to_owned)
    }

    pub(super) fn performance_controller_pad(
        &self,
        track: u8,
        scene: Option<u8>,
    ) -> Result<(), String> {
        let (generation, revision) = {
            let state = self.state();
            (state.generation, state.document.revision())
        };
        self.performance_pad(track, scene, generation, revision)
    }

    pub fn performance_stop_all(&self) {
        self.controller().performance_stop_all();
    }

    pub fn performance_scene(
        &self,
        scene: u8,
        generation: u64,
        revision: u64,
    ) -> Result<(), String> {
        let _recording = self.recording_idle()?;
        let state = self.state();
        if state.generation != generation || state.document.revision() != revision {
            return Err("The project changed before the scene launch".into());
        }
        let media = &state.document.project().settings.stage_media;
        let mut requests = [None; windfall_engine::performance::TRACKS];
        for pad in media.pads.iter().filter(|pad| pad.scene == scene) {
            requests[usize::from(pad.track)] = Some(LaunchRequest {
                track: pad.track,
                pattern: Some(pad.pattern),
                quantization: media.quantization,
            });
        }
        if requests.iter().all(Option::is_none) {
            return Err("This scene has no assigned pads".into());
        }
        self.controller()
            .performance_scene(requests)
            .map_err(str::to_owned)
    }

    /// Only paths already saved in this exact project can be decoded. Reading
    /// and stale recheck happen off the document lock; no asset-scheme wildcard.
    pub fn media_video_read(&self, path: &str, generation: u64) -> Result<Vec<u8>, String> {
        let allowed = || {
            let state = self.state();
            state.generation == generation
                && state
                    .document
                    .project()
                    .settings
                    .stage_media
                    .videos
                    .iter()
                    .any(|video| video.path == path)
        };
        if !allowed() {
            return Err("The video is not in this project".into());
        }
        let mut file =
            std::fs::File::open(path).map_err(|error| format!("Cannot open video: {error}"))?;
        let max = 64 * 1024 * 1024;
        if file.metadata().map_err(|e| e.to_string())?.len() > max {
            return Err("Video exceeds the 64 MiB decoder budget".into());
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        (&mut file)
            .take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > max {
            return Err("Video grew beyond the decoder budget".into());
        }
        if !allowed() {
            return Err("The project changed while reading video".into());
        }
        Ok(bytes)
    }

    /// Small audiovisual export snapshot. Offline render is the engine's real
    /// mix, including tempo, PDC and independent plugin render ownership.
    pub fn media_render_audio(
        &self,
        range: windfall_project::TickRange,
        generation: u64,
        revision: u64,
    ) -> Result<windfall_ipc::MediaRenderAudio, String> {
        let (project, pool, plugin_revision) = {
            let state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err("The project changed before audiovisual export".into());
            }
            let mut pool = state.pool.clone();
            pool.share_sampler_budget(&state.pool);
            (
                state.document.project().clone(),
                pool,
                self.plugin_revision(),
            )
        };
        let project = self.capture_plugins(project, plugin_revision)?;
        range.check().map_err(|e| e.to_string())?;
        // Worst-case slowest supported tempo bounds allocation before rendering.
        let max_ticks = windfall_core::PPQ as f64 * windfall_project::MIN_TEMPO_BPM / 2.0;
        if f64::from(range.end - range.start) > max_ticks {
            return Err(
                "Choose a shorter export range (at most 30 seconds at the slowest tempo)".into(),
            );
        }
        let rendered = windfall_engine::render_reporting(
            &project,
            &pool,
            &windfall_engine::RenderOptions {
                mode: windfall_ipc::PlayMode::Song,
                region: Some(range),
                sample_rate: 48000,
                tail_secs: 0.0,
                ..Default::default()
            },
            &mut |_| true,
        );
        if let Some(error) = rendered.plugin_error.or(rendered.timeline_error) {
            return Err(error);
        }
        if let Some(error) = rendered.sampler_error {
            return Err(error.to_string());
        }
        if rendered.dropped_clips != 0 {
            return Err("Audio clip capacity was exceeded".into());
        }
        Ok(windfall_ipc::MediaRenderAudio {
            sample_rate: 48000,
            channels: 2,
            samples: rendered.audio.samples().to_vec(),
        })
    }

    /// Explicit immediate stop is also useful to an external controller panic.
    pub fn performance_stop_track(&self, track: u8) -> Result<(), String> {
        self.controller()
            .performance_launch(LaunchRequest {
                track,
                pattern: None,
                quantization: LaunchQuantization::Immediate,
            })
            .map_err(str::to_owned)
    }
}
