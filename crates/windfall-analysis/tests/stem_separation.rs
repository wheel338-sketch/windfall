use std::time::Duration;
use windfall_analysis::*;
use windfall_analysis::separation::*;
use windfall_core::AudioBuffer;

fn work() -> Work { Work::new(CancelToken::default(), 100_000_000, Duration::from_secs(60)).unwrap() }
fn model(rate: u32, channels: u16) -> ModelManifest {
    ModelManifest { id: "reference-test-only".into(), version: "1".into(), revision: 1,
        sha256: [1; 32], bytes: 1, max_bytes: 1, sample_rate: rate, input_channels: channels,
        max_input_frames: 1_000_000,
        provenance: ModelProvenance { origin: "authored:test".into(), source_revision: "1".into(), author: "Windfall".into(),
            license_spdx: "CC0-1.0".into(), license_reference: "authored fixture".into(),
            adapter_id: ADAPTER_ID.into(), adapter_version: "1".into(), device: "cpu".into() },
        outputs: ["drums", "bass", "vocals", "other"].map(|role| OutputRole { role: role.into(), channels }).to_vec(),
        onnx: Some(OnnxContract { opset: 17, inputs: vec![TensorSpec { name: "mix".into(),
            shape: vec![TensorDimension::Fixed(1), TensorDimension::Fixed(channels.into()), TensorDimension::Symbol("frames".into())] }],
            outputs: vec![TensorSpec { name: "stems".into(), shape: vec![TensorDimension::Fixed(1), TensorDimension::Fixed(4),
                TensorDimension::Fixed(channels.into()), TensorDimension::Symbol("frames".into())] }],
            sample_rate: rate, segment_frames: 107, overlap_frames: 31, context_frames: 7,
            layout: WaveformLayout::ChannelsFirst, stem_roles: ["drums", "bass", "vocals", "other"].map(String::from).to_vec() }),
    }
}
struct Identity;
impl PreparedSeparation for Identity {
    fn run_chunk(&mut self, input: &[f32], _: &mut Work) -> Result<Vec<Vec<f32>>> {
        Ok(vec![input.to_vec(), vec![0.0; input.len()], vec![0.0; input.len()], vec![0.0; input.len()]])
    }
}
fn signal(frames: usize, rate: u32, channels: u16) -> Vec<f32> {
    (0..frames).flat_map(|i| (0..channels).map(move |c| {
        (i as f32*220.0*std::f32::consts::TAU/rate as f32).sin()*if c == 0 { 0.4 } else { -0.27 }
    })).collect()
}
fn run(input: &[f32], shape: AudioShape, model: &ModelManifest, backend: &mut dyn PreparedSeparation) -> Vec<Vec<f32>> {
    let mut stems = vec![vec![]; 4];
    separate_stream(&mut SliceSource { samples: input, shape }, model, backend, &mut work(), &mut |index, samples, _| {
        stems[index].extend_from_slice(samples); Ok(())
    }).unwrap();
    stems
}
#[test]
fn identity_overlap_add_reconstructs_odd_lengths_at_odd_rates_and_stereo() {
    for rate in [8000, 11025, 44100, 48000, 96000] { for channels in [1, 2] { for frames in [1, 75, 107, 109, 1003, 8191] {
        let input = signal(frames, rate, channels);
        let stems = run(&input, AudioShape { frames: frames as u64, channels, sample_rate: rate }, &model(rate, channels), &mut Identity);
        assert_eq!(stems[0].len(), input.len());
        assert_eq!(stems[0], input);
        assert!(stems[1..].iter().flatten().all(|x| *x == 0.0));
    }}}
}
#[test]
fn identity_tiling_after_rate_conversion_matches_absolute_resampler_without_seams() {
    let input = signal(4003, 44100, 2);
    let shape = AudioShape { frames: 4003, channels: 2, sample_rate: 44100 };
    let frames = rate_frames(shape.frames, 44100, 48000).unwrap();
    let mut reference = vec![0.0; frames as usize*2];
    read_resampled(&mut SliceSource { samples: &input, shape }, 48000, 0, &mut reference, &mut work()).unwrap();
    let output = run(&input, shape, &model(48000, 2), &mut Identity);
    assert_eq!(output[0].len(), reference.len());
    assert!(output[0].iter().zip(reference).all(|(a,b)| (a-b).abs() <= 1e-7));
}
#[test]
fn sinc_roundtrips_keep_exact_frame_count_and_low_frequency_signal() {
    for (from, to) in [(44100, 48000), (48000, 44100), (8000, 48000), (48000, 8000)] {
        let input = signal(8003, from, 2);
        let shape = AudioShape { frames: 8003, channels: 2, sample_rate: from };
        let frames = rate_frames(shape.frames, from, to).unwrap();
        let mut converted = vec![0.0; frames as usize*2];
        read_resampled(&mut SliceSource { samples: &input, shape }, to, 0, &mut converted, &mut work()).unwrap();
        let mut restored = vec![0.0; input.len()];
        read_resampled_to_length(&mut SliceSource { samples: &converted, shape: AudioShape { frames, channels: 2, sample_rate: to } },
            from, 0, &mut restored, shape.frames, &mut work()).unwrap();
        let (power, error) = input[128..input.len()-128].iter().zip(&restored[128..restored.len()-128])
            .fold((0.0f64, 0.0f64), |(p,e), (&x,&y)| (p+f64::from(x).powi(2), e+f64::from(x-y).powi(2)));
        let snr = 10.0*(power/error.max(1e-30)).log10();
        assert!(snr > 35.0, "{from}->{to}->{from}: {snr} dB");
        assert!(restored.iter().all(|x| x.is_finite()));
        assert!(restored[restored.len()-2].abs() > 0.01, "double rounding must not silently zero-pad the tail");
    }
}
#[test]
fn reference_stems_sum_to_mix_with_context_and_permuted_roles() {
    let mut model = model(44100, 2);
    model.onnx.as_mut().unwrap().stem_roles = ["other", "vocals", "bass", "drums"].map(String::from).to_vec();
    let input = signal(9001, 44100, 2);
    let mut prepared = ReferenceBackend.prepare(&model, &[1], &mut work()).unwrap();
    let output = run(&input, AudioShape { frames: 9001, channels: 2, sample_rate: 44100 }, &model, prepared.as_mut());
    for (i, &x) in input.iter().enumerate() {
        let sum: f32 = output.iter().map(|stem| stem[i]).sum();
        assert!((sum-x).abs() < 2e-6, "frame {i}: {sum} vs {x}");
    }
    assert!(output[2].iter().any(|x| x.abs() > 0.1));
    assert!(output[3].iter().any(|x| x.abs() > 0.001));
}
#[test]
fn hostile_waveform_manifests_are_refused_before_runtime_preparation() {
    let good = model(48000, 2);
    good.validate().unwrap();
    for change in 0..12 {
        let mut bad = good.clone();
        let onnx = bad.onnx.as_mut().unwrap();
        match change {
            0 => onnx.context_frames = u32::MAX,
            1 => onnx.segment_frames = 1536001,
            2 => onnx.overlap_frames = onnx.segment_frames-1,
            3 => onnx.inputs[0].shape[0] = TensorDimension::Symbol("batch".into()),
            4 => onnx.outputs[0].shape[1] = TensorDimension::Fixed(5),
            5 => onnx.inputs[0].shape[2] = TensorDimension::Symbol("time".into()),
            6 => onnx.stem_roles[0] = "bass".into(),
            7 => onnx.outputs[0].name = "bad\0name".into(),
            8 => onnx.inputs[0].shape[1] = TensorDimension::Fixed(1),
            9 => onnx.outputs.push(onnx.outputs[0].clone()),
            10 => bad.outputs[0].channels = 1,
            _ => bad.onnx = None,
        }
        assert!(bad.validate().is_err(), "hostile variant {change}");
    }
    let mut last = good;
    let spec = last.onnx.as_mut().unwrap();
    spec.layout = WaveformLayout::ChannelsLast;
    spec.inputs[0].shape.swap(1,2);
    spec.outputs[0].shape.swap(2,3);
    last.validate().unwrap();
}
#[test]
fn cancellation_after_chunk_returns_no_emission_and_file_capture_matches_memory() {
    struct CancelOnRun(CancelToken);
    impl PreparedSeparation for CancelOnRun {
        fn run_chunk(&mut self, input: &[f32], work: &mut Work) -> Result<Vec<Vec<f32>>> {
            let output = Identity.run_chunk(input, work)?;
            self.0.cancel(); Ok(output)
        }
    }
    let input = signal(1003, 48000, 2);
    let shape = AudioShape { frames: 1003, channels: 2, sample_rate: 48000 };
    let cancel = CancelToken::default();
    let mut work = Work::new(cancel.clone(), 100_000, Duration::from_secs(60)).unwrap();
    let mut emitted = 0;
    assert!(matches!(separate_stream(&mut SliceSource { samples: &input, shape }, &model(48000, 2),
        &mut CancelOnRun(cancel), &mut work, &mut |_, _, _| { emitted += 1; Ok(()) }), Err(AnalysisError::Cancelled)));
    assert_eq!(emitted, 0);
    let audio = AudioBuffer::from_interleaved(48000, 2, input.clone());
    let stamp = CaptureStamp { generation: 1, edit_revision: 1, source_key: 1, binding: [1;32], source_identity: audio.identity(),
        source_fingerprint: ContentFingerprint::audio(&audio, &mut self::work()).unwrap(), input_shape: shape, selection: FrameRange { start: 11, end: 997 } };
    let root = tempfile::tempdir().unwrap();
    let captured = CapturedInput::capture_stream(root.path(), stamp, InputLimits::default(), &mut self::work(), &mut |start, out, work| {
        SliceSource { samples: &input, shape }.read_frames(start, out, work)
    }).unwrap();
    assert!(captured.retained_bytes() <= 16384);
    assert_eq!(captured.disk_bytes().unwrap(), input.len() as u64*4);
    let mut source = captured.frame_source().unwrap();
    let mut selected = vec![0.0; (997-11)*2];
    source.read_frames(0, &mut selected, &mut self::work()).unwrap();
    assert_eq!(selected, input[22..997*2]);
    drop(source); drop(captured);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
#[test]
fn missing_runtime_is_a_capability_error_without_reference_fallback() {
    let root = tempfile::tempdir().unwrap();
    let backend = OrtBackend { choice: std::sync::Arc::new(std::sync::RwLock::new(Some(root.path().join("absent-runtime.dll")))),
        search_directories: Default::default() };
    assert!(backend.runtime_path().unwrap_err().to_string().contains("installed shared library"));
}
#[test]
fn long_song_reads_and_emits_bounded_blocks_without_retaining_song_outputs() {
    struct Procedural { shape: AudioShape, max_read: usize }
    impl FrameSource for Procedural {
        fn shape(&self) -> AudioShape { self.shape }
        fn read_frames(&mut self, start: u64, out: &mut [f32], work: &mut Work) -> Result<()> {
            self.max_read = self.max_read.max(out.len());
            work.checkpoint(out.len() as u64)?;
            for (i, x) in out.iter_mut().enumerate() { *x = (((start+i as u64)%257) as f32-128.0)/512.0; }
            Ok(())
        }
    }
    let frames = 8000*181+3; // exceeds the earlier two-minute editor cap
    let mut source = Procedural { shape: AudioShape { frames, channels: 1, sample_rate: 8000 }, max_read: 0 };
    let mut model = model(8000, 1);
    model.max_input_frames = frames;
    let spec = model.onnx.as_mut().unwrap();
    spec.segment_frames = 8192; spec.overlap_frames = 2048; spec.context_frames = 32;
    let mut counts = [0u64;4];
    let mut max_emit = 0;
    separate_stream(&mut source, &model, &mut Identity, &mut work(), &mut |stem, samples, _| {
        max_emit = max_emit.max(samples.len());
        for (i, &x) in samples.iter().enumerate() {
            let expected = if stem == 0 { (((counts[stem]+i as u64)%257) as f32-128.0)/512.0 } else { 0.0 };
            assert_eq!(x, expected);
        }
        counts[stem] += samples.len() as u64;
        Ok(())
    }).unwrap();
    assert_eq!(counts, [frames;4]);
    assert!(source.max_read <= 2082, "resampler read exceeded 2048-frame block + taps");
    assert!(max_emit <= 6144, "OLA emission exceeded configured hop");
}
