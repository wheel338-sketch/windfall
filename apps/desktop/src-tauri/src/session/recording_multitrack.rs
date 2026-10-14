//! Saved mixer arms become one capture group and one atomic history entry.
use super::*;
use windfall_project::file::sample_path_for;
use windfall_project::{ClipContent, ClipInit, Command, MixerRecordMode, SampleId, TrackId};

pub(super) struct ArmedRoute {
    pub name: String,
    pub source: RecordingSource,
    pub place: ClipPlace,
}
impl Session {
    pub(super) fn armed_recording_routes(
        &self,
        options: &RecordingSource,
        start: u32,
        track: Option<PlaylistTrackId>,
    ) -> Result<Vec<ArmedRoute>, String> {
        if options.armed_tracks != Some(true) {
            let Some(tap) = &options.mixer_tap else {
                return Ok(Vec::new());
            };
            let state = self.state();
            let mixer = state
                .document
                .project()
                .mixer
                .track(tap.track)
                .ok_or("Mixer recording track no longer exists.")?;
            return Ok(vec![ArmedRoute {
                name: mixer.name.clone(),
                source: options.clone(),
                place: ClipPlace {
                    start,
                    track,
                    mixer_track: Some(tap.track),
                },
            }]);
        }
        let state = self.state();
        let mut routes = Vec::new();
        for mixer in &state.document.project().mixer.tracks {
            let Some(recording) = &mixer.recording else {
                continue;
            };
            if !recording.armed {
                continue;
            }
            recording.check()?;
            if recording.mode == MixerRecordMode::Input && recording.input.is_none() {
                return Err(windfall_ipc::ui_message(
                    "recording.armedNeedsInput",
                    serde_json::json!({"track": mixer.name}),
                ));
            }
            let input = recording.input.as_ref();
            let mut alignment = options.alignment.clone().unwrap_or_default();
            alignment.synchronize = true;
            alignment.offset_ms += recording.offset_ms;
            if alignment.offset_ms.abs() > 1000.0 {
                return Err(windfall_ipc::ui_message(
                    "recording.offsetTooLarge",
                    serde_json::json!({"track": mixer.name}),
                ));
            }
            routes.push(ArmedRoute {
                name: mixer.name.clone(),
                place: ClipPlace {
                    start,
                    track: if routes.is_empty() { track } else { None },
                    mixer_track: Some(mixer.id),
                },
                source: RecordingSource {
                    host: input.map_or_else(String::new, |input| input.host.clone()),
                    device: input.map_or_else(String::new, |input| input.device.clone()),
                    left: input.map_or(0, |input| input.left),
                    right: input.and_then(|input| input.right),
                    alignment: Some(alignment),
                    loop_recording: options.loop_recording.clone(),
                    armed_tracks: None,
                    monitor: (recording.monitor && input.is_some()).then_some(
                        windfall_ipc::RecordingMonitorSettings {
                            track: mixer.id,
                            gain: recording.monitor_gain,
                            buffer_ms: recording.monitor_buffer_ms,
                        },
                    ),
                    mixer_tap: (recording.mode != MixerRecordMode::Input).then_some(
                        windfall_ipc::RecordingMixerTap {
                            track: mixer.id,
                            mode: recording.mode,
                        },
                    ),
                },
            });
        }
        if routes.is_empty() {
            return Err("Arm at least one mixer track before multitrack recording.".into());
        }
        Ok(routes)
    }
    pub(super) fn attach_multitrack_take(
        &self,
        take: &mut Take,
        selection: Option<RecordingTakeSelection>,
    ) -> Result<DispatchResult, String> {
        let frames = take.recorded_frames();
        let span = take.loop_frames.unwrap_or(frames as f64);
        let total = (frames as f64 / span).ceil() as u32;
        if total > 256 {
            return Err("Recording group exceeded 256 loop passes.".into());
        }
        let selected = match selection.unwrap_or(RecordingTakeSelection::All) {
            RecordingTakeSelection::All => (0..total).collect::<Vec<_>>(),
            RecordingTakeSelection::Latest => vec![total.saturating_sub(1)],
            RecordingTakeSelection::Only { indices } => indices,
            RecordingTakeSelection::Except { indices } => (0..total)
                .filter(|index| !indices.contains(index))
                .collect(),
        };
        let selected = selected
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        if selected.is_empty() || selected.iter().any(|index| *index >= total) {
            return Err("Choose an existing recording pass to keep.".into());
        }
        let latest = *selected.last().expect("kept pass");
        let inputs = std::iter::once((
            take.group_name.clone().expect("group root name"),
            take.path.clone(),
            take.place,
            take.printed,
        ))
        .chain(take.inputs.iter().map(|input| {
            (
                input.name.clone(),
                input.path.clone(),
                input.place,
                input.printed,
            )
        }))
        .collect::<Vec<_>>();
        let folder = take
            .path
            .parent()
            .ok_or("Recording folder is unavailable.")?
            .to_owned();
        let mut retained = inputs
            .iter()
            .map(|(name, ..)| windfall_project::AudioTakeLane {
                name: bounded_name(name),
                takes: Vec::new(),
            })
            .collect::<Vec<_>>();
        let mut imports = Vec::new();
        for (lane, (name, path, place, printed)) in inputs.into_iter().enumerate() {
            let audio = self.inner.cache.decode(&path)?;
            for &pass in &selected {
                let first = take_boundary(pass, span);
                let last = take_boundary(pass + 1, span).min(frames);
                let file = takes::reserve_take(&folder, pass)?;
                take.split_paths.push(file.clone());
                let mut writer = WavWriter::create(&file, take.rate, 2, WavSampleFormat::Float32)
                    .map_err(|error| error.to_string())?;
                for block in audio.samples()[first as usize * 2..last as usize * 2].chunks(16_384) {
                    writer.write(block).map_err(|error| error.to_string())?;
                }
                writer.finalize().map_err(|error| error.to_string())?;
                let buffer = self.inner.cache.decode(&file)?;
                let limit = take
                    .loop_region
                    .map_or(MAX_SONG_TICKS - place.start, |region| {
                        region.end - region.start
                    });
                let length = if take.loop_frames.is_some() && last == take_boundary(pass + 1, span)
                {
                    limit
                } else {
                    (self
                        .controller()
                        .song_tick_after_seconds(place.start, buffer.duration_secs())
                        - f64::from(place.start))
                    .ceil()
                    .max(1.0) as u32
                }
                .min(limit);
                imports.push((
                    name.clone(),
                    pass,
                    file,
                    buffer,
                    place,
                    length,
                    printed,
                    lane,
                ));
            }
        }
        let ticket = {
            let _recording = self.recording_finish_guard()?;
            let state = self.state();
            let mut next = state.document.project().next_id;
            let mut allocate = || {
                let id = next;
                next = next.saturating_add(1);
                id
            };
            let mut commands = Vec::new();
            let mut sources = Vec::new();
            for (name, pass, file, buffer, place, length, printed, lane) in imports {
                let sample = SampleId(allocate());
                let name = if take.loop_frames.is_some() {
                    format!("{name} take {}", pass + 1)
                } else {
                    format!("{name} recording")
                };
                commands.push(Command::AddSample {
                    name: name.clone(),
                    path: sample_path_for(&file, state.project_dir(), &self.inner.factory_dir),
                });
                let track = if let Some(track) = place.track.filter(|_| pass == latest) {
                    track
                } else {
                    let id = PlaylistTrackId(allocate());
                    commands.push(Command::AddPlaylistTrack {
                        name: Some(name),
                        index: None,
                    });
                    id
                };
                commands.push(Command::AddClips {
                    clips: vec![ClipInit {
                        track,
                        start: place.start,
                        length: Some(length),
                        offset: None,
                        muted: Some(pass != latest),
                        content: ClipContent::Audio {
                            sample,
                            mixer_track: place.mixer_track.unwrap_or(TrackId::MASTER),
                            output: if printed {
                                windfall_project::ClipAudioOutput::Direct
                            } else {
                                windfall_project::ClipAudioOutput::Mixer
                            },
                            normalize: false,
                            gain: 1.0,
                            pan: 0.0,
                            fade_in: 0,
                            fade_out: 0,
                            reverse: false,
                            pitch: 0.0,
                            stretch: Default::default(),
                        },
                    }],
                });
                let clip = windfall_project::ClipId(allocate());
                retained[lane].takes.push(windfall_project::AudioTakeRef {
                    pass: pass as u16,
                    clip,
                });
                sources.push((sample, buffer));
            }
            commands.push(Command::CreateAudioTakeGroup {
                name: "Multitrack recording".into(),
                lanes: retained,
            });
            self.sample_edit_ticket(
                &state,
                Command::Batch {
                    label: Some("Keep multitrack recording".into()),
                    commands,
                },
                None,
                sources,
            )?
            .on_install(take.installed.clone())
        };
        let mut prepared = ticket.prepare()?;
        let mut retirement = None;
        #[cfg(test)]
        self.pause("recording:prepared");
        let result = {
            let _recording = self.recording_finish_guard()?;
            prepared.commit(&mut self.state(), &mut retirement)
        };
        if let Some(retirement) = &mut retirement {
            self.retire_project(retirement);
        }
        result
    }
}
pub(super) fn reserve_input(
    folder: &std::path::Path,
    route: ArmedRoute,
    rate: u32,
) -> Result<RecordedInput, String> {
    let path = takes::reserve_take(folder, 0)?;
    let writer = match WavWriter::create(&path, rate, 2, WavSampleFormat::Float32) {
        Ok(writer) => writer,
        Err(error) => {
            let _ = fs::remove_file(&path);
            return Err(error.to_string());
        }
    };
    let printed = route.source.mixer_tap.is_some();
    Ok(RecordedInput {
        name: route.name,
        source: route.source,
        place: route.place,
        path,
        writer: Arc::new(Mutex::new(Some(writer))),
        written: Arc::new(AtomicU64::new(0)),
        alignment: Arc::new(AlignmentMetrics::default()),
        monitor: None,
        printed,
    })
}
fn bounded_name(name: &str) -> String {
    let mut bytes = 0;
    name.chars()
        .filter(|character| *character != '\0')
        .take_while(|character| {
            bytes += character.len_utf8();
            bytes <= 128
        })
        .collect()
}
