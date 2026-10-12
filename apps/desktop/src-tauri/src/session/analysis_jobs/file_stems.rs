//! Separation captures only a bounded render block, with selected PCM on disk.
use super::*;
impl Session {
    pub(super) fn submit_file_stems(&self, request: AnalysisSubmit, model: native::ModelManifest,
        selection: native::FrameRange, manager: Arc<native::JobManager>, work: &mut native::Work) -> Result<AnalysisJob, String> {
        let service = &self.inner.analysis;
        service.runtime.runtime_path().map_err(|e| error("unavailable", e))?;
        let (clip, sample, source, generation, edits, replacements, project_path, directory, project) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            let project = state.document.project();
            let clip = project.playlist.clips.iter().find(|c| c.id == request.clip).cloned().ok_or_else(|| error("stale", "Clip disappeared."))?;
            let id = clip.content.sample().ok_or_else(|| error("invalid", "Select an audio clip."))?;
            let sample = project.sample(id).cloned().ok_or_else(|| error("stale", "Sample disappeared."))?;
            let source = state.pool.get(id).cloned().ok_or_else(|| error("invalid", "Reload source audio."))?;
            (clip, sample, source, state.generation, state.edits, state.replacements, state.path.clone(), state.sample_dir.clone(), project.clone())
        };
        if number(&request.model_revision)? != model.revision { return Err(error("stale", "Model revision changed.")); }
        // The app's ordinary playback pool already owns source PCM. Verification
        // and separation add no second whole-song decode or rendered allocation.
        if native::AudioShape::of(&source).pcm_bytes().map_err(native_error)? > SONG_PCM_LIMIT {
            return Err(error("budget", "Song exceeds the 1.5 GiB playback asset limit."));
        }
        let path = super::super::samples::locate(&sample, directory.as_deref(), &self.inner.factory_dir)?;
        if path.as_os_str().len() > 1024 || sample.name.len() > 1024 { return Err(error("budget", "Source metadata too large.")); }
        let mut file = source_file(&path)?; // no-write/no-delete handle spans decode/hash/capture
        let fingerprint = native::ContentFingerprint::reader(&mut file, SONG_PCM_LIMIT, work).map_err(native_error)?;
        let mut offset = 0usize;
        let decoded = windfall_codec::decode_file_stream_strict(&path,
            &windfall_codec::DecodeOptions { max_decoded_bytes: SONG_PCM_LIMIT }, &mut |rate, channels, samples| {
                work.checkpoint(samples.len() as u64).map_err(|e| windfall_codec::CodecError::Corrupt(e.to_string()))?;
                let end = offset.checked_add(samples.len()).ok_or_else(|| windfall_codec::CodecError::Corrupt("sample overflow".into()))?;
                if rate != source.sample_rate() || channels != source.channels() || source.samples().get(offset..end)
                    .is_none_or(|loaded| loaded.iter().zip(samples).any(|(a,b)| a.to_bits() != b.to_bits())) {
                    return Err(windfall_codec::CodecError::Corrupt("Source bytes differ from loaded audio; reload samples.".into()));
                }
                offset = end;
                Ok(())
            });
        work.check().map_err(native_error)?;
        decoded.map_err(|e| error("stale", e))?;
        if offset != source.samples().len() { return Err(error("stale", "Source duration changed; reload audio.")); }
        let mut pool = SamplePool::new();
        pool.insert(sample.id, source.clone());
        let tempo = project.settings.tempo_bpm;
        let clock = windfall_engine::SongClock::new(&project)?;
        let timing = windfall_engine::audio_edit::view_timing(&pool, &clip, tempo, &clock)?;
        let frames = (timing.duration_frames-1e-6).ceil();
        if !frames.is_finite() || frames < 1.0 || frames > 3600.0*f64::from(source.sample_rate()) {
            return Err(error("budget", "Separation supports songs up to one hour."));
        }
        let shape = native::AudioShape { frames: frames as u64, channels: model.input_channels, sample_rate: source.sample_rate() };
        model.validate_input(shape, selection.frames().map_err(native_error)?).map_err(native_error)?;
        selection.validate(shape.frames).map_err(native_error)?;
        let encoded = serde_json::to_vec(&(&clip, &sample, tempo, timing, &directory, &project_path)).map_err(|e| error("invalid", e))?;
        let digest = native::ContentFingerprint::reader(std::io::Cursor::new(encoded), 16*1024, work).map_err(native_error)?.sha256;
        let stamp = native::CaptureStamp { generation, edit_revision: edits, source_key: u64::from(sample.id.0),
            binding: digest, source_identity: source.identity(), source_fingerprint: fingerprint, input_shape: shape, selection };
        let reader = windfall_engine::audio_edit::StreamedView::new_with_checkpoint(&pool, &clip, timing,
            &mut |samples| work.checkpoint(samples as u64).map_err(native_error))?;
        let stage = lock(&service.owned).stage.clone().ok_or_else(|| error("unavailable", "Analysis staging unavailable."))?;
        let capture_root = stage.join("Captures");
        std::fs::create_dir_all(&capture_root).map_err(|e| error("io", e))?;
        let limits = native::InputLimits { max_frames: 192_000*3600, ..input_limits() };
        let input = native::CapturedInput::capture_stream(&capture_root, stamp.clone(), limits, work, &mut |start, output, work| {
            work.check()?;
            if model.input_channels == 2 { reader.read(start, output).map_err(native::AnalysisError::Adapter) }
            else {
                let mut stereo = vec![0.0; output.len()*2];
                // capture mono blocks must still respect the stereo reader cap
                for (i, block) in stereo.chunks_mut(4096).enumerate() {
                    reader.read(start+(i*2048) as u64, block).map_err(native::AnalysisError::Adapter)?;
                }
                for (out, pair) in output.iter_mut().zip(stereo.chunks_exact(2)) { *out = (pair[0]+pair[1])*0.5; }
                Ok(())
            }
        }).map_err(native_error)?;
        let binding = Binding { clip, sample, path: path.clone(), project_path, sample_dir: directory,
            replacements, stamp, file_identity: crate::library::file_identity(&path).map_err(|e| error("io", e))?, tempo, clock };
        #[cfg(test)] self.pause("analysis:captured");
        {
            let _recording = self.recording_idle()?;
            if !binding.current(&self.state()) { return Err(error("stale", "Project/source changed during separation capture.")); }
        }
        work.check().map_err(native_error)?;
        let output_pcm = selection.frames().map_err(native_error)?.checked_mul(u64::from(shape.channels)*4*4).ok_or_else(|| error("budget", "Output size overflow."))?;
        let model_pcm = native::separation::rate_frames(selection.frames().map_err(native_error)?, shape.sample_rate, model.sample_rate)
            .map_err(native_error)?.checked_mul(u64::from(shape.channels)*4*4).ok_or_else(|| error("budget", "Model spool size overflow."))?;
        let id = manager.submit(native::JobRequest { amount: 1.0, task: native::InferenceTask::StemSeparation, input, model,
            budget: native::JobBudget { scratch_bytes: native::separation::SCRATCH_BYTES,
                output_bytes: output_pcm.checked_add(model_pcm).and_then(|n| n.checked_add(16*4096)).ok_or_else(|| error("budget", "Output size overflow."))?,
                work_units: 100_000_000_000, timeout: Duration::from_secs(600) } }).map_err(native_error)?;
        lock(&service.owned).bindings.insert(id, binding);
        Ok(job_wire(manager.snapshot(id).map_err(native_error)?))
    }
}
