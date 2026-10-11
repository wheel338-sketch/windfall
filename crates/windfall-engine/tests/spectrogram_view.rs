//! Short analyzer history through the public engine frame, without a device.

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
fn publication_packs_existing_history_oldest_first_without_audio_allocation() {
    let mut project = Project::new("Spectrogram fixture");
    let mut pool = SamplePool::new();
    pool.insert(
        SampleId(900),
        AudioBuffer::from_interleaved(
            48_000,
            2,
            (0..8192)
                .flat_map(|frame| {
                    let amplitude = 0.05 + 0.9 * frame as f32 / 8192.0;
                    [amplitude, -amplitude]
                })
                .collect(),
        ),
    );
    project.channels.push(Channel {
        id: ChannelId(901),
        name: "History source".into(),
        color: 0,
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        group: String::new(),
        voice: Default::default(),
        expression_map: Default::default(),
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
    assert!(controller.frame().spectrogram.is_empty());
    let mut first = [0.0; 2048];
    assert_eq!(
        test_alloc::allocator_calls(|| processor.process(&mut first)),
        0
    );
    let first_frame = controller.frame();
    assert_eq!(first_frame.spectrogram, first_frame.spectrum);
    let mut expected = vec![first_frame.spectrum];
    let mut hop = [0.0; 512];
    for _ in 0..12 {
        assert_eq!(
            test_alloc::allocator_calls(|| processor.process(&mut hop)),
            0
        );
        let frame = controller.frame();
        assert_eq!(frame.spectrum.len(), 64);
        expected.push(frame.spectrum.clone());
        if expected.len() > windfall_engine::analyzers::SPECTROGRAM_SLICES {
            expected.remove(0);
        }
        assert_eq!(frame.spectrogram, expected.concat(), "chronological rows");
        assert!(frame.spectrogram.len() <= 16 * 64);
        assert!(
            frame
                .spectrogram
                .iter()
                .all(|power| power.is_finite() && *power >= 0.0)
        );
        // Frame reads copy analyzer history; they never add synthetic rows.
        assert_eq!(controller.frame().spectrogram, frame.spectrogram);
    }
    let totals: Vec<_> = expected.iter().map(|row| row.iter().sum::<f32>()).collect();
    assert!(
        totals.windows(2).all(|pair| pair[0] < pair[1]),
        "fixture distinguishes row order"
    );
}

#[test]
fn empty_history_and_seek_project_stream_invalidation() {
    let (mut processor, controller) = Processor::new(48_000);
    assert!(controller.frame().spectrogram.is_empty());
    let mut pcm = [0.0; 2048];
    processor.process(&mut pcm);
    assert_eq!(controller.frame().spectrogram, vec![0.0; 64]);

    controller.seek(1.0);
    processor.process(&mut []);
    assert!(controller.frame().spectrogram.is_empty());
    processor.process(&mut pcm);
    assert_eq!(controller.frame().spectrogram, vec![0.0; 64]);

    controller.set_project(&Project::new("Replacement"), &SamplePool::new());
    assert!(
        controller.frame().spectrogram.is_empty(),
        "pending plan hides history"
    );
    processor.process(&mut []);
    assert!(controller.frame().spectrogram.is_empty());
    processor.process(&mut pcm);
    assert_eq!(controller.frame().spectrogram, vec![0.0; 64]);
    drop(processor);
    assert!(controller.frame().spectrogram.is_empty());
    let (mut processor, controller) = Processor::new(44_100);
    assert!(controller.frame().spectrogram.is_empty());
    processor.process(&mut pcm);
    assert_eq!(controller.frame().spectrogram, vec![0.0; 64]);
}

#[test]
fn overflow_hides_history_until_a_fresh_window() {
    let (mut processor, controller) = Processor::new(48_000);
    let mut pcm = [0.0; 2048];
    processor.process(&mut pcm);
    assert!(!controller.frame().spectrogram.is_empty());
    let mut overflow = [0.0; 33 * 256 * 2];
    assert_eq!(
        test_alloc::allocator_calls(|| processor.process(&mut overflow)),
        0
    );
    assert!(controller.frame().spectrogram.is_empty());
    processor.process(&mut pcm);
    assert_eq!(controller.frame().spectrogram, vec![0.0; 64]);
}
