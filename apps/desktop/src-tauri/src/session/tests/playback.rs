//! The transport, the realtime feed, the audio device and previews.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use windfall_codec::{WavSampleFormat, write_wav};
use windfall_core::AudioBuffer;
use windfall_ipc::{AudioHost, AudioSettings, PlayMode, RealtimeFrame, TransportPatch};
use windfall_project::{
    AutomationTarget, ClipContent, ClipId, ClipInit, ClipPatch, ClipUpdate, Command, PatternId,
    PlaylistTrackId, PlaylistTrackPatch, TrackId,
};

use super::{FakeDevice, Rig, SAMPLE_RATE, factory_file, rms, still_running};
use crate::events::Event;
use crate::session::{EMPTY_PLAYLIST, MUTED_PLAYLIST, SILENT_PLAYLIST};
use crate::settings::{SETTINGS_FILE, SettingsStore};
use crate::sync::lock;

fn step_on(rig: &Rig, channel: usize, step: u32) {
    rig.session
        .dispatch(
            Command::ToggleStep {
                pattern: rig.pattern(),
                channel: rig.channel(channel),
                step,
            },
            None,
        )
        .unwrap();
}

#[test]
fn the_transport_is_announced_when_it_changes_and_only_then() {
    let rig = Rig::new();
    let session = &rig.session;

    assert!(session.transport_play().unwrap().playing);
    assert!(session.transport_play().unwrap().playing);
    assert!(!session.transport_toggle().unwrap().playing);
    assert!(!session.transport_stop().playing);
    assert!(session.transport_toggle().unwrap().playing);
    let song = session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(true),
            ..TransportPatch::default()
        })
        .unwrap();
    assert_eq!(session.transport_state(), song);
    // Setting what is already set changes nothing.
    session.transport_set(TransportPatch::default()).unwrap();
    session.transport_seek(480.0);

    let states = rig.take_transport_states();
    let seen: Vec<(bool, PlayMode, bool)> = states
        .iter()
        .map(|state| (state.playing, state.mode, state.loop_song))
        .collect();
    assert_eq!(
        seen,
        [
            (true, PlayMode::Pattern, false),
            (false, PlayMode::Pattern, false),
            (true, PlayMode::Pattern, false),
            (true, PlayMode::Song, true),
        ]
    );
    assert_eq!(states.last(), Some(&song));
}

#[test]
fn the_transport_cannot_be_pointed_at_a_pattern_that_does_not_exist() {
    let rig = Rig::new();
    let before = rig.session.transport_state();
    let error = rig
        .session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            pattern: Some(PatternId(999)),
            ..TransportPatch::default()
        })
        .unwrap_err();
    assert_eq!(
        windfall_core::english::english(&error),
        "pattern 999 does not exist"
    );
    assert_eq!(rig.session.transport_state(), before);
    assert!(rig.events.take().is_empty());
}

#[test]
fn stopping_returns_the_playhead_to_where_playback_started() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    session.transport_seek(960.0);
    session.transport_play().unwrap();
    rig.run(SAMPLE_RATE as usize / 2);
    // Half a second at 120 bpm is one beat.
    let frame = session.realtime_tick();
    assert!((frame.tick - 1_920.0).abs() < 1.0, "tick {}", frame.tick);

    session.transport_stop();
    rig.run(480);
    assert_eq!(session.realtime_tick().tick, 960.0);
}

#[test]
fn every_subscriber_gets_the_same_frame_and_a_closed_one_is_dropped() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    step_on(&rig, 0, 0);

    let inbox = |open: bool| {
        let frames = Arc::new(Mutex::new(Vec::<RealtimeFrame>::new()));
        let sink = frames.clone();
        let sender = Box::new(move |frame: &RealtimeFrame| {
            lock(&sink).push(frame.clone());
            open
        });
        (frames, sender)
    };
    let (main, to_main) = inbox(true);
    let (detached, to_detached) = inbox(true);
    let (closing, to_closing) = inbox(false);
    session.subscribe_realtime("main", to_main);
    session.subscribe_realtime("mixer", to_detached);
    session.subscribe_realtime("closing", to_closing);

    session.transport_play().unwrap();
    rig.run(4_800);
    let first = session.realtime_tick();
    assert!(first.playing);
    assert!(first.meters[0] > 0.1, "{:?}", first.meters);
    assert_eq!(first.xruns, 0);

    // The meters are peaks since the last frame: read once, they are gone.
    // Both windows must therefore be sent the one frame that was read.
    assert_eq!(*lock(&main), std::slice::from_ref(&first));
    assert_eq!(*lock(&detached), std::slice::from_ref(&first));
    assert_eq!(*lock(&closing), [first]);

    rig.run(4_800);
    let second = session.realtime_tick();
    assert_eq!(lock(&main).len(), 2);
    assert_eq!(lock(&main)[1], second);
    assert_eq!(lock(&detached).len(), 2);
    assert_eq!(
        lock(&closing).len(),
        1,
        "a closed window is not sent to again"
    );

    // A window that subscribes again replaces its old channel.
    let (reloaded, to_reloaded) = inbox(true);
    session.subscribe_realtime("main", to_reloaded);
    session.unsubscribe_realtime("mixer");
    session.realtime_tick();
    assert_eq!(lock(&main).len(), 2);
    assert_eq!(lock(&detached).len(), 2);
    assert_eq!(lock(&reloaded).len(), 1);
}

#[test]
fn a_song_that_reaches_its_end_stops_and_the_ui_is_told() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    step_on(&rig, 0, 0);
    let track = session
        .dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0];
    session
        .dispatch(
            Command::AddClips {
                clips: vec![ClipInit {
                    track: PlaylistTrackId(track),
                    start: 0,
                    length: None,
                    offset: None,
                    muted: None,
                    content: ClipContent::Pattern {
                        pattern: rig.pattern(),
                    },
                }],
            },
            None,
        )
        .unwrap();
    session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(false),
            ..TransportPatch::default()
        })
        .unwrap();
    assert!(session.transport_play().unwrap().playing);
    rig.events.take();

    // One bar at 120 bpm is two seconds. Half-way through it still plays.
    rig.run(SAMPLE_RATE as usize);
    assert!(session.realtime_tick().playing);
    assert!(rig.events.take().is_empty());

    rig.run(SAMPLE_RATE as usize * 3 / 2);
    assert!(!session.realtime_tick().playing);
    let states = rig.take_transport_states();
    assert_eq!(states.len(), 1);
    assert!(!states[0].playing);
    assert_eq!(states[0].mode, PlayMode::Song);

    // Told once, not on every frame.
    session.realtime_tick();
    assert!(rig.events.take().is_empty());
}

#[test]
fn the_realtime_thread_sends_frames_and_ends_with_the_session() {
    let rig = Rig::new();
    let (send, receive) = mpsc::channel();
    rig.session
        .subscribe_realtime("main", Box::new(move |_| send.send(()).is_ok()));
    let thread = rig.session.spawn_realtime().unwrap();
    // Hosted CI can suspend this thread while other tests run. Test delivery
    // here; virtual-time tests in realtime.rs verify the 60 Hz schedule and
    // recovery without assuming a wall-clock scheduling percentage.
    for _ in 0..3 {
        receive.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    drop(rig);
    thread.join().unwrap();
}

#[test]
fn configuring_the_engine_remembers_the_settings_and_announces_the_status() {
    let rig = Rig::new();
    let session = &rig.session;
    assert_eq!(session.engine_status().buffer_frames, 480);
    session.transport_play().unwrap();
    rig.events.take();

    let settings = AudioSettings {
        processing_threads: None,
        output_channels: None,
        host: None,
        device: None,
        sample_rate: Some(44_100),
        buffer_frames: Some(128),
    };
    let status = session.engine_configure(settings.clone());
    assert!(status.running);
    assert_eq!((status.sample_rate, status.buffer_frames), (44_100, 128));
    assert_eq!(session.engine_status(), status);
    assert!(
        rig.events
            .take()
            .contains(&Event::EngineStatus(status.clone()))
    );

    // A device that cannot be opened is a status, not a failed call.
    let failed = session.engine_configure(AudioSettings {
        device: Some("Unplugged".to_owned()),
        ..settings.clone()
    });
    assert!(!failed.running);
    assert!(failed.error.as_deref().unwrap().contains("Unplugged"));
    assert_eq!(rig.events.take(), [Event::EngineStatus(failed)]);

    session.engine_configure(settings);
    let rig = rig.restart();
    // The fake device opens with what the settings file holds.
    assert_eq!(rig.session.engine_status(), status);
}

#[test]
fn a_status_that_changes_by_itself_is_announced_once() {
    let rig = Rig::new();
    rig.session.poll_engine_status();
    assert!(rig.events.take().is_empty());

    let mut lost = FakeDevice::describe(&AudioSettings::default());
    lost.running = false;
    lost.error = Some("the device was unplugged".to_owned());
    *lock(&rig.device.status) = lost.clone();
    rig.session.poll_engine_status();
    rig.session.poll_engine_status();
    assert_eq!(rig.events.take(), [Event::EngineStatus(lost)]);

    let back = FakeDevice::describe(&AudioSettings::default());
    *lock(&rig.device.status) = back.clone();
    rig.session.poll_engine_status();
    assert_eq!(rig.events.take(), [Event::EngineStatus(back)]);
}

#[test]
fn a_preview_plays_a_file_without_the_transport_and_can_be_stopped() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    let crash = factory_file("Drums/Cymbals/Crash.wav");

    session.preview_play(&crash).unwrap();
    assert!(rms(&rig.run(4_800)) > 0.01);
    assert!(!session.transport_state().playing);
    // The master meter shows it.
    assert!(session.realtime_tick().meters[0] > 0.01);

    session.preview_stop();
    rig.run(4_800);
    assert!(rms(&rig.run(4_800)) < 1e-4, "the preview is still sounding");

    let error = session.preview_play(&rig.file("nothing.wav")).unwrap_err();
    assert!(error.contains("nothing.wav"), "{error}");
    assert!(rig.events.take().is_empty());
}

#[test]
fn a_stop_cannot_be_overtaken_by_the_preview_it_stops() {
    let mut rig = Rig::new();
    let session = rig.session.clone();

    // The preview has found itself still wanted and is on its way to the
    // engine.
    let hold = session.hold("preview:send");
    let previewing = session
        .background(|session| session.preview_play(&factory_file("Drums/Cymbals/Crash.wav")));
    hold.wait();
    let stopping = session.background(|session| session.preview_stop());
    assert!(
        still_running(&stopping),
        "the stop got in ahead of the preview it was meant to stop"
    );

    hold.release();
    previewing.join().unwrap().unwrap();
    stopping.join().unwrap();
    rig.run(4_800);
    assert!(
        rms(&rig.run(4_800)) < 1e-4,
        "the preview plays although it was stopped after it started"
    );
}

#[test]
fn a_newer_preview_cannot_be_replaced_by_an_older_one() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    let silence = rig.file("silence.wav");
    let nothing = AudioBuffer::from_interleaved(SAMPLE_RATE, 1, vec![0.0; 48_000]);
    write_wav(&silence, &nothing, WavSampleFormat::Int16).unwrap();

    let hold = session.hold("preview:send");
    let older = session
        .background(|session| session.preview_play(&factory_file("Drums/Cymbals/Crash.wav")));
    hold.wait();
    let newer = session.background(move |session| session.preview_play(&silence));
    assert!(
        still_running(&newer),
        "the newer preview got in ahead of the older one"
    );

    hold.release();
    older.join().unwrap().unwrap();
    newer.join().unwrap().unwrap();
    rig.run(4_800);
    assert!(
        rms(&rig.run(4_800)) < 1e-4,
        "the older preview is the one that plays"
    );
}

#[test]
fn a_song_with_nothing_to_hear_is_not_started() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    step_on(&rig, 0, 0);
    let song = session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            ..TransportPatch::default()
        })
        .unwrap();
    rig.events.take();

    assert_eq!(
        EMPTY_PLAYLIST,
        "The playlist is empty. Add a clip to the playlist, or switch to pattern mode."
    );
    assert_eq!(session.transport_play().unwrap_err(), EMPTY_PLAYLIST);
    assert_eq!(session.transport_toggle().unwrap_err(), EMPTY_PLAYLIST);
    assert_eq!(session.transport_state(), song);
    rig.run(4_800);
    assert!(!session.realtime_tick().playing);
    assert!(rig.events.take().is_empty());

    // A clip that is muted, or sits on a muted track, is as good as none.
    let track = PlaylistTrackId(
        session
            .dispatch(
                Command::AddPlaylistTrack {
                    name: None,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let clip = ClipId(
        session
            .dispatch(
                Command::AddClips {
                    clips: vec![ClipInit {
                        track,
                        start: 0,
                        length: None,
                        offset: None,
                        muted: None,
                        content: ClipContent::Pattern {
                            pattern: rig.pattern(),
                        },
                    }],
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let mute_clip = |muted: bool| {
        session
            .dispatch(
                Command::UpdateClips {
                    updates: vec![ClipUpdate {
                        id: clip,
                        patch: ClipPatch {
                            muted: Some(muted),
                            ..ClipPatch::default()
                        },
                    }],
                },
                None,
            )
            .unwrap();
    };
    let mute_track = |muted: bool| {
        session
            .dispatch(
                Command::UpdatePlaylistTrack {
                    id: track,
                    patch: PlaylistTrackPatch {
                        muted: Some(muted),
                        ..PlaylistTrackPatch::default()
                    },
                },
                None,
            )
            .unwrap();
    };
    mute_clip(true);
    assert_eq!(session.transport_play().unwrap_err(), MUTED_PLAYLIST);
    mute_clip(false);
    mute_track(true);
    assert_eq!(session.transport_toggle().unwrap_err(), MUTED_PLAYLIST);
    assert!(!session.transport_state().playing);

    // With something to hear, it plays, and playing again is no error.
    mute_track(false);
    assert!(session.transport_toggle().unwrap().playing);
    assert!(session.transport_play().unwrap().playing);
    assert!(rms(&rig.run(4_800)) > 0.01);
    assert!(!session.transport_toggle().unwrap().playing);

    // Pattern mode never asks about the playlist.
    mute_clip(true);
    session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Pattern),
            ..TransportPatch::default()
        })
        .unwrap();
    assert!(session.transport_play().unwrap().playing);
}

#[test]
fn a_song_of_nothing_but_automation_is_not_started() {
    let mut rig = Rig::new();
    let session = rig.session.clone();
    step_on(&rig, 0, 0);
    session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            ..TransportPatch::default()
        })
        .unwrap();

    // An automation of the master's fader, with its clip, is all the
    // playlist holds: four bars in which nothing would be heard.
    let target = AutomationTarget::TrackVolume {
        track: TrackId::MASTER,
    };
    session.automate(target).unwrap();
    assert_eq!(rig.project().playlist.clips.len(), 1);
    rig.events.take();
    assert_eq!(
        SILENT_PLAYLIST,
        "Only automation clips would play, and they make no sound by themselves. Add or unmute a pattern or audio clip, or switch to pattern mode."
    );
    assert_eq!(session.transport_play().unwrap_err(), SILENT_PLAYLIST);
    assert_eq!(session.transport_toggle().unwrap_err(), SILENT_PLAYLIST);
    rig.run(4_800);
    assert!(!session.realtime_tick().playing);
    assert!(rig.events.take().is_empty());

    // A pattern clip beside it that is muted changes nothing.
    let track = rig.project().playlist.tracks[0].id;
    let clip = ClipId(
        session
            .dispatch(
                Command::AddClips {
                    clips: vec![ClipInit {
                        track,
                        start: 0,
                        length: None,
                        offset: None,
                        muted: Some(true),
                        content: ClipContent::Pattern {
                            pattern: rig.pattern(),
                        },
                    }],
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    assert_eq!(session.transport_play().unwrap_err(), SILENT_PLAYLIST);
    let mute = |muted: bool| ClipPatch {
        muted: Some(muted),
        ..ClipPatch::default()
    };
    let update = |id: ClipId, patch: ClipPatch| {
        let updates = vec![ClipUpdate { id, patch }];
        session
            .dispatch(Command::UpdateClips { updates }, None)
            .unwrap();
    };

    // Unmuted, the pattern is something to hear, and the song plays.
    update(clip, mute(false));
    assert!(session.transport_play().unwrap().playing);
    assert!(rms(&rig.run(4_800)) > 0.01);
    assert!(!session.transport_stop().playing);

    // With the automation muted as well as the pattern, every clip is
    // muted, and the message is the one for that.
    update(clip, mute(true));
    let clips = rig.project().playlist.clips;
    let automation = clips.iter().find(|other| other.id != clip).unwrap().id;
    update(automation, mute(true));
    assert_eq!(session.transport_play().unwrap_err(), MUTED_PLAYLIST);
}

#[test]
fn the_audio_request_is_remembered_as_the_user_made_it() {
    let rig = Rig::new();
    let session = &rig.session;
    assert_eq!(session.engine_settings(), AudioSettings::default());
    *lock(&rig.device.hosts) = vec![AudioHost {
        name: "Test".to_owned(),
        is_default: true,
        devices: vec![windfall_ipc::AudioDevice {
            output_channels: Vec::new(),
            name: "Speakers".to_owned(),
            is_default: true,
            sample_rates: vec![44_100, 48_000],
            min_buffer_frames: Some(64),
            max_buffer_frames: Some(2_048),
        }],
    }];
    let stored = || {
        SettingsStore::load(rig.folder.path().join(SETTINGS_FILE))
            .settings()
            .audio
            .clone()
    };

    // A rate and a buffer size the device cannot do: it opens with its
    // own, and the request stays as it was made.
    let request = AudioSettings {
        processing_threads: None,
        output_channels: None,
        host: None,
        device: None,
        sample_rate: Some(8_000),
        buffer_frames: Some(16),
    };
    let status = session.engine_configure(request.clone());
    assert!(status.running);
    assert_eq!(
        (status.sample_rate, status.buffer_frames),
        (SAMPLE_RATE, 480)
    );
    assert_eq!(session.engine_settings(), request);
    assert_eq!(stored(), request);

    // Each is judged by itself.
    let request = AudioSettings {
        sample_rate: Some(44_100),
        buffer_frames: Some(4_096),
        ..request
    };
    let status = session.engine_configure(request.clone());
    assert_eq!((status.sample_rate, status.buffer_frames), (44_100, 480));
    assert_eq!(session.engine_settings(), request);

    // What the device can do is opened as asked.
    let request = AudioSettings {
        buffer_frames: Some(128),
        ..request
    };
    let status = session.engine_configure(request.clone());
    assert_eq!((status.sample_rate, status.buffer_frames), (44_100, 128));

    // "Default" stays default, whatever numbers the device opens with.
    let status = session.engine_configure(AudioSettings::default());
    assert_eq!(
        (status.sample_rate, status.buffer_frames),
        (SAMPLE_RATE, 480)
    );
    assert_eq!(session.engine_settings(), AudioSettings::default());
    assert_eq!(stored(), AudioSettings::default());

    session.engine_configure(request.clone());
    let rig = rig.restart();
    assert_eq!(rig.session.engine_settings(), request);
}

#[test]
fn processing_threads_persist_and_change_without_reopening_or_moving_playback() {
    let mut rig = Rig::new();
    step_on(&rig, 0, 0);
    rig.session.transport_play().unwrap();
    let _ = rig.run(256);
    let before = rig.session.engine_status();
    let tick = rig.session.controller().playhead().tick;
    let settings = AudioSettings {
        processing_threads: Some(4),
        ..rig.session.engine_settings()
    };
    let status = rig.session.engine_configure(settings.clone());
    assert_eq!(
        status, before,
        "thread-only change must not reopen the stream"
    );
    assert_eq!(rig.session.controller().audio_threads(), 4);
    assert_eq!(rig.session.controller().playhead().tick, tick);
    assert_eq!(rig.session.engine_settings(), settings);
    assert_eq!(
        SettingsStore::load(rig.folder.path().join(SETTINGS_FILE))
            .settings()
            .audio,
        settings
    );
    let refused = rig.session.engine_configure(AudioSettings {
        processing_threads: Some(9),
        ..settings.clone()
    });
    assert!(refused.error.as_deref().unwrap().contains("1 through 8"));
    assert_eq!(rig.session.engine_settings(), settings);
    assert_eq!(rig.session.controller().audio_threads(), 4);
    let rig = rig.restart();
    assert_eq!(rig.session.engine_settings().processing_threads, Some(4));
    assert_eq!(rig.session.controller().audio_threads(), 4);
}
