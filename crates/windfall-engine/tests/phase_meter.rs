//! Existing stereo correlation through the public engine frame, without a device.

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
fn existing_correlation_reaches_frames_without_callback_allocation() {
    for (stereo, expected) in [([0.5, 0.5], 1.0), ([0.5, -0.5], -1.0)] {
        let mut project = Project::new("Phase meter fixture");
        let mut pool = SamplePool::new();
        pool.insert(
            SampleId(900),
            AudioBuffer::from_interleaved(
                48_000,
                2,
                stereo.into_iter().cycle().take(8192).collect(),
            ),
        );
        project.channels.push(Channel {
            id: ChannelId(901),
            name: "Stereo source".into(),
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
        assert_eq!(controller.frame().correlation, None);
        let mut pcm = [0.0; 2048];
        assert_eq!(
            test_alloc::allocator_calls(|| processor.process(&mut pcm)),
            0
        );
        assert_eq!(controller.frame().correlation, Some(expected));
        assert_eq!(controller.frame().correlation, Some(expected));

        controller.seek(1.0);
        processor.process(&mut []);
        assert_eq!(controller.frame().correlation, None);
        processor.process(&mut pcm);
        assert_eq!(controller.frame().correlation, Some(expected));
        controller.set_project(&Project::new("Replacement"), &SamplePool::new());
        assert_eq!(controller.frame().correlation, None);
        drop(processor);
        assert_eq!(controller.frame().correlation, None);
    }
}

#[test]
fn silent_frames_have_no_correlation() {
    let (mut processor, controller) = Processor::new(48_000);
    assert_eq!(controller.frame().correlation, None);
    let mut pcm = [0.0; 2048];
    processor.process(&mut pcm);
    assert_eq!(pcm, [0.0; 2048]);
    assert_eq!(controller.frame().correlation, None);
}
