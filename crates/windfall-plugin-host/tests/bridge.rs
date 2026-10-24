//! Real helper processes and callback allocator/time guards. Windows headless
//! evidence does not establish packaged installation or hardware deadlines.
#![cfg(windows)]
mod common;
use std::io::{BufRead, Read, Write};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};
use windfall_plugin_host::{
    PluginHost, PluginState,
    bridge::{
        adapter::{Audio, OfflineError, ParameterSpec, Signals},
        protocol::{Config, Identity, Kind},
        slots::{LocalWords, Region},
        supervisor::{self, Control, Launch},
    },
};

thread_local! {
    static WATCH: Cell<bool> = const { Cell::new(false) };
    static CALLS: Cell<usize> = const { Cell::new(0) };
}
struct Allocator;
fn count() {
    if WATCH.try_with(Cell::get).unwrap_or(false) {
        let _ = CALLS.try_with(|calls| calls.set(calls.get() + 1));
    }
}
// SAFETY: unchanged System allocation contract; counters are thread-local.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count();
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn guarded(work: impl FnOnce()) -> usize {
    CALLS.set(0);
    WATCH.set(true);
    work();
    WATCH.set(false);
    CALLS.get()
}
fn fixture(format: &str) -> PathBuf {
    if let Some(source) = std::env::var_os("WINDFALL_BRIDGE_FIXTURE") {
        // Publish each shared DLL completely before another test can load it.
        // Windows prevents overwriting a DLL once any helper has mapped it.
        static COPY: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _copy = COPY.lock().unwrap();
        let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("bridge-fixture-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let target = folder.join(format!("bridge.{format}"));
        if !target.exists() {
            std::fs::copy(source, &target).unwrap();
        }
        target
    } else {
        common::plugin_file("bridge-fixture", &format!("bridge.{format}"))
    }
}
fn options(
    format: &str,
    id: &str,
    kind: Kind,
    parameters: Vec<ParameterSpec>,
    audio_timeout: Duration,
) -> Launch {
    let identity = supervisor::fresh_identity(1, 7, 99).unwrap();
    Launch {
        wrapper: Default::default(),
        helper: PathBuf::from(env!("CARGO_BIN_EXE_windfall-plugin-audio")),
        plugin: fixture(format),
        id: id.into(),
        format: format.into(),
        config: Config {
            identity,
            sample_rate: 48_000,
            block: 64,
            native_latency: 0,
            kind,
        },
        approved_binary: windfall_plugin_host::paths::plugin_file_identity(&fixture(format))
            .unwrap(),
        parameters,
        state: vec![],
        offline: false,
        startup_timeout: Duration::from_secs(5),
        audio_timeout,
        cancelled: None,
    }
}
fn launch(
    format: &str,
    id: &str,
    kind: Kind,
    parameters: Vec<ParameterSpec>,
    audio_timeout: Duration,
) -> (Control, Audio) {
    let options = options(format, id, kind, parameters, audio_timeout);
    let (control, audio, _) = supervisor::launch(options).unwrap();
    (control, audio)
}
fn gain(format: &str) -> ParameterSpec {
    ParameterSpec {
        id: 7,
        min: 0.0,
        max: if format == "clap" { 2.0 } else { 1.0 },
        value: 0.5,
        read_only: false,
        stepped: false,
    }
}
fn vst_id(index: usize) -> String {
    format!("{:032X}", index + 1)
}
fn wait_failed(control: &Control) {
    let end = Instant::now() + Duration::from_secs(3);
    while !control.status().reaped {
        assert!(
            Instant::now() < end,
            "helper was not reclaimed: {:?}",
            control.status()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(control.status().failed);
}

#[test]
fn fresh_helper_describes_native_metadata_state_and_refuses_playback_disruption() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            common::GAIN.to_owned()
        } else {
            vst_id(0)
        };
        let (control, mut audio) = launch(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(2),
        );
        let (state, metadata, layout) = control.describe(Duration::from_secs(2)).unwrap();
        let host = PluginHost::windfall();
        let module = host.load(&fixture(format)).unwrap();
        let mut reference = module.create(&id).unwrap();
        reference
            .load_state(&PluginState::from_bytes(state))
            .unwrap();
        assert_eq!(layout, *reference.layout());
        let expected: Vec<_> = reference
            .params()
            .iter()
            .filter(|param| !param.hidden)
            .cloned()
            .collect();
        assert_eq!(metadata.len(), expected.len());
        for (received, native) in metadata.iter().zip(expected) {
            assert_eq!(received.spec.id, native.id);
            assert_eq!(received.name, native.name);
            assert_eq!(received.automatable, native.automatable);
            assert_eq!(
                (received.spec.min, received.spec.max),
                (native.min, native.max)
            );
            assert_eq!(
                (received.spec.read_only, received.spec.stepped),
                (native.read_only, native.stepped)
            );
            assert_eq!(
                received.spec.value,
                reference.param_value(native.id).unwrap_or(native.default)
            );
        }
        let mut left = [1.0; 64];
        let mut right = left;
        for _ in 0..8 {
            left.fill(1.0);
            right.fill(1.0);
            audio.process(&mut left, &mut right);
            std::thread::sleep(Duration::from_millis(3));
        }
        assert!(audio.health().completed_blocks > 0);
        assert!(
            control
                .describe(Duration::from_secs(2))
                .unwrap_err()
                .contains("fresh unprocessed")
        );
        assert!(
            !control.status().failed,
            "refused inspection must not retire playback"
        );
        let captured = control
            .capture(
                1,
                audio.desired_generation(),
                &[gain(format)],
                Duration::from_secs(2),
            )
            .unwrap();
        assert!(!captured.state.is_empty());
    }
}

#[test]
fn callback_guards_cover_startup_full_late_reset_saturation_and_failed_removal() {
    let region = Region::initialize(
        LocalWords::new(),
        Config {
            identity: Identity {
                session: 1,
                token: 1,
                revision: 1,
                binding: 1,
            },
            sample_rate: 48_000,
            block: 256,
            native_latency: 17,
            kind: Kind::Effect,
        },
    )
    .unwrap();
    let signals = Arc::new(Signals::default());
    let mut audio = Audio::new(region, signals.clone(), &[gain("clap")]).unwrap();
    let mut left = [0.25; 512];
    let mut right = left;
    let mut max_callback = Duration::ZERO;
    let mut total = Duration::ZERO;
    let mut callbacks = 0;
    // Calibration proves this counter observes alloc, realloc and free.
    assert!(
        guarded(|| {
            let mut value = Vec::with_capacity(1);
            value.push(1);
            value.reserve(100);
            std::hint::black_box(value);
        }) >= 3
    );
    for frames in [1, 7, 64, 480, 512] {
        for _ in 0..64 {
            let started = Instant::now();
            let calls = guarded(|| {
                audio.set_param(7, 0.75);
                audio.note_on(60, 1.0);
                audio.note_off(60);
                audio.process(&mut left[..frames], &mut right[..frames]);
            });
            let elapsed = started.elapsed();
            max_callback = max_callback.max(elapsed);
            total += elapsed;
            callbacks += 1;
            assert_eq!(calls, 0);
            assert!(left[..frames].iter().all(|sample| sample.is_finite()));
        }
    }
    assert_eq!(
        guarded(|| {
            for _ in 0..1024 {
                audio.note_on(60, 1.0);
            }
            for _ in 0..8 {
                for key in 0..128 {
                    audio.note_off(key);
                }
                audio.all_notes_off();
            }
            audio.reset_timeline();
            signals.failed.store(true, Ordering::Release);
            audio.process(&mut left, &mut right);
        }),
        0
    );
    // Actual heap/map retirement remains control-side, outside the guard.
    drop(audio);
    eprintln!(
        "bridge callback headless guard: {callbacks} calls, max={}us average={}us, allocator/free calls=0",
        max_callback.as_micros(),
        total.as_micros() / callbacks
    );
}

#[test]
fn native_vst3_editor_report_confirms_visible_window_and_close_in_helper() {
    let (control, _audio) = launch(
        "vst3",
        &vst_id(30),
        Kind::Effect,
        vec![gain("vst3")],
        Duration::from_secs(2),
    );
    control
        .editor_with_options(
            true,
            windfall_plugin_host::EditorOptions {
                take_focus: false,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(control.editor_report().unwrap().0);
    let report = control.editor_diagnostics().unwrap();
    assert_eq!(report["open"], true);
    assert_eq!(report["visible"], true);
    assert_eq!(report["width"], 320);
    assert!(report["bounds"].is_array());
    control.editor(false).unwrap();
    assert!(!control.editor_report().unwrap().0);
    assert_eq!(control.editor_diagnostics().unwrap()["open"], false);
}

#[test]
fn native_clap_vst3_healthy_audio_and_state_use_independent_helpers() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            common::GAIN.to_owned()
        } else {
            vst_id(0)
        };
        let (control, mut audio) = launch(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(2),
        );
        let (other, mut independent) = launch(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(2),
        );
        assert_ne!(control.status().process_id, other.status().process_id);
        assert_eq!(audio.latency(), if format == "clap" { 128 } else { 145 });
        control.editor(true).unwrap();
        control.editor(false).unwrap();
        assert!(audio.set_param(7, 0.75));
        let mut left = [1.0; 64];
        let mut right = left;
        for _ in 0..8 {
            left.fill(1.0);
            right.fill(1.0);
            assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
            std::thread::sleep(Duration::from_millis(3));
        }
        assert!(audio.health().completed_blocks >= 4, "{:?}", audio.health());
        assert!(audio.health().acknowledged_generation >= 2);
        let expected = if format == "clap" { 0.75 } else { 1.5 };
        assert!(
            left.iter().all(|sample| (*sample - expected).abs() < 1e-7),
            "{format}: {left:?}"
        );
        let captured = control
            .capture(
                1,
                audio.desired_generation(),
                &[ParameterSpec {
                    value: 0.75,
                    ..gain(format)
                }],
                Duration::from_secs(2),
            )
            .unwrap();
        assert!(!captured.state.is_empty());
        assert!(control.last_valid_state().is_some());
        assert!((captured.parameters[0].value - 0.75).abs() < 1e-9);
        let host = PluginHost::windfall();
        let module = host.load(&fixture(format)).unwrap();
        let mut reference = module.create(&id).unwrap();
        reference
            .load_state(&PluginState::from_bytes(captured.state))
            .unwrap();
        assert!((reference.param_value(7).unwrap() - 0.75).abs() < 1e-9);
        let mut native = reference.activate(48_000.0, 64).unwrap();
        left.fill(1.0);
        right.fill(1.0);
        native.process(&mut left, &mut right);
        assert!(left.iter().all(|sample| (*sample - expected).abs() < 1e-7));
        reference.deactivate(native).unwrap();
        // Retiring one process does not end the other owner's process/audio.
        drop(audio);
        drop(control);
        for _ in 0..5 {
            left.fill(1.0);
            right.fill(1.0);
            independent.process(&mut left, &mut right);
            std::thread::sleep(Duration::from_millis(3));
        }
        assert!(!other.status().failed);
        assert!(independent.health().completed_blocks > 0);
        drop(independent);
        drop(other);
    }
}

#[test]
fn unprocessed_capture_intent_is_separate_from_native_dsp_acknowledgement() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            common::GAIN.to_owned()
        } else {
            vst_id(0)
        };
        let (control, mut audio) = launch(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(2),
        );
        assert!(audio.set_param(7, 0.75));
        let pending = [ParameterSpec {
            value: 0.75,
            ..gain(format)
        }];
        let captured = control
            .capture(
                1,
                audio.desired_generation(),
                &pending,
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(captured.processed_generation, 0);
        assert_eq!(audio.health().acknowledged_generation, 0);
        assert_eq!(captured.parameters[0].value, 0.75);
        // Neither format is deactivated for a capture any more, so neither
        // reconciles the pending value into the plugin: it stays intent.
        assert_eq!(captured.reconciled_generation, 0, "{format}");
        let host = PluginHost::windfall();
        let module = host.load(&fixture(format)).unwrap();
        let mut restored = module.create(&id).unwrap();
        restored
            .load_state(&PluginState::from_bytes(captured.state.clone()))
            .unwrap();
        // Opaque state remains actual native state; the pending value is a
        // separate restore companion (asserted above), for VST3 as for CLAP
        // since a capture stopped deactivating the plugin.
        assert_eq!(restored.param_value(7).unwrap(), 0.5, "{format}");
        let malformed = [ParameterSpec {
            id: 123,
            ..pending[0]
        }];
        assert!(
            control
                .capture(1, 2, &malformed, Duration::from_secs(2))
                .is_err()
        );
        assert_eq!(control.last_valid_state().unwrap().state, captured.state);
        assert!(!control.status().failed);
        drop(audio);
        drop(control);
    }
}

#[test]
fn default_block_native_helpers_measure_extra_512_frames() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            common::GAIN.to_owned()
        } else {
            vst_id(0)
        };
        let mut options = options(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(2),
        );
        options.config.block = windfall_plugin_host::bridge::protocol::DEFAULT_BLOCK;
        options.offline = true;
        let (control, mut audio, _) = supervisor::launch(options).unwrap();
        let mut left = [0.0; 1024];
        let mut right = left;
        left[0] = 1.0;
        right[0] = 1.0;
        audio
            .process_offline(
                &mut left,
                &mut right,
                Instant::now() + Duration::from_secs(2),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(left.iter().position(|value| *value != 0.0), Some(512));
        assert_eq!(left[512], if format == "clap" { 0.5 } else { 1.0 });
        assert_eq!(audio.latency(), if format == "clap" { 512 } else { 529 });
        assert_eq!(audio.health().missed_blocks, 0);
        drop(audio);
        drop(control);
    }
}

// Test-only process ownership; even a failing RED assertion has bounded reap.
struct TestChild(std::process::Child);
impl TestChild {
    fn finish(&mut self, kill: bool) -> std::process::ExitStatus {
        if kill {
            let _ = self.0.kill();
        }
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < end, "test child did not exit");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
impl Drop for TestChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let end = Instant::now() + Duration::from_secs(2);
        while self.0.try_wait().ok().flatten().is_none() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
#[ignore = "subprocess role used by the authentication regression"]
fn auth_impostor_client_process() {
    let endpoint = std::env::var("WINDFALL_TEST_AUTH_ENDPOINT").unwrap();
    if let Ok(version) = std::env::var("WINDFALL_TEST_OLD_HELLO") {
        let version: u32 = version.parse().unwrap();
        assert!(matches!(version, 1..=4));
        let mut bootstrap = [0u8; 56];
        std::io::stdin().read_exact(&mut bootstrap).unwrap();
        assert_eq!(&bootstrap[..4], b"WFAP");
        let mut socket = std::net::TcpStream::connect(&endpoint).unwrap();
        let mut hello = [0u8; 40];
        hello[..4].copy_from_slice(b"WFAH");
        hello[4..8].copy_from_slice(&version.to_le_bytes());
        hello[8..].copy_from_slice(&bootstrap[8..40]); // Correct private child key.
        bootstrap.fill(0);
        socket.write_all(&hello).unwrap();
        hello.fill(0);
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = [0u8; 1024];
        match socket.read(&mut bytes) {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            result => panic!("old authenticated helper received metadata: {result:?}"),
        }
        return;
    }
    let mut stalled: Vec<_> = (0..8)
        .map(|_| std::net::TcpStream::connect(&endpoint).unwrap())
        .collect();
    let mut wrong = std::net::TcpStream::connect(&endpoint).unwrap();
    let mut hello = [0u8; 40];
    hello[..4].copy_from_slice(b"WFAH");
    hello[4..8].copy_from_slice(&4u32.to_le_bytes());
    wrong.write_all(&hello).unwrap();
    let mut old = std::net::TcpStream::connect(&endpoint).unwrap();
    hello[4..8].copy_from_slice(&2u32.to_le_bytes());
    old.write_all(&hello).unwrap();
    println!("CONNECTED");
    std::io::stdout().flush().unwrap();
    for socket in stalled.iter_mut().chain([&mut wrong, &mut old]) {
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut buffer = [0u8; 1024];
        match socket.read(&mut buffer) {
            Ok(0) => {}
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => {}
            result => panic!("unrelated client was not closed without disclosure: {result:?}"),
        }
    }
}

#[test]
fn unrelated_first_and_stalled_clients_receive_no_load_before_real_child_authentication() {
    use std::process::{Command, Stdio};
    use windfall_plugin_host::bridge::{
        auth,
        control::{Decoder, Message, Packet},
        mapping::Mapping,
    };
    let key = auth::Key::generate().unwrap();
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let mut impostor = TestChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "auth_impostor_client_process",
                "--nocapture",
            ])
            .env("WINDFALL_TEST_AUTH_ENDPOINT", address.to_string())
            .env_remove("WINDFALL_BRIDGE_FIXTURE")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let stdout = impostor.0.stdout.take().unwrap();
    let (ready, received) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stdout);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if line.contains("CONNECTED") {
                let _ = ready.try_send(());
            }
        }
    });
    let connected = received.recv_timeout(Duration::from_secs(2));
    if connected.is_err() {
        impostor.finish(true);
        reader.join().unwrap();
        panic!("unrelated client did not connect before startup");
    }
    let options = options(
        "clap",
        common::GAIN,
        Kind::Effect,
        vec![gain("clap")],
        Duration::from_secs(2),
    );
    let name = format!(
        "Local\\Windfall-Audio-{:016x}-{:016x}",
        options.config.identity.session, options.config.identity.token
    );
    let _region = Region::initialize(Mapping::create(&name).unwrap(), options.config).unwrap();
    let mut child = TestChild(
        Command::new(&options.helper)
            .args(["--windfall-audio-helper", &address.to_string()])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    key.write_to_child(
        &mut child.0,
        options.config.identity.session,
        Instant::now() + Duration::from_secs(5),
        None,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut socket = auth::accept(&listener, &mut child.0, &key, deadline, None).unwrap();
    socket.set_nonblocking(false).unwrap();
    let mut load = Packet::new(
        1,
        options.config.identity.into(),
        Message::Load {
            wrapper: Box::new(options.wrapper.clone()),
            mapping: name,
            settings: options.config.into(),
            path: options.plugin.to_string_lossy().into_owned(),
            id: options.id,
            format: options.format,
            approved_binary: options.approved_binary,
            parameters: options.parameters.into_iter().map(Into::into).collect(),
            offline: false,
        },
    );
    let host = PluginHost::windfall();
    let module = host.load(&options.plugin).unwrap();
    let mut reference = module.create(common::GAIN).unwrap();
    assert!(reference.set_param(7, 0.625));
    load.state = reference.save_state().unwrap().into_bytes();
    load.write(&mut socket).unwrap();
    socket.set_nonblocking(true).unwrap();
    let mut decoder = Decoder::default();
    loop {
        assert!(Instant::now() < deadline);
        if let Some(packet) = decoder.poll(&mut socket).unwrap() {
            assert!(matches!(packet.body, Message::Ready { .. }));
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    child.finish(true);
    let status = impostor.finish(false);
    reader.join().unwrap();
    let mut diagnostic = String::new();
    impostor
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut diagnostic)
        .unwrap();
    assert!(status.success(), "unrelated client failed: {diagnostic}");
}

#[test]
fn native_output_event_saturation_is_visible_and_cannot_acknowledge_desired_controls() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            "org.windfall.test.bridge-event-flood".to_owned()
        } else {
            vst_id(17)
        };
        let (control, mut audio) = launch(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(2),
        );
        assert!(audio.set_param(7, 0.75));
        let mut left = [0.25; 64];
        let mut right = left;
        for _ in 0..3 {
            assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            audio.health().native_drops > 0,
            "native output losses hidden for {format}"
        );
        assert!(audio.health().unknown_blocks > 0);
        assert_eq!(audio.health().acknowledged_generation, 0);
        assert!(audio.set_param(7, 0.5));
        for _ in 0..3 {
            assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            audio.health().acknowledged_generation,
            audio.desired_generation(),
            "accepted desired control did not recover after native flood"
        );
        assert!(!control.status().failed);
        assert!(control.terminate().reaped);
    }
}

#[test]
fn successful_native_state_at_the_raw_limit_refuses_capture_without_losing_the_owner() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            "org.windfall.test.bridge-state-boundary".to_owned()
        } else {
            vst_id(18)
        };
        let mut settings = options(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(5),
        );
        settings.offline = true;
        let (control, mut audio, _) = supervisor::launch(settings).unwrap();
        let process_id = control.status().process_id;
        audio
            .process_offline(
                &mut [0.25; 512],
                &mut [0.25; 512],
                Instant::now() + Duration::from_secs(2),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        let old = control
            .capture(
                1,
                audio.desired_generation(),
                &[gain(format)],
                Duration::from_secs(2),
            )
            .unwrap();
        assert!(audio.set_param(7, 0.75));
        audio
            .process_offline(
                &mut [0.25; 512],
                &mut [0.25; 512],
                Instant::now() + Duration::from_secs(2),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        let pending = [ParameterSpec {
            value: 0.75,
            ..gain(format)
        }];
        let result = control.capture(
            1,
            audio.desired_generation(),
            &pending,
            Duration::from_secs(5),
        );
        assert!(
            result.is_err(),
            "full raw payload cannot fit its WFPS wrapper"
        );
        assert!(
            result
                .unwrap_err()
                .contains("wrapper exceeds bridge control budget")
        );
        assert_eq!(control.last_valid_state().unwrap().state, old.state);
        assert!(audio.set_param(7, 0.5));
        audio
            .process_offline(
                &mut [0.25; 512],
                &mut [0.25; 512],
                Instant::now() + Duration::from_secs(2),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .expect("successful native save over wrapped budget must retain healthy owner");
        let healthy = control
            .capture(
                1,
                audio.desired_generation(),
                &[gain(format)],
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(healthy.state, old.state);
        assert_eq!(control.status().process_id, process_id);
        assert!(!control.status().failed);
        assert!(control.terminate().reaped);
    }
}

#[test]
fn clap_ignored_failed_state_write_preserves_cached_state_and_owner() {
    let mut settings = options(
        "clap",
        "org.windfall.test.bridge-ignored-stream-error",
        Kind::Effect,
        vec![gain("clap")],
        Duration::from_secs(5),
    );
    settings.offline = true;
    let (control, mut audio, _) = supervisor::launch(settings).unwrap();
    let old = control
        .capture(
            1,
            audio.desired_generation(),
            &[gain("clap")],
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(audio.set_param(7, 0.75));
    audio
        .process_offline(
            &mut [0.25; 512],
            &mut [0.25; 512],
            Instant::now() + Duration::from_secs(2),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    let pending = [ParameterSpec {
        value: 0.75,
        ..gain("clap")
    }];
    // Actual native CLAP save reaches the existing256MiB writer cap; no invalid
    // borrowed pointer or invented oversized slice is passed to the host stream.
    assert!(
        control
            .capture(
                1,
                audio.desired_generation(),
                &pending,
                Duration::from_secs(5)
            )
            .is_err()
    );
    assert_eq!(control.last_valid_state().unwrap().state, old.state);
    assert!(audio.set_param(7, 0.5));
    audio
        .process_offline(
            &mut [0.25; 512],
            &mut [0.25; 512],
            Instant::now() + Duration::from_secs(2),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .expect("refused state must leave native owner usable");
    let healthy = control
        .capture(
            1,
            audio.desired_generation(),
            &[gain("clap")],
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(healthy.state, old.state);
    assert!(!control.status().failed);
    assert!(control.terminate().reaped);
}

#[test]
fn native_capture_exit_hang_and_partial_state_failure_preserve_the_last_cache() {
    let (other, mut live) = launch(
        "clap",
        common::GAIN,
        Kind::Effect,
        vec![gain("clap")],
        Duration::from_secs(2),
    );
    for format in ["clap", "vst3"] {
        for (suffix, index) in [
            ("capture-exit", 13),
            ("capture-hang", 14),
            ("invalid-stream", 15),
        ] {
            let id = if format == "clap" {
                format!("org.windfall.test.bridge-{suffix}")
            } else {
                vst_id(index)
            };
            let mut settings = options(
                format,
                &id,
                Kind::Effect,
                vec![gain(format)],
                Duration::from_secs(2),
            );
            settings.offline = true;
            let (control, mut audio, _) = supervisor::launch(settings).unwrap();
            let old = control
                .capture(
                    1,
                    audio.desired_generation(),
                    &[gain(format)],
                    Duration::from_secs(1),
                )
                .unwrap();
            assert!(audio.set_param(7, 0.75));
            audio
                .process_offline(
                    &mut [0.25; 512],
                    &mut [0.25; 512],
                    Instant::now() + Duration::from_secs(2),
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
            let pending = [ParameterSpec {
                value: 0.75,
                ..gain(format)
            }];
            let start = Instant::now();
            assert!(
                control
                    .capture(
                        1,
                        audio.desired_generation(),
                        &pending,
                        Duration::from_millis(100)
                    )
                    .is_err()
            );
            assert!(start.elapsed() < Duration::from_secs(2));
            assert_eq!(control.last_valid_state().unwrap().state, old.state);
            if suffix == "invalid-stream" {
                assert!(
                    !control.status().failed,
                    "native refused partial state should not kill healthy helper"
                );
                assert!(audio.set_param(7, 0.5));
                audio
                    .process_offline(
                        &mut [0.25; 512],
                        &mut [0.25; 512],
                        Instant::now() + Duration::from_secs(2),
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                    .unwrap();
                control
                    .capture(
                        1,
                        audio.desired_generation(),
                        &[gain(format)],
                        Duration::from_secs(1),
                    )
                    .unwrap();
            } else {
                wait_failed(&control);
                for _ in 0..6 {
                    let mut left = [0.25; 64];
                    let mut right = left;
                    assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
                }
            }
            assert!(control.terminate().reaped);
            let mut left = [0.25; 512];
            let mut right = left;
            assert_eq!(guarded(|| live.process(&mut left, &mut right)), 0);
            std::thread::sleep(Duration::from_millis(5));
            assert!(!other.status().failed);
        }
    }
    assert!(live.health().completed_blocks > 0);
    assert!(other.terminate().reaped);
}

#[test]
fn unsupported_actual_native_latency_is_rejected_before_audio_installation() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            "org.windfall.test.bridge-bad-latency".to_owned()
        } else {
            vst_id(16)
        };
        let settings = options(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(1),
        );
        let start = Instant::now();
        let result = supervisor::launch(settings);
        let error = result
            .err()
            .expect("bad native latency produced an installed facade");
        let error = windfall_core::english::english(&error);
        assert!(
            error.contains("unsupported native latency"),
            "unsupported latency was hidden by {error}"
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}

#[test]
fn wrong_child_key_and_startup_cancellation_fail_closed_with_bounded_reap() {
    use std::process::{Command, Stdio};
    use windfall_plugin_host::bridge::auth;
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let key = auth::Key::generate().unwrap();
    let wrong = auth::Key::generate().unwrap();
    let mut child = TestChild(
        Command::new(env!("CARGO_BIN_EXE_windfall-plugin-audio"))
            .args([
                "--windfall-audio-helper",
                &listener.local_addr().unwrap().to_string(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wrong
        .write_to_child(
            &mut child.0,
            7,
            Instant::now() + Duration::from_secs(5),
            None,
        )
        .unwrap();
    let start = Instant::now();
    assert!(
        auth::accept(
            &listener,
            &mut child.0,
            &key,
            start + Duration::from_millis(100),
            None
        )
        .is_err()
    );
    child.finish(true);
    assert!(start.elapsed() < Duration::from_secs(1));
    let flag = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let mut settings = options(
        "clap",
        common::GAIN,
        Kind::Effect,
        vec![gain("clap")],
        Duration::from_secs(2),
    );
    settings.cancelled = Some(flag);
    let start = Instant::now();
    assert!(supervisor::launch(settings).is_err());
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn note_only_overflow_reconciles_the_contiguous_next_block_before_acknowledging() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            "org.windfall.test.bridge-note-probe".to_owned()
        } else {
            vst_id(12)
        };
        let parameters = if format == "clap" {
            vec![]
        } else {
            vec![gain(format)]
        };
        let (control, mut audio) = launch(
            format,
            &id,
            Kind::Instrument,
            parameters,
            Duration::from_secs(2),
        );
        assert_eq!(
            guarded(|| {
                for _ in 0..1024 {
                    assert!(audio.note_on(60, 0.5));
                }
                assert!(!audio.note_on(69, 0.75));
            }),
            0
        );
        let desired = audio.desired_generation();
        let mut left = [0.0; 64];
        let mut right = left;
        for _ in 0..2 {
            assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
        assert_eq!(
            audio.health().acknowledged_generation,
            0,
            "incomplete block falsely acknowledged"
        );
        assert_eq!(audio.health().unknown_blocks, 1);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
        let ack = audio.health().acknowledged_generation;
        let marker = left;
        assert_eq!(ack, desired, "whole snapshot not recovered");
        assert!(
            marker.iter().all(|sample| *sample == 1.0),
            "{format} acknowledged an actually missing key69: {marker:?}"
        );
        assert_eq!(
            guarded(|| {
                assert!(audio.note_off(69));
            }),
            0
        );
        for _ in 0..3 {
            assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            left.iter().all(|sample| *sample == 0.0),
            "native recovered key was not actually released"
        );
        assert_eq!(
            audio.health().acknowledged_generation,
            audio.desired_generation()
        );
        assert!(control.terminate().reaped);
    }
}

#[test]
fn healthy_idle_reset_and_turnover_do_not_arm_false_stalls_or_dsp_acknowledgements() {
    let mut settings = options(
        "clap",
        common::GAIN,
        Kind::Effect,
        vec![gain("clap")],
        Duration::from_millis(50),
    );
    settings.config.block = 256;
    let (control, mut audio, _) = supervisor::launch(settings).unwrap();
    std::thread::sleep(Duration::from_millis(120));
    assert!(!control.status().failed && !control.status().reaped);
    assert_eq!(audio.health().acknowledged_generation, 0);
    assert_eq!(guarded(|| audio.reset_timeline()), 0);
    std::thread::sleep(Duration::from_millis(120));
    assert!(!control.status().failed && !control.status().reaped);
    assert_eq!(audio.health().acknowledged_generation, 0);
    for count in [1, 7, 64, 480, 512].into_iter().cycle().take(120) {
        let mut left = [0.25; 512];
        let mut right = left;
        assert_eq!(
            guarded(|| audio.process(&mut left[..count], &mut right[..count])),
            0
        );
        std::thread::sleep(Duration::from_millis(3));
    }
    assert!(!control.status().failed && !control.status().reaped);
    assert!(audio.health().completed_blocks > 0);
    assert_eq!(guarded(|| audio.reset_timeline()), 0);
    std::thread::sleep(Duration::from_millis(120));
    assert!(!control.status().failed && !control.status().reaped);
    assert_eq!(audio.health().acknowledged_generation, 0);
    assert!(control.terminate().reaped);
}

#[test]
fn idle_native_hang_is_reaped_while_callbacks_keep_turning_over_ready_slots() {
    let mut settings = options(
        "clap",
        "org.windfall.test.bridge-idle-hang",
        Kind::Effect,
        vec![gain("clap")],
        Duration::from_millis(30),
    );
    settings.config.block = 256;
    let (control, mut audio, _) = supervisor::launch(settings).unwrap();
    let end = Instant::now() + Duration::from_millis(400);
    let mut calls = 0;
    while Instant::now() < end && !control.status().reaped {
        let mut left = [0.25; 256];
        let mut right = left;
        assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
        calls += 1;
        std::thread::sleep(Duration::from_millis(1));
    }
    let result = control.status();
    assert!(control.terminate().reaped);
    assert!(calls >= 3);
    assert!(
        result.reaped && result.failed,
        "idle hang evaded watchdog: {result:?}"
    );
    assert_eq!(audio.health().acknowledged_generation, 0);
}

#[test]
fn authenticated_production_launch_accepts_large_valid_native_state_under_backpressure() {
    large_native_state_launch(16 << 20, Duration::from_secs(5));
}

#[test]
fn authenticated_production_launch_drains_48_mib_with_the_callers_startup_budget() {
    large_native_state_launch(48 << 20, Duration::from_secs(15));
}

fn large_native_state_launch(bytes: usize, startup_timeout: Duration) {
    let host = PluginHost::windfall();
    let module = host.load(&fixture("clap")).unwrap();
    let mut native = module.create(common::GAIN).unwrap();
    assert!(native.set_param(7, 0.625));
    assert!(native.set_param(9, 1.0)); // Opaque state owns inversion; Load table only sets gain.
    let mut state = native.save_state().unwrap().into_bytes();
    state.resize(bytes, 0); // Fixture accepts trailing opaque native bytes.
    native
        .load_state(&PluginState::from_bytes(state.clone()))
        .unwrap();
    assert_eq!(native.param_value(7), Some(0.625));
    assert_eq!(native.param_value(9), Some(1.0));
    let mut settings = options(
        "clap",
        common::GAIN,
        Kind::Effect,
        vec![ParameterSpec {
            value: 0.625,
            ..gain("clap")
        }],
        Duration::from_secs(2),
    );
    settings.state = state;
    settings.startup_timeout = startup_timeout;
    settings.offline = true;
    settings.helper = PathBuf::from(env!("CARGO_BIN_EXE_windfall-plugin-audio-backpressure"));
    // Actual production launch, with the native helper paused before Load reads.
    let (control, mut audio, _) = supervisor::launch(settings).unwrap();
    let mut left = [1.0; 512];
    let mut right = left;
    audio
        .process_offline(
            &mut left,
            &mut right,
            Instant::now() + Duration::from_secs(2),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    assert!(left[128..].iter().all(|value| *value == -0.625));
    let captured = control
        .capture(
            1,
            audio.desired_generation(),
            &[ParameterSpec {
                value: 0.625,
                ..gain("clap")
            }],
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(native.set_param(7, 0.0));
    assert!(native.set_param(9, 0.0));
    native
        .load_state(&PluginState::from_bytes(captured.state))
        .unwrap();
    assert_eq!(native.param_value(7), Some(0.625));
    assert_eq!(native.param_value(9), Some(1.0));
    assert!(control.terminate().reaped);
}

#[test]
fn unsupported_startup_budget_is_rejected_before_paths_or_mapping_resources() {
    for duration in [
        Duration::ZERO,
        Duration::from_secs(60) + Duration::from_nanos(1),
        Duration::MAX,
    ] {
        let mut settings = options(
            "clap",
            common::GAIN,
            Kind::Effect,
            vec![gain("clap")],
            Duration::from_secs(2),
        );
        settings.startup_timeout = duration;
        settings.plugin = PathBuf::from("missing-budget-probe.clap");
        settings.helper = PathBuf::from("missing-budget-probe.exe");
        let name = format!(
            "Local\\Windfall-Audio-{:016x}-{:016x}",
            settings.config.identity.session, settings.config.identity.token
        );
        let error = match supervisor::launch(settings) {
            Ok(_) => panic!("unsupported startup budget launched"),
            Err(error) => error,
        };
        assert!(
            error.contains("unsupported helper startup budget"),
            "{error}"
        );
        assert!(windfall_plugin_host::bridge::mapping::Mapping::open(&name).is_err());
    }
}

#[test]
fn backpressured_startup_cancellation_and_deadline_reclaim_the_mapping() {
    for cancel in [false, true] {
        let patience = Instant::now() + Duration::from_secs(15);
        loop {
            let host = PluginHost::windfall();
            let module = host.load(&fixture("clap")).unwrap();
            let mut state = module
                .create(common::GAIN)
                .unwrap()
                .save_state()
                .unwrap()
                .into_bytes();
            state.resize(16 << 20, 0);
            let mut settings = options(
                "clap",
                common::GAIN,
                Kind::Effect,
                vec![gain("clap")],
                Duration::from_secs(2),
            );
            settings.helper =
                PathBuf::from(env!("CARGO_BIN_EXE_windfall-plugin-audio-backpressure"));
            settings.state = state;
            settings.startup_timeout = if cancel {
                Duration::from_secs(5)
            } else {
                // Includes spawning and authenticating under the suite's load.
                // The private helper holds Load unread for three seconds.
                Duration::from_secs(1)
            };
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            settings.cancelled = Some(flag.clone());
            let mapping = format!(
                "Local\\Windfall-Audio-{:016x}-{:016x}",
                settings.config.identity.session, settings.config.identity.token
            );
            let ready = std::env::temp_dir().join(format!(
                "windfall-backpressure-{:016x}.ready",
                settings.config.identity.session
            ));
            let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let done = finished.clone();
            let waiting = ready.clone();
            let canceller = cancel.then(|| {
                std::thread::spawn(move || {
                    while !waiting.is_file() {
                        if done.load(Ordering::Acquire) {
                            return None;
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    std::thread::sleep(Duration::from_millis(100));
                    let cancelled_at = Instant::now();
                    flag.store(true, Ordering::Release);
                    Some(cancelled_at)
                })
            });
            let started = Instant::now();
            let error = match supervisor::launch(settings) {
                Ok(_) => panic!("partial cancelled/expired Load was accepted"),
                Err(error) => error,
            };
            finished.store(true, Ordering::Release);
            let cancelled_at = canceller.and_then(|canceller| canceller.join().unwrap());
            let reached_load = ready.is_file();
            if reached_load {
                std::fs::remove_file(&ready).unwrap();
            }
            assert!(
                windfall_plugin_host::bridge::mapping::Mapping::open(&mapping).is_err(),
                "failed launch retained its map"
            );
            if !reached_load {
                // A slow spawn/authentication is not the condition under test.
                // Retry with a fresh identity, but never accept an untested Load.
                assert!(
                    Instant::now() < patience,
                    "helper never reached Load: {error}"
                );
                assert!(error.contains("timed out"), "{error}");
                continue;
            }
            assert!(
                started.elapsed() < Duration::from_secs(if cancel { 6 } else { 2 }),
                "startup bound: {error}"
            );
            if cancel {
                // Keep the original two-second cancellation/reap bound; its
                // clock starts at the request, after the fixture is ready.
                assert!(
                    cancelled_at.expect("Load was cancelled").elapsed() < Duration::from_secs(2)
                );
            }
            assert!(
                error.contains(if cancel {
                    "startup cancelled"
                } else {
                    "startup deadline"
                }),
                "{error}"
            );
            break;
        }
    }
}

#[test]
fn capture_after_an_incomplete_new_epoch_cannot_relabel_old_dsp_proof() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            common::GAIN.to_owned()
        } else {
            vst_id(0)
        };
        let mut settings = options(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(2),
        );
        settings.offline = true;
        let (control, mut audio, _) = supervisor::launch(settings).unwrap();
        audio
            .process_offline(
                &mut [1.0; 512],
                &mut [1.0; 512],
                Instant::now() + Duration::from_secs(2),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        let first = control
            .capture(
                1,
                audio.desired_generation(),
                &[gain(format)],
                Duration::from_secs(1),
            )
            .unwrap();
        assert!(first.processed_generation > 0);
        audio.reset_timeline();
        for _ in 0..1024 {
            assert!(audio.note_on(60, 0.5));
        }
        assert!(!audio.note_on(69, 0.75));
        // Submit only one native block in epoch2. It is actually incomplete;
        // a successful serialized capture then runs after that native call.
        audio.process(&mut [1.0; 64], &mut [1.0; 64]);
        std::thread::sleep(Duration::from_millis(20));
        let incomplete = control
            .capture(
                2,
                audio.desired_generation(),
                &[gain(format)],
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(
            incomplete.processed_generation, 0,
            "{format} relabelled old DSP proof after incomplete new-epoch processing"
        );
        for _ in 0..3 {
            audio.process(&mut [1.0; 64], &mut [1.0; 64]);
            std::thread::sleep(Duration::from_millis(20));
        }
        let healthy = control
            .capture(
                2,
                audio.desired_generation(),
                &[gain(format)],
                Duration::from_secs(1),
            )
            .unwrap();
        assert_eq!(healthy.processed_generation, audio.desired_generation());
        assert!(control.terminate().reaped);
    }
}

#[test]
fn vst3_capture_does_not_relabel_dsp_proof_after_a_reset_without_processing() {
    let mut options = options(
        "vst3",
        &vst_id(0),
        Kind::Effect,
        vec![gain("vst3")],
        Duration::from_secs(2),
    );
    options.offline = true;
    let (control, mut audio, _) = supervisor::launch(options).unwrap();
    audio
        .process_offline(
            &mut [1.0; 512],
            &mut [1.0; 512],
            Instant::now() + Duration::from_secs(2),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    let first = control
        .capture(
            1,
            audio.desired_generation(),
            &[gain("vst3")],
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(first.processed_generation > 0);
    audio.reset_timeline();
    let second = control
        .capture(
            2,
            audio.desired_generation(),
            &[gain("vst3")],
            Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(second.epoch, 2);
    assert_eq!(second.processed_generation, 0);
    assert_eq!(audio.health().acknowledged_generation, 0);
    drop(audio);
    drop(control);
}

#[test]
fn previous_authenticated_helper_hellos_are_closed_before_any_load_metadata() {
    use std::process::{Command, Stdio};
    use windfall_plugin_host::bridge::auth;
    // Hello4 is the mapping-ABI-ten helper, before ramps were handed over.
    for version in [1, 2, 3, 4] {
        let key = auth::Key::generate().unwrap();
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut child = TestChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "auth_impostor_client_process",
                    "--nocapture",
                ])
                .env(
                    "WINDFALL_TEST_AUTH_ENDPOINT",
                    listener.local_addr().unwrap().to_string(),
                )
                .env("WINDFALL_TEST_OLD_HELLO", version.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        key.write_to_child(&mut child.0, 1, deadline, None).unwrap();
        assert!(auth::accept(&listener, &mut child.0, &key, deadline, None).is_err());
        let status = child.finish(false);
        let mut failure = String::new();
        child
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut failure)
            .unwrap();
        assert!(
            status.success(),
            "Hello{version} disclosed metadata or failed its child assertion: {failure}"
        );
    }
}

#[test]
fn authenticated_new_helper_refuses_old_mapping_abis_before_native_activation() {
    use std::process::{Command, Stdio};
    use windfall_plugin_host::bridge::{
        auth,
        control::{Message, Packet},
        mapping::Mapping,
        protocol::TIMELINE_EPOCH,
        slots::WordStorage,
    };
    for (version, epoch) in [(1, 0), (2, 0), (3, 0), (3, 1)] {
        let settings = options(
            "clap",
            common::GAIN,
            Kind::Effect,
            vec![gain("clap")],
            Duration::from_secs(2),
        );
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let name = format!(
            "Local\\Windfall-Audio-{:016x}-{:016x}",
            settings.config.identity.session, settings.config.identity.token
        );
        let storage = Mapping::create(&name).unwrap();
        let _region = Region::initialize(storage.clone(), settings.config).unwrap();
        storage.words()[1].store(u32::to_le(version), Ordering::Release);
        storage.words()[TIMELINE_EPOCH].store(u32::to_le(epoch), Ordering::Release);
        let key = auth::Key::generate().unwrap();
        let mut child = TestChild(
            Command::new(&settings.helper)
                .args([
                    "--windfall-audio-helper",
                    &listener.local_addr().unwrap().to_string(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        key.write_to_child(
            &mut child.0,
            settings.config.identity.session,
            deadline,
            None,
        )
        .unwrap();
        let mut socket = auth::accept(&listener, &mut child.0, &key, deadline, None).unwrap();
        socket.set_nonblocking(false).unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        Packet::new(
            1,
            settings.config.identity.into(),
            Message::Load {
                wrapper: Box::new(settings.wrapper.clone()),
                mapping: name,
                settings: settings.config.into(),
                path: settings.plugin.to_string_lossy().into_owned(),
                id: settings.id,
                format: settings.format,
                approved_binary: settings.approved_binary,
                parameters: settings.parameters.into_iter().map(Into::into).collect(),
                offline: true,
            },
        )
        .write(&mut socket)
        .unwrap();
        let mut reply = Vec::new();
        socket.read_to_end(&mut reply).unwrap();
        assert!(reply.is_empty(), "old map received an activation reply");
        assert_eq!(child.finish(false).code(), Some(70));
    }
}

#[test]
fn real_helpers_capture_repeated_resets_without_dsp_and_refuse_stale_or_future_epochs() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            "org.windfall.test.bridge-delayed".into()
        } else {
            vst_id(10)
        };
        let mut settings = options(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_secs(2),
        );
        settings.offline = true;
        let (control, mut audio, _) = supervisor::launch(settings).unwrap();
        audio
            .process_offline(
                &mut [0.25; 256],
                &mut [0.25; 256],
                Instant::now() + Duration::from_secs(2),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(
            control
                .capture(1, 1, &[gain(format)], Duration::from_secs(2))
                .unwrap()
                .processed_generation,
            1
        );
        let desired = audio.desired_generation();
        for epoch in 2..=5 {
            assert_eq!(guarded(|| audio.reset_timeline()), 0);
            assert_eq!(audio.desired_generation(), desired);
            assert_eq!(audio.completed_proof(), None);
            assert_eq!(audio.health().acknowledged_generation, 0);
            for invalid in [epoch - 1, epoch + 1] {
                assert!(
                    control
                        .capture(invalid, desired, &[gain(format)], Duration::from_secs(2))
                        .is_err()
                );
            }
            let current = control
                .capture(epoch, desired, &[gain(format)], Duration::from_secs(2))
                .unwrap();
            assert_eq!((current.epoch, current.processed_generation), (epoch, 0));
            assert_eq!(control.last_valid_state().unwrap().epoch, epoch);
        }
        audio
            .process_offline(
                &mut [0.25; 256],
                &mut [0.25; 256],
                Instant::now() + Duration::from_secs(2),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(
            control
                .capture(5, desired, &[gain(format)], Duration::from_secs(2))
                .unwrap()
                .processed_generation,
            desired
        );
        assert_eq!(guarded(|| audio.reset_timeline()), 0);
        assert!(audio.note_on(60, 0.5)); // Actual new intent, collected but unsubmitted.
        let pending = control
            .capture(
                6,
                audio.desired_generation(),
                &[gain(format)],
                Duration::from_secs(2),
            )
            .unwrap();
        assert_eq!(pending.processed_generation, 0);
        assert_eq!(audio.health().acknowledged_generation, 0);
        assert!(control.terminate().reaped);
    }
}

#[test]
fn corrupt_reset_authority_and_late_output_have_zero_callback_allocator_calls() {
    use windfall_plugin_host::bridge::{
        protocol::{OUTPUT_OK, TIMELINE_EPOCH},
        slots::{InputBlock, OutputBlock, WordStorage},
    };
    for corrupt in [0, 1, u32::MAX] {
        let storage = LocalWords::new();
        let config = Config {
            identity: Identity {
                session: 1,
                token: 2,
                revision: 3,
                binding: 4,
            },
            sample_rate: 48_000,
            block: 64,
            native_latency: 0,
            kind: Kind::Instrument,
        };
        let region = Region::initialize(storage.clone(), config).unwrap();
        let signals = Arc::new(Signals::default());
        let mut audio = Audio::new(region.clone(), signals.clone(), &[]).unwrap();
        assert_eq!(guarded(|| audio.process(&mut [0.0; 64], &mut [0.0; 64])), 0);
        let mut input = InputBlock::new();
        let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
        let late = OutputBlock {
            epoch: 1,
            processed_generation: 1,
            left: [1.0; 512],
            right: [1.0; 512],
            native_drops: 0,
            ..OutputBlock::silent()
        };
        assert_eq!(guarded(|| audio.reset_timeline()), 0);
        region.complete(slot, sequence, &late, OUTPUT_OK);
        storage.words()[TIMELINE_EPOCH].store(corrupt.to_le(), Ordering::Release);
        assert_eq!(guarded(|| audio.reset_timeline()), 0);
        assert!(signals.failed.load(Ordering::Acquire));
        for count in [1, 7, 64, 480, 512] {
            let mut left = [0.0; 512];
            let mut right = left;
            assert_eq!(
                guarded(|| audio.process(&mut left[..count], &mut right[..count])),
                0
            );
            assert!(left.iter().chain(&right).all(|sample| *sample == 0.0));
        }
        assert_eq!(audio.health().acknowledged_generation, 0);
        assert_eq!(audio.completed_proof(), None);
    }
}

#[test]
fn permanent_native_process_hangs_are_killed_with_aligned_dry_fallback() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            "org.windfall.test.bridge-process-hang".to_owned()
        } else {
            vst_id(11)
        };
        let (control, mut audio) = launch(
            format,
            &id,
            Kind::Effect,
            vec![gain(format)],
            Duration::from_millis(20),
        );
        let mut left = [0.25; 64];
        let mut right = left;
        audio.process(&mut left, &mut right);
        wait_failed(&control);
        for _ in 0..10 {
            left.fill(0.25);
            right.fill(0.25);
            assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
        }
        assert_eq!(audio.latency(), 165);
        assert!(left.iter().all(|value| *value == 0.25));
        assert_eq!(audio.health().acknowledged_generation, 0);
        drop(audio);
        drop(control);
    }
}

#[test]
fn truthful_native_delay_aligns_healthy_audio_dry_fallback_and_latency_report() {
    for format in ["clap", "vst3"] {
        for block in [64, 256] {
            let id = if format == "clap" {
                "org.windfall.test.bridge-delayed".to_owned()
            } else {
                vst_id(10)
            };
            let mut options = options(
                format,
                &id,
                Kind::Effect,
                vec![gain(format)],
                Duration::from_secs(2),
            );
            options.config.block = block;
            options.offline = true;
            let (control, mut audio, _) = supervisor::launch(options).unwrap();
            let expected_latency = if block == 64 { 165 } else { 549 };
            assert_eq!(audio.latency(), expected_latency);
            let mut left = [0.0; 2048];
            let mut right = left;
            left[0] = 1.0;
            right[0] = 1.0;
            audio
                .process_offline(
                    &mut left,
                    &mut right,
                    Instant::now() + Duration::from_secs(2),
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
            assert_eq!(
                left.iter().position(|value| *value != 0.0),
                Some(expected_latency)
            );
            assert_eq!(left[expected_latency], 0.5);
            assert_eq!(audio.health().missed_blocks, 0);
            audio.reset_timeline();
            assert!(control.terminate().reaped);
            left.fill(0.0);
            right.fill(0.0);
            left[0] = 1.0;
            right[0] = 1.0;
            assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
            assert_eq!(
                left.iter().position(|value| *value != 0.0),
                Some(expected_latency)
            );
            assert_eq!(left[expected_latency], 1.0);
            assert_eq!(audio.health().acknowledged_generation, 0);
            drop(audio);
            drop(control);
        }
    }
}

#[test]
fn offline_deadline_and_cancellation_reap_only_the_render_helper() {
    let (live_control, mut live_audio) = launch(
        "clap",
        common::GAIN,
        Kind::Effect,
        vec![gain("clap")],
        Duration::from_secs(2),
    );
    for cancel in [false, true] {
        let mut options = options(
            "clap",
            common::SLOW,
            Kind::Effect,
            vec![],
            Duration::from_secs(2),
        );
        options.offline = true;
        let (control, mut audio, _) = supervisor::launch(options).unwrap();
        let mut left = [0.25; 128];
        let mut right = left;
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        audio
            .process_offline(
                &mut left,
                &mut right,
                Instant::now() + Duration::from_secs(2),
                &cancelled,
            )
            .unwrap();
        cancelled.store(cancel, Ordering::Release);
        left.fill(0.125);
        right.fill(0.125);
        let result = audio.process_offline(
            &mut left,
            &mut right,
            Instant::now() + Duration::from_millis(1),
            &cancelled,
        );
        assert_eq!(
            result,
            Err(if cancel {
                OfflineError::Cancelled
            } else {
                OfflineError::Deadline
            })
        );
        assert!(left.iter().chain(&right).all(|value| *value == 0.0));
        let status = control.terminate();
        assert!(status.reaped);
        assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
        drop(audio);
        drop(control);
        assert!(!live_control.status().failed);
        let mut left = [1.0; 64];
        let mut right = left;
        for _ in 0..5 {
            left.fill(1.0);
            right.fill(1.0);
            live_audio.process(&mut left, &mut right);
            std::thread::sleep(Duration::from_millis(3));
        }
        assert!(left.iter().all(|value| *value == 0.5));
    }
    drop(live_audio);
    drop(live_control);
}

#[test]
fn native_process_crash_nan_and_stall_leave_parent_alive_and_dry_bounded() {
    for (id, timeout) in [
        (common::PROCESS_PANIC, Duration::from_secs(1)),
        (common::NAN, Duration::from_secs(1)),
        (common::SLOW, Duration::from_millis(10)),
    ] {
        let (control, mut audio) = launch("clap", id, Kind::Effect, vec![], timeout);
        let mut left = [0.25; 64];
        let mut right = left;
        audio.process(&mut left, &mut right);
        wait_failed(&control);
        for _ in 0..6 {
            left.fill(0.25);
            right.fill(0.25);
            assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
        }
        assert_eq!(left, [0.25; 64]);
        assert_eq!(audio.health().acknowledged_generation, 0);
        assert!(
            control
                .capture(1, 0, &[], Duration::from_millis(100))
                .is_err()
        );
        drop(audio);
        drop(control);
    }
}

#[test]
fn native_vst3_error_and_nan_do_not_escape_the_process_boundary() {
    for index in [4, 5] {
        let (control, mut audio) = launch(
            "vst3",
            &vst_id(index),
            Kind::Effect,
            vec![],
            Duration::from_secs(1),
        );
        let mut left = [0.5; 64];
        let mut right = left;
        audio.process(&mut left, &mut right);
        wait_failed(&control);
        for _ in 0..5 {
            left.fill(0.5);
            right.fill(0.5);
            assert_eq!(guarded(|| audio.process(&mut left, &mut right)), 0);
        }
        assert_eq!(left, [0.5; 64]);
        drop(audio);
        drop(control);
    }
}

#[test]
fn independent_offline_helpers_match_native_audio_for_unsplit_variable_callbacks() {
    for format in ["clap", "vst3"] {
        for frames in [1, 7, 64, 480, 512] {
            let id = if format == "clap" {
                common::GAIN.to_owned()
            } else {
                vst_id(0)
            };
            let mut options = options(
                format,
                &id,
                Kind::Effect,
                vec![gain(format)],
                Duration::from_secs(2),
            );
            options.offline = true;
            let (control, mut audio, _) = supervisor::launch(options).unwrap();
            let host = PluginHost::windfall();
            let module = host.load(&fixture(format)).unwrap();
            let mut reference = module.create(&id).unwrap();
            assert!(reference.set_param(7, 0.5));
            let mut native = reference.activate(48_000.0, 64).unwrap();
            native.set_realtime(false);
            let mut native_left: Vec<f32> =
                (0..4096).map(|index| (index % 31) as f32 / 31.0).collect();
            let mut native_right: Vec<f32> = native_left.iter().map(|value| -*value).collect();
            native.process(&mut native_left, &mut native_right);
            reference.deactivate(native).unwrap();
            let cancelled = std::sync::atomic::AtomicBool::new(false);
            let mut result = vec![];
            for source in (0..4096).step_by(frames) {
                let count = frames.min(4096 - source);
                let mut left = vec![0.0; count];
                let mut right = left.clone();
                for (index, (left, right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                    let input = ((source + index) % 31) as f32 / 31.0;
                    *left = input;
                    *right = -input;
                }
                audio
                    .process_offline(
                        &mut left,
                        &mut right,
                        Instant::now() + Duration::from_secs(2),
                        &cancelled,
                    )
                    .unwrap();
                result.extend_from_slice(&left);
            }
            // Measure actual added bridge delay, independently of a native
            // fixture's advertised latency. This VST3 fixture advertises 17
            // but intentionally has no native audio delay implementation.
            let extra = 128;
            assert!(result[..extra].iter().all(|sample| *sample == 0.0));
            for (index, actual) in result[extra..].iter().enumerate() {
                let expected = native_left[index];
                assert!(
                    (*actual - expected).abs() < 1e-7,
                    "{format} callback {frames}, sample {index}: {actual} vs {expected}"
                );
            }
            assert_eq!(audio.health().missed_blocks, 0);
            assert!(audio.health().completed_blocks > 0);
            drop(audio);
            drop(control);
        }
    }
}

#[test]
fn clap_instrument_measures_real_native_plus_bridge_latency_and_note_release() {
    let parameter = ParameterSpec {
        id: 1,
        min: 0.0,
        max: 1.0,
        value: 0.5,
        read_only: false,
        stepped: false,
    };
    let mut options = options(
        "clap",
        common::SINE,
        Kind::Instrument,
        vec![parameter],
        Duration::from_secs(2),
    );
    options.offline = true;
    let (control, mut audio, _) = supervisor::launch(options).unwrap();
    assert_eq!(audio.latency(), 160);
    assert!(audio.note_on(69, 1.0));
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let mut left = [0.0; 512];
    let mut right = left;
    audio
        .process_offline(
            &mut left,
            &mut right,
            Instant::now() + Duration::from_secs(2),
            &cancelled,
        )
        .unwrap();
    // A sine begins at zero; first nonzero is one frame after the exact onset.
    assert_eq!(
        left.iter().position(|sample| sample.abs() > 1e-6),
        Some(161)
    );
    assert!(audio.note_off(69));
    for _ in 0..3 {
        audio
            .process_offline(
                &mut left,
                &mut right,
                Instant::now() + Duration::from_secs(2),
                &cancelled,
            )
            .unwrap();
    }
    assert!(left.iter().all(|sample| *sample == 0.0));
    assert_eq!(audio.voices(), 0);
    drop(audio);
    drop(control);
}

#[test]
fn native_clap_instance_expression_audio_matches_reference_after_bridge_latency() {
    use windfall_plugin_host::{PluginNoteControls, bridge::slots::NoteSnapshot};
    let mut opts = options(
        "clap",
        common::SINE,
        Kind::Instrument,
        vec![],
        Duration::from_secs(2),
    );
    opts.offline = true;
    let (control, mut audio, _) = supervisor::launch(opts).unwrap();
    let host = PluginHost::windfall();
    let module = host.load(&fixture("clap")).unwrap();
    let mut instance = module.create(common::SINE).unwrap();
    let mut reference = instance.prepare_instrument(48_000.0, 64).unwrap();
    assert!(audio.supports_note_instances());
    assert!(audio.supports_note_pitch());
    let controls = PluginNoteControls {
        volume: 1.0,
        pan: -0.5,
        pitch: 69.5,
        modulation_x: 0.5,
        modulation_y: 0.5,
    };
    let a = NoteSnapshot {
        expression: None,
        id: windfall_plugin_host::FIRST_NOTE_INSTANCE_ID,
        key: 69,
        channel: 2,
        velocity: 0.5,
        controls,
    };
    let b = NoteSnapshot {
        expression: None,
        id: a.id + 1,
        controls: PluginNoteControls {
            pan: 0.5,
            pitch: 81.0,
            ..controls
        },
        ..a
    };
    assert_eq!(
        guarded(|| {
            assert!(audio.note_on_instance(a));
            assert!(audio.note_on_instance(b));
        }),
        0
    );
    assert!(reference.note_on_instance(a.id, a.key, a.channel, a.velocity, a.controls));
    assert!(reference.note_on_instance(b.id, b.key, b.channel, b.velocity, b.controls));
    let mut expected = Vec::new();
    let mut actual = Vec::new();
    for block in 0..12 {
        if block == 4 {
            assert_eq!(
                guarded(|| {
                    assert!(audio.note_off_instance(a.id, 0.25));
                }),
                0
            );
            assert!(reference.note_off_instance(a.id, 0.25));
        }
        let mut left = [0.0; 64];
        let mut right = left;
        reference.process(&mut left, &mut right);
        expected.extend(left.into_iter().zip(right));
        audio
            .process_offline(
                &mut left,
                &mut right,
                Instant::now() + Duration::from_secs(2),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        actual.extend(left.into_iter().zip(right));
    }
    assert_eq!(audio.voices(), 1);
    for ((left, right), (el, er)) in actual[128..].iter().zip(expected.iter()) {
        assert!(
            (left - el).abs() < 1e-6 && (right - er).abs() < 1e-6,
            "bridge expression PCM {left}/{right}, expected {el}/{er}"
        );
    }
    assert!(
        expected[320..]
            .iter()
            .any(|(l, r)| l.abs() + r.abs() > 0.01)
    );
    instance.release_instrument(reference).unwrap();
    drop(audio);
    assert!(control.terminate().reaped);
}

#[test]
fn helper_fixed_buffer_policy_reports_and_measures_its_added_latency() {
    let mut options = options(
        "clap",
        common::GAIN,
        Kind::Effect,
        vec![],
        Duration::from_secs(2),
    );
    options.offline = true;
    options.wrapper.fixed_block = Some(32);
    let (control, mut audio, _) = supervisor::launch(options).unwrap();
    assert_eq!(audio.latency(), 128 + 32);
    let cancelled = std::sync::atomic::AtomicBool::new(false);
    let mut observed = Vec::new();
    for block in 0..12 {
        let mut left =
            std::array::from_fn::<_, 64, _>(|index| ((block * 64 + index) % 31) as f32 / 64.0);
        let mut right = left.map(|value| -value);
        audio
            .process_offline(
                &mut left,
                &mut right,
                Instant::now() + Duration::from_secs(2),
                &cancelled,
            )
            .unwrap();
        observed.extend(left.into_iter().zip(right));
    }
    assert!(observed[..160].iter().all(|(l, r)| *l == 0.0 && *r == 0.0));
    for (index, (left, right)) in observed[160..].iter().enumerate() {
        let expected = (index % 31) as f32 / 64.0;
        assert!((left - expected).abs() < 1e-7 && (right + expected).abs() < 1e-7);
    }
    drop(audio);
    assert!(control.terminate().reaped);
}

#[test]
fn vst3_expression_identity_and_controls_replay_after_helper_timeline_reset() {
    use windfall_plugin_host::{
        FIRST_NOTE_INSTANCE_ID, PluginNoteControls, bridge::slots::NoteSnapshot,
    };
    let mut options = options(
        "vst3",
        &vst_id(23),
        Kind::Instrument,
        vec![],
        Duration::from_secs(2),
    );
    options.offline = true;
    let (control, mut audio, _) = supervisor::launch(options).unwrap();
    assert!(audio.supports_note_instances());
    assert!(audio.supports_note_pitch());
    assert_eq!(audio.latency(), 128);
    let a = NoteSnapshot {
        expression: None,
        id: FIRST_NOTE_INSTANCE_ID,
        key: 69,
        channel: 2,
        velocity: 0.5,
        controls: PluginNoteControls {
            volume: 1.0,
            pan: -0.5,
            pitch: 69.5,
            modulation_x: 0.25,
            modulation_y: 0.75,
        },
    };
    let b = NoteSnapshot {
        expression: None,
        id: a.id + 1,
        velocity: 0.25,
        controls: PluginNoteControls {
            volume: 1.0,
            pan: 0.5,
            pitch: 75.0,
            modulation_x: 0.5,
            modulation_y: 0.25,
        },
        ..a
    };
    assert_eq!(
        guarded(|| {
            assert!(audio.note_on_instance(a));
            assert!(audio.note_on_instance(b));
        }),
        0
    );
    let changed = PluginNoteControls {
        volume: 1.0,
        pan: -1.0,
        pitch: 72.0,
        modulation_x: 0.75,
        modulation_y: 0.5,
    };
    let mark_a = 0.5_f64 * (1.0 + 0.5 / 12.0 + 0.75 / 4.0 + 0.25 / 8.0);
    let mark_b = 0.25_f64 * (1.0 + 6.0 / 12.0 + 0.25 / 4.0 + 0.5 / 8.0);
    let changed_b = 0.25_f64 * (1.0 + 3.0 / 12.0 + 0.5 / 4.0 + 0.75 / 8.0);
    for phase in 0..5 {
        let expected = match phase {
            0 => [mark_a * 0.75 + mark_b * 0.25, mark_a * 0.25 + mark_b * 0.75],
            1 => {
                assert_eq!(
                    guarded(|| {
                        assert!(audio.set_note_controls(b.id, changed));
                    }),
                    0
                );
                [mark_a * 0.75 + changed_b, mark_a * 0.25]
            }
            2 => {
                assert_eq!(
                    guarded(|| {
                        assert!(audio.note_off_instance(a.id, 0.375));
                    }),
                    0
                );
                [changed_b, 0.0]
            }
            3 => {
                assert_eq!(guarded(|| audio.reset_timeline()), 0);
                [changed_b, 0.0]
            }
            _ => {
                assert_eq!(guarded(|| audio.all_notes_off()), 0);
                [0.0; 2]
            }
        };
        for block in 0..6 {
            let mut left = [0.0; 64];
            let mut right = left;
            audio
                .process_offline(
                    &mut left,
                    &mut right,
                    Instant::now() + Duration::from_secs(2),
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
            // Collection+scheduling add 128 frames. After reset allow the
            // documented 64-frame transition, then compare every actual frame.
            if block >= 3 {
                for (l, r) in left.iter().zip(&right) {
                    assert!(
                        (f64::from(*l) - expected[0]).abs() < 1e-6
                            && (f64::from(*r) - expected[1]).abs() < 1e-6,
                        "phase {phase}: {l}/{r} vs {expected:?}"
                    );
                }
            }
        }
        assert_eq!(audio.health().native_drops, 0);
        assert!(audio.health().acknowledged_generation > 0);
        assert_eq!(
            audio.voices(),
            if phase < 2 {
                2
            } else if phase < 4 {
                1
            } else {
                0
            }
        );
    }
    drop(audio);
    assert!(control.terminate().reaped);
}

#[test]
fn dynamic_clap_and_vst3_helpers_readmit_live_capabilities_into_fresh_mappings() {
    for format in ["clap", "vst3"] {
        let id = if format == "clap" {
            "org.windfall.test.dynamic".into()
        } else {
            vst_id(25)
        };
        let mut specs = vec![
            ParameterSpec {
                value: if format == "clap" { 1.0 } else { 0.5 },
                ..gain(format)
            },
            ParameterSpec {
                id: 12000,
                min: 0.0,
                max: 6.0,
                value: 0.0,
                read_only: false,
                stepped: true,
            },
            ParameterSpec {
                id: 31,
                min: 0.0,
                max: 1.0,
                value: 0.5,
                read_only: false,
                stepped: false,
            },
        ];
        let mut settings = options(
            format,
            &id,
            Kind::Effect,
            specs.clone(),
            Duration::from_secs(2),
        );
        settings.offline = true;
        let (mut control, mut audio, _) = supervisor::launch(settings).unwrap();
        for phase in 1..=6 {
            assert!(audio.set_param(12000, f64::from(phase)));
            specs
                .iter_mut()
                .find(|spec| spec.id == 12000)
                .unwrap()
                .value = f64::from(phase);
            for _ in 0..8 {
                audio
                    .process_offline(
                        &mut [0.0; 64],
                        &mut [0.0; 64],
                        Instant::now() + Duration::from_secs(2),
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                    .unwrap();
                if control.readmission_requested() {
                    break;
                }
            }
            assert!(control.readmission_requested(), "{format} phase {phase}");
            let old_owner = control.owner();
            let (state, parameters, layout) =
                control.readmit(1, &specs, Duration::from_secs(2)).unwrap();
            assert_eq!(layout.audio_outputs.len(), if phase == 2 { 3 } else { 1 });
            assert_eq!(layout.note_inputs, if phase == 4 { 2 } else { 1 });
            assert_eq!(parameters.iter().any(|p| p.spec.id == 31), phase != 3);
            assert_eq!(control.last_valid_readmission().unwrap().0, state);
            assert!(control.terminate().reaped);
            drop(audio);
            specs = parameters
                .into_iter()
                .filter(|param| !param.spec.read_only)
                .map(|param| param.spec.into())
                .collect();
            let mut settings = options(
                format,
                &id,
                Kind::Effect,
                specs.clone(),
                Duration::from_secs(2),
            );
            settings.offline = true;
            settings.state = state;
            let launched = supervisor::launch(settings).unwrap();
            control = launched.0;
            audio = launched.1;
            assert_ne!(control.owner().session, old_owner.session);
            assert_eq!(audio.latency(), 128 + if phase == 1 { 37 } else { 0 });
            for _ in 0..8 {
                audio
                    .process_offline(
                        &mut [0.0; 64],
                        &mut [0.0; 64],
                        Instant::now() + Duration::from_secs(2),
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                    .unwrap();
            }
            let mut measured = Vec::new();
            for block in 0..5 {
                let mut left = [0.0; 64];
                let mut right = left;
                if block == 0 {
                    left[0] = 1.0;
                    right[0] = 1.0;
                }
                audio
                    .process_offline(
                        &mut left,
                        &mut right,
                        Instant::now() + Duration::from_secs(2),
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                    .unwrap();
                measured.extend(left);
                if phase == 2 && block == 4 {
                    let mut mono = [[0.0; 2]; 64];
                    let mut stereo = mono;
                    assert_eq!(
                        guarded(|| {
                            assert!(audio.copy_output(1, &mut mono));
                            assert!(audio.copy_output(2, &mut stereo));
                        }),
                        0
                    );
                    assert_eq!(mono, [[0.11, 0.11]; 64]);
                    assert_eq!(stereo, [[0.21, 0.22]; 64]);
                }
            }
            assert_eq!(
                measured.iter().position(|value| *value != 0.0),
                Some(audio.latency()),
                "{format} phase {phase}"
            );
        }
        assert!(control.terminate().reaped);
    }
}
