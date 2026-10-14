//! The `.windfall` file format: saving, loading, backups and sample paths.

use std::fs;
use std::path::{Path, PathBuf};

use windfall_project::file::{
    BACKUP_FOLDER, DEFAULT_BACKUP_COUNT, FILE_EXTENSION, backup_origin, from_json, load,
    load_with_session, resolve_sample_path, sample_dir, sample_path_for, save, save_with,
    session_from_json, to_json, to_json_with, write_backup, write_backup_with,
};
use windfall_project::*;

/// A small project that uses every kind of thing a file can hold.
fn demo_project() -> Project {
    let mut doc = Document::new(Project::new("Demo"));
    let mut run = |command| doc.dispatch(command, None).expect("the command is valid");
    let sample = run(Command::AddSample {
        name: "Kick".to_owned(),
        path: SamplePath::Factory("drums/kick.wav".to_owned()),
    });
    let channel = run(Command::AddChannel {
        name: None,
        sample: Some(SampleId(sample.created[0])),
        instrument: None,
        index: None,
        mixer_track: None,
    });
    run(Command::ToggleStep {
        pattern: PatternId(1),
        channel: ChannelId(channel.created[0]),
        step: 4,
    });
    let track = run(Command::AddPlaylistTrack {
        name: None,
        index: None,
    });
    run(Command::AddClips {
        clips: vec![ClipInit {
            track: PlaylistTrackId(track.created[0]),
            start: 0,
            length: None,
            offset: None,
            muted: None,
            content: ClipContent::Pattern {
                pattern: PatternId(1),
            },
        }],
    });
    doc.project().clone()
}

#[test]
fn sampler_loops_round_trip_and_legacy_defaults_stay_omitted() {
    let legacy = from_json(DEMO_JSON).unwrap();
    let ChannelSource::Sampler(settings) = &legacy.channels[0].source else {
        panic!()
    };
    assert_eq!(settings.loop_mode, SamplerLoopMode::Off);
    assert_eq!((settings.loop_start, settings.loop_end), (0.0, 1.0));
    let json = to_json(&legacy).unwrap();
    assert!(!json.contains("loopMode"));
    assert!(!json.contains("loopStart"));
    assert!(!json.contains("loopEnd"));
    for mode in [SamplerLoopMode::Forward, SamplerLoopMode::PingPong] {
        let mut project = legacy.clone();
        let ChannelSource::Sampler(settings) = &mut project.channels[0].source else {
            panic!()
        };
        settings.loop_mode = mode;
        settings.loop_start = 0.125;
        settings.loop_end = 0.875;
        let json = to_json(&project).unwrap();
        assert_eq!(from_json(&json).unwrap(), project);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("loop.windfall");
        save(&project, &path).unwrap();
        assert_eq!(load(&path).unwrap(), project);
    }
}

#[test]
fn malformed_persisted_sampler_loop_points_are_rejected() {
    for (start, end) in [(0.7, 0.3), (0.5, 0.5), (-0.1, 1.0), (0.0, 1.1)] {
        let mut value: serde_json::Value = serde_json::from_str(DEMO_JSON).unwrap();
        value["channels"][0]["source"]["loopStart"] = start.into();
        value["channels"][0]["source"]["loopEnd"] = end.into();
        assert!(from_json(&value.to_string()).is_err());
    }
}

const DEMO_JSON: &str = r#"{
  "formatVersion": 1,
  "nextId": 8,
  "settings": {
    "name": "Demo",
    "tempoBpm": 120.0,
    "timeSignature": {
      "numerator": 4,
      "denominator": 4
    },
    "swing": 0.0
  },
  "samples": [
    {
      "id": 2,
      "name": "Kick",
      "path": {
        "kind": "factory",
        "path": "drums/kick.wav"
      }
    }
  ],
  "channels": [
    {
      "id": 3,
      "name": "Kick",
      "color": 15026253,
      "volume": 0.8,
      "pan": 0.0,
      "muted": false,
      "solo": false,
      "mixerTrack": 4,
      "source": {
        "type": "sampler",
        "sample": 2,
        "rootKey": 60,
        "tune": 0.0,
        "gain": 1.0,
        "start": 0.0,
        "end": 1.0,
        "reverse": false,
        "envelope": null,
        "cutSelf": false,
        "cutGroup": 0
      }
    }
  ],
  "patterns": [
    {
      "id": 1,
      "name": "Pattern 1",
      "color": 15026253,
      "lengthSteps": 16,
      "lanes": [
        {
          "channel": 3,
          "notes": [
            {
              "id": 5,
              "start": 960,
              "length": 240,
              "key": 60,
              "velocity": 0.8,
              "pan": 0.0
            }
          ]
        }
      ]
    }
  ],
  "mixer": {
    "tracks": [
      {
        "id": 0,
        "name": "Master",
        "color": 9145752,
        "volume": 1.0,
        "pan": 0.0,
        "muted": false,
        "solo": false,
        "output": null,
        "sends": [],
        "effects": []
      },
      {
        "id": 4,
        "name": "Kick",
        "color": 15026253,
        "volume": 1.0,
        "pan": 0.0,
        "muted": false,
        "solo": false,
        "output": 0,
        "sends": [],
        "effects": []
      }
    ]
  },
  "playlist": {
    "tracks": [
      {
        "id": 6,
        "name": "Track 1",
        "muted": false
      }
    ],
    "clips": [
      {
        "id": 7,
        "track": 6,
        "start": 0,
        "length": 3840,
        "offset": 0,
        "muted": false,
        "content": {
          "type": "pattern",
          "pattern": 1
        }
      }
    ]
  },
  "automations": []
}
"#;

// Current writer schema; DEMO_JSON remains the legacy reader fixture.
const CURRENT_DEMO_JSON: &str = r#"{
  "formatVersion": 1,
  "nextId": 8,
  "settings": {
    "name": "Demo",
    "tempoBpm": 120.0,
    "timeSignature": {
      "numerator": 4,
      "denominator": 4
    },
    "swing": 0.0
  },
  "samples": [
    {
      "id": 2,
      "name": "Kick",
      "path": {
        "kind": "factory",
        "path": "drums/kick.wav"
      }
    }
  ],
  "channels": [
    {
      "voice": {
        "arpeggiator": {
          "mode": "off",
          "rate": "sixteenth",
          "gate": 0.75,
          "rangeOctaves": 1
        },
        "echo": {
          "enabled": false,
          "time": {
            "unit": "milliseconds",
            "ms": 250.0
          },
          "feedback": 0.5,
          "pitchSemitones": 0,
          "repeats": 3
        },
        "polyphony": {
          "maxVoices": 32,
          "monoLegato": false,
          "portamentoMs": 0.0
        },
        "envelopes": {
          "filter": {
            "enabled": false,
            "attackMs": 2.0,
            "decayMs": 200.0,
            "sustain": 0.8,
            "releaseMs": 150.0,
            "depth": 0.0
          },
          "pitch": {
            "enabled": false,
            "attackMs": 2.0,
            "decayMs": 200.0,
            "sustain": 0.8,
            "releaseMs": 150.0,
            "depth": 0.0
          },
          "pan": {
            "enabled": false,
            "attackMs": 2.0,
            "decayMs": 200.0,
            "sustain": 0.8,
            "releaseMs": 150.0,
            "depth": 0.0
          },
          "lfo": {
            "enabled": false,
            "shape": "sine",
            "target": "filter",
            "rateHz": 5.0,
            "depth": 0.0
          }
        }
      },
      "id": 3,
      "name": "Kick",
      "color": 15026253,
      "volume": 0.8,
      "pan": 0.0,
      "muted": false,
      "solo": false,
      "mixerTrack": 4,
      "source": {
        "type": "sampler",
        "sample": 2,
        "rootKey": 60,
        "tune": 0.0,
        "gain": 1.0,
        "start": 0.0,
        "end": 1.0,
        "reverse": false,
        "envelope": null,
        "cutSelf": false,
        "cutGroup": 0
      }
    }
  ],
  "patterns": [
    {
      "id": 1,
      "name": "Pattern 1",
      "color": 15026253,
      "lengthSteps": 16,
      "lanes": [
        {
          "channel": 3,
          "notes": [
            {
              "id": 5,
              "start": 960,
              "length": 240,
              "key": 60,
              "velocity": 0.8,
              "pan": 0.0
            }
          ]
        }
      ]
    }
  ],
  "mixer": {
    "tracks": [
      {
        "current": false,
        "id": 0,
        "name": "Master",
        "color": 9145752,
        "volume": 1.0,
        "pan": 0.0,
        "muted": false,
        "solo": false,
        "output": null,
        "sends": [],
        "effects": [],
        "latencyOffsetMs": 0.0
      },
      {
        "current": false,
        "id": 4,
        "name": "Kick",
        "color": 15026253,
        "volume": 1.0,
        "pan": 0.0,
        "muted": false,
        "solo": false,
        "output": 0,
        "sends": [],
        "effects": [],
        "latencyOffsetMs": 0.0
      }
    ]
  },
  "playlist": {
    "tracks": [
      {
        "id": 6,
        "name": "Track 1",
        "muted": false
      }
    ],
    "clips": [
      {
        "id": 7,
        "track": 6,
        "start": 0,
        "length": 3840,
        "offset": 0,
        "muted": false,
        "content": {
          "type": "pattern",
          "pattern": 1
        }
      }
    ]
  },
  "automations": []
}
"#;

#[test]
fn the_json_is_camel_case_and_stable() {
    assert_eq!(to_json(&demo_project()).unwrap(), CURRENT_DEMO_JSON);
}

#[test]
fn the_documented_json_loads_back() {
    assert_eq!(from_json(DEMO_JSON).unwrap(), demo_project());
}

#[test]
fn a_file_from_before_mixer_tracks_had_effects_still_loads() {
    // Nor did it have automations.
    let before = DEMO_JSON
        .replace(
            ",
        \"effects\": []",
            "",
        )
        .replace(
            ",
  \"automations\": []",
            "",
        );
    assert!(!before.contains("effects") && !before.contains("automations"));
    let project = from_json(&before).unwrap();
    assert_eq!(project, demo_project());
    assert!(project.mixer.tracks.iter().all(|t| t.effects.is_empty()));
    // Saved again, it is in the format of today.
    assert_eq!(to_json(&project).unwrap(), CURRENT_DEMO_JSON);
}

#[test]
fn commands_use_camel_case_json_too() {
    let command = Command::AddChannel {
        name: None,
        sample: None,
        instrument: None,
        index: Some(2),
        mixer_track: Some(TrackId(4)),
    };
    assert_eq!(
        serde_json::to_string(&command).unwrap(),
        r#"{"type":"addChannel","name":null,"sample":null,"instrument":null,"index":2,"mixerTrack":4}"#
    );
    let parsed: Command =
        serde_json::from_str(r#"{"type":"toggleStep","pattern":1,"channel":3,"step":4}"#).unwrap();
    assert_eq!(
        parsed,
        Command::ToggleStep {
            pattern: PatternId(1),
            channel: ChannelId(3),
            step: 4,
        }
    );
    let parsed: Command = serde_json::from_str(r#"{"type":"addChannel"}"#).unwrap();
    assert_eq!(
        parsed,
        Command::AddChannel {
            name: None,
            sample: None,
            instrument: None,
            index: None,
            mixer_track: None,
        }
    );
}

#[test]
fn save_then_load_gives_the_same_project() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Demo.windfall");
    let project = demo_project();
    save(&project, &path).unwrap();
    assert_eq!(load(&path).unwrap(), project);
    assert_eq!(fs::read_to_string(&path).unwrap(), CURRENT_DEMO_JSON);
}

fn file_names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn save_replaces_the_file_and_leaves_no_temporary_file() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Song.windfall");
    save(&Project::new("First"), &path).unwrap();
    save(&Project::new("Second"), &path).unwrap();
    assert_eq!(load(&path).unwrap().settings.name, "Second");
    assert_eq!(file_names(folder.path()), ["Song.windfall"]);
}

#[test]
fn save_creates_missing_folders() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("New Song").join("New Song.windfall");
    save(&Project::new("New Song"), &path).unwrap();
    assert_eq!(load(&path).unwrap().settings.name, "New Song");
}

#[test]
fn save_reports_where_it_failed() {
    let folder = tempfile::tempdir().unwrap();
    // A folder is in the way of the file.
    let path = folder.path().join("Taken.windfall");
    fs::create_dir(&path).unwrap();
    let error = save(&Project::new("Song"), &path).unwrap_err();
    assert!(matches!(error, SaveError::Io { .. }));
    assert!(error.to_string().contains("Taken.windfall"));
    assert_eq!(file_names(folder.path()), ["Taken.windfall"]);
}

#[test]
fn a_project_that_breaks_a_rule_is_not_saved() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Song.windfall");
    save(&demo_project(), &path).unwrap();

    let mut broken = demo_project();
    broken.settings.tempo_bpm = 9000.0;
    let error = save(&broken, &path).unwrap_err();
    assert!(matches!(error, SaveError::Invalid(_)), "{error}");
    assert!(
        windfall_core::english::english(&error.to_string()).contains("tempo 9000"),
        "{error}"
    );
    // The good file is still there, and nothing was left beside it.
    assert_eq!(load(&path).unwrap(), demo_project());
    assert_eq!(file_names(folder.path()), ["Song.windfall"]);

    let mut dangling = demo_project();
    dangling.samples.clear();
    let fresh = folder.path().join("Fresh.windfall");
    assert!(matches!(
        save(&dangling, &fresh),
        Err(SaveError::Invalid(_))
    ));
    assert!(!fresh.exists());

    // A backup is a save, and is refused the same way.
    let error = write_backup(&broken, &path, "2026-10-06 18-13-05", 5).unwrap_err();
    assert!(matches!(error, SaveError::Invalid(_)), "{error}");
    assert_eq!(file_names(folder.path()), ["Song.windfall"]);
}

#[test]
fn a_temporary_file_left_by_a_crash_does_not_block_saving() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Song.windfall");
    // The one name every save used to write through, with something in the
    // way that cannot be overwritten.
    let leftover = folder.path().join(".Song.windfall.tmp");
    fs::create_dir(&leftover).unwrap();

    save(&demo_project(), &path).unwrap();
    assert_eq!(load(&path).unwrap(), demo_project());
    assert_eq!(
        file_names(folder.path()),
        [".Song.windfall.tmp", "Song.windfall"]
    );
}

#[test]
fn saves_racing_to_one_file_leave_one_whole_project() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Song.windfall");
    // Projects of very different sizes, so that one written over another
    // would not parse.
    let projects: Vec<Project> = (0..8)
        .map(|writer| {
            let mut project = demo_project();
            project.settings.name = format!("Writer {writer} ").repeat(1 + writer * 200);
            project
        })
        .collect();

    let saved = std::thread::scope(|scope| {
        let writers: Vec<_> = projects
            .iter()
            .map(|project| {
                let path = &path;
                scope.spawn(move || (0..20).filter(|_| save(project, path).is_ok()).count())
            })
            .collect();
        writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .sum::<usize>()
    });
    // Windows can refuse a rename onto a file that another rename is
    // replacing right then. Such a save fails cleanly; none may half work.
    assert!(saved > 0);
    let published = load(&path).unwrap();
    assert!(projects.contains(&published));
}

#[test]
fn load_reports_a_missing_file() {
    let folder = tempfile::tempdir().unwrap();
    let error = load(folder.path().join("Nope.windfall")).unwrap_err();
    assert!(matches!(error, LoadError::Io { .. }));
    assert!(error.to_string().contains("Nope.windfall"));
}

#[test]
fn load_refuses_text_that_is_not_a_project() {
    for text in [
        "",
        "not json",
        "[1, 2, 3]",
        "{}",
        r#"{"formatVersion": "one"}"#,
    ] {
        let error = from_json(text).unwrap_err();
        assert!(matches!(error, LoadError::Parse(_)), "{text:?}: {error}");
        assert!(
            windfall_core::english::english(&error.to_string())
                .starts_with("this is not a Windfall project")
        );
    }
    // The version is right but the rest is missing.
    let error = from_json(r#"{"formatVersion": 1}"#).unwrap_err();
    assert!(matches!(error, LoadError::Parse(_)));
}

#[test]
fn load_refuses_a_file_from_a_newer_version() {
    let newer = DEMO_JSON.replace(r#""formatVersion": 1"#, r#""formatVersion": 2"#);
    let error = from_json(&newer).unwrap_err();
    assert!(matches!(
        error,
        LoadError::TooNew {
            found: 2,
            supported: FORMAT_VERSION
        }
    ));
    assert_eq!(
        windfall_core::english::english(&error.to_string()),
        "this project was saved by a newer version of Windfall (file format 2; this version reads up to format 1)"
    );

    // A newer file may not even have the shape this version knows.
    let error = from_json(r#"{"formatVersion": 99, "tracks": {}}"#).unwrap_err();
    assert!(matches!(error, LoadError::TooNew { found: 99, .. }));
}

#[test]
fn load_refuses_format_version_zero() {
    let zero = DEMO_JSON.replace(r#""formatVersion": 1"#, r#""formatVersion": 0"#);
    let error = from_json(&zero).unwrap_err();
    assert!(matches!(error, LoadError::Invalid(_)), "{error}");
}

#[test]
fn load_refuses_a_project_that_breaks_the_rules_and_says_why() {
    let cases = [
        (
            r#""mixerTrack": 4"#,
            r#""mixerTrack": 40"#,
            "mixer track 40",
        ),
        (
            r#""tempoBpm": 120.0"#,
            r#""tempoBpm": 9000.0"#,
            "tempo 9000",
        ),
        (r#""pattern": 1"#, r#""pattern": 3"#, "plays pattern 3"),
        (r#""length": 240"#, r#""length": 0"#, "has no length"),
        (r#""nextId": 8"#, r#""nextId": 7"#, "not below the next id"),
    ];
    for (from, to, words) in cases {
        assert!(DEMO_JSON.contains(from));
        let error = from_json(&DEMO_JSON.replace(from, to)).unwrap_err();
        assert!(matches!(error, LoadError::Invalid(_)), "{error}");
        let message = windfall_core::english::english(&error.to_string());
        assert!(message.starts_with("the project file is damaged: "));
        assert!(message.contains(words), "\"{message}\" lacks \"{words}\"");
    }
}

#[test]
fn load_ignores_fields_it_does_not_know() {
    let extended = DEMO_JSON.replace(
        r#""nextId": 8,"#,
        r#""nextId": 8, "addedLater": { "anything": [1, 2] },"#,
    );
    assert_eq!(from_json(&extended).unwrap(), demo_project());
}

#[test]
fn a_file_keeps_how_the_project_was_played_beside_the_project() {
    let project = demo_project();
    let played = ProjectSession {
        mode: PlayMode::Song,
        loop_song: true,
        pattern: Some(project.patterns[0].id),
    };

    // The text is the project's own, to the letter, with one key after it.
    let plain = to_json(&project).unwrap();
    assert_eq!(to_json_with(&project, None).unwrap(), plain);
    let text = to_json_with(&project, Some(&played)).unwrap();
    let pattern = project.patterns[0].id.0;
    let block = format!(
        ",\n  \"session\": {{\n    \"mode\": \"song\",\n    \"loopSong\": true,\n    \"pattern\": {pattern}\n  }}\n}}\n"
    );
    assert_eq!(text, format!("{}{block}", plain.trim_end_matches("\n}\n")));

    // It is not part of the project: the same project comes back, with it
    // or without it, and the session comes back beside it.
    assert_eq!(from_json(&text).unwrap(), project);
    assert_eq!(session_from_json(&text), Some(played));
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Song.windfall");
    save_with(&project, Some(&played), &path).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert_eq!(load(&path).unwrap(), project);
    assert_eq!(
        load_with_session(&path).unwrap(),
        (project.clone(), Some(played))
    );
    // A backup holds what the save holds.
    let backup = write_backup_with(&project, Some(&played), &path, "2026-10-06 18-13-05", 5);
    assert_eq!(fs::read_to_string(backup.unwrap()).unwrap(), text);

    // Saved without one, the file is what it always was and has none.
    save(&project, &path).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), plain);
    assert_eq!(load_with_session(&path).unwrap(), (project.clone(), None));
    assert_eq!(session_from_json(DEMO_JSON), None);
}

#[test]
fn a_session_that_cannot_be_read_does_not_stop_the_project_loading() {
    let with = |session: &str| {
        let text = DEMO_JSON.replace(
            r#""nextId": 8,"#,
            &format!(r#""nextId": 8, "session": {session},"#),
        );
        assert_eq!(from_json(&text).unwrap(), demo_project(), "{session}");
        session_from_json(&text)
    };
    // What a later version might write, and what a slip of the hand might.
    for broken in [
        r#"{ "mode": "sideways" }"#,
        r#"{ "loopSong": true }"#,
        r#"{ "mode": "song", "loopSong": "yes" }"#,
        "7",
        "[]",
    ] {
        assert_eq!(with(broken), None, "{broken}");
    }
    assert_eq!(with("null"), None);
    // The mode alone is enough, and keys it does not know are passed over.
    assert_eq!(
        with(r#"{ "mode": "song", "zoom": 2 }"#),
        Some(ProjectSession {
            mode: PlayMode::Song,
            loop_song: false,
            pattern: None,
        })
    );
    assert_eq!(
        with(r#"{ "mode": "pattern", "loopSong": true, "pattern": 99 }"#),
        Some(ProjectSession {
            mode: PlayMode::Pattern,
            loop_song: true,
            pattern: Some(PatternId(99)),
        })
    );
}

fn backup(folder: &Path, name: &str, timestamp: &str, keep: usize) -> PathBuf {
    let project_path = folder.join(format!("{name}.{FILE_EXTENSION}"));
    write_backup(&demo_project(), project_path, timestamp, keep).unwrap()
}

#[test]
fn a_backup_goes_into_the_backup_folder() {
    let folder = tempfile::tempdir().unwrap();
    let path = backup(
        folder.path(),
        "My Song",
        "2026-10-06 18-13-05",
        DEFAULT_BACKUP_COUNT,
    );
    assert_eq!(
        path,
        folder
            .path()
            .join(BACKUP_FOLDER)
            .join("My Song 2026-10-06 18-13-05.windfall")
    );
    assert_eq!(load(&path).unwrap(), demo_project());
    assert_eq!(file_names(folder.path()), ["Backup"]);
}

#[test]
fn only_the_newest_backups_are_kept() {
    let folder = tempfile::tempdir().unwrap();
    let backups = folder.path().join(BACKUP_FOLDER);
    // Written out of order: "newest" goes by the timestamp, not by when the
    // file was made.
    for minute in [3, 1, 5, 2, 4] {
        backup(
            folder.path(),
            "Song",
            &format!("2026-10-06 18-0{minute}-00"),
            10,
        );
    }
    // Things in the same folder that are not this project's backups.
    backup(folder.path(), "Song 2", "2026-10-06 18-00-00", 10);
    backup(folder.path(), "Other", "2026-10-06 18-00-00", 10);
    fs::write(backups.join("Song notes.txt"), "keep me").unwrap();
    fs::write(backups.join("Song 2026-10-06 18-00.windfall"), "{}").unwrap();

    backup(folder.path(), "Song", "2026-10-06 18-06-00", 3);
    assert_eq!(
        file_names(&backups),
        [
            "Other 2026-10-06 18-00-00.windfall",
            "Song 2 2026-10-06 18-00-00.windfall",
            "Song 2026-10-06 18-00.windfall",
            "Song 2026-10-06 18-04-00.windfall",
            "Song 2026-10-06 18-05-00.windfall",
            "Song 2026-10-06 18-06-00.windfall",
            "Song notes.txt",
        ]
    );

    // Asking to keep none still keeps the backup just written.
    backup(folder.path(), "Song", "2026-10-06 18-07-00", 0);
    let left: Vec<String> = file_names(&backups)
        .into_iter()
        .filter(|name| name.starts_with("Song 2026-10-06 18-0") && name.len() > 30)
        .collect();
    assert_eq!(left, ["Song 2026-10-06 18-07-00.windfall"]);
}

#[test]
fn a_backup_with_the_same_timestamp_replaces_the_old_one() {
    let folder = tempfile::tempdir().unwrap();
    backup(folder.path(), "Song", "2026-10-06 18-00-00", 5);
    backup(folder.path(), "Song", "2026-10-06 18-00-00", 5);
    assert_eq!(
        file_names(&folder.path().join(BACKUP_FOLDER)),
        ["Song 2026-10-06 18-00-00.windfall"]
    );
}

#[test]
fn a_backup_timestamp_must_fit_in_a_file_name() {
    let folder = tempfile::tempdir().unwrap();
    let project_path = folder.path().join("Song.windfall");
    for timestamp in ["", "18:13:05", "2026/10/06", "a\\b", "new\nline"] {
        let error = write_backup(&demo_project(), &project_path, timestamp, 5).unwrap_err();
        assert!(matches!(error, SaveError::BadName(_)), "{timestamp:?}");
    }
    assert!(file_names(folder.path()).is_empty());
}

#[test]
fn a_backup_is_byte_for_byte_what_a_save_writes() {
    let folder = tempfile::tempdir().unwrap();
    let project_path = folder.path().join("Song.windfall");
    let mut project = demo_project();
    project.samples[0].path = SamplePath::Project("sounds/kick.wav".to_owned());
    save(&project, &project_path).unwrap();
    let backup = write_backup(&project, &project_path, "2026-10-06 18-13-05", 5).unwrap();
    assert_eq!(fs::read(&backup).unwrap(), fs::read(&project_path).unwrap());

    // Its project samples are found from where the backup is.
    assert_eq!(backup_origin(&backup), Some(project_path.clone()));
    let samples = sample_dir(&backup).unwrap();
    assert_eq!(samples, folder.path());
    let loaded = load(&backup).unwrap();
    assert_eq!(
        resolve_sample_path(
            &loaded.samples[0].path,
            Some(&samples),
            Path::new("factory")
        ),
        Some(folder.path().join("sounds").join("kick.wav"))
    );
}

#[test]
fn a_backup_is_known_by_its_folder_and_its_name() {
    let project = Path::new("music").join("My Song");
    let backups = project.join(BACKUP_FOLDER);
    let origin = |path: PathBuf| backup_origin(&path);

    assert_eq!(
        origin(backups.join("My Song 2026-10-06 18-13-05.windfall")),
        Some(project.join("My Song.windfall"))
    );
    // The name of the project may itself look like part of a timestamp.
    assert_eq!(
        origin(backups.join("Take 2026-10-06 2026-10-06 18-13-05.WINDFALL")),
        Some(project.join("Take 2026-10-06.windfall"))
    );

    // A project file, wherever it is, is not a backup.
    for path in [
        project.join("My Song.windfall"),
        project.join("My Song 2026-10-06 18-13-05.windfall"),
        backups.join("My Song.windfall"),
        backups.join("My Song 2026-10-06 18-13.windfall"),
        backups.join("My Song 2026-10-06T18-13-05.windfall"),
        backups.join("My Song 2026-10-06 18-13-0\u{665}.windfall"),
        backups.join("2026-10-06 18-13-05.windfall"),
        backups.join(" 2026-10-06 18-13-05.windfall"),
        backups.join("My Song 2026-10-06 18-13-05.wav"),
        backups
            .join("deeper")
            .join("My Song 2026-10-06 18-13-05.windfall"),
        PathBuf::from("My Song 2026-10-06 18-13-05.windfall"),
    ] {
        assert_eq!(backup_origin(&path), None, "{}", path.display());
        assert_eq!(
            sample_dir(&path).as_deref(),
            path.parent(),
            "{}",
            path.display()
        );
    }
    assert_eq!(
        sample_dir(&backups.join("My Song 2026-10-06 18-13-05.windfall")),
        Some(project)
    );
}

#[test]
fn sample_paths_resolve_against_their_folders() {
    let project = Path::new("projects").join("Song");
    let factory = Path::new("content").join("factory");
    let resolve = |sample: SamplePath| resolve_sample_path(&sample, Some(&project), &factory);

    assert_eq!(
        resolve(SamplePath::Factory("drums/kick.wav".to_owned())),
        Some(factory.join("drums").join("kick.wav"))
    );
    assert_eq!(
        resolve(SamplePath::Project("samples/take 1.wav".to_owned())),
        Some(project.join("samples").join("take 1.wav"))
    );
    assert_eq!(
        resolve(SamplePath::External("/home/me/snare.wav".to_owned())),
        Some(PathBuf::from("/home/me/snare.wav"))
    );

    // A project that was never saved has no folder for project samples.
    let unsaved = SamplePath::Project("samples/take 1.wav".to_owned());
    assert_eq!(resolve_sample_path(&unsaved, None, &factory), None);
    let shipped = SamplePath::Factory("drums/kick.wav".to_owned());
    assert!(resolve_sample_path(&shipped, None, &factory).is_some());
}

#[test]
fn stored_paths_cannot_leave_their_folder() {
    let project = Path::new("projects").join("Song");
    let factory = Path::new("content").join("factory");
    let escapes = [
        "",
        "../secret.wav",
        "samples/../../secret.wav",
        "/etc/passwd",
        "./kick.wav",
        "samples//kick.wav",
        "samples\\kick.wav",
    ];
    for path in escapes {
        for sample in [
            SamplePath::Project(path.to_owned()),
            SamplePath::Factory(path.to_owned()),
        ] {
            assert_eq!(
                resolve_sample_path(&sample, Some(&project), &factory),
                None,
                "{sample:?}"
            );
            assert!(sample.problem().is_some(), "{sample:?}");
        }
    }
    if cfg!(windows) {
        let drive = SamplePath::Project("C:/Windows/kick.wav".to_owned());
        assert_eq!(resolve_sample_path(&drive, Some(&project), &factory), None);
    }
}

#[test]
fn a_file_is_stored_relative_to_the_folder_it_is_inside() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("projects").join("Song");
    let factory = root.path().join("content").join("factory");
    let store = |file: &Path| sample_path_for(file, Some(&project), &factory);

    let inside_project = project.join("samples").join("take 1.wav");
    assert_eq!(
        store(&inside_project),
        SamplePath::Project("samples/take 1.wav".to_owned())
    );
    let inside_factory = factory.join("drums").join("808").join("kick.wav");
    assert_eq!(
        store(&inside_factory),
        SamplePath::Factory("drums/808/kick.wav".to_owned())
    );
    let elsewhere = root.path().join("downloads").join("snare.wav");
    assert_eq!(
        store(&elsewhere),
        SamplePath::External(elsewhere.to_string_lossy().into_owned())
    );
    // A folder whose name merely starts the same way is not inside.
    let sibling = root.path().join("projects").join("Song 2").join("kick.wav");
    assert!(matches!(store(&sibling), SamplePath::External(_)));
    // The folder itself is not a file inside it.
    assert!(matches!(store(&project), SamplePath::External(_)));

    // Without a project folder, project files are external.
    assert!(matches!(
        sample_path_for(&inside_project, None, &factory),
        SamplePath::External(_)
    ));

    // Storing and resolving are inverses.
    for file in [&inside_project, &inside_factory, &elsewhere] {
        let stored = store(file);
        assert_eq!(stored.problem(), None);
        assert_eq!(
            resolve_sample_path(&stored, Some(&project), &factory).as_ref(),
            Some(file)
        );
    }
}

#[test]
fn the_project_folder_wins_over_a_factory_folder_around_it() {
    let factory = Path::new("content");
    let project = factory.join("demo songs").join("Song");
    let file = project.join("kick.wav");
    assert_eq!(
        sample_path_for(&file, Some(&project), factory),
        SamplePath::Project("kick.wav".to_owned())
    );
}

#[cfg(windows)]
#[test]
fn folder_names_match_without_regard_to_case_on_windows() {
    let stored = sample_path_for(
        Path::new(r"C:\Music\SONG\Samples\Kick.wav"),
        Some(Path::new(r"c:\music\Song")),
        Path::new(r"C:\Factory"),
    );
    assert_eq!(stored, SamplePath::Project("Samples/Kick.wav".to_owned()));
}

/// A project file as Windfall wrote it before there were audio clips,
/// automation, or effects on mixer tracks: no `automations`, no `effects`,
/// and clips that can only play patterns.
const OLDER_FILE: &str = r#"{
  "formatVersion": 1,
  "nextId": 9,
  "settings": {
    "name": "Older",
    "tempoBpm": 97.5,
    "timeSignature": { "numerator": 3, "denominator": 4 },
    "swing": 0.25
  },
  "samples": [
    { "id": 2, "name": "kick", "path": { "kind": "factory", "path": "Drums/Kicks/Kick Punch.wav" } }
  ],
  "channels": [
    {
      "id": 3, "name": "Kick", "color": 15026253, "volume": 0.8, "pan": 0.0,
      "muted": false, "solo": false, "mixerTrack": 4,
      "source": {
        "type": "sampler", "sample": 2, "rootKey": 60, "tune": 0.0, "gain": 1.0,
        "start": 0.0, "end": 1.0, "reverse": false, "envelope": null,
        "cutSelf": false, "cutGroup": 0
      }
    }
  ],
  "patterns": [
    {
      "id": 1, "name": "Pattern 1", "color": 15026253, "lengthSteps": 16,
      "lanes": [
        { "channel": 3, "notes": [
          { "id": 5, "start": 0, "length": 240, "key": 60, "velocity": 0.8, "pan": 0.0 }
        ] }
      ]
    }
  ],
  "mixer": {
    "tracks": [
      { "id": 0, "name": "Master", "color": 9145752, "volume": 1.0, "pan": 0.0,
        "muted": false, "solo": false, "output": null, "sends": [] },
      { "id": 4, "name": "Kick", "color": 15026253, "volume": 1.0, "pan": 0.0,
        "muted": false, "solo": false, "output": 0, "sends": [] }
    ]
  },
  "playlist": {
    "tracks": [ { "id": 6, "name": "Track 1", "muted": false } ],
    "clips": [
      { "id": 7, "track": 6, "start": 0, "length": 3840, "offset": 0, "muted": false,
        "content": { "type": "pattern", "pattern": 1 } },
      { "id": 8, "track": 6, "start": 3840, "length": 1920, "offset": 480, "muted": true,
        "content": { "type": "pattern", "pattern": 1 } }
    ]
  }
}"#;

#[test]
fn a_file_from_before_audio_clips_and_automation_still_loads() {
    let project = from_json(OLDER_FILE).unwrap();
    assert_eq!(project.settings.name, "Older");
    assert_eq!(project.settings.tempo_bpm, 97.5);
    assert!(project.automations.is_empty());
    assert!(project.mixer.tracks.iter().all(|t| t.effects.is_empty()));
    let clips = &project.playlist.clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(
        clips[1].content,
        ClipContent::Pattern {
            pattern: PatternId(1)
        }
    );
    assert_eq!((clips[1].offset, clips[1].muted), (480, true));

    // It can be edited with everything there is now, and what is written
    // back loads again as the same project.
    let mut doc = Document::new(project);
    let added = doc
        .dispatch(
            Command::AddAutomation {
                name: None,
                target: AutomationTarget::TrackVolume { track: TrackId(4) },
                points: None,
            },
            None,
        )
        .unwrap();
    let clips = vec![
        ClipInit {
            track: PlaylistTrackId(6),
            start: 0,
            length: None,
            offset: None,
            muted: None,
            content: ClipContent::Automation {
                automation: AutomationId(added.created[0]),
            },
        },
        ClipInit {
            track: PlaylistTrackId(6),
            start: 960,
            length: Some(1_000),
            offset: None,
            muted: None,
            content: ClipContent::Audio {
                sample: SampleId(2),
                mixer_track: TrackId(4),
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
    ];
    doc.dispatch(Command::AddClips { clips }, None).unwrap();
    let json = to_json(doc.project()).unwrap();
    assert!(json.contains("\"automations\""));
    assert_eq!(&from_json(&json).unwrap(), doc.project());
}
