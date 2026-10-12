//! Explicit operator-provided runtime + licensed weights; never ordinary CI.
use super::*;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use serde_json::json;

fn sdr(source: &[f32], estimate: &[f32]) -> f64 {
    let (power, error) = source.iter().zip(estimate).fold((0.0f64,0.0f64), |(p,e),(&s,&y)|
        (p+f64::from(s).powi(2),e+(f64::from(s)-f64::from(y)).powi(2)));
    10.0*(power.max(1e-30)/error.max(1e-30)).log10()
}
fn synthetic(rate: u32, channels: u16, frames: usize) -> Vec<Vec<f32>> {
    let mut sources = vec![vec![0.0; frames*channels as usize]; 4];
    for frame in 0..frames {
        let t = frame as f64/f64::from(rate);
        let beat = t % 0.5;
        let kick = if beat < 0.18 { 0.21*(-beat*30.0).exp()*(std::f64::consts::TAU*(70.0*beat+30.0*(1.0-(-beat*22.0).exp())/22.0)).sin() } else { 0.0 };
        let bass_hz = [55.0, 65.406, 73.416, 49.0][(t/2.0) as usize%4];
        let bass = 0.13*(std::f64::consts::TAU*bass_hz*t).sin()*(beat*40.0).min(1.0)*(1.0-beat*0.35);
        let fundamental = 185.0+3.0*(std::f64::consts::TAU*5.2*t).sin();
        let vowel: f64 = (1..=14).map(|h| {
            let f = h as f64*fundamental;
            let formant = (-((f-730.0)/180.0).powi(2)).exp()+0.7*(-((f-1250.0)/220.0).powi(2)).exp()+0.2;
            formant/h as f64*(std::f64::consts::TAU*f*t).sin()
        }).sum::<f64>()*0.085*(std::f64::consts::PI*(t%2.0)/2.0).sin().powi(2);
        let pad = [261.626, 329.628, 391.995].iter().map(|hz| (std::f64::consts::TAU*hz*t).sin()
            +0.2*(std::f64::consts::TAU*hz*2.0*t).sin()).sum::<f64>()*0.033*(t/0.7).min(1.0)*((12.0-t)/0.7).min(1.0).max(0.0);
        for channel in 0..channels as usize {
            let gains = if channel == 0 { [1.0,1.0,1.0,0.8] } else { [0.9,1.0,0.95,1.0] };
            for (stem, value) in [kick,bass,vowel,pad].into_iter().enumerate() {
                sources[stem][frame*channels as usize+channel] = (value*gains[stem]) as f32;
            }
        }
    }
    sources
}
#[cfg(windows)]
fn peak_memory_bytes() -> Option<u64> {
    #[repr(C)]
    struct Counters { cb: u32, faults: u32, peak_working: usize, working: usize,
        peak_paged: usize, paged: usize, peak_nonpaged: usize, nonpaged: usize,
        pagefile: usize, peak_pagefile: usize }
    #[link(name="kernel32")]
    unsafe extern "system" { fn GetCurrentProcess() -> *mut std::ffi::c_void; }
    #[link(name="psapi")]
    unsafe extern "system" { fn GetProcessMemoryInfo(process: *mut std::ffi::c_void, counters: *mut Counters, cb: u32) -> i32; }
    let mut counters = Counters { cb: size_of::<Counters>() as u32, faults: 0, peak_working: 0, working: 0,
        peak_paged: 0, paged: 0, peak_nonpaged: 0, nonpaged: 0, pagefile: 0, peak_pagefile: 0 };
    // SAFETY: current-process pseudo handle remains valid, and the writable
    // repr(C) buffer and cb match PROCESS_MEMORY_COUNTERS for this architecture.
    let result = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size_of::<Counters>() as u32) };
    (result != 0).then_some(counters.peak_working as u64)
}
#[cfg(target_os="linux")]
fn peak_memory_bytes() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status").ok()?.lines().find(|line| line.starts_with("VmHWM:"))?
        .split_whitespace().nth(1)?.parse::<u64>().ok()?.checked_mul(1024)
}
#[cfg(not(any(windows, target_os="linux")))]
fn peak_memory_bytes() -> Option<u64> { None }

#[test]
#[ignore = "requires WINDFALL_ORT_DYLIB and a licensed WINDFALL_STEM_MODEL_PACK; writes real-model quality evidence"]
fn real_onnx_four_stem_synthetic_mix_evidence() {
    let library = PathBuf::from(std::env::var_os("WINDFALL_ORT_DYLIB").expect("Set WINDFALL_ORT_DYLIB"));
    let pack = PathBuf::from(std::env::var_os("WINDFALL_STEM_MODEL_PACK").expect("Set WINDFALL_STEM_MODEL_PACK"));
    assert!(library.is_absolute() && library.is_file()); assert!(pack.is_dir());
    let epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    let evidence = std::env::var_os("WINDFALL_STEM_EVIDENCE").map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/stem-evidence"))
        .join(format!("{epoch}-{}", std::process::id()));
    std::fs::create_dir_all(&evidence).unwrap();
    let bytes = std::fs::read(pack.join("manifest.json")).unwrap(); assert!(bytes.len() <= 16384);
    let spec: windfall_ipc::AnalysisModel = serde_json::from_slice(&bytes).unwrap();
    let model = manifest(spec.clone()).unwrap(); native::separation::contract(&model).unwrap();
    assert_eq!(model.provenance.adapter_id, native::separation::ADAPTER_ID);
    let before_peak = peak_memory_bytes();
    let rig = Rig::new();
    rig.session.analysis_runtime_configure(AnalysisRuntimeSettings { library_path: Some(crate::paths::display(&library)), ..Default::default() }).unwrap();
    rig.session.analysis_model_import(&crate::paths::display(&pack.join("model.onnx")), spec).unwrap();
    let rate = 48000;
    let frames = 12*rate as usize;
    // Stereo playback view; mono model packs are explicitly downmixed by job capture.
    let channels = model.input_channels;
    let sources = synthetic(rate, channels, frames);
    let mix: Vec<f32> = (0..frames*channels as usize).map(|i| sources.iter().map(|s| s[i]).sum()).collect();
    for (role, samples) in ["drums","bass","vocals","other"].iter().zip(&sources) {
        windfall_codec::write_wav(evidence.join(format!("source-{role}.wav")), &AudioBuffer::from_interleaved(rate, channels, samples.clone()), windfall_codec::WavSampleFormat::Float32).unwrap();
    }
    let mix_path = evidence.join("mix.wav");
    windfall_codec::write_wav(&mix_path, &AudioBuffer::from_interleaved(rate, channels, mix), windfall_codec::WavSampleFormat::Float32).unwrap();
    let result = rig.session.add_audio_clip_from_file(&crate::paths::display(&mix_path), ClipPlace { start: 0, track: None, mixer_track: None }).unwrap();
    let clip = ClipId(*result.created.last().unwrap());
    let start = Instant::now();
    let submit_start = Instant::now();
    let job = rig.session.analysis_submit(AnalysisSubmit { amount: None, task: AnalysisTask::StemSeparation, clip,
        model_id: model.id.clone(), model_version: model.version.clone(), model_revision: model.revision.to_string(), start_frame: "0".into(), end_frame: frames.to_string() });
    let capture_ms = submit_start.elapsed().as_secs_f64()*1000.0;
    let job = match job {
        Ok(job) => job,
        Err(error) => {
            std::fs::write(evidence.join("report.json"), serde_json::to_vec_pretty(&json!({ "failure": error, "model": model_wire(&model), "captureMs": capture_ms, "peakMemoryBytes": peak_memory_bytes() })).unwrap()).unwrap();
            panic!("Separation capability failed; see {}", evidence.display());
        }
    };
    let worker_start = Instant::now();
    let snapshot = rig.session.inner.analysis.test_manager().wait(native::JobId(job.job.parse().unwrap())).unwrap();
    let worker_ms = worker_start.elapsed().as_secs_f64()*1000.0;
    if snapshot.status != native::JobStatus::Ready {
        std::fs::write(evidence.join("report.json"), serde_json::to_vec_pretty(&json!({ "failure": snapshot.failure, "status": snapshot.status, "model": model_wire(&model), "captureMs": capture_ms, "workerMs": worker_ms, "peakMemoryBytes": peak_memory_bytes() })).unwrap()).unwrap();
        panic!("Runtime job failed; see {}", evidence.display());
    }
    let review = rig.session.analysis_review(&job.ticket).unwrap();
    let apply_start = Instant::now();
    rig.session.analysis_apply(apply(&job)).unwrap();
    let apply_ms = apply_start.elapsed().as_secs_f64()*1000.0;
    let project = rig.project();
    let mut scores = Vec::new();
    let mut own_is_closest = true;
    for (index, role) in ["drums","bass","vocals","other"].iter().enumerate() {
        let sample = project.samples.iter().find(|s| s.name.ends_with(&format!("({role})"))).unwrap();
        let audio = rig.session.state().pool.get(sample.id).unwrap().clone();
        assert_eq!(audio.frames(), frames); assert_eq!(audio.sample_rate(), rate); assert_eq!(audio.channels(), channels);
        windfall_codec::write_wav(evidence.join(format!("stem-{role}.wav")), &audio, windfall_codec::WavSampleFormat::Float32).unwrap();
        let against: Vec<_> = sources.iter().map(|source| sdr(source, audio.samples())).collect();
        let closest = (0..4).max_by(|&a,&b| against[a].total_cmp(&against[b])).unwrap();
        let closest_is_own = closest == index && (0..4).filter(|&i| i != index).all(|i| against[index] > against[i]);
        own_is_closest &= closest_is_own;
        scores.push(json!({ "role": role, "sdrDb": against[index], "sdrAgainstAllSourcesDb": against,
            "closestSource": (["drums","bass","vocals","other"][closest]), "closestIsOwn": closest_is_own }));
    }
    let peak = peak_memory_bytes();
    let report = json!({ "runtimePath": library, "model": model_wire(&model), "sampleRate": rate, "frames": frames,
        "captureMs": capture_ms, "workerMs": worker_ms, "applyMs": apply_ms, "totalMs": start.elapsed().as_secs_f64()*1000.0,
        "processPeakMemoryBytesBefore": before_peak, "peakMemoryBytes": peak, "memoryMeasurement": "process peak working set / VmHWM; includes harness, codec and ORT; not allocator-only",
        "jobReservation": format!("{:?}", rig.session.inner.analysis.test_manager().usage()),
        "artifacts": review.artifacts, "stems": scores, "pass": own_is_closest });
    std::fs::write(evidence.join("report.json"), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    assert!(peak.is_some(), "Peak memory unavailable on this OS; report retained");
    assert!(own_is_closest, "Each stem must be closest to its own independently known source; see {}", evidence.display());
}
