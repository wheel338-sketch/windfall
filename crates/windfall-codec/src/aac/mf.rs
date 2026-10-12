//! Thread-affine synchronous Media Foundation transforms. All work is performed
//! on an export/import worker, never the audio callback. COM is owned by RAII.
//! https://learn.microsoft.com/en-us/windows/win32/medfound/aac-encoder
//! https://learn.microsoft.com/en-us/windows/win32/medfound/aac-decoder
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::rc::Rc;
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use crate::{AacConfig, AacEncoderAdapter, AacPacket, AacTrim, AudioTags, CodecError, DecodeOptions, M4aWriter, MAX_AAC_PACKET_BYTES, MAX_M4A_PACKETS};
use windfall_core::AudioBuffer;

const ENCODER: GUID = GUID::from_u128(0x93af0c51_2275_45d2_a35b_f2ba21caed00);
const DECODER: GUID = GUID::from_u128(0x32d186a7_218f_4c75_8876_dd77273a8999);
fn error(e: windows::core::Error) -> CodecError {
    CodecError::Encoding(format!("Windows AAC / Media Foundation: {e}. Ensure the Windows Media Feature Pack is installed"))
}
fn fail(what: &str) -> CodecError { CodecError::Encoding(format!("Windows AAC: {what}")) }

#[derive(Debug)]
struct Runtime { com: bool, _thread: PhantomData<Rc<()>> }
impl Runtime {
    fn start() -> Result<Self, CodecError> {
        // SAFETY: startup and shutdown occur on this worker's thread. Changed
        // apartment mode means the caller already owns COM; do not uninitialize it.
        unsafe {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            let com = hr.is_ok();
            if !com && hr.0 != 0x80010106_u32 as i32 { return Err(error(windows::core::Error::from_hresult(hr))); }
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                if com { CoUninitialize(); }
                return Err(error(e));
            }
            Ok(Self { com, _thread: PhantomData })
        }
    }
}
impl Drop for Runtime {
    fn drop(&mut self) { unsafe { let _ = MFShutdown(); if self.com { CoUninitialize(); } } }
}
fn media(config: AacConfig, subtype: &GUID) -> Result<IMFMediaType, CodecError> {
    unsafe {
        let ty = MFCreateMediaType().map_err(error)?;
        ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(error)?;
        ty.SetGUID(&MF_MT_SUBTYPE, subtype).map_err(error)?;
        ty.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, u32::from(config.channels)).map_err(error)?;
        ty.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, config.sample_rate).map_err(error)?;
        ty.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(error)?;
        Ok(ty)
    }
}
fn pcm_type(config: AacConfig) -> Result<IMFMediaType, CodecError> {
    let ty = media(config, &MFAudioFormat_PCM)?;
    unsafe {
        let align = u32::from(config.channels) * 2;
        ty.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, align).map_err(error)?;
        ty.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, align * config.sample_rate).map_err(error)?;
        ty.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1).map_err(error)?;
    }
    Ok(ty)
}
fn asc(config: AacConfig) -> [u8; 2] {
    let index = if config.sample_rate == 44100 { 4 } else { 3 };
    ((2_u16 << 11) | (index << 7) | (config.channels << 3)).to_be_bytes()
}
fn aac_type(config: AacConfig, bitrate: u16) -> Result<IMFMediaType, CodecError> {
    let ty = media(config, &MFAudioFormat_AAC)?;
    unsafe {
        ty.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, u32::from(bitrate) * 125).map_err(error)?;
        ty.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0).map_err(error)?;
        ty.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0x29).map_err(error)?;
    }
    Ok(ty)
}
fn sample(bytes: &[u8], time: i64, duration: i64) -> Result<IMFSample, CodecError> {
    if bytes.is_empty() || bytes.len() > MAX_AAC_PACKET_BYTES || duration <= 0 { return Err(fail("invalid input packet size/duration")); }
    unsafe {
        let buffer = MFCreateMemoryBuffer(bytes.len() as u32).map_err(error)?;
        let mut data = std::ptr::null_mut();
        buffer.Lock(&mut data, None, None).map_err(error)?;
        // SAFETY: the OS allocated bytes.len() bytes and Lock succeeded.
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        buffer.Unlock().map_err(error)?;
        buffer.SetCurrentLength(bytes.len() as u32).map_err(error)?;
        let sample = MFCreateSample().map_err(error)?;
        sample.AddBuffer(&buffer).map_err(error)?;
        sample.SetSampleTime(time).map_err(error)?;
        sample.SetSampleDuration(duration).map_err(error)?;
        Ok(sample)
    }
}
fn sample_bytes(sample: &IMFSample) -> Result<Vec<u8>, CodecError> {
    unsafe {
        let buffer = sample.ConvertToContiguousBuffer().map_err(error)?;
        let size = buffer.GetCurrentLength().map_err(error)? as usize;
        if size == 0 || size > MAX_AAC_PACKET_BYTES { return Err(fail("invalid output packet size")); }
        let mut data = std::ptr::null_mut(); let mut current = 0;
        buffer.Lock(&mut data, None, Some(&mut current)).map_err(error)?;
        // SAFETY: locked buffer's current length is checked before copying.
        let bytes = if current as usize == size && !data.is_null() { Some(std::slice::from_raw_parts(data, size).to_vec()) } else { None };
        buffer.Unlock().map_err(error)?;
        bytes.ok_or_else(|| fail("output buffer changed while locked"))
    }
}
#[derive(Debug)]
struct Transform { mft: IMFTransform, config: AacConfig, decoding: bool }
impl Transform {
    fn encoder(config: AacConfig, bitrate: u16) -> Result<(Self, Vec<u8>), CodecError> {
        unsafe {
            let mft: IMFTransform = CoCreateInstance(&ENCODER, None, CLSCTX_INPROC_SERVER).map_err(error)?;
            mft.SetOutputType(0, &aac_type(config, bitrate)?, 0).map_err(error)?;
            mft.SetInputType(0, &pcm_type(config)?, 0).map_err(error)?;
            let output = mft.GetOutputCurrentType(0).map_err(error)?;
            let n = output.GetBlobSize(&MF_MT_USER_DATA).map_err(error)?;
            if n != 14 { return Err(fail("encoder did not supply the supported AAC-LC AudioSpecificConfig")); }
            let mut blob = vec![0; n as usize];
            output.GetBlob(&MF_MT_USER_DATA, &mut blob, None).map_err(error)?;
            if blob[12..] != asc(config) { return Err(fail("encoder returned a different AAC configuration")); }
            let t = Self { mft, config, decoding: false }; t.begin()?;
            Ok((t, blob[12..].to_vec()))
        }
    }
    fn decoder(config: AacConfig, config_bytes: &[u8]) -> Result<Self, CodecError> {
        if config_bytes != asc(config) { return Err(fail("unsupported AAC decoder configuration")); }
        unsafe {
            let mft: IMFTransform = CoCreateInstance(&DECODER, None, CLSCTX_INPROC_SERVER).map_err(error)?;
            let ty = aac_type(config, 192)?;
            let mut user = vec![0; 12]; user[2] = 0x29; user.extend_from_slice(config_bytes);
            ty.SetBlob(&MF_MT_USER_DATA, &user).map_err(error)?;
            mft.SetInputType(0, &ty, 0).map_err(error)?;
            mft.SetOutputType(0, &pcm_type(config)?, 0).map_err(error)?;
            let t = Self { mft, config, decoding: true }; t.begin()?; Ok(t)
        }
    }
    fn begin(&self) -> Result<(), CodecError> {
        unsafe {
            self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0).map_err(error)?;
            self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0).map_err(error)
        }
    }
    fn output(&self, emit: &mut dyn FnMut(Vec<u8>) -> Result<(), CodecError>) -> Result<usize, CodecError> {
        let mut emitted = 0;
        for _ in 0..4096 {
            unsafe {
                let info = self.mft.GetOutputStreamInfo(0).map_err(error)?;
                let provided = info.dwFlags & 0x100 != 0;
                let own = if provided { None } else {
                    if info.cbSize as usize > MAX_AAC_PACKET_BYTES { return Err(fail("output allocation budget exceeded")); }
                    let sample = MFCreateSample().map_err(error)?;
                    let buffer = MFCreateMemoryBuffer(info.cbSize.max(16384)).map_err(error)?;
                    sample.AddBuffer(&buffer).map_err(error)?; Some(sample)
                };
                let mut out = MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(own), dwStatus: 0, pEvents: ManuallyDrop::new(None) };
                let mut status = 0;
                let result = self.mft.ProcessOutput(0, std::slice::from_mut(&mut out), &mut status);
                // Both fields own COM references, including on an error. The
                // windows ABI struct uses ManuallyDrop, so explicitly release.
                let sample = ManuallyDrop::take(&mut out.pSample);
                drop(ManuallyDrop::take(&mut out.pEvents));
                match result {
                    Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(emitted),
                    Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE && self.decoding => {
                        self.mft.SetOutputType(0, &pcm_type(self.config)?, 0).map_err(error)?;
                    }
                    Err(e) => return Err(error(e)),
                    Ok(()) => {
                        if let Some(sample) = sample {
                            let bytes = sample_bytes(&sample)?; emit(bytes)?; emitted += 1;
                        } else { return Err(fail("transform returned no output sample")); }
                    }
                }
            }
        }
        Err(fail("transform failed to converge within the output budget"))
    }
    fn input(&self, bytes: &[u8], time: i64, duration: i64, emit: &mut dyn FnMut(Vec<u8>) -> Result<(), CodecError>) -> Result<(), CodecError> {
        let input = sample(bytes, time, duration)?;
        unsafe {
            match self.mft.ProcessInput(0, &input, 0) {
                Err(e) if e.code() == MF_E_NOTACCEPTING => {
                    if self.output(emit)? == 0 { return Err(fail("encoder/decoder made no progress")); }
                    self.mft.ProcessInput(0, &input, 0).map_err(error)?;
                }
                Err(e) => return Err(error(e)),
                Ok(()) => (),
            }
        }
        self.output(emit)?; Ok(())
    }
    fn drain(&self, emit: &mut dyn FnMut(Vec<u8>) -> Result<(), CodecError>) -> Result<(), CodecError> {
        unsafe {
            self.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0).map_err(error)?;
            self.mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0).map_err(error)?;
        }
        self.output(emit)?; Ok(())
    }
}
fn time(frame: u64, rate: u32) -> i64 { ((frame * 10_000_000) / u64::from(rate)) as i64 }
fn pcm_bytes(pcm: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(pcm.len() * 2);
    for &v in pcm { let v = (v.clamp(-1.0, 1.0) * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
        bytes.extend_from_slice(&v.to_le_bytes()); }
    bytes
}
fn decode_packets(config: AacConfig, asc: &[u8], packets: impl IntoIterator<Item = impl AsRef<[u8]>>, max_frames: usize) -> Result<Vec<f32>, CodecError> {
    let decoder = Transform::decoder(config, asc)?;
    let mut pcm = Vec::new();
    let mut emit = |bytes: Vec<u8>| {
        if bytes.len() % (usize::from(config.channels) * 2) != 0 { return Err(fail("decoder returned incomplete PCM")); }
        if pcm.len() + bytes.len() / 2 > max_frames * usize::from(config.channels) { return Err(fail("decoded frame budget exceeded")); }
        pcm.try_reserve(bytes.len() / 2).map_err(|_| CodecError::OutOfMemory)?;
        pcm.extend(bytes.chunks_exact(2).map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0));
        Ok(())
    };
    for (index, packet) in packets.into_iter().enumerate() {
        let frame = index as u64 * 1024;
        decoder.input(packet.as_ref(), time(frame, config.sample_rate), time(frame + 1024, config.sample_rate) - time(frame, config.sample_rate), &mut emit)?;
    }
    decoder.drain(&mut emit)?; Ok(pcm)
}
fn measure_delay(config: AacConfig, bitrate: u16) -> Result<u32, CodecError> {
    // The OS does not document a stable priming count. Measure the installed
    // encoder using a bounded original signal and a separate decoder instead
    // of guessing a constant or inferring delay from packet timestamps.
    let (encoder, asc) = Transform::encoder(config, bitrate)?;
    let mut seed = 0x5a17_8321_u32;
    let mut probe = vec![0.0_f32; 8192 * usize::from(config.channels)];
    let mut smooth = 0.0;
    for frame in 0..4096 {
        seed ^= seed << 13; seed ^= seed >> 17; seed ^= seed << 5;
        smooth = 0.75 * smooth + 0.25 * ((seed as i32 as f32) / i32::MAX as f32);
        for ch in 0..usize::from(config.channels) { probe[frame * usize::from(config.channels) + ch] = smooth * 0.6; }
    }
    let mut packets = Vec::new();
    let mut emit = |p: Vec<u8>| { if packets.len() >= 32 { return Err(fail("calibration packet budget exceeded")); } packets.push(p); Ok(()) };
    for (index, block) in probe.chunks(1024 * usize::from(config.channels)).enumerate() {
        let frame = index as u64 * 1024;
        encoder.input(&pcm_bytes(block), time(frame, config.sample_rate), time(frame + 1024, config.sample_rate) - time(frame, config.sample_rate), &mut emit)?;
    }
    encoder.drain(&mut emit)?;
    let decoded = decode_packets(config, &asc, &packets, 16384)?;
    let channels = usize::from(config.channels);
    let mut best = (0_u32, -1.0_f64);
    for delay in 0..=4096 {
        if decoded.len() / channels < delay + 3072 { break; }
        let (mut dot, mut a2, mut b2) = (0.0, 0.0, 0.0);
        for frame in 1024..3072 {
            let a = f64::from(probe[frame * channels]); let b = f64::from(decoded[(frame + delay) * channels]);
            dot += a * b; a2 += a * a; b2 += b * b;
        }
        let score = dot / (a2 * b2).sqrt().max(1e-20);
        if score > best.1 { best = (delay as u32, score); }
    }
    if best.1 < 0.90 { return Err(fail("could not establish encoder priming/alignment with the installed OS codec")); }
    Ok(best.0)
}
#[derive(Debug)]
pub(super) struct Encoder {
    transform: Transform, config: AacConfig, asc: Vec<u8>, pending: Vec<f32>,
    input_frames: u64, submitted: u64, packets: u32, delay: u32, failed: bool,
    // Declared last: transform interfaces are released before MFShutdown.
    _runtime: Runtime,
}
impl Encoder {
    pub(super) fn create(config: AacConfig, bitrate: u16) -> Result<Self, CodecError> {
        super::check_layout(config.sample_rate, config.channels, bitrate)?;
        let runtime = Runtime::start()?;
        let delay = measure_delay(config, bitrate)?;
        let (transform, asc) = Transform::encoder(config, bitrate)?;
        Ok(Self { transform, config, asc, pending: Vec::with_capacity(1024 * usize::from(config.channels)),
            input_frames: 0, submitted: 0, packets: 0, delay, failed: false, _runtime: runtime })
    }
    fn submit(&mut self, pcm: &[f32], emit: &mut dyn FnMut(AacPacket) -> Result<(), CodecError>) -> Result<(), CodecError> {
        let frames = (pcm.len() / usize::from(self.config.channels)) as u64;
        let mut packets = self.packets;
        self.transform.input(&pcm_bytes(pcm), time(self.submitted, self.config.sample_rate),
            time(self.submitted + frames, self.config.sample_rate) - time(self.submitted, self.config.sample_rate),
            &mut |bytes| { packets += 1; if packets as usize > MAX_M4A_PACKETS { return Err(fail("packet budget exceeded")); } emit(AacPacket { bytes }) })?;
        self.packets = packets; self.submitted += frames; Ok(())
    }
}
impl AacEncoderAdapter for Encoder {
    fn config(&self) -> AacConfig { self.config }
    fn encode(&mut self, pcm: &[f32], emit: &mut dyn FnMut(AacPacket) -> Result<(), CodecError>) -> Result<(), CodecError> {
        if self.failed { return Err(fail("encoder failed earlier")); }
        self.failed = true;
        let channels = usize::from(self.config.channels);
        if pcm.len() % channels != 0 || pcm.iter().any(|s| !s.is_finite()) { return Err(fail("finite complete PCM frames are required")); }
        self.input_frames = self.input_frames.checked_add((pcm.len() / channels) as u64)
            .filter(|n| *n <= MAX_M4A_PACKETS as u64 * 1024 - 8192).ok_or_else(|| fail("input duration budget exceeded"))?;
        for &v in pcm {
            self.pending.push(v);
            if self.pending.len() == 1024 * channels {
                let block = std::mem::replace(&mut self.pending, Vec::with_capacity(1024 * channels));
                self.submit(&block, emit)?;
            }
        }
        self.failed = false; Ok(())
    }
    fn finish(&mut self, emit: &mut dyn FnMut(AacPacket) -> Result<(), CodecError>) -> Result<AacTrim, CodecError> {
        if self.failed { return Err(fail("encoder failed earlier")); }
        self.failed = true;
        if self.input_frames == 0 { return Err(CodecError::NoAudio); }
        if !self.pending.is_empty() { let block = std::mem::take(&mut self.pending); self.submit(&block, emit)?; }
        // Ensure the complete last analysis window is emitted even on OS
        // versions whose drain drops a short trailing input. All this padding
        // is removed by the exact edit-list duration below.
        let silence = vec![0.0; 1024 * usize::from(self.config.channels)];
        for _ in 0..4 { self.submit(&silence, emit)?; }
        let mut packets = self.packets;
        self.transform.drain(&mut |bytes| { packets += 1;
            if packets as usize > MAX_M4A_PACKETS { return Err(fail("packet budget exceeded")); }
            emit(AacPacket { bytes }) })?;
        self.packets = packets;
        let padding = u64::from(packets) * 1024;
        let padding = padding.checked_sub(self.input_frames + u64::from(self.delay)).ok_or_else(|| fail("encoder omitted source frames"))?;
        Ok(AacTrim { delay: self.delay, padding: u32::try_from(padding).map_err(|_| fail("padding overflow"))? })
    }
}
#[derive(Debug)]
pub struct NativeAacWriter { encoder: Encoder, mux: M4aWriter }
impl NativeAacWriter {
    pub fn create(path: impl AsRef<Path>, config: AacConfig, bitrate: u16, tags: &AudioTags<'_>) -> Result<Self, CodecError> {
        let encoder = Encoder::create(config, bitrate)?;
        let mux = M4aWriter::create_with_asc(path, config, tags, &encoder.asc)?;
        Ok(Self { encoder, mux })
    }
    pub fn write(&mut self, pcm: &[f32]) -> Result<(), CodecError> {
        let mux = &mut self.mux; self.encoder.encode(pcm, &mut |p| mux.write_packet(&p.bytes))
    }
    pub fn finalize(mut self) -> Result<(), CodecError> {
        let mux = &mut self.mux;
        let trim = self.encoder.finish(&mut |p| mux.write_packet(&p.bytes))?;
        self.mux.finalize(trim)
    }
}
pub(super) fn decode_container(bytes: &[u8], options: &DecodeOptions) -> Result<AudioBuffer, CodecError> {
    let parsed = crate::inspect_m4a(bytes)?;
    let frames = parsed.frames();
    if parsed.trim.delay > 4096 || parsed.trim.padding > 8192 {
        return Err(CodecError::Corrupt("M4A: priming/padding exceeds the native adapter's bounded trim contract".into()));
    }
    if frames * u64::from(parsed.config.channels) * 4 > options.max_decoded_bytes {
        return Err(CodecError::TooLarge { limit_bytes: options.max_decoded_bytes });
    }
    let _runtime = Runtime::start()?;
    // Keep priming/padding in the decoder's allocation budget, then trim them.
    let mut pcm = decode_packets(parsed.config, &parsed.audio_specific_config,
        parsed.packet_ranges.iter().map(|r| &bytes[r.clone()]), parsed.packet_ranges.len() * 1024 + 2048)?;
    let channels = usize::from(parsed.config.channels);
    let start = parsed.trim.delay as usize * channels;
    let end = start.checked_add(frames as usize * channels).ok_or_else(|| fail("trim overflow"))?;
    if end > pcm.len() { return Err(fail("decoder returned fewer frames than the MP4 edit list")); }
    pcm.copy_within(start..end, 0); pcm.truncate(end - start);
    Ok(AudioBuffer::from_interleaved(parsed.config.sample_rate, parsed.config.channels, pcm))
}
