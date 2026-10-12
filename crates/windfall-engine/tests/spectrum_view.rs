//! Current spectrum through the public engine frame, without an audio device.

use windfall_core::AudioBuffer;
use windfall_engine::{Processor, SamplePool};
use windfall_project::{
    Channel, ChannelId, ChannelSource, Project, SampleId, SamplerSettings, TrackId,
};

#[path = "../src/test_alloc.rs"]
mod test_alloc;

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

#[test]
fn mix_power_reaches_frames_without_cancellation_or_callback_allocation() {
    let mut project = Project::new("Spectrum fixture");
    let mut pool = SamplePool::new();
    pool.insert(
        SampleId(900),
        AudioBuffer::from_interleaved(
            48_000,
            2,
            [0.5, -0.5].into_iter().cycle().take(8192).collect(),
        ),
    );
    project.channels.push(Channel {
        sample_playback: None,
        id: ChannelId(901),
        name: "Spectrum source".into(),
        color: 0,
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        group: String::new(),
        voice: Default::default(),
        expression_map: Default::default(),
            sample_instrument: None,
        timing: Default::default(),
        mixer_track: TrackId::MASTER,
        source: ChannelSource::Sampler(SamplerSettings {
            sample: Some(SampleId(900)),
            ..Default::default()
        }),
    });
    let (mut processor, controller) = Processor::new(48_000);
    controller.set_project(&project, &pool);
    controller.note_on(ChannelId(901), 60, 1.0);
    assert!(controller.frame().spectrum.is_empty());
    let mut pcm = [0.0; 2048];
    assert_eq!(
        test_alloc::allocator_calls(|| processor.process(&mut pcm)),
        0
    );
    let frame = controller.frame();
    assert_eq!(frame.spectrum.len(), 64);
    assert!(
        frame
            .spectrum
            .iter()
            .all(|power| power.is_finite() && *power >= 0.0)
    );
    // Parseval's mean-square reference for the existing periodic Hann window.
    // Compute power from the actual output PCM, not from a duplicate FFT.
    let mut power = 0.0;
    let mut window_square = 0.0;
    for (index, samples) in pcm.as_chunks::<2>().0.iter().enumerate() {
        let window = 0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / 1024.0).cos();
        window_square += window * window;
        power += window
            * window
            * samples
                .iter()
                .map(|&sample| f64::from(sample).powi(2))
                .sum::<f64>()
            / 2.0;
    }
    let expected = power / window_square;
    assert!(expected > 0.01, "opposing channels must not cancel");
    assert!((f64::from(frame.spectrum.iter().sum::<f32>()) - expected).abs() < 1e-6);

    controller.seek(1.0);
    processor.process(&mut []);
    assert!(controller.frame().spectrum.is_empty());
    processor.process(&mut pcm);
    assert_eq!(controller.frame().spectrum.len(), 64);
    controller.set_project(&Project::new("Replacement"), &SamplePool::new());
    assert!(
        controller.frame().spectrum.is_empty(),
        "hide old plan before audio adoption"
    );
    processor.process(&mut []);
    assert!(controller.frame().spectrum.is_empty());
    processor.process(&mut pcm);
    assert_eq!(controller.frame().spectrum.len(), 64);
    drop(processor);
    assert!(controller.frame().spectrum.is_empty());
}

#[test]
fn bounded_queue_recovers_and_a_new_stream_has_no_old_spectrum() {
    let (mut processor, controller) = Processor::new(48_000);
    let mut large = [0.0; 33 * 256 * 2];
    assert_eq!(
        test_alloc::allocator_calls(|| processor.process(&mut large)),
        0
    );
    assert!(controller.frame().spectrum.is_empty());
    let mut pcm = [0.0; 2048];
    processor.process(&mut pcm);
    assert_eq!(controller.frame().spectrum, vec![0.0; 64]);
    drop(processor);
    assert!(controller.frame().spectrum.is_empty());
    let (mut processor, controller) = Processor::new(44_100);
    assert!(controller.frame().spectrum.is_empty());
    processor.process(&mut pcm);
    assert_eq!(controller.frame().spectrum, vec![0.0; 64]);
}
