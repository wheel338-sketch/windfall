//! Authored acceptance harness; the implementation worker does not execute it.
use super::*;
use crate::EmbeddedSample;
use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::events::{clap_event_header, clap_input_events};
use windfall_project::{
    Channel, ChannelId, ChannelSource, Clip, ClipContent, ClipId, ExternalOutputRoute, Lane, Note,
    NoteId, PlaylistTrack, PlaylistTrackId, SampleAsset, SampleId, SamplePath, SamplerSettings,
    TrackId,
};

thread_local! {
    static METER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}
struct Metered;
#[global_allocator]
static ALLOCATOR: Metered = Metered;
fn count_memory() {
    let _ = METER.try_with(|meter| {
        if let Some(count) = meter.get() {
            meter.set(Some(count + 1));
        }
    });
}
unsafe impl std::alloc::GlobalAlloc for Metered {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_memory();
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        count_memory();
        unsafe {
            std::alloc::System.dealloc(ptr, layout);
        }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        count_memory();
        unsafe { std::alloc::System.realloc(ptr, layout, size) }
    }
}
fn realtime<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            METER.with(|meter| meter.set(None));
        }
    }
    METER.with(|meter| meter.set(Some(0)));
    let guard = Guard;
    let result = operation();
    let count = METER.with(|meter| meter.get().unwrap());
    drop(guard);
    (result, count)
}

fn program() -> Program {
    let mut program = Program::default();
    program.project.next_id = 20;
    program.project.samples.push(SampleAsset {
        soundfont: false,
        regions: Vec::new(),
        processing: None,
        id: SampleId(2),
        name: "source".into(),
        path: SamplePath::Project("source.wav".into()),
    });
    program.samples.push(EmbeddedSample {
        id: SampleId(2),
        sample_rate: 48_000,
        channels: 2,
        data: (0..4096)
            .flat_map(|frame| {
                let value = 0.125 + (frame % 31) as f32 / 64.0;
                [value, -value]
            })
            .collect(),
    });
    program.project.channels.push(Channel {
        sample_instrument: None,
        sample_playback: None,
        id: ChannelId(3),
        name: "sampler".into(),
        group: String::new(),
        timing: Default::default(),
        voice: Default::default(),
        expression_map: Default::default(),
        color: 0,
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        mixer_track: TrackId::MASTER,
        source: ChannelSource::Sampler(SamplerSettings {
            sample: Some(SampleId(2)),
            ..Default::default()
        }),
    });
    // Project output route chooses port 2, not merely a declared but silent bus.
    program.project.mixer.tracks[0].external_output = Some(ExternalOutputRoute {
        left: 2,
        right: Some(3),
        exclusive: false,
    });
    program.project.patterns[0].lanes.push(Lane {
        channel: ChannelId(3),
        notes: vec![Note {
            id: NoteId(4),
            start: 960,
            length: 480,
            key: windfall_project::DEFAULT_KEY,
            velocity: 1.0,
            pan: 0.0,
            expression: Default::default(),
        }],
    });
    program.project.playlist.tracks.push(PlaylistTrack {
        id: PlaylistTrackId(5),
        name: "Song".into(),
        muted: false,
        solo: false,
        color: 0,
        height: 0,
    });
    program.project.playlist.clips.push(Clip {
        id: ClipId(6),
        track: PlaylistTrackId(5),
        start: 0,
        length: 3840,
        offset: 0,
        muted: false,
        content: ClipContent::Pattern {
            pattern: program.project.patterns[0].id,
        },
    });
    program.validate().unwrap();
    program
}

#[test]
fn hard_reset_flushes_sampler_effect_and_compensation_audio_without_memory_calls() {
    let mut program = program();
    program.project.mixer.tracks[0]
        .effects
        .push(windfall_project::EffectSlot {
            impulse: None,
            id: windfall_project::EffectId(21),
            enabled: true,
            mix: 1.0,
            params: windfall_project::EffectParams::Delay(windfall_dsp::DelayParams {
                sync: false,
                time_ms: 10.0,
                feedback: 0.8,
                mix: 0.5,
                ..Default::default()
            }),
        });
    program.project.next_id = 22;
    let mut engine = GuestEngine::activate(&program, 48_000.0).unwrap();
    engine.midi(0, windfall_project::DEFAULT_KEY, 127);
    let mut energy = 0.0f64;
    for _ in 0..8 {
        energy += engine
            .render(256)
            .unwrap()
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>();
    }
    assert!(
        energy > 1.0,
        "real voice and delay must be excited before reset"
    );
    let (_, memory) = realtime(|| engine.reset());
    assert_eq!(memory, 0);
    for _ in 0..16 {
        let (peak, memory) = realtime(|| {
            engine
                .render(256)
                .unwrap()
                .iter()
                .fold(0.0f32, |peak, v| peak.max(v.abs()))
        });
        assert_eq!(memory, 0);
        assert_eq!(peak, 0.0, "reset left a voice, filter or delay tail");
    }
    let (_, memory) = realtime(|| {
        engine.reset();
        engine.reset();
    });
    assert_eq!(memory, 0);
}
#[test]
fn host_meter_override_is_transient_checked_and_preserves_saved_document() {
    let program = program();
    let bytes = program.encode().unwrap();
    let mut engine = GuestEngine::activate(&program, 48_000.0).unwrap();
    assert!(engine.sync_meter(7, 8));
    assert_eq!(engine.meter(), (7, 8));
    assert!(!engine.sync_meter(4, 3));
    assert_eq!(engine.meter(), (7, 8));
    assert!(engine.sync_meter(0, 0));
    assert_eq!(engine.meter(), (7, 8));
    assert_eq!(program.encode().unwrap(), bytes);
}

#[test]
fn host_clock_places_note_on_exact_frame_and_bends_tempo_without_restarting() {
    let program = program();
    for (tempo, onset) in [(120.0, 240usize), (240.0, 120)] {
        let mut engine = GuestEngine::activate(&program, 48_000.0).unwrap();
        assert!(engine.sync_transport(0.99, tempo, true));
        let pcm = engine.render(256).unwrap();
        assert!(
            pcm.as_chunks::<OUTPUT_CHANNELS>()
                .0
                .iter()
                .take(onset)
                .all(|frame| frame[2] == 0.0)
        );
        assert!((pcm[onset * OUTPUT_CHANNELS + 2] - 0.125).abs() < 1e-6);
        assert_eq!(
            pcm[onset * OUTPUT_CHANNELS + 3],
            -pcm[onset * OUTPUT_CHANNELS + 2]
        );
        assert!(
            pcm.as_chunks::<OUTPUT_CHANNELS>()
                .0
                .iter()
                .all(|frame| frame[0] == 0.0 && frame[1] == 0.0)
        );
    }
    let mut engine = GuestEngine::activate(&program, 48_000.0).unwrap();
    assert!(engine.sync_transport(0.99, 120.0, true));
    assert!(
        engine
            .render(120)
            .unwrap()
            .iter()
            .all(|sample| *sample == 0.0)
    );
    assert!(engine.sync_transport(0.995, 240.0, true));
    let pcm = engine.render(100).unwrap();
    let onset = pcm
        .as_chunks::<OUTPUT_CHANNELS>()
        .0
        .iter()
        .position(|frame| frame[2] != 0.0)
        .unwrap();
    assert_eq!(onset, 60);
    assert!(!engine.sync_transport(0.0, f64::NAN, true));
}

#[test]
fn embedded_state_restores_identical_multibus_pcm_and_host_sample_rate() {
    let program = program();
    let restored = Program::decode(&program.encode().unwrap()).unwrap();
    assert_eq!(restored.project, program.project);
    assert_eq!(restored.samples[0].data, program.samples[0].data);
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut a = GuestEngine::activate(&program, rate).unwrap();
        let mut b = GuestEngine::activate(&restored, rate).unwrap();
        assert_eq!(a.sample_rate(), rate as u32);
        a.midi(0, windfall_project::DEFAULT_KEY, 127);
        b.midi(0, windfall_project::DEFAULT_KEY, 127);
        for frames in [1, 7, 64, 256, 13] {
            assert_eq!(a.render(frames), b.render(frames));
        }
    }
    let mut corrupt = restored;
    corrupt.samples[0].data[0] = f32::NAN;
    assert!(corrupt.validate().is_err());
}

unsafe extern "C" fn host_extension(_: *const clap_host, _: *const c_char) -> *const c_void {
    ptr::null()
}
unsafe extern "C" fn host_request(_: *const clap_host) {}
fn host() -> clap_host {
    clap_host {
        clap_version: CLAP_VERSION,
        host_data: ptr::null_mut(),
        name: c"Acceptance host".as_ptr(),
        vendor: c"Windfall".as_ptr(),
        url: c"".as_ptr(),
        version: c"1".as_ptr(),
        get_extension: Some(host_extension),
        request_restart: Some(host_request),
        request_process: Some(host_request),
        request_callback: Some(host_request),
    }
}

struct Source {
    bytes: Vec<u8>,
    offset: usize,
    quantum: usize,
}
unsafe extern "C" fn read(stream: *const clap_istream, data: *mut c_void, count: u64) -> i64 {
    let source = unsafe { &mut *((*stream).ctx as *mut Source) };
    let count = (count as usize)
        .min(source.quantum)
        .min(source.bytes.len() - source.offset);
    unsafe {
        ptr::copy_nonoverlapping(source.bytes.as_ptr().add(source.offset), data.cast(), count);
    }
    source.offset += count;
    count as i64
}
struct Destination {
    bytes: Vec<u8>,
    fail: bool,
}
unsafe extern "C" fn write(stream: *const clap_ostream, data: *const c_void, count: u64) -> i64 {
    let target = unsafe { &mut *((*stream).ctx as *mut Destination) };
    if target.fail {
        return -1;
    }
    let count = (count as usize).min(17);
    target
        .bytes
        .extend_from_slice(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), count) });
    count as i64
}
struct Events {
    event: clap_event_midi,
}
unsafe extern "C" fn size(_: *const clap_input_events) -> u32 {
    1
}
unsafe extern "C" fn get(events: *const clap_input_events, index: u32) -> *const clap_event_header {
    if index != 0 {
        return ptr::null();
    }
    let events = unsafe { &*((*events).ctx as *const Events) };
    &events.event.header
}

#[test]
fn clap_boundary_handles_partial_state_streams_sample_offsets_and_lifecycle() {
    let host = host();
    let plugin = unsafe { create(&FACTORY, &host, ID.as_ptr()) };
    assert!(!plugin.is_null());
    assert!(unsafe { init(plugin) });
    let mut source = Source {
        bytes: program().encode().unwrap(),
        offset: 0,
        quantum: 19,
    };
    let input = clap_istream {
        ctx: (&mut source as *mut Source).cast(),
        read: Some(read),
    };
    assert!(unsafe { load(plugin, &input) });
    let mut destination = Destination {
        bytes: Vec::new(),
        fail: false,
    };
    let output = clap_ostream {
        ctx: (&mut destination as *mut Destination).cast(),
        write: Some(write),
    };
    assert!(unsafe { save(plugin, &output) });
    assert_eq!(destination.bytes, source.bytes);
    destination.fail = true;
    assert!(!unsafe { save(plugin, &output) });
    assert!(unsafe { activate(plugin, 48_000.0, 1, 512) });
    source.offset = 0;
    assert!(
        unsafe { load(plugin, &input) },
        "active load prepares a complete replacement"
    );
    source.offset = 0;
    assert!(
        !unsafe { load(plugin, &input) },
        "one publication must settle before another"
    );
    assert!(unsafe { start(plugin) });
    let mut pcm = [[[0.0f32; 64]; 2]; OUTPUT_PORTS];
    let mut channels: [[*mut f32; 2]; OUTPUT_PORTS] =
        std::array::from_fn(|port| [pcm[port][0].as_mut_ptr(), pcm[port][1].as_mut_ptr()]);
    let mut buffers: [clap_audio_buffer; OUTPUT_PORTS] =
        std::array::from_fn(|port| clap_audio_buffer {
            data32: channels[port].as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        });
    let events = Events {
        event: clap_event_midi {
            header: clap_event_header {
                size: size_of::<clap_event_midi>() as u32,
                time: 7,
                space_id: CLAP_CORE_EVENT_SPACE_ID,
                type_: CLAP_EVENT_MIDI,
                flags: 0,
            },
            port_index: 0,
            data: [0x90, windfall_project::DEFAULT_KEY, 127],
        },
    };
    let input_events = clap_input_events {
        ctx: (&events as *const Events).cast_mut().cast(),
        size: Some(size),
        get: Some(get),
    };
    let block = clap_process {
        steady_time: 0,
        frames_count: 64,
        transport: ptr::null(),
        audio_inputs: ptr::null(),
        audio_outputs: buffers.as_mut_ptr(),
        audio_inputs_count: 0,
        audio_outputs_count: OUTPUT_PORTS as u32,
        in_events: &input_events,
        out_events: ptr::null(),
    };
    let (status, memory_calls) = realtime(|| unsafe { process(plugin, &block) });
    assert_eq!(status, CLAP_PROCESS_CONTINUE);
    assert_eq!(
        memory_calls, 0,
        "CLAP process must allocate and free nothing"
    );
    assert!(pcm[0].iter().flatten().all(|sample| *sample == 0.0));
    assert!(pcm[1][0][..7].iter().all(|sample| *sample == 0.0));
    for (index, (&left, &right)) in pcm[1][0].iter().zip(&pcm[1][1]).enumerate().skip(7) {
        let expected = 0.125 + ((index - 7) % 31) as f32 / 64.0;
        assert!((left - expected).abs() < 1e-6, "frame {index}");
        assert_eq!(right, -left);
    }
    unsafe {
        stop(plugin);
        deactivate(plugin);
        destroy(plugin);
    }
}
