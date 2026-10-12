//! Real OS encode/decode acceptance. These tests are enabled and nonignored on
//! Windows; a missing OS codec fails the acceptance instead of skipping it.
#![cfg(windows)]
use windfall_codec::*;
use std::sync::atomic::{AtomicU32, Ordering};
static SERIAL: AtomicU32 = AtomicU32::new(0);
struct Folder(std::path::PathBuf);
impl Folder {
    fn new() -> Self { let p = std::env::temp_dir().join(format!("windfall-aac-{}-{}", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed))); std::fs::create_dir(&p).unwrap(); Self(p) }
    fn file(&self) -> std::path::PathBuf { self.0.join("test.m4a") }
}
impl Drop for Folder { fn drop(&mut self) { std::fs::remove_dir_all(&self.0).unwrap(); } }
fn signal(rate: u32, channels: u16, frames: usize) -> Vec<f32> {
    (0..frames).flat_map(|i| (0..channels).map(move |ch| {
        let t = i as f64 / f64::from(rate);
        let frequencies = if ch == 0 { [443.0, 1297.0, 3019.0] } else { [677.0, 2017.0, 5003.0] };
        (frequencies.iter().map(|f| (std::f64::consts::TAU * f * t + 0.37).sin()).sum::<f64>() * 0.15) as f32
    })).collect()
}
fn amplitude(pcm: &[f32], rate: u32, channels: u16, channel: usize, frequency: f64) -> f64 {
    let frames = pcm.len() / usize::from(channels);
    let (mut re, mut im, mut window_sum) = (0.0, 0.0, 0.0);
    for i in 2048..frames - 2048 {
        let window = 0.5 - 0.5 * (std::f64::consts::TAU * (i - 2048) as f64 / (frames - 4096) as f64).cos();
        let phase = std::f64::consts::TAU * frequency * i as f64 / f64::from(rate);
        let sample = f64::from(pcm[i * usize::from(channels) + channel]) * window;
        re += sample * phase.cos(); im += sample * phase.sin(); window_sum += window;
    }
    2.0 * re.hypot(im) / window_sum
}
#[test]
fn os_aac_multitone_decode_measures_layout_duration_alignment_spectral_peaks_and_snr() {
    m4a_capability().expect("Windows OS AAC encoder and decoder must be installed for acceptance");
    let folder = Folder::new();
    for rate in [44100, 48000] { for channels in [1, 2] { for bitrate in [96, 128, 160, 192] {
        let frames = rate as usize + 317;
        let pcm = signal(rate, channels, frames);
        let mut encoder = Encoder::open_with_tags(folder.file(), &EncoderSettings::M4a { bitrate }, rate, channels,
            &AudioTags { title: "Original multitone", author: "Windfall QA", ..Default::default() }).unwrap();
        for block in pcm.chunks(137 * usize::from(channels)) { encoder.write(block).unwrap(); }
        assert!(!folder.file().exists()); encoder.finalize().unwrap();
        let bytes = std::fs::read(folder.file()).unwrap();
        let container = inspect_m4a(&bytes).unwrap();
        assert_eq!(container.config, AacConfig { sample_rate: rate, channels });
        assert_eq!(container.frames(), frames as u64);
        assert!(container.trim.delay <= 4096); assert!(container.trim.padding > 0);
        assert!(bytes.windows(18).any(|b| b == b"Original multitone"));
        let decoded = decode_m4a_bytes(&bytes, &DecodeOptions::default()).unwrap();
        assert_eq!((decoded.sample_rate(), decoded.channels()), (rate, channels));
        assert!(decoded.frames().abs_diff(frames) <= 1024);
        assert_eq!(decoded.frames(), frames); // our edit list represents exact duration
        let channels_usize = usize::from(channels);
        let mut energy = 0.0; let mut noise = 0.0;
        for i in 2048 * channels_usize..(frames - 2048) * channels_usize {
            energy += f64::from(pcm[i]).powi(2);
            noise += f64::from(pcm[i] - decoded.samples()[i]).powi(2);
        }
        assert!(10.0 * (energy / noise.max(1e-20)).log10() > 20.0, "{rate}, {channels}, {bitrate}: aligned SNR");
        for ch in 0..channels_usize {
            let peaks = if ch == 0 { [443.0, 1297.0, 3019.0] } else { [677.0, 2017.0, 5003.0] };
            for f in peaks {
                let peak = amplitude(decoded.samples(), rate, channels, ch, f);
                assert!(peak > 0.10, "missing peak {f}");
                let floor = amplitude(decoded.samples(), rate, channels, ch, f + 37.0);
                assert!(peak > floor.max(1e-8) * 30.0, "peak {f} too near noise floor");
            }
        }
        std::fs::remove_file(folder.file()).unwrap();
    } } }
}
#[test]
fn aac_short_sources_and_partitioning_keep_exact_edit_list_length() {
    let folder = Folder::new();
    for frames in [1, 17, 1023, 1024, 1025, 4099] {
        let pcm = signal(48000, 2, frames);
        let mut encoder = Encoder::open(folder.file(), &EncoderSettings::M4a { bitrate: 192 }, 48000, 2).unwrap();
        for frame in pcm.chunks_exact(2) { encoder.write(frame).unwrap(); }
        encoder.finalize().unwrap();
        let bytes = std::fs::read(folder.file()).unwrap(); assert_eq!(inspect_m4a(&bytes).unwrap().frames(), frames as u64);
        assert_eq!(decode_m4a_bytes(&bytes, &DecodeOptions::default()).unwrap().frames(), frames);
        std::fs::remove_file(folder.file()).unwrap();
    }
}
#[test]
fn aac_cancel_failure_invalid_config_and_empty_input_leave_no_partial_file() {
    let folder = Folder::new(); let path = folder.file();
    let settings = EncoderSettings::M4a { bitrate: 192 };
    { let mut encoder = Encoder::open(&path, &settings, 48000, 2).unwrap(); encoder.write(&signal(48000, 2, 5000)).unwrap(); }
    assert!(!path.exists()); assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 0);
    std::fs::write(&path, b"previous").unwrap();
    let mut encoder = Encoder::open(&path, &settings, 48000, 2).unwrap();
    assert!(encoder.write(&[f32::INFINITY, 0.0]).is_err()); assert!(encoder.finalize().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous");
    let encoder = Encoder::open(&path, &settings, 48000, 2).unwrap(); assert!(encoder.finalize().is_err());
    assert!(Encoder::open(&path, &settings, 96000, 2).is_err());
    assert!(Encoder::open(&path, &EncoderSettings::M4a { bitrate: 320 }, 48000, 2).is_err());
    assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), b"previous");
}
