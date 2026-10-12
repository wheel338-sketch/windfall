use std::fs::File;
use std::io::Cursor;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;

use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_PCM_F32BE, CODEC_ID_PCM_F32LE, CODEC_ID_PCM_F64BE, CODEC_ID_PCM_F64LE,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioCodecParameters, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatReader, Track, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, ReadOnlySource};
use windfall_core::AudioBuffer;

use crate::error::{CodecError, UNREADABLE_CODEC, from_symphonia};
use crate::guard;

/// Default for [`DecodeOptions::max_decoded_bytes`]: 1.5 GB of samples, which
/// is a little over an hour of stereo at 48 kHz.
pub const DEFAULT_MAX_DECODED_BYTES: u64 = 1536 * 1024 * 1024;

/// Most bytes of decoded audio one byte of file is believed to hold. A header
/// can claim any length, and a claim beyond this ratio is ignored: the audio
/// is then measured as it is decoded instead. PCM is at most 4 to 1 and a
/// 128 kbit/s MP3 about 22 to 1. A file compressed harder than this only
/// loses the shortcut of being sized up front.
const MAX_BELIEVABLE_EXPANSION: u64 = 64;

const BYTES_PER_SAMPLE: u64 = size_of::<f32>() as u64;

/// Most samples set aside on a header's word, before any audio is decoded:
/// 1 MiB of them. A header is believed up to [`MAX_BELIEVABLE_EXPANSION`]
/// times the size of its file, which a file holding next to no audio can
/// claim as well, so the room beyond this is taken as the audio arrives.
const MAX_SAMPLES_UP_FRONT: usize = 1024 * 1024 / size_of::<f32>();

/// Limits applied while decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeOptions {
    /// Largest decoded size to accept, in bytes of 32-bit float samples
    /// (frames Ã— channels Ã— 4). Longer audio is rejected with
    /// [`CodecError::TooLarge`] instead of being cut short.
    pub max_decoded_bytes: u64,
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            max_decoded_bytes: DEFAULT_MAX_DECODED_BYTES,
        }
    }
}

/// What a file says about itself, read without decoding the audio.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInfo {
    pub sample_rate: u32,
    pub channels: u16,
    /// Length in frames, when the container states it. Encoder delay and
    /// padding are already left out.
    pub frames: Option<u64>,
    /// Length in seconds, when the container states it.
    pub duration_secs: Option<f64>,
    /// Short name of the container, such as `wave`, `flac`, `mp3` or `ogg`.
    pub format: String,
    /// Short name of the codec, such as `pcm_s16le`, `flac`, `mp3` or `vorbis`.
    pub codec: String,
    /// Bit depth of the stored samples. Lossy codecs do not have one.
    pub bits_per_sample: Option<u32>,
}

/// Decodes a whole audio file with the default limits. See
/// [`decode_file_with`].
pub fn decode_file(path: impl AsRef<Path>) -> Result<AudioBuffer, CodecError> {
    decode_file_with(path, &DecodeOptions::default())
}

/// Decodes a whole audio file into interleaved 32-bit float at the file's own
/// sample rate and channel count.
///
/// Encoder delay and padding are trimmed when the file records them. If the
/// file is damaged or cut short part-way through, the audio decoded before the
/// damage is returned; an error is returned only when nothing could be decoded.
///
/// A file that states more than 64 channels or a sample rate above 768 kHz
/// is refused with [`CodecError::UnsupportedFormat`], whatever else it holds.
pub fn decode_file_with(
    path: impl AsRef<Path>,
    options: &DecodeOptions,
) -> Result<AudioBuffer, CodecError> {
    let path = path.as_ref();
    if is_drumsynth(extension_of(path)) {
        return Ok(crate::render_drumsynth_file(path, options)?.audio);
    }
    try_both_ways(|seekable| {
        let (file, len) = open_file(path)?;
        let source = as_source(file, seekable);
        decode(source, len, extension_of(path), options, false)
    })
}

/// Decodes with the ordinary parser, shape and byte limits, refusing nonfinite
/// decoded samples with [`CodecError::Corrupt`] before sanitation, even when
/// earlier samples were finite. Ordinary decoding still replaces them with zero.
pub fn decode_file_strict_with(
    path: impl AsRef<Path>,
    options: &DecodeOptions,
) -> Result<AudioBuffer, CodecError> {
    let path = path.as_ref();
    if is_drumsynth(extension_of(path)) {
        return Ok(crate::render_drumsynth_file(path, options)?.audio);
    }
    try_both_ways(|seekable| {
        let (file, len) = open_file(path)?;
        decode(
            as_source(file, seekable),
            len,
            extension_of(path),
            options,
            true,
        )
    })
}

/// Streams strict finite PCM without retaining the complete source. The
/// callback also receives an empty block before each packet, allowing a caller
/// to cancel before IO/decoding. No retry/replay occurs after callback effects.
/// Decoder packets are capped at 65536 samples; emitted blocks at 4096 samples.
/// Unlike ordinary damaged-file recovery, decoder errors fail closed.
pub fn decode_file_blocks<E: From<CodecError>>(
    path: impl AsRef<Path>,
    options: &DecodeOptions,
    emit: impl FnMut(u32, u16, &[f32]) -> Result<(), E>,
) -> Result<(u32, u16, u64), E> {
    // Preserve ordinary decoder panic containment, with no fallback that
    // could replay the caller's file-writing or verification side effects.
    panic::catch_unwind(AssertUnwindSafe(|| {
        decode_file_blocks_inner(path.as_ref(), options, emit)
    }))
    .unwrap_or_else(|_| {
        Err(CodecError::Corrupt("the decoder could not make sense of it".into()).into())
    })
}

fn decode_file_blocks_inner<E: From<CodecError>>(
    path: &Path,
    options: &DecodeOptions,
    mut emit: impl FnMut(u32, u16, &[f32]) -> Result<(), E>,
) -> Result<(u32, u16, u64), E> {
    let (file, _) = open_file(path)?;
    let mut format = open_format(as_source(file, true), extension_of(path))?;
    let (track, params) = audio_track(format.as_ref())?;
    check_stated_layout(params)?;
    let id = track.id;
    let rate = params
        .sample_rate
        .ok_or_else(|| CodecError::Corrupt("missing stream rate".into()))?;
    let channels = channel_count(
        params
            .channels
            .as_ref()
            .map_or(0, |channels| channels.count()),
    )?;
    if rate == 0 || channels == 0 || channels > 2 {
        return Err(CodecError::UnsupportedFormat("streaming requires mono/stereo".into()).into());
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(from_symphonia)?;
    let mut block = vec![0.0_f32; 65536];
    let mut total = 0_u64;
    loop {
        emit(rate, channels, &[])?;
        let Some(packet) = format.next_packet().map_err(from_symphonia)? else {
            break;
        };
        if packet.track_id != id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(from_symphonia)?;
        if decoded.frames() == 0 {
            continue;
        }
        if decoded.spec().rate() != rate
            || decoded.spec().channels().count() != usize::from(channels)
        {
            return Err(CodecError::Corrupt("stream layout changed".into()).into());
        }
        let count = decoded.samples_interleaved();
        if count > block.len() {
            return Err(CodecError::TooLarge {
                limit_bytes: 65536 * 4,
            }
            .into());
        }
        total = total
            .checked_add(count as u64)
            .filter(|samples| *samples <= options.max_decoded_bytes / 4)
            .ok_or(CodecError::TooLarge {
                limit_bytes: options.max_decoded_bytes,
            })?;
        decoded.copy_to_slice_interleaved(&mut block[..count]);
        if block[..count].iter().any(|sample| !sample.is_finite()) {
            return Err(CodecError::Corrupt("nonfinite streamed sample".into()).into());
        }
        for samples in block[..count].chunks(4096 / usize::from(channels) * usize::from(channels)) {
            emit(rate, channels, samples)?;
        }
    }
    if total == 0 {
        return Err(CodecError::NoAudio.into());
    }
    Ok((rate, channels, total / u64::from(channels)))
}

/// Memory counterpart of [`decode_file_strict_with`].
pub fn decode_bytes_strict_with(
    bytes: &[u8],
    hint_ext: Option<&str>,
    options: &DecodeOptions,
) -> Result<AudioBuffer, CodecError> {
    if is_drumsynth(hint_ext) {
        return Ok(crate::render_drumsynth_source(bytes, options)?.audio);
    }
    if bytes.is_empty() {
        return Err(CodecError::NoAudio);
    }
    try_both_ways(|seekable| {
        decode(
            as_source(Cursor::new(bytes), seekable),
            bytes.len() as u64,
            hint_ext,
            options,
            true,
        )
    })
}

/// Decodes audio held in memory with the default limits. See
/// [`decode_bytes_with`].
pub fn decode_bytes(bytes: &[u8], hint_ext: Option<&str>) -> Result<AudioBuffer, CodecError> {
    decode_bytes_with(bytes, hint_ext, &DecodeOptions::default())
}

/// Decodes audio held in memory, exactly as [`decode_file_with`] decodes a
/// file. `hint_ext` is the file extension the bytes came with, if any. It
/// only speeds up format detection; a wrong hint does no harm.
pub fn decode_bytes_with(
    bytes: &[u8],
    hint_ext: Option<&str>,
    options: &DecodeOptions,
) -> Result<AudioBuffer, CodecError> {
    if is_drumsynth(hint_ext) {
        return Ok(crate::render_drumsynth_source(bytes, options)?.audio);
    }
    if bytes.is_empty() {
        return Err(CodecError::NoAudio);
    }
    try_both_ways(|seekable| {
        let source = as_source(Cursor::new(bytes), seekable);
        decode(source, bytes.len() as u64, hint_ext, options, false)
    })
}

/// Reads a file's sample rate, channel count, length and format from its
/// headers, without decoding the audio. A file that [`decode_file_with`]
/// refuses for its channel count or sample rate is refused here as well.
pub fn probe_file(path: impl AsRef<Path>) -> Result<AudioInfo, CodecError> {
    let path = path.as_ref();
    if is_drumsynth(extension_of(path)) {
        return Ok(drumsynth_info(
            &crate::render_drumsynth_file(path, &DecodeOptions::default())?.audio,
        ));
    }
    try_both_ways(|seekable| {
        let (file, _) = open_file(path)?;
        probe(as_source(file, seekable), extension_of(path))
    })
}

/// Reads the same information as [`probe_file`] from audio held in memory.
pub fn probe_bytes(bytes: &[u8], hint_ext: Option<&str>) -> Result<AudioInfo, CodecError> {
    if is_drumsynth(hint_ext) {
        return Ok(drumsynth_info(
            &crate::render_drumsynth_source(bytes, &DecodeOptions::default())?.audio,
        ));
    }
    if bytes.is_empty() {
        return Err(CodecError::NoAudio);
    }
    try_both_ways(|seekable| probe(as_source(Cursor::new(bytes), seekable), hint_ext))
}

/// Opens a file and returns it with its length in bytes.
fn open_file(path: &Path) -> Result<(File, u64), CodecError> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    if len == 0 {
        return Err(CodecError::NoAudio);
    }
    Ok((file, len))
}

fn extension_of(path: &Path) -> Option<&str> {
    path.extension().and_then(|extension| extension.to_str())
}

fn is_drumsynth(ext: Option<&str>) -> bool {
    ext.is_some_and(|ext| ext.eq_ignore_ascii_case("ds"))
}
fn drumsynth_info(audio: &AudioBuffer) -> AudioInfo {
    AudioInfo {
        sample_rate: audio.sample_rate(),
        channels: audio.channels(),
        frames: Some(audio.frames() as u64),
        duration_secs: Some(audio.duration_secs()),
        format: "drumsynth".into(),
        codec: "drumsynth-approximate".into(),
        bits_per_sample: None,
    }
}

/// Boxes a source for Symphonia, either as it is or as a stream that can only
/// be read forwards.
fn as_source<'s, R>(source: R, seekable: bool) -> Box<dyn MediaSource + 's>
where
    R: MediaSource + 's,
{
    if seekable {
        Box::new(source)
    } else {
        Box::new(ReadOnlySource::new(source))
    }
}

/// Runs `attempt` on a seekable source and, if the file turns out to be
/// damaged, once more on a forward-only one.
///
/// Given a seekable source, the AIFF and Ogg readers look at the end of the
/// file before they hand out any audio, and give up if a cut-off file ends
/// early. Reading the same file as a stream gets the audio that is there.
fn try_both_ways<T>(
    mut attempt: impl FnMut(bool) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    match guarded(|| attempt(true)) {
        Err(CodecError::Corrupt(reason)) => {
            guarded(|| attempt(false)).map_err(|_| CodecError::Corrupt(reason))
        }
        result => result,
    }
}

/// Runs decoder code and turns a panic inside it into an error. The decoders
/// are fed whatever the user drops on the app, and one of them tripping over a
/// malformed file must not take the app down.
fn guarded<T>(work: impl FnOnce() -> Result<T, CodecError>) -> Result<T, CodecError> {
    panic::catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| {
        Err(CodecError::Corrupt(
            "the decoder could not make sense of it".to_owned(),
        ))
    })
}

fn open_format<'s>(
    source: Box<dyn MediaSource + 's>,
    hint_ext: Option<&str>,
) -> Result<Box<dyn FormatReader + 's>, CodecError> {
    let stream = MediaSourceStream::new(source, Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = hint_ext {
        hint.with_extension(extension);
    }
    guard::open(stream, &hint).map_err(from_symphonia)
}

fn audio_track(format: &dyn FormatReader) -> Result<(&Track, &AudioCodecParameters), CodecError> {
    let no_track = || CodecError::UnsupportedFormat("it has no audio track".to_owned());
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(no_track)?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(no_track)?;
    Ok((track, params))
}

fn probe<'s>(
    source: Box<dyn MediaSource + 's>,
    hint_ext: Option<&str>,
) -> Result<AudioInfo, CodecError> {
    let format = open_format(source, hint_ext)?;
    let (track, params) = audio_track(format.as_ref())?;
    check_stated_layout(params)?;

    let sample_rate = params
        .sample_rate
        .ok_or_else(|| CodecError::Corrupt("the sample rate is missing".to_owned()))?;
    let channels = params
        .channels
        .as_ref()
        .map(|channels| channels.count())
        .filter(|&count| count > 0)
        .ok_or_else(|| CodecError::Corrupt("the channel count is missing".to_owned()))?;
    let codec = symphonia::default::get_codecs()
        .get_audio_decoder(params.codec)
        .ok_or_else(|| CodecError::UnsupportedFormat(UNREADABLE_CODEC.to_owned()))?
        .codec
        .info
        .short_name;

    let duration_secs = match track.num_frames {
        Some(frames) => Some(frames as f64 / f64::from(sample_rate)),
        None => track
            .time_base
            .zip(track.duration)
            .and_then(|(time_base, duration)| time_base.calc_duration(duration))
            .map(|time| time.as_secs_f64()),
    };

    Ok(AudioInfo {
        sample_rate,
        channels: channel_count(channels)?,
        frames: track.num_frames,
        duration_secs,
        format: format.format_info().short_name.to_owned(),
        codec: codec.to_owned(),
        bits_per_sample: params.bits_per_sample.or(float_bits(params.codec)),
    })
}

/// Turns a track down for a sample rate or channel count that no real file
/// has. It runs before a decoder is made, since the decoders size their
/// buffers by what the track states. The guard refuses most such files at
/// the header itself; this covers MP3, which it does not follow, and
/// anything a reader takes from a header in a way the guard does not.
fn check_stated_layout(params: &AudioCodecParameters) -> Result<(), CodecError> {
    if params.sample_rate == Some(0) {
        return Err(CodecError::Corrupt("the sample rate is zero".to_owned()));
    }
    let channels = params
        .channels
        .as_ref()
        .map_or(0, |channels| channels.count());
    guard::check_claim(channels, params.sample_rate.unwrap_or(0))
}

/// Bit depth of the float PCM codecs, which the container readers leave out.
fn float_bits(codec: AudioCodecId) -> Option<u32> {
    match codec {
        CODEC_ID_PCM_F32LE | CODEC_ID_PCM_F32BE => Some(32),
        CODEC_ID_PCM_F64LE | CODEC_ID_PCM_F64BE => Some(64),
        _ => None,
    }
}

fn channel_count(channels: usize) -> Result<u16, CodecError> {
    u16::try_from(channels)
        .map_err(|_| CodecError::UnsupportedFormat(format!("it has {channels} channels")))
}

/// The decoded samples so far, grown without ever aborting on a failed
/// allocation and without exceeding the caller's limit.
struct Sink {
    samples: Vec<f32>,
    /// The length the header promises, or zero if it promises none that is
    /// believed.
    promised: usize,
    limit_samples: usize,
    limit_bytes: u64,
}

impl Sink {
    fn new(options: &DecodeOptions) -> Self {
        Self {
            samples: Vec::new(),
            promised: 0,
            limit_samples: usize::try_from(options.max_decoded_bytes / BYTES_PER_SAMPLE)
                .unwrap_or(usize::MAX),
            limit_bytes: options.max_decoded_bytes,
        }
    }

    fn too_large(&self) -> CodecError {
        CodecError::TooLarge {
            limit_bytes: self.limit_bytes,
        }
    }

    /// Takes the length the header promises. A length over the limit is
    /// rejected right away, before any time is spent decoding. Otherwise the
    /// memory for it is set aside in one piece, up to
    /// [`MAX_SAMPLES_UP_FRONT`]; a longer buffer grows as it fills.
    fn expect(&mut self, samples: u64) -> Result<(), CodecError> {
        let samples = usize::try_from(samples)
            .ok()
            .filter(|&samples| samples <= self.limit_samples)
            .ok_or_else(|| self.too_large())?;
        self.promised = samples;
        self.samples
            .try_reserve_exact(samples.min(MAX_SAMPLES_UP_FRONT))
            .map_err(|_| CodecError::OutOfMemory)
    }

    /// Makes room for `additional` more samples and returns the slice to fill.
    fn grow(&mut self, additional: usize) -> Result<&mut [f32], CodecError> {
        let start = self.samples.len();
        let needed = start
            .checked_add(additional)
            .filter(|&needed| needed <= self.limit_samples)
            .ok_or_else(|| self.too_large())?;

        if needed > self.samples.capacity() {
            let doubled = needed.max(self.samples.capacity().saturating_mul(2));
            // Doubling stops at the promised length while the audio is still
            // within it, so a file that keeps its promise ends up in a buffer
            // of exactly its size.
            let target = if needed <= self.promised {
                doubled.min(self.promised)
            } else {
                doubled.min(self.limit_samples)
            };
            self.samples
                .try_reserve_exact(target - start)
                .map_err(|_| CodecError::OutOfMemory)?;
        }

        self.samples.resize(needed, 0.0);
        Ok(&mut self.samples[start..])
    }
}

fn decode<'s>(
    source: Box<dyn MediaSource + 's>,
    source_len: u64,
    hint_ext: Option<&str>,
    options: &DecodeOptions,
    strict: bool,
) -> Result<AudioBuffer, CodecError> {
    let mut format = open_format(source, hint_ext)?;
    let (track, params) = audio_track(format.as_ref())?;
    check_stated_layout(params)?;
    let track_id = track.id;
    let promised = track
        .num_frames
        .zip(params.channels.as_ref())
        .map(|(frames, channels)| frames.saturating_mul(channels.count() as u64));

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(from_symphonia)?;

    // Nothing is set aside for a file that turns out to have no decoder.
    let mut sink = Sink::new(options);
    if let Some(samples) = promised
        && samples.saturating_mul(BYTES_PER_SAMPLE)
            <= source_len.saturating_mul(MAX_BELIEVABLE_EXPANSION)
    {
        sink.expect(samples)?;
    }

    // Sample rate and channel count of the audio decoded so far.
    let mut layout: Option<(u32, usize)> = None;

    // The loop ends with the error that stopped it, or `None` at a clean end.
    let stopped_by = loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break None,
            Err(error) => break Some(error),
        };
        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(error) => break Some(error),
        };
        if decoded.frames() == 0 {
            continue;
        }

        let found = (decoded.spec().rate(), decoded.spec().channels().count());
        match layout {
            None => layout = Some(found),
            // One buffer holds one layout. The readers end the stream where a
            // second one with another layout is glued on, but that is theirs
            // to change, so it is checked here and only the first part kept.
            Some(first) if first != found => break None,
            Some(_) => {}
        }

        let block = sink.grow(decoded.samples_interleaved())?;
        decoded.copy_to_slice_interleaved(&mut *block);
        // Float files can carry NaN and infinity, which would spread through
        // every filter and meter downstream.
        for sample in block {
            if !sample.is_finite() {
                if strict {
                    return Err(CodecError::Corrupt(
                        "nonfinite decoded audio sample".to_owned(),
                    ));
                }
                *sample = 0.0;
            }
        }
    };

    let Some((sample_rate, channels)) = layout.filter(|_| !sink.samples.is_empty()) else {
        return Err(stopped_by.map_or(CodecError::NoAudio, from_symphonia));
    };
    if sample_rate == 0 || channels == 0 {
        return Err(CodecError::Corrupt(
            "the sample rate or channel count is zero".to_owned(),
        ));
    }

    Ok(AudioBuffer::from_interleaved(
        sample_rate,
        channel_count(channels)?,
        sink.samples,
    ))
}

#[cfg(test)]
mod tests {
    use symphonia::core::audio::Channels;

    use super::*;

    fn sink_limited_to(max_decoded_bytes: u64) -> Sink {
        Sink::new(&DecodeOptions { max_decoded_bytes })
    }

    #[test]
    fn a_panicking_decoder_becomes_an_error() {
        let result: Result<(), CodecError> = guarded(|| panic!("decoder bug"));
        assert!(matches!(result, Err(CodecError::Corrupt(_))));
    }

    #[test]
    fn the_sink_refuses_to_pass_its_limit() {
        let mut sink = sink_limited_to(40);
        assert_eq!(sink.grow(6).unwrap().len(), 6);
        assert_eq!(sink.grow(4).unwrap().len(), 4);
        assert!(matches!(
            sink.grow(1),
            Err(CodecError::TooLarge { limit_bytes: 40 })
        ));
        assert_eq!(sink.samples.len(), 10);
    }

    #[test]
    fn a_promised_length_over_the_limit_is_refused_up_front() {
        let mut sink = sink_limited_to(40);
        assert!(matches!(sink.expect(11), Err(CodecError::TooLarge { .. })));
        assert!(matches!(
            sink.expect(u64::MAX),
            Err(CodecError::TooLarge { .. })
        ));
        sink.expect(10).unwrap();
        assert!(sink.samples.capacity() >= 10);
    }

    #[test]
    fn a_track_stating_what_no_file_has_gets_no_decoder() {
        let track = |channels: u16, sample_rate: u32| {
            let mut params = AudioCodecParameters::new();
            params
                .with_channels(Channels::Discrete(channels))
                .with_sample_rate(sample_rate);
            params
        };
        assert!(check_stated_layout(&track(64, 768_000)).is_ok());
        // Whether a track that leaves both out can be decoded is for the
        // decoder to say.
        assert!(check_stated_layout(&AudioCodecParameters::new()).is_ok());

        let refusal = |channels, sample_rate| {
            check_stated_layout(&track(channels, sample_rate))
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            refusal(65, 44_100),
            "this is not a supported audio file (it has 65 channels)"
        );
        assert_eq!(
            refusal(2, 768_001),
            "this is not a supported audio file (it has a sample rate of 768001 Hz)"
        );
        assert_eq!(
            refusal(2, 0),
            "the audio file is damaged (the sample rate is zero)"
        );
    }

    #[test]
    fn a_long_promise_is_set_aside_only_in_part() {
        let mut sink = sink_limited_to(u64::MAX);
        sink.expect(100 * MAX_SAMPLES_UP_FRONT as u64).unwrap();
        assert_eq!(sink.samples.capacity(), MAX_SAMPLES_UP_FRONT);
    }

    #[test]
    fn a_kept_promise_ends_in_a_buffer_of_its_size() {
        let promised = 5 * MAX_SAMPLES_UP_FRONT + 123;
        let mut sink = sink_limited_to(u64::MAX);
        sink.expect(promised as u64).unwrap();
        while sink.samples.len() < promised {
            let block = 4_096.min(promised - sink.samples.len());
            sink.grow(block).unwrap();
            // Never more than twice what has arrived.
            let held = sink.samples.len().max(MAX_SAMPLES_UP_FRONT);
            assert!(sink.samples.capacity() <= 2 * held);
        }
        assert_eq!(sink.samples.capacity(), promised);

        // Audio the header did not promise is taken as well.
        assert_eq!(sink.grow(4_096).unwrap().len(), 4_096);
        assert_eq!(sink.samples.len(), promised + 4_096);
    }

    #[test]
    fn the_default_limit_is_over_an_hour_of_stereo() {
        let seconds = DEFAULT_MAX_DECODED_BYTES / BYTES_PER_SAMPLE / 2 / 48_000;
        assert!((3_600..7_200).contains(&seconds), "{seconds} s");
    }
}
