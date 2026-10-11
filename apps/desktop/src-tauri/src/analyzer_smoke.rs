//! Opt-in debug shell fixture. No audio callback, release-build or ordinary
//! startup invokes it. Window values are collected by the frontend, not here.
use crate::session::Session;

pub(crate) fn enabled() -> bool {
    cfg!(debug_assertions)
        && std::env::var("WINDFALL_ANALYZER_SMOKE").is_ok_and(|value| value == "1")
        && std::env::var("WINDFALL_CONFIG_DIR")
            .is_ok_and(|path| crate::paths::absolute(&path).is_ok())
}

pub(crate) fn control(
    session: &Session,
    operation: &str,
    value: Option<f64>,
) -> Result<serde_json::Value, String> {
    if !enabled() {
        return Err(
            "Analyzer smoke is disabled (debug build and explicit isolated configuration required)"
                .into(),
        );
    }
    #[cfg(debug_assertions)]
    {
        debug::control(session, operation, value)
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (session, operation, value);
        Err("Analyzer smoke is absent in release builds".into())
    }
}

#[cfg(debug_assertions)]
mod debug {
    use crate::{
        paths,
        session::{ClipPlace, Session},
    };
    use serde_json::{Value, json};
    use std::path::Path;
    use windfall_codec::{WavSampleFormat, write_wav};
    use windfall_core::AudioBuffer;
    use windfall_ipc::{PlayMode, TransportPatch};
    use windfall_project::{Command, EffectId, EffectKind, MixerTrackPatch, TickRange, TrackId};

    fn tone(
        session: &Session,
        directory: &Path,
        name: &str,
        hz: f32,
        amplitude: f32,
        pan: f32,
    ) -> Result<TrackId, String> {
        let rate = 48_000;
        let pcm = (0..rate * 12)
            .flat_map(|frame| {
                let sample =
                    amplitude * (std::f32::consts::TAU * hz * frame as f32 / rate as f32).sin();
                [sample, sample]
            })
            .collect();
        let path = directory.join(name);
        write_wav(
            &path,
            &AudioBuffer::from_interleaved(rate, 2, pcm),
            WavSampleFormat::Float32,
        )
        .map_err(|error| error.to_string())?;
        let added = session.add_audio_clip_from_file(
            &paths::display(&path),
            ClipPlace {
                track: None,
                start: 0,
                mixer_track: None,
            },
        )?;
        let track = TrackId(
            *added
                .created
                .get(2)
                .ok_or("Tone fixture did not create its track")?,
        );
        session.dispatch(
            Command::UpdateMixerTrack {
                id: track,
                patch: MixerTrackPatch {
                    pan: Some(pan),
                    ..Default::default()
                },
            },
            None,
        )?;
        Ok(track)
    }

    fn effect(session: &Session, track: TrackId, kind: EffectKind) -> Result<EffectId, String> {
        Ok(EffectId(
            session
                .dispatch(
                    Command::AddEffect {
                        track,
                        kind,
                        index: None,
                    },
                    None,
                )?
                .created[0],
        ))
    }

    fn param(
        session: &Session,
        track: TrackId,
        effect: EffectId,
        kind: EffectKind,
        id: &str,
        value: f32,
    ) -> Result<(), String> {
        let param = kind
            .descriptors()
            .iter()
            .position(|info| info.id == id)
            .ok_or("Missing analyzer fixture setting")?;
        session.dispatch(
            Command::SetEffectParam {
                track,
                effect,
                param: param as u32,
                value,
            },
            None,
        )?;
        Ok(())
    }

    pub(super) fn control(
        session: &Session,
        operation: &str,
        value: Option<f64>,
    ) -> Result<Value, String> {
        match operation {
            "prepare" => {
                session.transport_stop();
                session.project_new()?;
                let directory = paths::absolute(
                    &std::env::var("WINDFALL_CONFIG_DIR").map_err(|error| error.to_string())?,
                )?
                .join("analyzer-smoke");
                std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
                let a = tone(session, &directory, "Left 440 Hz.wav", 440.0, 0.8, -1.0)?;
                let b = tone(session, &directory, "Right 1875 Hz.wav", 1875.0, 0.2, 1.0)?;
                let limiter = effect(session, a, EffectKind::Limiter)?;
                param(session, a, limiter, EffectKind::Limiter, "ceilingDb", -12.0)?;
                let tuner = effect(session, a, EffectKind::Tuner)?;
                param(session, a, tuner, EffectKind::Tuner, "referenceHz", 432.0)?;
                let eq = effect(session, a, EffectKind::Eq)?;
                let project = paths::display(&directory.join("Analyzer smoke.windfall"));
                session.project_save(Some(&project))?;
                session.project_open(&project)?;
                session.transport_set(TransportPatch {
                    mode: Some(PlayMode::Song),
                    loop_song: Some(true),
                    ..Default::default()
                })?;
                Ok(
                    json!({"project":project,"tracks":[a,b],"limiter":limiter,"tuner":tuner,"eq":eq,"toneHz":[440,1875],"referenceHz":432}),
                )
            }
            "play" => {
                session.transport_play()?;
                Ok(json!({"playing":true}))
            }
            "stop" => {
                session.transport_stop();
                Ok(json!({"playing":false}))
            }
            "seek" => {
                let tick = value
                    .filter(|tick| tick.is_finite() && *tick >= 0.0)
                    .ok_or("A finite positive seek is required")?;
                session.transport_seek(tick);
                Ok(json!({"tick":tick}))
            }
            "loop" => {
                let guard = session.timeline_state();
                let snapshot = session.document_snapshot();
                session.timeline_region(
                    Some(TickRange {
                        start: 960,
                        end: 1920,
                    }),
                    guard.generation,
                    snapshot.revision,
                )?;
                session.transport_seek(960.0);
                Ok(json!({"start":960,"end":1920}))
            }
            "replace" => {
                session.project_new()?;
                Ok(json!({"replaced":true}))
            }
            _ => Err("Unknown analyzer smoke operation".into()),
        }
    }
}
