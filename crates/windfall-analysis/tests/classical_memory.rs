//! Deterministic aggregate bounds, including measured worker heap peaks.
use std::cell::Cell;
use std::time::Duration;
use windfall_analysis::{AnalysisError, CancelToken, Work, classical, classical_memory};
use windfall_core::AudioBuffer;
use windfall_project::*;

struct TrackingAllocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static BYTES: Cell<(isize, usize)> = const { Cell::new((0, 0)) };
}
fn event(delta: isize) {
    let _ = TRACK.try_with(|track| {
        if track.get() {
            BYTES.with(|state| {
                let (live, peak) = state.get();
                let next = live + delta;
                state.set((next, peak.max(next.max(0) as usize)));
            });
        }
    });
}
#[global_allocator]
static ALLOCATOR: TrackingAllocator = TrackingAllocator;
unsafe impl std::alloc::GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let pointer = unsafe { std::alloc::System.alloc(layout) };
        if !pointer.is_null() {
            event(layout.size() as isize);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
        event(-(layout.size() as isize));
        unsafe {
            std::alloc::System.dealloc(pointer, layout);
        }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        let next = unsafe { std::alloc::System.realloc(pointer, layout, size) };
        if !next.is_null() {
            // Reallocation can transiently hold both the new and old blocks.
            event(size as isize);
            event(-(layout.size() as isize));
        }
        next
    }
}
fn measured(run: impl FnOnce()) -> u64 {
    BYTES.with(|state| state.set((0, 0)));
    TRACK.with(|track| track.set(true));
    run();
    TRACK.with(|track| track.set(false));
    BYTES.with(|state| {
        let (live, peak) = state.get();
        assert_eq!(live, 0, "all measured results retire inside scope");
        peak as u64
    })
}
fn work(limit: u64, retained: u64) -> Work {
    Work::new(
        CancelToken::default(),
        10_000_000_000_000,
        Duration::from_secs(600),
    )
    .unwrap()
    .with_memory_limit(limit, retained)
    .unwrap()
}
fn source(channels: u16) -> AudioBuffer {
    // Full declared 120 seconds, without silence or arbitrary random fixtures.
    AudioBuffer::from_interleaved(
        8000,
        channels,
        (0..8000 * 120)
            .flat_map(|frame| {
                let sample = (std::f32::consts::TAU * 220.0 * frame as f32 / 8000.0).sin() * 0.2;
                (0..channels).map(move |channel| if channel == 0 { sample } else { sample * 0.5 })
            })
            .collect(),
    )
}
fn recipe(audio: &AudioBuffer, pitch: bool) -> AudioEditRecipe {
    AudioEditRecipe {
        source_clip: Clip {
            id: ClipId(8),
            track: PlaylistTrackId(7),
            start: 0,
            length: 1920,
            offset: 0,
            muted: false,
            content: ClipContent::Audio {
                sample: SampleId(6),
                mixer_track: TrackId::MASTER,
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
        },
        source_timing: None,
        analysis: AudioAnalysisData {
            frames: audio.frames() as u32,
            sample_rate: audio.sample_rate(),
            polyphonic: false,
            notes: if pitch {
                vec![DetectedAudioNote {
                    start_frame: 0,
                    end_frame: audio.frames() as u32,
                    midi_pitch: 57.0,
                    confidence: 1.0,
                    rms: 0.14,
                }]
            } else {
                vec![]
            },
            transients: vec![],
        },
        overrides: if pitch {
            vec![AudioNoteOverride {
                note: 0,
                midi_pitch: 58.0,
                start_frame: 0,
                end_frame: audio.frames() as u32,
            }]
        } else {
            vec![]
        },
        markers: vec![
            AudioWarpMarker {
                source_frame: 0,
                target_frame: 0,
            },
            AudioWarpMarker {
                source_frame: audio.frames() as u32,
                target_frame: audio.frames() as u32 / if pitch { 1 } else { 2 },
            },
        ],
        denoise: None,
    }
}

#[test]
fn long_denoise_accounts_f64_accumulators_and_retained_result_peak() {
    let audio = source(2);
    let retained = 3 * 1024 * 1024; // A previous retained preview/result.
    let required = retained + audio.retained_bytes() + classical_memory::denoising(&audio).unwrap();
    let settings = AudioDenoiseSettings {
        profile_start: 0,
        profile_end: 2048,
        reduction_db: 12.0,
        wet: 0.5,
    };
    let mut refused = work(required - 1, retained);
    assert!(matches!(
        classical::denoise(&audio, settings, &mut refused),
        Err(AnalysisError::Budget("classical aggregate memory"))
    ));
    assert_eq!(refused.peak_memory_bytes(), retained);
    let mut accepted = work(required, retained);
    let peak = measured(|| {
        let result = classical::denoise(&audio, settings, &mut accepted).unwrap();
        assert_eq!(result.frames(), audio.frames());
        assert!(result.samples().iter().all(|sample| sample.is_finite()));
        drop(result);
    });
    assert!(peak + retained + audio.retained_bytes() <= accepted.peak_memory_bytes());
    assert_eq!(accepted.peak_memory_bytes(), required);
    assert!(required <= classical_memory::CLASSICAL_MEMORY_BYTES);
}

#[test]
fn long_time_and_pitch_render_account_context_planar_output_and_retained_preview() {
    let audio = source(1);
    for (pitch, denoise) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut recipe = recipe(&audio, pitch);
        if denoise {
            recipe.denoise = Some(AudioDenoiseSettings {
                profile_start: 0,
                profile_end: 2048,
                reduction_db: 12.0,
                wet: 0.5,
            });
        }
        let retained = classical_memory::recipe_bytes(&recipe).unwrap() + 4 * 1024 * 1024;
        let required = retained
            + audio.retained_bytes()
            + classical_memory::rendering(&audio, &recipe).unwrap();
        let mut refused = work(required - 1, retained);
        assert!(matches!(
            classical::render(&audio, &recipe, &mut refused),
            Err(AnalysisError::Budget("classical aggregate memory"))
        ));
        let mut accepted = work(required, retained);
        let peak = measured(|| {
            let result = classical::render(&audio, &recipe, &mut accepted).unwrap();
            assert_eq!(result.frames(), recipe.output_frames().unwrap() as usize);
            assert!(result.samples().iter().any(|sample| sample.abs() > 0.01));
            drop(result);
        });
        assert!(
            peak + retained + audio.retained_bytes() <= accepted.peak_memory_bytes(),
            "pitch={pitch}: measured={peak}, charged={required}"
        );
        assert!(accepted.peak_memory_bytes() <= classical_memory::CLASSICAL_MEMORY_BYTES);
    }
}

#[test]
fn maximum_rate_fft_and_stretch_runtime_stay_inside_declared_allowances() {
    let audio = AudioBuffer::from_interleaved(192000, 2, vec![0.1; 192000 * 2]);
    let settings = AudioDenoiseSettings {
        profile_start: 0,
        profile_end: 8192,
        reduction_db: 12.0,
        wet: 0.5,
    };
    let denoise_required = audio.retained_bytes() + classical_memory::denoising(&audio).unwrap();
    let mut budget = work(denoise_required, 0);
    let peak = measured(|| {
        let result = classical::denoise(&audio, settings, &mut budget).unwrap();
        assert_eq!(result.frames(), audio.frames());
        drop(result);
    });
    assert!(peak + audio.retained_bytes() <= budget.peak_memory_bytes());
    let recipe = recipe(&audio, false);
    let render_required =
        audio.retained_bytes() + classical_memory::rendering(&audio, &recipe).unwrap();
    let mut budget = work(render_required, 0);
    let peak = measured(|| {
        let result = classical::render(&audio, &recipe, &mut budget).unwrap();
        assert_eq!(result.frames(), 96000);
        drop(result);
    });
    assert!(peak + audio.retained_bytes() <= budget.peak_memory_bytes());
    assert!(budget.peak_memory_bytes() <= classical_memory::CLASSICAL_MEMORY_BYTES);
}

#[test]
fn long_mono_and_poly_score_detection_account_estimates_and_retained_notes() {
    let audio = source(1);
    let retained = 2 * 1024 * 1024;
    let required = retained + audio.retained_bytes() + classical_memory::detection(&audio).unwrap();
    for polyphonic in [false, true] {
        let mut refused = work(required - 1, retained);
        assert!(matches!(
            classical::detect(&audio, polyphonic, &mut refused),
            Err(AnalysisError::Budget("classical aggregate memory"))
        ));
        let mut accepted = work(required, retained);
        let peak = measured(|| {
            let result = classical::detect(&audio, polyphonic, &mut accepted).unwrap();
            assert!(!result.notes.is_empty());
            assert_eq!(result.frames, audio.frames() as u32);
            drop(result);
        });
        assert!(peak + retained + audio.retained_bytes() <= accepted.peak_memory_bytes());
        assert_eq!(accepted.peak_memory_bytes(), required);
    }
}

#[test]
fn unused_input_and_recipe_capacities_are_charged_and_overflow_refuses() {
    let mut samples = Vec::with_capacity(1024 * 1024);
    samples.resize(8000, 0.0);
    let audio = AudioBuffer::from_interleaved(8000, 1, samples);
    assert!(audio.retained_bytes() >= 4 * 1024 * 1024);
    let mut recipe = recipe(&audio, false);
    recipe.overrides.reserve_exact(1024 * 1024);
    assert!(classical_memory::recipe_bytes(&recipe).unwrap() >= 16 * 1024 * 1024);
    let mut budget = work(u64::MAX, u64::MAX - 4);
    assert!(matches!(
        budget.require_memory(8),
        Err(AnalysisError::Exhausted)
    ));
}

#[test]
fn long_file_capture_and_dereverb_heap_peak_is_independent_of_total_frames() {
    use windfall_analysis::dereverb::{DereverbMode, DereverbSettings, DereverbStream};
    use windfall_analysis::{
        AudioShape, CaptureStamp, CapturedInput, ContentFingerprint, FrameRange, InputLimits,
    };
    let root = tempfile::tempdir().unwrap();
    // Only this identity stays alive. The 3- and 6-minute source streams and
    // their output are never materialized in RAM by the fixture.
    let identity = AudioBuffer::from_interleaved(8000, 2, vec![0.0; 2]);
    let fingerprint = ContentFingerprint::audio(&identity, &mut work(u64::MAX, 0)).unwrap();
    let mut peaks = Vec::new();
    for seconds in [180_u64, 360] {
        let stamp = CaptureStamp {
            generation: 1,
            edit_revision: 1,
            source_key: 1,
            binding: [1; 32],
            source_identity: identity.identity(),
            source_fingerprint: fingerprint.clone(),
            input_shape: AudioShape {
                frames: seconds * 8000,
                channels: 2,
                sample_rate: 8000,
            },
            selection: FrameRange {
                start: 137,
                end: seconds * 8000 - 29,
            },
        };
        let mut budget = work(u64::MAX, 0);
        let mut count = 0_u64;
        let peak = measured(|| {
            let input = CapturedInput::capture_stream(
                root.path(),
                stamp.clone(),
                InputLimits {
                    max_frames: 360 * 8000,
                    max_bytes: 32 * 1024 * 1024,
                    ..Default::default()
                },
                &mut budget,
                |work, emit| {
                    let block = [0.0_f32; 4096];
                    let mut remaining = seconds * 8000 * 2;
                    while remaining > 0 {
                        let n = remaining.min(4096) as usize;
                        emit(&block[..n], work)?;
                        remaining -= n as u64;
                    }
                    Ok(())
                },
            )
            .unwrap();
            assert!(input.is_file_backed());
            assert_eq!(
                input.disk_bytes().unwrap(),
                stamp.selection.frames().unwrap() * 8
            );
            assert!(input.retained_bytes() <= 64 * 1024);
            let mut processor = DereverbStream::new(
                8000,
                2,
                DereverbSettings {
                    mode: DereverbMode::General,
                    amount: 0.5,
                },
            )
            .unwrap();
            let mut emit = |samples: &[f32], _: &mut Work| {
                assert!(samples.iter().all(|sample| *sample == 0.0));
                count += samples.len() as u64;
                Ok(())
            };
            input
                .for_each_selected(&mut budget, |samples, work| {
                    processor.push(samples, work, &mut emit)
                })
                .unwrap();
            processor.finish(&mut budget, &mut emit).unwrap();
            drop(processor);
            drop(input);
        });
        assert_eq!(count, stamp.selection.frames().unwrap() * 2);
        assert!(peak <= windfall_analysis::DEREVERB_SCRATCH_BYTES + 64 * 1024);
        peaks.push(peak);
    }
    assert!(peaks[1] <= peaks[0] + 64 * 1024, "peaks={peaks:?}");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn file_capture_cancellation_incomplete_and_over_quota_never_return_a_result() {
    use windfall_analysis::{
        AudioShape, CaptureStamp, CapturedInput, ContentFingerprint, FrameRange, InputLimits,
    };
    let root = tempfile::tempdir().unwrap();
    let audio = AudioBuffer::from_interleaved(8000, 1, vec![0.0; 8000]);
    let stamp = CaptureStamp {
        generation: 1,
        edit_revision: 1,
        source_key: 1,
        binding: [0; 32],
        source_identity: audio.identity(),
        source_fingerprint: ContentFingerprint::audio(&audio, &mut work(u64::MAX, 0)).unwrap(),
        input_shape: AudioShape::of(&audio),
        selection: FrameRange {
            start: 0,
            end: 8000,
        },
    };
    let limits = InputLimits {
        max_bytes: 31999,
        ..Default::default()
    };
    let mut called = false;
    assert!(matches!(
        CapturedInput::capture_stream(
            root.path(),
            stamp.clone(),
            limits,
            &mut work(u64::MAX, 0),
            |_, _| {
                called = true;
                Ok(())
            }
        ),
        Err(AnalysisError::Budget(_))
    ));
    assert!(!called);
    assert!(matches!(
        CapturedInput::capture_stream(
            root.path(),
            stamp.clone(),
            InputLimits::default(),
            &mut work(u64::MAX, 0),
            |work, emit| emit(&[0.0; 4096], work)
        ),
        Err(AnalysisError::Invalid("incomplete stream capture"))
    ));
    let token = CancelToken::default();
    let mut budget = Work::new(token.clone(), 1_000_000, Duration::from_secs(30)).unwrap();
    assert!(matches!(
        CapturedInput::capture_stream(
            root.path(),
            stamp,
            InputLimits::default(),
            &mut budget,
            |work, emit| {
                emit(&[0.0; 4096], work)?;
                token.cancel();
                emit(&[0.0; 3904], work)
            }
        ),
        Err(AnalysisError::Cancelled)
    ));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[test]
fn selected_file_capture_charges_selection_and_repeated_reads_keep_exact_origin() {
    use windfall_analysis::{
        AudioShape, CaptureStamp, CapturedInput, ContentFingerprint, FrameRange, InputLimits,
    };
    let root = tempfile::tempdir().unwrap();
    let audio = AudioBuffer::from_interleaved(
        8000,
        1,
        (0..8000).map(|frame| frame as f32 / 8000.0).collect(),
    );
    let selection = FrameRange {
        start: 4093,
        end: 4101,
    };
    let stamp = CaptureStamp {
        generation: 1,
        edit_revision: 1,
        source_key: 1,
        binding: [0; 32],
        source_identity: audio.identity(),
        source_fingerprint: ContentFingerprint::audio(&audio, &mut work(u64::MAX, 0)).unwrap(),
        input_shape: AudioShape::of(&audio),
        selection,
    };
    let input = CapturedInput::capture_stream(
        root.path(),
        stamp,
        InputLimits {
            max_bytes: 32,
            ..Default::default()
        },
        &mut work(u64::MAX, 0),
        |work, emit| {
            for block in audio.samples().chunks(4096) {
                emit(block, work)?;
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(input.disk_bytes().unwrap(), 32);
    for _ in 0..2 {
        let mut read = Vec::new();
        input
            .for_each_selected(&mut work(u64::MAX, 0), |block, _| {
                read.extend_from_slice(block);
                Ok(())
            })
            .unwrap();
        assert_eq!(read, audio.samples()[4093..4101]);
    }
}
