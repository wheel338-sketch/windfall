//! Explicit, runtime-only monitoring of saved mixer input routes.
use super::{Session, State};
use crate::sync::lock;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, atomic::Ordering};
use windfall_engine::recording::{AlignmentMetrics, Capture, CaptureRoute};
use windfall_engine::recording_monitor::MonitorTelemetry;
use windfall_ipc::{
    InputMonitorState, InputMonitorTrackInfo, RecordingMonitorSettings, RecordingSource,
};
use windfall_project::TrackId;

#[derive(Default)]
pub(super) struct Monitors {
    live: Option<Live>,
    error: Option<String>,
}
struct Live {
    capture: Capture,
    rate: u32,
    tracks: Vec<(TrackId, String, Arc<AlignmentMetrics>, MonitorTelemetry)>,
}

impl Session {
    pub(super) fn refresh_input_monitor_signature(&self, state: &State) -> u64 {
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        state.generation.hash(&mut hash);
        for track in &state.document.project().mixer.tracks {
            let Some(recording) = &track.recording else {
                continue;
            };
            if !recording.monitor {
                continue;
            }
            track.id.hash(&mut hash);
            if let Some(input) = &recording.input {
                input.host.hash(&mut hash);
                input.device.hash(&mut hash);
                input.left.hash(&mut hash);
                input.right.hash(&mut hash);
            }
            recording.monitor_gain.to_bits().hash(&mut hash);
            recording.monitor_buffer_ms.hash(&mut hash);
        }
        let signature = hash.finish();
        self.inner
            .input_monitor_signature
            .store(signature, Ordering::Release);
        signature
    }

    pub fn input_monitor_start(&self) -> Result<InputMonitorState, String> {
        let _configuring = lock(&self.inner.configuring);
        let _recording = self.recording_idle()?;
        let mut monitors = lock(&self.inner.input_monitors);
        // Join the old owner before installing new readers: its cleanup may
        // remove readers, and must never remove those of the next operation.
        drop(monitors.live.take());
        monitors.error = None;
        let status = self.engine_status();
        if !status.running || status.sample_rate == 0 {
            return Err("Open an audio output before monitoring inputs.".into());
        }
        let (selected, signature) = {
            let state = self.state();
            let mut selected = Vec::new();
            for track in &state.document.project().mixer.tracks {
                let Some(recording) = &track.recording else {
                    continue;
                };
                if !recording.monitor {
                    continue;
                }
                recording.check()?;
                let input = recording.input.as_ref().ok_or_else(|| {
                    format!("Choose an input for monitored track {}.", track.name)
                })?;
                let setting = RecordingMonitorSettings {
                    track: track.id,
                    gain: recording.monitor_gain,
                    buffer_ms: recording.monitor_buffer_ms,
                };
                let source = RecordingSource {
                    host: input.host.clone(),
                    device: input.device.clone(),
                    left: input.left,
                    right: input.right,
                    alignment: None,
                    loop_recording: None,
                    monitor: Some(setting.clone()),
                    armed_tracks: None,
                    mixer_tap: None,
                };
                selected.push((track.id, track.name.clone(), source, setting));
            }
            if selected.is_empty() {
                return Err("Enable input monitoring on at least one mixer track.".into());
            }
            (selected, self.refresh_input_monitor_signature(&state))
        };
        let settings = selected.iter().map(|track| track.3.clone()).collect();
        let writers = self
            .controller()
            .start_input_monitors(settings, status.sample_rate)?;
        let mut tracks = Vec::new();
        let routes = selected
            .into_iter()
            .zip(writers)
            .map(|((id, name, source, _), monitor)| {
                let metrics = Arc::new(AlignmentMetrics::default());
                tracks.push((id, name, metrics.clone(), monitor.telemetry.clone()));
                CaptureRoute {
                    source,
                    metrics,
                    monitor: Some(monitor),
                    disk: None,
                    sink: Box::new(|_| Ok(())),
                }
            })
            .collect();
        let capture = match Capture::start_monitor_group(
            routes,
            status.sample_rate,
            self.controller().recording_clock(),
            self.inner.input_monitor_signature.clone(),
            signature,
            self.controller().clone(),
        ) {
            Ok(capture) => capture,
            Err(error) => {
                self.controller().stop_input_monitor();
                monitors.error = Some(error.clone());
                return Err(error);
            }
        };
        monitors.live = Some(Live {
            capture,
            rate: status.sample_rate,
            tracks,
        });
        Ok(monitors.snapshot())
    }

    pub fn input_monitor_state(&self) -> InputMonitorState {
        let mut monitors = lock(&self.inner.input_monitors);
        if monitors
            .live
            .as_ref()
            .is_some_and(|live| live.capture.failed())
        {
            let live = monitors.live.take().expect("finished monitor group");
            monitors.error = live.capture.finish().err();
        }
        monitors.snapshot()
    }

    pub fn input_monitor_stop(&self) -> InputMonitorState {
        let mut monitors = lock(&self.inner.input_monitors);
        drop(monitors.live.take());
        monitors.error = None;
        monitors.snapshot()
    }
}
impl Monitors {
    fn snapshot(&self) -> InputMonitorState {
        InputMonitorState {
            active: self.live.is_some(),
            sample_rate: self.live.as_ref().map_or(0, |live| live.rate),
            error: self.error.clone(),
            tracks: self.live.as_ref().map_or_else(Vec::new, |live| {
                live.tracks
                    .iter()
                    .map(|(id, name, alignment, monitor)| InputMonitorTrackInfo {
                        mixer_track: *id,
                        name: name.clone(),
                        alignment: alignment.snapshot(),
                        monitor: monitor.snapshot(),
                    })
                    .collect()
            }),
        }
    }
}
