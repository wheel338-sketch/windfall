//! Native source decode/peaks and note timing against an independent pulse.
use super::Rig;
use windfall_codec::{WavSampleFormat, write_wav};
use windfall_core::AudioBuffer;
use windfall_project::{Command, NoteInit, SampleId};

#[test]
fn piano_waveform_reference_loads_native_peaks_and_retains_note_alignment_on_reopen() {
    let rig = Rig::new();
    let source = rig.folder.path().join("reference.wav");
    let mut pcm = vec![0.0; 48_000];
    pcm[24_000..24_480].fill(0.75);
    write_wav(&source, &AudioBuffer::from_interleaved(48_000, 1, pcm), WavSampleFormat::Float32).unwrap();
    let channel = rig.channel(0);
    rig.session.set_channel_sample_from_file(channel, &source.to_string_lossy()).unwrap();
    let project = rig.project();
    let sample = project.samples.iter().find(|sample| sample.name == "reference").unwrap().id;
    let info = rig.session.sample_info_by_id(sample).unwrap();
    assert_eq!(info.frames, 48_000);
    assert_eq!(info.duration_secs, 1.0);
    let buckets = info.peaks.len() / 2;
    let pulse = info.peaks.chunks_exact(2).position(|pair| pair[1] > 0.7).unwrap();
    let pulse_ticks = pulse as f64 / buckets as f64 * 1920.0;
    assert!((pulse_ticks - 960.0).abs() <= 1920.0 / buckets as f64);
    rig.session.dispatch(Command::AddNotes { pattern: rig.pattern(), channel,
        notes: vec![NoteInit { start: 960, length: 240, key: 60, velocity: None, pan: None, expression: None }] }, None).unwrap();
    let saved = rig.session.project_save(Some(&rig.file("waveform-reference.windfall"))).unwrap();
    rig.session.undo().unwrap();
    rig.session.project_open(&saved).unwrap();
    let reopened = rig.project();
    assert!(reopened.pattern(rig.pattern()).unwrap().lane(channel).unwrap().notes.iter().any(|note| note.start == 960));
    assert_eq!(rig.session.sample_info_by_id(sample).unwrap().peaks, info.peaks);
    rig.session.project_new().unwrap();
    assert!(rig.session.sample_info_by_id(SampleId(u32::MAX)).is_err());
}
