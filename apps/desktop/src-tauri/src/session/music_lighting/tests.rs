use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone)]
struct Request {
    method: String,
    path: String,
    body: Value,
}
struct Server {
    endpoint: String,
    requests: Arc<Mutex<Vec<Request>>>,
    stopped: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("{base}/razer/chromasdk");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let thread = std::thread::spawn(move || {
            while !stop.load(Ordering::Acquire) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("accept failed: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0; 1024];
                let (header_end, length) = loop {
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                    assert!(bytes.len() < 16384);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.split_once(':')
                                    .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        break (end + 4, length);
                    }
                };
                while bytes.len() < header_end + length {
                    let count = stream.read(&mut chunk).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
                let mut first = headers.lines().next().unwrap().split_whitespace();
                let method = first.next().unwrap().to_owned();
                let path = first.next().unwrap().to_owned();
                let body = if length == 0 {
                    Value::Null
                } else {
                    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
                };
                lock(&seen).push(Request {
                    method: method.clone(),
                    path: path.clone(),
                    body,
                });
                let response = if method == "POST" {
                    json!({"sessionid":7,"uri":format!("{base}/session/7")})
                } else if path.ends_with("heartbeat") {
                    json!({"tick":1})
                } else {
                    json!({"result":0})
                };
                let body = serde_json::to_vec(&response).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        Self {
            endpoint,
            requests,
            stopped,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

#[test]
fn local_rest_server_records_session_heartbeat_cadence_all_device_payloads_and_clean_shutdown() {
    let server = Server::new();
    let mut driver = Driver::new(&server.endpoint).unwrap();
    let colors = [Color {
        device: 0,
        rgb: [4, 5, 6],
    }];
    assert!(driver.tick(0, &colors).unwrap());
    assert_eq!(lock(&server.requests).len(), 7);
    assert!(driver.tick(999, &colors).unwrap());
    assert!(
        !lock(&server.requests)
            .iter()
            .any(|r| r.path.ends_with("heartbeat"))
    );
    driver.tick(1000, &colors).unwrap();
    driver.tick(1999, &colors).unwrap();
    driver.tick(2000, &colors).unwrap();
    assert_eq!(
        lock(&server.requests)
            .iter()
            .filter(|r| r.path.ends_with("heartbeat"))
            .count(),
        2
    );
    driver.stop().unwrap();
    drop(driver);
    let requests = lock(&server.requests);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/razer/chromasdk");
    assert_eq!(requests[0].body["device_supported"], json!(DEVICES));
    for (index, device) in DEVICES.iter().enumerate() {
        assert_eq!(requests[index + 1].method, "PUT");
        assert_eq!(requests[index + 1].path, format!("/session/7/{device}"));
        assert_eq!(
            requests[index + 1].body,
            json!({"effect":"CHROMA_STATIC","param":{"color":0x060504}})
        );
    }
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.body["effect"] == "CHROMA_NONE")
            .count(),
        6
    );
    assert_eq!(requests.iter().filter(|r| r.method == "DELETE").count(), 1);
    assert_eq!(requests.last().unwrap().path, "/session/7");
}

#[test]
fn local_effect_cadence_combines_routes_and_clears_removed_devices() {
    let server = Server::new();
    let mut driver = Driver::new(&server.endpoint).unwrap();
    driver
        .tick(
            0,
            &[
                Color {
                    device: 1,
                    rgb: [10, 20, 30],
                },
                Color {
                    device: 1,
                    rgb: [30, 10, 40],
                },
            ],
        )
        .unwrap();
    assert_eq!(
        lock(&server.requests)[1].body["param"]["color"],
        json!(0x28141e)
    );
    driver
        .tick(
            49,
            &[Color {
                device: 2,
                rgb: [1, 2, 3],
            }],
        )
        .unwrap();
    assert_eq!(lock(&server.requests).len(), 2);
    driver
        .tick(
            50,
            &[Color {
                device: 2,
                rgb: [1, 2, 3],
            }],
        )
        .unwrap();
    assert_eq!(
        lock(&server.requests)[2].body,
        json!({"effect":"CHROMA_NONE"})
    );
    assert_eq!(lock(&server.requests)[3].path, "/session/7/mouse");
    assert!(!driver.tick(51, &[]).unwrap());
    assert_eq!(lock(&server.requests).last().unwrap().method, "DELETE");
}

#[test]
fn absent_service_has_a_clear_capability_message_and_external_session_uris_are_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/razer/chromasdk", listener.local_addr().unwrap());
    drop(listener);
    let mut driver = Driver::new(&endpoint).unwrap();
    let error = driver
        .tick(
            0,
            &[Color {
                device: 1,
                rgb: [1, 2, 3],
            }],
        )
        .unwrap_err();
    assert!(error.contains("Chroma REST service unavailable"));
    assert!(error.contains("local Chroma SDK service"));
    assert!(driver.adapter.uri.is_none());
    assert!(loopback("https://127.0.0.1/session").is_err());
    assert!(loopback("http://example.com/session").is_err());
    assert!(loopback("http://user:secret@localhost/session").is_err());
}

#[test]
fn real_mixer_meters_musical_clock_and_reported_automation_drive_lighting_colors() {
    use windfall_project::*;
    let mut doc = Document::new(Project::new("Lighting relay"));
    let sample = SampleId(
        doc.dispatch(
            Command::AddSample {
                name: "Meter source".into(),
                path: SamplePath::External("C:/meter.wav".into()),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let channel = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: None,
                sample: Some(sample),
                instrument: None,
                index: None,
                mixer_track: Some(TrackId::MASTER),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let effect = EffectId(
        doc.dispatch(
            Command::AddEffect {
                track: TrackId::MASTER,
                kind: EffectKind::MusicLighting,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    doc.dispatch(
        Command::Batch {
            label: None,
            commands: vec![
                Command::SetEffectParam {
                    track: TrackId::MASTER,
                    effect,
                    param: 0,
                    value: 1.0,
                },
                Command::SetEffectParam {
                    track: TrackId::MASTER,
                    effect,
                    param: 1,
                    value: 1.0,
                },
            ],
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::AddNotes {
            pattern: doc.project().patterns[0].id,
            channel,
            notes: vec![NoteInit {
                start: 0,
                length: 1920,
                key: 60,
                velocity: Some(1.0),
                pan: None,
                expression: None,
            }],
        },
        None,
    )
    .unwrap();
    let mut pool = windfall_engine::SamplePool::new();
    pool.insert(
        sample,
        windfall_core::AudioBuffer::from_interleaved(48000, 1, vec![0.5; 48000]),
    );
    let (mut processor, controller) = windfall_engine::Processor::new(48000);
    controller.set_project(doc.project(), &pool);
    controller.play();
    let mut pcm = vec![0.0; 12000];
    processor.process(&mut pcm);
    let frame = controller.frame();
    let head = controller.playhead();
    let colors = colors_for_project(doc.project(), &frame, &head);
    assert!(frame.meters[0] > 0.1);
    assert_eq!(colors.len(), 1);
    assert_eq!(colors[0].device, 1);
    assert!(colors[0].rgb[0] > 20);
    let expected = ((frame.meters[0].powi(2) + frame.meters[1].powi(2)) * 0.5).sqrt();
    assert_eq!(colors[0].rgb[0], (expected * 255.0).round() as u8);
    doc.dispatch(
        Command::SetEffectParam {
            track: TrackId::MASTER,
            effect,
            param: 0,
            value: 0.0,
        },
        None,
    )
    .unwrap();
    controller.set_project(doc.project(), &pool);
    processor.process(&mut pcm);
    let first =
        colors_for_project(doc.project(), &controller.frame(), &controller.playhead())[0].rgb;
    let before = controller.playhead().tick;
    doc.dispatch(
        Command::UpdateSettings {
            patch: SettingsPatch {
                tempo_bpm: Some(60.0),
                ..Default::default()
            },
        },
        None,
    )
    .unwrap();
    controller.set_project(doc.project(), &pool);
    processor.process(&mut pcm);
    let head = controller.playhead();
    assert!((head.ticks_per_second - 960.0).abs() < 1e-5);
    assert!((head.tick - before - 120.0).abs() < 1e-5);
    assert_ne!(
        colors_for_project(doc.project(), &controller.frame(), &head)[0].rgb,
        first
    );
    let automation = AutomationId(
        doc.dispatch(
            Command::AddAutomation {
                name: None,
                target: AutomationTarget::EffectParam {
                    track: TrackId::MASTER,
                    effect,
                    param: 2,
                },
                points: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let mut frame = controller.frame();
    frame.automated.push(windfall_ipc::AutomatedValue {
        automation,
        value: 0.0,
    });
    assert_eq!(
        colors_for_project(doc.project(), &frame, &head)[0].rgb,
        [0; 3]
    );
    let mut disabled = doc.project().clone();
    disabled.mixer.tracks[0]
        .effects
        .iter_mut()
        .find(|slot| slot.id == effect)
        .unwrap()
        .enabled = false;
    assert!(colors_for_project(&disabled, &frame, &head).is_empty());
}

#[test]
fn actual_lighting_worker_connects_off_audio_and_joins_with_session_uninitialisation() {
    let server = Server::new();
    let service = Service {
        endpoint: server.endpoint.clone(),
        ..Service::default()
    };
    service.publish(vec![Color {
        device: 1,
        rgb: [8, 16, 32],
    }]);
    let until = Instant::now() + Duration::from_secs(3);
    while !lock(&service.status).connected {
        assert!(
            Instant::now() < until,
            "worker status: {:?}",
            lock(&service.status)
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(
        lock(&server.requests)[1].body,
        json!({"effect":"CHROMA_STATIC","param":{"color":0x201008}})
    );
    drop(service);
    assert_eq!(lock(&server.requests).last().unwrap().method, "DELETE");
    assert_eq!(
        lock(&server.requests)
            .iter()
            .filter(|request| request.method == "DELETE")
            .count(),
        1
    );
}
