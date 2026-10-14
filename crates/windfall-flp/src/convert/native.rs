//! Public native layouts, not plugin execution. Attribution: DawVert
//! c6c8be9e0660d9e75a3acecce43a13b0dbe0029c (GPL-3.0-or-later),
//! data_main/datadef/fl_studio.ddef, data_main/dataset/fl_studio.dset,
//! functions_plugin/flp_dec_plugins.py; PyFLP f937126b888ce94271bfea631b89166c74056530
//! (GPL-3.0-or-later), pyflp/plugin.py. See NOTICE.md and coverage.md.
//!
//! A layout offset is NEVER assumed to be an automation number. Numbers
//! below are only the explicit `num` properties in the public dataset.
//! When the sources give a range but no physical-unit formula, we disclose
//! a linear range fit to the stand-in descriptor range, not acoustic parity.
use super::effects::{ParamLink, Translated};
use windfall_dsp::{EffectKind, InstrumentKind, InstrumentParams, ParamInfo};

#[derive(Clone, Copy)]
enum Scalar {
    Signed,
    Unsigned,
    Float,
    Byte,
    Word,
}

struct Field {
    offset: usize,
    scalar: Scalar,
    name: String,
    target: String,
    range: (f64, f64),
    number: Option<u16>,
    formula: Option<fn(f64) -> f64>,
}

impl Field {
    fn read(&self, bytes: &[u8]) -> Option<f64> {
        let b = bytes.get(self.offset..)?;
        let value = match self.scalar {
            Scalar::Byte => f64::from(*b.first()?),
            Scalar::Word => f64::from(u16::from_le_bytes(b.get(..2)?.try_into().ok()?)),
            Scalar::Signed => f64::from(i32::from_le_bytes(b.get(..4)?.try_into().ok()?)),
            Scalar::Unsigned => f64::from(u32::from_le_bytes(b.get(..4)?.try_into().ok()?)),
            Scalar::Float => f64::from(f32::from_le_bytes(b.get(..4)?.try_into().ok()?)),
        };
        value.is_finite().then_some(value)
    }
    fn link(&self, descriptors: &[ParamInfo]) -> Option<ParamLink> {
        let info = descriptors.iter().find(|p| p.id == self.target)?;
        Some(ParamLink {
            fl_param: self.number?,
            fl_range: self.range,
            id: info.id,
            convert: self.formula.unwrap_or(|v| v),
            destination_range: self
                .formula
                .is_none()
                .then_some((f64::from(info.min), f64::from(info.max))),
        })
    }
    fn value(&self, raw: f64, info: &ParamInfo) -> f32 {
        self.formula.map_or_else(
            || {
                let n =
                    ((raw - self.range.0) / (self.range.1 - self.range.0)).clamp(0.0, 1.0) as f32;
                info.min + n * (info.max - info.min)
            },
            |f| f(raw) as f32,
        )
    }
}

struct Layout {
    fields: Vec<Field>,
    minimum: usize,
    sslf: bool,
}
impl Layout {
    fn words(names: &str) -> Self {
        let fields: Vec<_> = names
            .split_whitespace()
            .enumerate()
            .map(|(i, name)| Field {
                offset: i * 4,
                scalar: Scalar::Signed,
                name: name.into(),
                target: String::new(),
                range: (0.0, 1.0),
                number: None,
                formula: None,
            })
            .collect();
        Self {
            minimum: fields.len() * 4,
            fields,
            sslf: false,
        }
    }
    fn map(&mut self, name: &str, target: &str, min: f64, max: f64, number: Option<u16>) {
        if let Some(f) = self.fields.iter_mut().find(|f| f.name == name) {
            f.target = target.into();
            f.range = (min, max);
            f.number = number;
        }
    }
    fn formula(&mut self, name: &str, f: fn(f64) -> f64) {
        if let Some(field) = self.fields.iter_mut().find(|field| field.name == name) {
            field.formula = Some(f);
        }
    }
    fn payload<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]> {
        if !self.sslf {
            return Some(bytes);
        }
        if bytes.get(..4)? != b"SSLF" {
            return None;
        }
        let len = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
        bytes.get(8..8usize.checked_add(len)?)
    }
    fn values(&self, bytes: &[u8], descriptors: &[ParamInfo]) -> Vec<(usize, f32)> {
        let Some(bytes) = self.payload(bytes).filter(|b| b.len() >= self.minimum) else {
            return Vec::new();
        };
        self.fields
            .iter()
            .filter_map(|f| {
                let (i, info) = descriptors
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.id == f.target)?;
                Some((i, f.value(f.read(bytes)?, info)))
            })
            .collect()
    }
    fn usable(&self, bytes: &[u8]) -> bool {
        self.payload(bytes).is_some_and(|bytes| {
            bytes.len() >= self.minimum
                && self
                    .fields
                    .iter()
                    .filter(|f| {
                        !f.target.is_empty()
                            || matches!(f.name.as_str(), "on" | "mute" | "channel" | "state")
                    })
                    .all(|f| {
                        f.read(bytes)
                            .is_some_and(|v| (f.range.0..=f.range.1).contains(&v))
                    })
        })
    }
}

fn instrument_layout(name: &str, size: usize) -> Option<(InstrumentKind, Layout)> {
    let (kind, mut l) = match name {
        "boobass" => {
            let mut l = Layout::words("version bass mid treble");
            // PyFLP explicitly supports both the old 12 and new 16 byte state.
            if size == 12 {
                l.fields.remove(0);
                for f in &mut l.fields {
                    f.offset -= 4;
                }
                l.minimum = 12;
            }
            l.map("bass", "body", 0.0, 65535.0, None);
            l.map("mid", "damping", 0.0, 65535.0, None);
            l.map("treble", "tone", 0.0, 65535.0, None);
            (InstrumentKind::FingerBass, l)
        }
        "plucked!" => {
            let mut l = Layout::words("decay color norm_decay release wide");
            l.map("decay", "decaySeconds", 0.0, 256.0, None);
            l.map("color", "brightness", 0.0, 128.0, None);
            (InstrumentKind::Pluck, l)
        }
        "fruit kick" | "fruity kick" => {
            let mut l =
                Layout::words("version max_freq min_freq decay_freq decay_vol osc_click osc_dist");
            l.map("min_freq", "pitchHz", -1200.0, 1200.0, None);
            // DawVert data_ext/plugts/flstudio_vst.pltr: min cents/100-36,
            // note2freq uses C4 as zero. Maximum frequency is handled jointly below.
            l.formula("min_freq", |v| {
                261.6255653005986 * 2.0_f64.powf((v / 100.0 - 36.0) / 12.0)
            });
            l.map("decay_freq", "pitchDecayMs", 0.0, 256.0, None);
            l.map("decay_vol", "decayMs", 0.0, 256.0, None);
            l.map("osc_click", "click", 0.0, 64.0, None);
            (InstrumentKind::Kick, l)
        }
        "fruity dx10" => {
            let mut l = Layout::words(
                "version amp_att amp_dec amp_rel mod_course mod_fine mod_init mod_time mod_sus mod_rel mod_velsen vibrato waveform mod_thru lforate mod2_course mod2_fine mod2_init mod2_time mod2_sus mod2_rel mod2_velsen octave",
            );
            for (source, target) in [
                ("amp_att", "operators.0.envelope.attackMs"),
                ("amp_dec", "operators.0.envelope.decayMs"),
                ("amp_rel", "operators.0.envelope.releaseMs"),
                ("mod_course", "operators.1.ratio"),
                ("mod_init", "operators.1.level"),
                ("mod_time", "operators.1.envelope.decayMs"),
                ("mod_sus", "operators.1.envelope.sustain"),
                ("mod_rel", "operators.1.envelope.releaseMs"),
                ("mod2_course", "operators.2.ratio"),
                ("mod2_init", "operators.2.level"),
                ("mod2_time", "operators.2.envelope.decayMs"),
                ("mod2_sus", "operators.2.envelope.sustain"),
                ("mod2_rel", "operators.2.envelope.releaseMs"),
            ] {
                l.map(source, target, 0.0, 65536.0, None);
            }
            // DawVert plugins/plugconv/ableton__n_flstudio.py, not a
            // linear knob fit: attack^6, decay^2, release^3 times 5 seconds.
            l.formula("amp_att", |v| 5000.0 * (v / 65536.0).powi(6));
            l.formula("amp_dec", |v| 5000.0 * (v / 65536.0).powi(2));
            l.formula("amp_rel", |v| 5000.0 * (v / 65536.0).powi(3));
            for s in ["mod_time", "mod2_time"] {
                l.formula(s, |v| 5000.0 * (v / 65536.0).powi(2));
            }
            for s in ["mod_rel", "mod2_rel"] {
                l.formula(s, |v| 5000.0 * (v / 65536.0).powi(3));
            }
            for s in ["mod_init", "mod2_init", "mod_sus", "mod2_sus"] {
                l.formula(s, |v| (v / 65536.0).powi(2));
            }
            for s in ["mod_course", "mod2_course"] {
                l.formula(s, |v| 2.0 * ((v / 65536.0).powi(2) * 40.0).floor());
            }
            (InstrumentKind::FourOp, l)
        }
        "fruity granulizer" => {
            let mut l = Layout::words(
                "reserved attack hold grain_spacing wave_spacing depth speed rand pan loop hold_2 time_Start trans_hold trans_mode key_to",
            );
            for (s, t, m) in [
                ("attack", "grainSizeMs", 500.0),
                ("grain_spacing", "density", 1000.0),
                ("rand", "spray", 256.0),
                ("time_Start", "position", 4096.0),
            ] {
                l.map(s, t, 0.0, m, None);
            }
            (InstrumentKind::GrainCloud, l)
        }
        "bassdrum" => {
            let mut l = Layout::words(
                "out_level sampling_ratio duration butt drive attack base peak slide_time var_base var_peak var_slide_time var_phase click_cut click_freq click_amount noise_a noise_d noise_s noise_start noise_pitch noise_mix noise_filter",
            );
            l.sslf = true;
            l.minimum = 1284 + l.fields.len() * 4;
            for f in &mut l.fields {
                f.offset += 1284;
                f.scalar = Scalar::Float;
            }
            for (s, t) in [
                ("out_level", "level"),
                ("sampling_ratio", "sampleMix"),
                ("duration", "decayMs"),
                ("base", "pitchHz"),
                ("slide_time", "pitchDecayMs"),
                ("click_amount", "click"),
            ] {
                l.map(s, t, -f64::from(f32::MAX), f64::from(f32::MAX), None);
                l.formula(s, |v| v);
            }
            (InstrumentKind::LayerKick, l)
        }
        "slicex" => {
            // Public header, followed by crossfade and the filename/WAV
            // payload decoded separately by `slices`.
            let mut l = Layout::words(
                "version play_pause live_selection master_level master_random_level master_lfo_level master_pitch mod_x mod_y layering",
            );
            l.map("master_level", "level", 0.0, 12800.0, None);
            l.formula("master_level", |v| v / 12800.0);
            l.fields[0].name = "version_1".into();
            l.fields[0].scalar = Scalar::Word;
            l.fields.push(Field {
                offset: 2,
                scalar: Scalar::Word,
                name: "version_2".into(),
                target: String::new(),
                range: (0.0, 65535.0),
                number: None,
                formula: None,
            });
            for (offset, name, scalar) in [
                (40, "crossfade", Scalar::Signed),
                (89, "data_exists", Scalar::Byte),
                (92, "sample_header", Scalar::Unsigned),
            ] {
                l.fields.push(Field {
                    offset,
                    scalar,
                    name: name.into(),
                    target: String::new(),
                    range: (0.0, 1.0),
                    number: None,
                    formula: None,
                });
            }
            (InstrumentKind::SliceDeck, l)
        }
        "fruity slicer" => {
            let mut l = Layout::words("version beats bpm pitch fitlen stretchtype attack decay");
            for f in &mut l.fields {
                if matches!(f.name.as_str(), "beats" | "bpm") {
                    f.scalar = Scalar::Float;
                }
            }
            l.map("decay", "releaseMs", 0.0, 128.0, None);
            l.formula("decay", |v| v / 128.0 * 100.0);
            (InstrumentKind::SliceMap, l)
        }
        _ => return None,
    };
    // Unsigned documented words; reserved/version fields do not become knobs.
    for f in &mut l.fields {
        if f.range.0 >= 0.0 && !matches!(f.scalar, Scalar::Float | Scalar::Byte | Scalar::Word) {
            f.scalar = Scalar::Unsigned;
        }
    }
    Some((kind, l))
}

pub(super) fn instrument(name: &str, state: &[u8]) -> Option<InstrumentParams> {
    let (kind, l) = instrument_layout(&name.trim().to_ascii_lowercase(), state.len())?;
    if !l.usable(state) {
        return None;
    }
    let mut params = kind.default_params();
    for (i, v) in l.values(state, kind.descriptors()) {
        params.set(i, v);
    }
    if name.trim().eq_ignore_ascii_case("Fruity DX10") {
        let word = |i: usize| {
            state
                .get(i * 4..i * 4 + 4)
                .and_then(|b| b.try_into().ok())
                .map(i32::from_le_bytes)
        };
        let mut set = |id: &str, v: f32| {
            if let Some(i) = kind.descriptors().iter().position(|p| p.id == id) {
                params.set(i, v);
            }
        };
        set("algorithm", 2.0); // FanIn: the two modulator branches feed carrier 1.
        set("operators.3.level", 0.0);
        for (coarse, fine, ratio, level) in [
            (4, 5, "operators.1.ratio", "operators.1.level"),
            (15, 16, "operators.2.ratio", "operators.2.level"),
        ] {
            let c = f64::from(word(coarse).unwrap_or(0)) / 65536.0;
            let f = f64::from(word(fine).unwrap_or(0)) / 65536.0;
            set(
                ratio,
                (2.0 * (c * c * 40.0).floor() * 2.0_f64.powf(f.powi(4) * 180.0 / 1200.0)) as f32,
            );
            if c == 0.0 && f == 0.0 {
                set(level, 0.0);
            }
        }
        if let Some(octave) = word(22).filter(|v| (-3..=3).contains(v)) {
            set(
                "operators.0.ratio",
                [0.0, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0][(octave + 3) as usize],
            );
        }
        if let Some(decay) = word(2) {
            set(
                "operators.0.envelope.sustain",
                (decay as f32 / 65536.0).powi(2),
            );
        }
    }
    if name.trim().eq_ignore_ascii_case("BassDrum")
        && let Some(drop) = bassdrum_drop(&l, state)
        && let Some(i) = kind
            .descriptors()
            .iter()
            .position(|p| p.id == "pitchDropSemitones")
    {
        params.set(i, drop);
    }
    if matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "fruit kick" | "fruity kick"
    ) && state.len() >= 28
    {
        let max = i32::from_le_bytes(state[4..8].try_into().ok()?);
        let min = i32::from_le_bytes(state[8..12].try_into().ok()?);
        let drop = ((f64::from(max) - f64::from(min)) / 100.0 + 48.0) as f32;
        if let Some(i) = kind
            .descriptors()
            .iter()
            .position(|p| p.id == "pitchDropSemitones")
        {
            params.set(i, drop);
        }
    }
    Some(params.sanitized())
}

/// BassDrum's sweep runs from `peak` down to `base`, both read in hertz
/// like `base` itself: the stand-in's drop is 12 log2(peak / base)
/// semitones, within its 0..60. Nothing is set unless both are positive.
fn bassdrum_drop(l: &Layout, state: &[u8]) -> Option<f32> {
    let bytes = l.payload(state)?;
    let read = |name: &str| l.fields.iter().find(|f| f.name == name)?.read(bytes);
    let (base, peak) = (read("base")?, read("peak")?);
    (base > 0.0 && peak > 0.0).then(|| (12.0 * (peak / base).log2()).clamp(0.0, 60.0) as f32)
}

/// The pitch classes of the stand-in's scales above their root, C = bit 0,
/// in the order of its `scale` choice (`PitchScale` in windfall-dsp).
const PITCH_SCALES: [u16; 4] = [0x0fff, 0x0ab5, 0x05ad, 0x0295];

/// Pitcher's twelve enabled keys, C first, as the pitch classes they allow.
fn pitcher_keys(l: &Layout, state: &[u8]) -> Option<u16> {
    let bytes = l.payload(state)?;
    let mut keys = 0_u16;
    for key in 0..12 {
        let on = l
            .fields
            .iter()
            .find(|f| f.name == format!("key_on.{key}"))?
            .read(bytes)?;
        if on != 0.0 {
            keys |= 1 << key;
        }
    }
    Some(keys)
}

/// The stand-in's root and scale with exactly these pitch classes: all
/// twelve are the chromatic scale; otherwise the first scale, then the
/// lowest root, that matches. None when no scale does.
fn pitcher_scale(keys: u16) -> Option<(u8, usize)> {
    if keys == 0x0fff {
        return Some((0, 0));
    }
    for (scale, mask) in PITCH_SCALES.iter().enumerate().skip(1) {
        for root in 0..12_u8 {
            let rotated = (0..12).fold(0_u16, |rotated, step| {
                if mask & (1 << step) != 0 {
                    rotated | 1 << ((step + u16::from(root)) % 12)
                } else {
                    rotated
                }
            });
            if rotated == keys {
                return Some((root, scale));
            }
        }
    }
    None
}

fn effect_layout(name: &str, size: usize) -> Option<(EffectKind, Layout)> {
    let (kind, mut l) = match name {
        "fruity center" => {
            let mut l = Layout::words("version on");
            if size == 4 {
                l.fields.remove(0);
                l.fields[0].offset = 0;
                l.minimum = 4;
            }
            (EffectKind::DcBlock, l)
        }
        "fruity mute 2" => {
            let mut l = Layout::words("version mute channel");
            for f in l.fields.iter_mut().skip(1) {
                f.range = (0.0, 1024.0);
            }
            (EffectKind::ChannelMute, l)
        }
        "fruity phase inverter" => {
            let mut l = Layout::words("version state");
            l.fields[1].range = (0.0, 1024.0);
            (EffectKind::Polarity, l)
        }
        "fruity stereo shaper" => {
            let mut l = Layout::words("reserved r2l l2l r2r l2r delay dephase iodiff prepost");
            for (s, t, n) in [
                ("r2l", "lr", 0),
                ("l2l", "ll", 1),
                ("r2r", "rr", 2),
                ("l2r", "rl", 3),
            ] {
                l.map(s, t, -25600.0, 25600.0, Some(n));
                l.formula(s, |v| v / 12800.0);
            }
            // Signed in the public dataset (-4096..4096): read as such.
            for f in l
                .fields
                .iter_mut()
                .filter(|f| matches!(f.name.as_str(), "delay" | "dephase"))
            {
                f.range = (-4096.0, 4096.0);
            }
            (EffectKind::StereoMatrix, l)
        }
        "fruity soft clipper" => {
            let mut l = Layout::words("threshold postgain");
            l.map("threshold", "ceilingDb", 0.0, 127.0, Some(0));
            (EffectKind::SoftClipper, l)
        }
        "fruity fast dist" => {
            let mut l = Layout::words("pre threshold type mix post");
            l.map("pre", "driveDb", 0.0, 192.0, Some(0));
            l.map("threshold", "shape", 0.0, 10.0, Some(1));
            l.map("post", "outputDb", 0.0, 128.0, Some(4));
            (EffectKind::Distortion, l)
        }
        "fruity fast lp" => {
            let mut l = Layout::words("version cutoff reso");
            l.map("cutoff", "cutoffHz", 0.0, 1024.0, Some(0));
            l.map("reso", "q", 0.0, 1024.0, Some(1));
            l.formula("cutoff", |v| {
                (100.0 * (0.2 + v / 1024.0).powf(1.5)).powi(2) * 925.0 / 2048.0
            });
            l.formula("reso", |v| 1.0 + 6.0 * v / 1024.0);
            (EffectKind::FastLowpass, l)
        }
        "fruity free filter" => {
            let mut l = Layout::words("version type freq q gain");
            l.minimum = 21;
            l.fields.push(Field {
                offset: 20,
                scalar: Scalar::Byte,
                name: "center".into(),
                target: String::new(),
                range: (0.0, 1.0),
                number: None,
                formula: None,
            });
            l.map("type", "mode", 0.0, 700.0, Some(0));
            l.formula("type", |v| {
                [0.0, 2.0, 1.0, 3.0, 4.0, 5.0, 6.0]
                    [((v / 700.0 / 0.125).floor() as i32 - 1).clamp(0, 6) as usize]
            });
            l.map("freq", "frequencyHz", 0.0, 1024.0, Some(1));
            l.map("q", "q", 0.0, 1024.0, Some(2));
            l.map("gain", "gainDb", 0.0, 1024.0, Some(3));
            // The public converter and dataset disagree on type/frequency
            // knob ranges. Preserve explicit dataset numbering, use the
            // converter's full seven-response range and frequency/Q/gain
            // fit for native values and automation, and report the dispute.
            l.formula("freq", |v| {
                10.0 * 1600.0_f64.powf((v / 1024.0).clamp(0.0, 1.0).powf(0.6))
            });
            l.formula("q", |v| ((0.01 + 0.99 * v / 1024.0) / 0.1).powf(0.8));
            l.formula("gain", |v| 18.0 * v / 1024.0);
            (EffectKind::SelectableFilter, l)
        }
        "fruity bass boost" => {
            let mut l = Layout::words("version freq amount");
            l.map("freq", "frequencyHz", 0.0, 1024.0, Some(0));
            l.map("amount", "gainDb", 0.0, 1024.0, Some(1));
            (EffectKind::BassShelf, l)
        }
        "fruity blood overdrive" => {
            let mut l = Layout::words(
                "version preband color preamp x100 postfilter postgain unknown_1 unknown_2",
            );
            if size == 32 {
                l.fields.remove(0);
                for f in &mut l.fields {
                    f.offset -= 4;
                }
                l.minimum = 32;
            }
            for (s, t, n) in [
                ("preband", "preBandHz", 0),
                ("color", "emphasis", 1),
                ("preamp", "driveDb", 2),
                ("postfilter", "toneHz", 4),
                ("postgain", "outputDb", 5),
            ] {
                l.map(s, t, 0.0, 10000.0, Some(n));
            }
            l.map("x100", "hot", 0.0, 1.0, Some(3));
            l.formula("x100", |v| v);
            (EffectKind::Overdrive, l)
        }
        "fruity chorus" => {
            let mut l = Layout::words(
                "version delay depth stereo lfo1_freq lfo2_freq lfo3_freq lfo1_wave lfo2_wave lfo3_wave crosstype crosscutoff wetonly",
            );
            for (s, t, max, n) in [
                ("delay", "delayMs", 1024.0, 0),
                ("depth", "depthMs", 5000.0, 1),
                ("stereo", "stereoPhase", 1024.0, 2),
                ("lfo1_freq", "rate1Hz", 5000.0, 3),
                ("lfo2_freq", "rate2Hz", 5000.0, 4),
                ("lfo3_freq", "rate3Hz", 5000.0, 5),
            ] {
                l.map(s, t, 0.0, max, Some(n));
            }
            // "Wet Only" (0..1024, number 11): above half the stand-in plays
            // wet only (mix 1, which it documents as wet only); below, its
            // even mix of 0.5 stays, as before this field was read.
            l.map("wetonly", "mix", 0.0, 1024.0, Some(11));
            l.formula("wetonly", |v| if v > 512.0 { 1.0 } else { 0.5 });
            (EffectKind::Chorus, l)
        }
        "fruity flanger" => {
            let mut l = Layout::words(
                "version delay depth rate phase damp shape feed inv_feedback inv_wet dry wet cross",
            );
            for (s, t, max, n) in [
                ("delay", "delayMs", 1024.0, 0),
                ("depth", "depthMs", 5000.0, 1),
                ("rate", "rateHz", 5000.0, 2),
                ("phase", "stereoPhase", 360.0, 3),
                ("damp", "damping", 5000.0, 4),
                ("shape", "shape", 1024.0, 5),
                ("feed", "feedback", 100.0, 6),
                ("inv_wet", "invertWet", 1024.0, 8),
                ("wet", "mix", 5000.0, 10),
            ] {
                l.map(s, t, 0.0, max, Some(n));
            }
            l.formula("rate", |v| 5.0 * (v / 5000.0).powi(7));
            l.formula("delay", |v| 20.0 * (v / 5000.0).powf(2.58495));
            l.formula("feed", |v| v / 100.0);
            l.formula("inv_wet", |v| if v > 512.0 { 1.0 } else { 0.0 });
            (EffectKind::Flanger, l)
        }
        "fruity phaser" => {
            let mut l = Layout::words(
                "version sweep_freq depth_min depth_max freq_range stereo num_stages feedback drywet gain",
            );
            for (s, t, max, n) in [
                ("sweep_freq", "rateHz", 5000.0, 0),
                ("depth_min", "minHz", 1000.0, 1),
                ("depth_max", "maxHz", 1000.0, 2),
                ("stereo", "stereoPhase", 1024.0, 4),
                ("feedback", "feedback", 1000.0, 6),
                ("drywet", "mix", 1024.0, 7),
            ] {
                l.map(s, t, 0.0, max, Some(n));
            }
            (EffectKind::Phaser, l)
        }
        "fruity flangus" => {
            let mut l = Layout::words("ord depth speed delay spread cross dry wet");
            for (s, t, max, n) in [
                ("ord", "order", 9.0, 0),
                ("depth", "depthMs", 1024.0, 1),
                ("speed", "rate1Hz", 1024.0, 2),
                ("delay", "delayMs", 128.0, 3),
                ("spread", "spread", 128.0, 4),
            ] {
                l.map(s, t, 0.0, max, Some(n));
            }
            l.map("wet", "mix", -128.0, 128.0, Some(7));
            (EffectKind::StackedFlanger, l)
        }
        "fruity 7 band eq" => {
            let mut l = Layout::words("version band_1 band_2 band_3 band_4 band_5 band_6 band_7");
            for i in 0..7 {
                l.map(
                    &format!("band_{}", i + 1),
                    &format!("gainsDb.{i}"),
                    -1800.0,
                    1800.0,
                    Some(i as u16),
                );
                l.formula(&format!("band_{}", i + 1), |v| v / 100.0);
            }
            (EffectKind::SevenBand, l)
        }
        "fruity stereo enhancer" => {
            let mut l = Layout::words("pan vol stereo phase_offs prepost phaseinvert");
            l.map("pan", "balance", -128.0, 128.0, Some(0));
            l.formula("pan", |v| v / 128.0);
            l.map("stereo", "width", -96.0, 96.0, Some(2));
            l.formula("stereo", |v| 1.0 - v / 96.0);
            l.map("phase_offs", "phaseOffsetMs", -512.0, 512.0, Some(3));
            l.map("phaseinvert", "invert", 0.0, 2.0, Some(5));
            l.formula("phaseinvert", |v| v);
            (EffectKind::StereoEnhancer, l)
        }
        "soundgoodizer" => {
            let mut l = Layout::words("version mode amount");
            if size == 8 {
                l.fields.remove(0);
                for f in &mut l.fields {
                    f.offset -= 4;
                }
                l.minimum = 8;
            }
            l.map("amount", "amount", 0.0, 1000.0, None);
            (EffectKind::OneKnob, l)
        }
        "transient processor" => {
            let mut l = Layout::words(
                "version attack drive release gain attack_shape release_shape split_freq split_balance effect_on display_flags display_scroll",
            );
            l.map("attack", "attack", 0.0, 160.0, None);
            l.map("release", "sustain", 0.0, 160.0, None);
            l.map("drive", "sensitivity", 0.0, 160.0, None);
            (EffectKind::TransientShaper, l)
        }
        "frequency shifter" => {
            let mut l = Layout::words(
                "version mix type frequency lr_phase shape_left shape_right feedback stereo freqtype start_phase mixer_track",
            );
            l.map("mix", "mix", 0.0, 10000.0, None);
            l.map("frequency", "shiftHz", -70000.0, 70000.0, None);
            (EffectKind::FrequencyShifter, l)
        }
        "spreader" => {
            let mut l = Layout::words("version spread stereo_sep low_bypass mono enabled theme");
            l.minimum = 30;
            for (i, n) in [(28, "prefx"), (29, "postfx")] {
                l.fields.push(Field {
                    offset: i,
                    scalar: Scalar::Byte,
                    name: n.into(),
                    target: String::new(),
                    range: (0.0, 1.0),
                    number: None,
                    formula: None,
                });
            }
            l.map("spread", "amount", 0.0, 10000.0, Some(0));
            l.map("stereo_sep", "width", -10000.0, 10000.0, Some(1));
            (EffectKind::Spreader, l)
        }
        "fruity panomatic" => {
            let mut l = Layout::words("pan vol lfo_shape lfo_target lfo_amount lfo_speed");
            l.map("pan", "pan", 0.0, 256.0, Some(0));
            l.formula("pan", |v| v / 128.0 - 1.0);
            l.map("vol", "gain", 0.0, 320.0, Some(1));
            l.formula("vol", |v| v / 256.0);
            l.map("lfo_shape", "shape", 0.0, 2.0, Some(2));
            l.formula("lfo_shape", |v| if v == 2.0 { 3.0 } else { v });
            l.map("lfo_amount", "depth", 0.0, 128.0, Some(4));
            l.map("lfo_speed", "rateHz", 0.0, 55000.0, Some(5));
            (EffectKind::PanLfo, l)
        }
        "fruity limiter" => {
            let mut l = Layout::words(
                "reserved gain sat limiter_ceil limiter_att limiter_att_curve limiter_rel limiter_rel_curve limiter_sus comp_thres comp_knee comp_ratio comp_att comp_rel comp_att_curve comp_sus noise_gain noise_thres noise_rel s_gain s_sat s_limiter_ceil s_limiter_att s_limiter_att_curve s_limiter_rel s_limiter_rel_curve s_limiter_sus s_comp_thres s_comp_knee s_comp_ratio s_comp_att s_comp_rel s_comp_att_curve s_comp_sus s_noise_gain s_noise_thres s_noise_rel",
            );
            l.map("limiter_ceil", "ceilingDb", 0.0, 2000.0, None);
            l.map("limiter_rel", "releaseMs", 0.0, 10000.0, None);
            for (s, t, m) in [
                ("gain", "inputGainDb", 2000.0),
                ("limiter_att", "lookaheadMs", 10000.0),
                ("comp_thres", "thresholdDb", 1000.0),
                ("comp_knee", "kneeDb", 1000.0),
                ("comp_ratio", "ratio", 1000.0),
                ("comp_att", "attackMs", 10000.0),
                ("noise_thres", "gateThresholdDb", 1000.0),
                ("noise_rel", "gateReleaseMs", 10000.0),
            ] {
                l.map(s, t, 0.0, m, None);
            }
            let names = "visual_scroll_speed unknown_1 unknown_2 unknown_3 mode unknown_5 unknown_6 unknown_7 visual_input_peak visual_output_peak visual_analysis visual_lvl_markers unknown_12 unknown_13 unknown_14 unknown_15 sidechain_number unknown_17 unknown_18 unknown_19 limiter_safety";
            for (i, n) in names.split_whitespace().enumerate() {
                l.fields.push(Field {
                    offset: 148 + i,
                    scalar: Scalar::Byte,
                    name: n.into(),
                    target: String::new(),
                    range: (0.0, 1.0),
                    number: None,
                    formula: None,
                });
            }
            l.minimum = 169;
            l.map("mode", "mode", 0.0, 1.0, None);
            l.formula("mode", |v| v);
            (EffectKind::Limiter, l)
        }
        "fruity delay 3" => {
            let mut l = Layout::words(
                "version wet_input delay_type tempo_sync keep_pitch delay_time offset smoothing stereo mod_rate mod_time mod_cutoff diffusion_level diffusion_spread feedbackdist_type feedback_level unknown_1 feedback_cutoff feedback_reso feedbackdist_level feedbackdist_knee feedbackdist_symmetry feedback_sample_rate feedback_bits wet tone dry",
            );
            for (s, t, max) in [
                ("feedback_level", "feedback", 6000.0),
                ("wet", "mix", 6000.0),
                ("offset", "stereoOffsetMs", 6000.0),
                ("mod_rate", "modRateHz", 6000.0),
                ("mod_time", "modDepthMs", 6000.0),
                ("feedback_cutoff", "highCutHz", 6000.0),
                ("feedbackdist_level", "saturation", 6000.0),
            ] {
                l.map(s, t, 0.0, max, None);
            }
            l.map("delay_time", "timeMs", 0.0, 768.0, None);
            (EffectKind::Delay, l)
        }
        "fruity delay bank" => {
            let mut l = Layout::words("version dry wet in fb");
            for (s, t, m) in [
                ("dry", "dry", 128.0),
                ("wet", "wet", 128.0),
                ("in", "inputGain", 128.0),
                ("fb", "feedback", 128.0),
            ] {
                l.map(s, t, 0.0, m, None);
                l.formula(s, |v| v / 128.0);
            }
            let names = "pan vol filter_cutoff filter_res filter_gain feedback_pan feedback_vol feedback_sep feedback_time feedback_ofs fbfilter_cut fbfilter_res fbfilter_gain out_pan out_vol out_next";
            let targets = [
                "inputPan",
                "inputGain",
                "filter.frequencyHz",
                "filter.q",
                "filter.gain",
                "feedbackPan",
                "feedback",
                "separation",
                "timeMs",
                "stereoOffsetMs",
                "feedbackFilter.frequencyHz",
                "feedbackFilter.q",
                "feedbackFilter.gain",
                "outputPan",
                "outputGain",
                "nextSend",
            ];
            let ranges = [
                (-128.0, 128.0),
                (0.0, 160.0),
                (0.0, 65536.0),
                (0.0, 65536.0),
                (0.0, 256.0),
                (-128.0, 128.0),
                (0.0, 128.0),
                (0.0, 128.0),
                (0.0, 7680.0),
                (-512.0, 512.0),
                (0.0, 65536.0),
                (0.0, 65536.0),
                (0.0, 256.0),
                (-128.0, 128.0),
                (-160.0, 160.0),
                (-160.0, 160.0),
            ];
            for bank in 0..8 {
                for (i, n) in names.split_whitespace().enumerate() {
                    l.fields.push(Field {
                        offset: 20 + bank * 64 + i * 4,
                        scalar: Scalar::Signed,
                        name: format!("bank_{}_v/{n}", bank + 1),
                        target: format!("units.{bank}.{}", targets[i]),
                        range: ranges[i],
                        number: None,
                        formula: None,
                    });
                }
            }
            l.fields.push(Field {
                offset: 532,
                scalar: Scalar::Unsigned,
                name: "2x_oversampling".into(),
                target: String::new(),
                range: (0.0, 1.0),
                number: None,
                formula: None,
            });
            for bank in 0..8 {
                let base = 536 + bank * 32;
                for (offset, n, t, max, scalar) in [
                    (0, "filter_type", "filter.mode", 7.0, Scalar::Signed),
                    (4, "filter_mode", "filter.sections", 2.0, Scalar::Signed),
                    (
                        8,
                        "fbfilter_type",
                        "feedbackFilter.mode",
                        7.0,
                        Scalar::Signed,
                    ),
                    (
                        12,
                        "fbfilter_mode",
                        "feedbackFilter.sections",
                        2.0,
                        Scalar::Signed,
                    ),
                    (16, "filter_post", "filterPost", 1.0, Scalar::Byte),
                    (17, "feedback_mode", "feedbackMode", 3.0, Scalar::Byte),
                    (20, "grain_div", "", 4096.0, Scalar::Signed),
                    (25, "grain_sh", "", 128.0, Scalar::Signed),
                    (29, "enabled", "enabled", 1.0, Scalar::Byte),
                    (30, "feedback_leave_first_echo", "", 1.0, Scalar::Byte),
                    (31, "feedback_tempo", "sync", 1.0, Scalar::Byte),
                ] {
                    l.fields.push(Field {
                        offset: base + offset,
                        scalar,
                        name: format!("bank_{}_e/{n}", bank + 1),
                        target: if t.is_empty() {
                            String::new()
                        } else {
                            format!("units.{bank}.{t}")
                        },
                        range: (0.0, max),
                        number: None,
                        formula: Some(|v| v),
                    });
                }
            }
            l.minimum = 792;
            (EffectKind::EchoBank, l)
        }
        "multiband delay" => {
            let mut l = Layout::words("version reserved");
            // The public definition is marked UNFINISHED. All eight named
            // banks of 16 delay/volume/pan floats are nevertheless explicit,
            // and each lands on the stand-in's sixteen-band bank of the same
            // number.
            for bank in 0..8 {
                for row in 0..3 {
                    for band in 0..16 {
                        let n = ["delay", "vol", "pan"][row];
                        l.fields.push(Field {
                            offset: 8 + bank * 192 + row * 64 + band * 4,
                            scalar: Scalar::Float,
                            name: format!("bank_{}/{n}.{band}", bank + 1),
                            // Raw float taken in the stand-in's own unit
                            // (ms, gain, pan) and clamped to its range: the
                            // public definition names the arrays, no units.
                            target: format!(
                                "banks.{bank}.{}.{band}",
                                ["timeMs", "level", "pan"][row]
                            ),
                            range: (-f64::from(f32::MAX), f64::from(f32::MAX)),
                            number: None,
                            formula: Some(|v| v),
                        });
                    }
                }
            }
            l.minimum = 1544;
            (EffectKind::BandDelay, l)
        }
        "pitcher" => {
            let mut l = Layout::words("header correction_speed stereo gender finetune bypass_all");
            // The header is four bytes in the SSLF payload; subsequent named
            // controls are floats, including the twelve bypass values.
            let tail = "voice1_pan voice2_pan voice3_pan voice4_pan reserved_2 formant hz ref_chan use_midi octaves harmonize selector_key selector_type harmonize_mix harmonize_vel min_freq";
            for i in 0..12 {
                l.fields.push(Field {
                    offset: 24 + i * 4,
                    scalar: Scalar::Float,
                    name: format!("bypass.{i}"),
                    target: String::new(),
                    range: (0.0, 1.0),
                    number: None,
                    formula: None,
                });
            }
            for (i, n) in tail.split_whitespace().enumerate() {
                l.fields.push(Field {
                    offset: 72 + i * 4,
                    scalar: Scalar::Float,
                    name: n.into(),
                    target: String::new(),
                    range: (0.0, 1.0),
                    number: None,
                    formula: None,
                });
            }
            for f in l.fields.iter_mut().skip(1) {
                f.scalar = Scalar::Float;
            }
            for (s, t) in [
                ("correction_speed", "speedMs"),
                ("hz", "tuningHz"),
                ("harmonize_mix", "harmonyMix"),
            ] {
                l.map(s, t, -f64::from(f32::MAX), f64::from(f32::MAX), None);
                l.formula(s, |v| v);
            }
            l.formula("hz", |v| 415.3 + v * 50.9);
            for i in 0..12 {
                l.fields.push(Field {
                    offset: 136 + i,
                    scalar: Scalar::Byte,
                    name: format!("key_on.{i}"),
                    target: String::new(),
                    range: (0.0, 1.0),
                    number: None,
                    formula: None,
                });
            }
            l.sslf = true;
            l.minimum = 152;
            (EffectKind::PitchCorrect, l)
        }
        "vintage chorus" => {
            let mut l = Layout::words(
                "version mode reserved mod_speed mod_shape_sine mod_phase_lr delay_time1 delay_time2 wet mono_input invert_wet delay_feedback delay_hp noise_gain noise noise_gate mod_tempo_sync mod_phase_start",
            );
            for (s, t, max) in [
                ("mod_speed", "rateHz", 245760.0),
                ("delay_time1", "delayMs", 6000.0),
                ("wet", "mix", 6000.0),
                ("mod_phase_lr", "spread", 6000.0),
            ] {
                l.map(s, t, 0.0, max, None);
            }
            l.map("mode", "mode", 0.0, 2.0, None);
            l.formula("mode", |v| v);
            // A switch without a public range: any value but zero is on.
            // The synced cycle is set from the fitted rate below.
            l.map("mod_tempo_sync", "sync", 0.0, f64::from(u32::MAX), None);
            l.formula("mod_tempo_sync", |v| if v != 0.0 { 1.0 } else { 0.0 });
            (EffectKind::VintageChorus, l)
        }
        "vintage phaser" => {
            let mut l = Layout::words(
                "version phaser_min phaser_max phaser_feedback phaser_manual phaser_freq phaser_hq mod_speed mod_tempo mod_shape_sine mod_tension mod_phase_start mod_phase_lr delay_on delay_keeppitch delay_mix delay_stereo delay_time delay_tempo delay_feedback delay_lp delay_hp levels_mix levels_mono levels_noise levels_noise_gate color_gain color_freq input_hp feedback_hp rate_freq1 rate_freq2 lfo_scale lfo_pull",
            );
            for (s, t, max) in [
                ("phaser_min", "minHz", 2048000.0),
                ("phaser_max", "maxHz", 2048000.0),
                ("phaser_feedback", "feedback", 6000.0),
                ("mod_speed", "rateHz", 245760.0),
                ("mod_phase_lr", "stereoPhase", 6000.0),
                ("levels_mix", "mix", 100.0),
                // The dataset's range for the feedback color, fitted like
                // the sweep's minimum and maximum to the stand-in's range.
                ("color_freq", "colorHz", 650000.0),
            ] {
                l.map(s, t, 0.0, max, None);
            }
            // A switch without a public range: any value but zero is on.
            // The synced cycle is set from the fitted rate below.
            l.map("mod_tempo", "sync", 0.0, f64::from(u32::MAX), None);
            l.formula("mod_tempo", |v| if v != 0.0 { 1.0 } else { 0.0 });
            (EffectKind::VintagePhaser, l)
        }
        "fruity multiband compressor" => {
            let mut l = Layout::words(
                "version gain limiter filter_type hi_state hi_freq hi_gain hi_thres hi_ratio hi_att hi_rel hi_knee mid_state mid_freq_l mid_freq_h mid_gain mid_thres mid_ratio mid_att mid_rel mid_knee low_state low_freq low_gain low_thres low_ratio low_att low_rel low_knee unknown visual_speed",
            );
            l.map("low_freq", "lowCrossoverHz", 0.0, 20000.0, None);
            l.formula("low_freq", |v| v);
            l.map("hi_freq", "highCrossoverHz", 0.0, 20000.0, None);
            l.formula("hi_freq", |v| v);
            for (s, t) in [("low", "low"), ("mid", "mid"), ("hi", "high")] {
                for (field, target, max) in [
                    ("thres", "thresholdDb", 65536.0),
                    ("ratio", "ratio", 65536.0),
                    ("gain", "makeupDb", 65536.0),
                    ("att", "attackMs", 1000.0),
                    ("rel", "releaseMs", 10000.0),
                ] {
                    l.map(
                        &format!("{s}_{field}"),
                        &format!("{t}.{target}"),
                        0.0,
                        max,
                        None,
                    );
                    if matches!(field, "att" | "rel") {
                        l.formula(&format!("{s}_{field}"), |v| v);
                    }
                }
            }
            (EffectKind::MultibandCompressor, l)
        }
        "tuner" => {
            let mut l = Layout::words("version refrence reactivity bass display_mode");
            l.map("refrence", "referenceHz", 0.0, 6000.0, None);
            l.formula("refrence", |v| 400.0 + v / 75.0);
            (EffectKind::Tuner, l)
        }
        "pitch shifter" => {
            let mut l = Layout::words(
                "version pitch formant channel wet duration density jitter rand delay feedback delay_mode formant_correction pitch_mode reserved",
            );
            // Public definition is explicitly NOT_FINISHED; only its named prefix
            // is consumed. No assumption is made about the trailing 256 bytes.
            l.map("pitch", "semitones", -2400.0, 2400.0, None);
            l.formula("pitch", |v| v / 100.0);
            l.map("formant", "formantSemitones", -1200.0, 1200.0, None);
            l.formula("formant", |v| v / 100.0);
            // The unfinished public definition gives `wet` no range. It is
            // read as 0..10000, the range the public dataset gives the mix
            // of Frequency Shifter and the amount of Spreader, effects of
            // the same generation, and clamped to the stand-in's 0..1. Any
            // value is admitted, so a larger one cannot void the layout.
            l.map("wet", "mix", f64::from(i32::MIN), f64::from(i32::MAX), None);
            l.formula("wet", |v| (v / 10000.0).clamp(0.0, 1.0));
            // A formant correction switch: on (any value but zero) keeps the
            // voice's formants while it is shifted, which is what the
            // stand-in's Voice mode does; off plays its full-mix mode.
            l.map(
                "formant_correction",
                "mode",
                f64::from(i32::MIN),
                f64::from(i32::MAX),
                None,
            );
            l.formula("formant_correction", |v| if v != 0.0 { 1.0 } else { 0.0 });
            for (offset, name) in [
                (70, "display_speed"),
                (73, "display_on"),
                (74, "display_mono"),
            ] {
                l.fields.push(Field {
                    offset,
                    scalar: Scalar::Byte,
                    name: name.into(),
                    target: String::new(),
                    range: (0.0, 255.0),
                    number: None,
                    formula: None,
                });
            }
            (EffectKind::PitchShift, l)
        }
        "effector" => {
            // The only public candidate is commented out in fl_studio.ddef.
            // Consume its named float prefix explicitly; do not infer its
            // model ordering or automation numbers from the record offsets.
            let mut l = Layout::words(
                "version effect bypass wet x_param y_param mod_rate tempo x_mod y_mod mod_shape input_level output_gain",
            );
            for f in l.fields.iter_mut().skip(1) {
                f.scalar = Scalar::Float;
            }
            for (source, target) in [("wet", "mix"), ("x_param", "x"), ("y_param", "y")] {
                l.map(source, target, 0.0, 1.0, None);
                l.formula(source, |v| v);
            }
            (EffectKind::PerformanceRack, l)
        }
        _ => return None,
    };
    for f in &mut l.fields {
        if f.range.0 >= 0.0 && matches!(f.scalar, Scalar::Signed) {
            f.scalar = Scalar::Unsigned;
        }
    }
    Some((kind, l))
}

pub(super) fn effect(name: &str, state: &[u8], tempo: f64) -> Option<Translated> {
    let name = name.trim().to_ascii_lowercase();
    let (kind, l) = effect_layout(&name, state.len()).or_else(|| dynamic_layout(&name, state))?;
    if !l.usable(state) {
        return None;
    }
    let mut params = kind.default_params();
    for (i, v) in l.values(state, kind.descriptors()) {
        params.set(i, v);
    }
    let word = |i: usize| {
        state
            .get(i * 4..i * 4 + 4)
            .and_then(|b| b.try_into().ok())
            .map(i32::from_le_bytes)
    };
    let mut set = |id: &str, v: f32| {
        if let Some(i) = kind.descriptors().iter().position(|p| p.id == id) {
            params.set(i, v);
        }
    };
    if name == "fruity phase inverter" && state.len() >= 8 {
        // Public range 0..1024 is fitted to binary inversion of both sides.
        // Sources give no left/right bitmask; none is inferred.
        let invert = u8::from(word(1).is_some_and(|v| v > 512)) as f32;
        set("left", invert);
        set("right", invert);
    }
    if name == "fruity mute 2" && state.len() >= 12 {
        let mute = word(1).unwrap_or(0) > 512;
        let ch = word(2).unwrap_or(1024) / 341;
        // Public LMMS converter outputs left gain=max(ch<=1,mute),
        // right gain=max(ch>=1,mute). Windfall controls mean mute, not gain.
        set("left", u8::from(!mute && ch > 1) as f32);
        set("right", u8::from(!mute && ch < 1) as f32);
    }
    if name == "fruity flanger" && state.len() >= 52 && word(8).is_some_and(|v| v > 512) {
        set("feedback", -(word(7).unwrap_or(0) as f32 / 100.0));
    }
    if name == "frequency shifter" && state.len() >= 48 {
        let n = word(3).unwrap_or(0) as f64 / 70000.0;
        set(
            "shiftHz",
            (n.powi(7) * if word(9) == Some(0) { 20000.0 } else { 200.0 }) as f32,
        );
    }
    if name == "effector" {
        set("xyEnabled", 1.0);
    }
    if name == "multiband delay" {
        // The public layout names no playing bank: the first one plays.
        set("layout", 1.0);
        set("bank", 1.0);
    }
    if name == "fruity delay 3" && state.len() >= 108 && word(3).is_some_and(|v| v != 0) {
        set("sync", 0.0);
        set(
            "timeMs",
            (f64::from(word(5).unwrap_or(0)) / 192.0 * 60000.0 / tempo) as f32,
        );
    }
    if name == "pitcher"
        && let Some((root, scale)) = pitcher_keys(&l, state).and_then(pitcher_scale)
    {
        set("root", f32::from(root));
        set("scale", scale as f32);
    }
    if name == "fruity stereo shaper" && state.len() >= 24 {
        // Signed -4096..4096 in the public dataset, with no unit or side:
        // a positive delay holds the right side back, a negative one the
        // left, by |delay| / 4096 of the stand-in's 50 ms.
        let delay = f64::from(word(5).unwrap_or(0)).clamp(-4096.0, 4096.0);
        let ms = (delay.abs() / 4096.0 * 50.0) as f32;
        set("leftDelayMs", if delay < 0.0 { ms } else { 0.0 });
        set("rightDelayMs", if delay > 0.0 { ms } else { 0.0 });
    }
    let mut links: Vec<ParamLink> = l
        .fields
        .iter()
        .filter_map(|f| f.link(kind.descriptors()))
        .collect();
    if name == "fruity panomatic" && state.len() >= 24 {
        // "LFO Target", 0..2 in the public dataset, which names none of the
        // three values. The stand-in has two depths, pan and volume, and
        // these are the three ways one amount can drive them: 0 pan (as
        // the amount was read before), 1 volume, 2 both.
        let target = word(3).unwrap_or(0);
        let amount = kind
            .descriptors()
            .iter()
            .position(|p| p.id == "depth")
            .and_then(|i| params.get(i))
            .unwrap_or(0.0);
        let mut set = |id: &str, v: f32| {
            if let Some(i) = kind.descriptors().iter().position(|p| p.id == id) {
                params.set(i, v);
            }
        };
        match target {
            1 => {
                set("depth", 0.0);
                set("volumeDepth", amount);
                // Automation of the amount moves the volume depth instead.
                for link in links.iter_mut().filter(|link| link.fl_param == 4) {
                    link.id = "volumeDepth";
                }
            }
            2 => set("volumeDepth", amount),
            _ => set("volumeDepth", 0.0),
        }
    }
    if matches!(name.as_str(), "vintage chorus" | "vintage phaser") {
        // Synced, the fitted rate becomes the cycle that plays it at the
        // import tempo: tempo / 60 / rate beats, within the stand-in's
        // 1/8 to 16.
        let value = |id: &str| {
            kind.descriptors()
                .iter()
                .position(|p| p.id == id)
                .and_then(|i| params.get(i))
        };
        if value("sync").is_some_and(|sync| sync != 0.0) {
            let rate = f64::from(value("rateHz").unwrap_or(0.0)).max(1e-6);
            let beats = (tempo / 60.0 / rate).clamp(0.125, 16.0) as f32;
            if let Some(i) = kind.descriptors().iter().position(|p| p.id == "cycleBeats") {
                params.set(i, beats);
            }
        }
    }
    if name == "fruity waveshaper" {
        waveshaper_curve(&mut params, state);
    }
    Some(Translated {
        params: params.sanitized(),
        notes: vec![
            "public native state translated; range-only controls use a disclosed linear fit to stand-in ranges, not acoustic equivalence; original blob retained",
        ],
        links,
    })
}

/// Why a decoded field has no stand-in control of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Unmapped {
    /// Version words, payload markers and padding: not a control.
    Structure,
    /// Editor display or transport state: nothing heard depends on it.
    Display,
    /// Named in the public layout without any meaning.
    NoMeaning,
    /// Translated by a rule outside the field table, which the text names.
    Elsewhere(&'static str),
    /// A sound control the stand-in does not have yet.
    Open,
}

/// Sorts a field that has no target in its layout. Anything not named here
/// is a sound control the stand-in still lacks; the test at the end of this
/// file pins that list, so it only changes on purpose.
pub(super) fn unmapped(device: &str, field: &str) -> Unmapped {
    use Unmapped::{Display, Elsewhere, NoMeaning, Open, Structure};
    if field.starts_with("version")
        || field.starts_with("reserved")
        || matches!(field, "header" | "data_exists" | "sample_header")
    {
        return Structure;
    }
    if field.starts_with("unknown") || field.starts_with("unk_") {
        return NoMeaning;
    }
    if field.starts_with("visual_") || field.starts_with("display_") || field == "theme" {
        return Display;
    }
    match (device, field) {
        ("fruity dx10", "mod_fine" | "mod2_fine") => {
            Elsewhere("DawVert fine=(raw/65536)^4*180 cents folded into the operator ratio")
        }
        ("fruity dx10", "octave") => Elsewhere("the carrier octave sets operators.0.ratio"),
        ("fruity dx10", "vibrato" | "lforate") => {
            Elsewhere("vibrato amount and rate drive the channel pitch LFO")
        }
        ("fruit kick" | "fruity kick", "max_freq") => {
            Elsewhere("with min_freq it gives pitchDropSemitones")
        }
        ("fruity slicer", "beats" | "bpm" | "attack" | "pitch") | ("slicex", "master_pitch") => {
            Elsewhere("read by the slice reader into the sample playback")
        }
        ("slicex", "play_pause" | "live_selection") => Display,
        ("fruity center", "on")
        | ("spreader", "enabled")
        | ("transient processor", "effect_on") => {
            Elsewhere("the native enable state switches the imported effect slot")
        }
        ("effector", "bypass") => Elsewhere("bypass switches the imported effect slot"),
        ("effector", "effect") => NoMeaning,
        ("fruity mute 2", "mute" | "channel") => {
            Elsewhere("mute and channel set the left and right mutes")
        }
        ("fruity phase inverter", "state") => {
            Elsewhere("0..1024 fitted to inverting both sides above 512")
        }
        ("fruity flanger", "inv_feedback") => Elsewhere("above 512 it negates feedback"),
        ("frequency shifter", "freqtype") => {
            Elsewhere("it selects the 20 kHz or 200 Hz range of shiftHz")
        }
        ("fruity delay 3", "tempo_sync") => {
            Elsewhere("when set, delay_time is taken in steps at the import tempo")
        }
        ("tuner", "reactivity" | "bass") => Display,
        ("fruity stereo shaper", "delay") => Elsewhere(
            "a positive delay holds the right side back and a negative one the left, by |delay| / 4096 of the stand-in's 50 ms",
        ),
        ("fruity panomatic", "lfo_target") => {
            Elsewhere("0 sends the LFO amount to the pan depth, 1 to the volume depth, 2 to both")
        }
        ("bassdrum", "peak") => {
            Elsewhere("with base it gives pitchDropSemitones = 12 log2(peak / base)")
        }
        ("pitcher", field) if field.starts_with("key_on.") => Elsewhere(
            "the twelve enabled keys set root and scale when they form one of the stand-in's scales",
        ),
        _ => Open,
    }
}

/// The code of the review line that names a decoded sound control without a
/// stand-in control. Such a line is filed as dropped, not approximated.
const OMITTED: &str = "flp.native.fieldOmitted";

/// Described fields outside the native layout table that the stand-in has
/// no control for, by the device the review lists them under (in lower
/// case). The converter files each one it decodes as omitted, and the test
/// at the end of this file pins them with the table's fields.
pub(super) const OMITTED_ELSEWHERE: &[(&str, &[&str])] = &[
    (
        "simsynth",
        &[
            "osc1_warm",
            "osc2_warm",
            "osc3_warm",
            "lfo_trackamp",
            "chorus_on",
        ],
    ),
    (
        "3x osc",
        &[
            "osc1_ofs",
            "osc1_detune",
            "osc2_ofs",
            "osc2_detune",
            "osc3_ofs",
            "osc3_detune",
            "phase_rand",
            "osc1_invert",
            "osc2_invert",
            "osc3_invert",
            "osc3_am",
        ],
    ),
    (
        "channel envelope",
        &[
            "predelay",
            "hold",
            "attack_tension",
            "decay_tension",
            "release_tension",
            "tempo_sync",
        ],
    ),
    (
        "fruity reeverb 2",
        &[
            "bass",
            "cross",
            "stereo",
            "mod_speed",
            "tempo_predelay",
            "mid_side",
        ],
    ),
    ("fruity reeverb", &["color"]),
    ("fruity delay 2", &["input_pan"]),
    (
        "fruity parametric eq 2",
        &[
            "1_order", "2_order", "3_order", "4_order", "5_order", "6_order", "7_order",
        ],
    ),
    ("channel lfo", &["predelay", "attack", "tempo_sync"]),
    ("channel arpeggiator", &["chord", "repeat", "slide"]),
    ("channel note echo", &["ping_pong", "fat_mode", "pan"]),
    ("channel time stretch", &["formant"]),
];

/// Whether `field` of `device` is in [`OMITTED_ELSEWHERE`].
pub(super) fn omitted_elsewhere(device: &str, field: &str) -> bool {
    OMITTED_ELSEWHERE
        .iter()
        .any(|(known, fields)| known.eq_ignore_ascii_case(device.trim()) && fields.contains(&field))
}

/// The field an omitted line of [`reports`] is about, for the list of
/// omitted controls the review shows by device.
pub(super) fn omitted_field(line: &str) -> Option<String> {
    let notice = windfall_core::notice::decode(line)?;
    (notice.code == OMITTED).then(|| notice.text("field"))?
}

/// The review outcome of one line of [`reports`]: an omitted control is not
/// in the Windfall project; everything else is there in some form.
pub(super) fn outcome(line: &str) -> crate::report::Outcome {
    if windfall_core::notice::code_of(line).as_deref() == Some(OMITTED) {
        crate::report::Outcome::Dropped
    } else {
        crate::report::Outcome::Approximated
    }
}

/// Report every field independently, including absent tails and decoded fields
/// for which the stand-in has no control. No generic defaults warning hides a
/// known layout. This runs off the audio thread during review.
pub(super) fn reports(name: &str, state: &[u8]) -> Vec<String> {
    let key = name.trim().to_ascii_lowercase();
    if key == "simsynth" {
        let names = "osc1_pw osc1_crs osc1_fine osc1_lvl osc1_lfo osc1_env osc1_shape osc2_pw osc2_crs osc2_fine osc2_lvl osc2_lfo osc2_env osc2_shape osc3_pw osc3_crs osc3_fine osc3_lvl osc3_lfo osc3_env osc3_shape lfo_del lfo_rate unk_1 lfo_shape unk_2 svf_cut svf_emph svf_env svf_lfo svf_kb unk_3 svf_high svf_band unk_4 amp_att amp_dec amp_sus amp_rel amp_lvl unk_5 svf_att svf_dec svf_sus svf_rel";
        let mut lines = Vec::new();
        for (i, field) in names.split_whitespace().enumerate() {
            let raw = state
                .get(i * 8..i * 8 + 8)
                .and_then(|b| b.try_into().ok())
                .map(f64::from_le_bytes);
            lines.push(
                windfall_core::notice!(
                    "flp.native.simsynthFloat",
                    device = name,
                    field = field,
                    value = format!("{:?}", raw),
                    offset = i * 8
                )
                .to_string(),
            );
        }
        for (i,field) in "osc1_on osc1_o1 osc1_o2 osc1_warm osc2_on osc2_o1 osc2_o2 osc2_warm osc3_on osc3_o1 osc3_o2 osc3_warm lfo_on lfo_retrigger svf_on unk_6 lfo_trackamp unk_7 chorus_on unk_8".split_whitespace().enumerate(){
            let raw=state.get(436+i*4..440+i*4).and_then(|b|b.try_into().ok()).map(u32::from_le_bytes);
            // The three oscillator synth has no warmth (detuned copy),
            // amplitude tracking or chorus of its own.
            lines.push(if omitted_elsewhere(&key, field) {
                windfall_core::notice!("flp.native.fieldOmitted", device = name, field = field, value = format!("{:?}", raw)).to_string()
            } else {
                windfall_core::notice!("flp.native.simsynthFlag", device = name, field = field, value = format!("{:?}", raw), offset = 436+i*4).to_string()
            });
        }
        return lines;
    }
    // Devices the effect translation reads by number (`effects`): the
    // words and bytes of the public layout that it does not carry.
    let elsewhere: &[(usize, bool, &str)] = match key.as_str() {
        "fruity reeverb 2" => &[
            (8, false, "bass"),
            (9, false, "cross"),
            (10, false, "stereo"),
            (14, false, "mod_speed"),
            (64, true, "tempo_predelay"),
            (65, true, "mid_side"),
        ],
        "fruity reeverb" => &[(6, false, "color")],
        "fruity delay 2" => &[(0, false, "input_pan")],
        "fruity parametric eq 2" => &[
            (29, false, "1_order"),
            (30, false, "2_order"),
            (31, false, "3_order"),
            (32, false, "4_order"),
            (33, false, "5_order"),
            (34, false, "6_order"),
            (35, false, "7_order"),
        ],
        _ => &[],
    };
    if !elsewhere.is_empty() {
        let numbers = crate::plugin::Numbers(state);
        return elsewhere
            .iter()
            .filter_map(|&(at, byte, field)| {
                let v = if byte {
                    i64::from(numbers.byte(at)?)
                } else {
                    i64::from(numbers.get(at)?)
                };
                Some(windfall_core::notice!("flp.native.fieldOmitted", device = name, field = field, value = v).to_string())
            })
            .collect();
    }
    if key == "3x osc" {
        // DawVert fl_studio.ddef `3x_osc`: per oscillator pan, shape,
        // coarse, fine, offset, stereo detune and (not the third) mix
        // level, then phase_rand, three invert bytes and osc3_am. Shape,
        // tuning, pan and the mix levels are carried (see `synth`); these
        // have no control on the subtractive synth that stands in.
        let numbers = crate::plugin::Numbers(state);
        let mut lines = Vec::new();
        for osc in 0..3 {
            for (offset, field) in [(4, "ofs"), (5, "detune")] {
                if let Some(v) = numbers.get(1 + osc * 7 + offset) {
                    lines.push(
                        windfall_core::notice!(
                            "flp.native.fieldOmitted",
                            device = name,
                            field = format!("osc{}_{field}", osc + 1),
                            value = v
                        )
                        .to_string(),
                    );
                }
            }
        }
        if let Some(v) = numbers.get(21) {
            lines.push(
                windfall_core::notice!("flp.native.fieldOmitted", device = name, field = "phase_rand", value = v)
                    .to_string(),
            );
        }
        for (offset, field) in [
            (88, "osc1_invert"),
            (89, "osc2_invert"),
            (90, "osc3_invert"),
            (91, "osc3_am"),
        ] {
            if let Some(v) = numbers.byte(offset) {
                lines.push(
                    windfall_core::notice!("flp.native.fieldOmitted", device = name, field = field, value = v)
                        .to_string(),
                );
            }
        }
        return lines;
    }
    let layout = instrument_layout(&key, state.len())
        .map(|(k, l)| (k.descriptors(), l))
        .or_else(|| {
            effect_layout(&key, state.len())
                .or_else(|| dynamic_layout(&key, state))
                .map(|(k, l)| (k.descriptors(), l))
        });
    let Some((descriptors, l)) = layout else {
        if key == "fruity vocoder" {
            return vec![
                windfall_core::notice!("flp.native.vocoderLayout", device = name).to_string(),
            ];
        }
        if matches!(
            key.as_str(),
            "3x osc"
                | "simsynth"
                | "fruity balance"
                | "fruity send"
                | "fruity parametric eq"
                | "fruity parametric eq 2"
                | "fruity compressor"
                | "fruity reeverb"
                | "fruity reeverb 2"
                | "fruity delay"
                | "fruity delay 2"
        ) {
            return Vec::new();
        }
        if super::synth::DEFAULT_INSTRUMENTS
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
            || super::effects::DEFAULT_EFFECTS
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
        {
            return vec![
                windfall_core::notice!("flp.native.noLayoutScope", device = name).to_string(),
            ];
        }
        return Vec::new();
    };
    let Some(bytes) = l.payload(state) else {
        return vec![windfall_core::notice!("flp.native.badHeader", device = name).to_string()];
    };
    if bytes.len() < l.minimum {
        return vec![
            windfall_core::notice!(
                "flp.native.tooShort",
                device = name,
                required = l.minimum,
                found = bytes.len()
            )
            .to_string(),
        ];
    }
    if !l.usable(state) {
        return vec![windfall_core::notice!("flp.native.unusable", device = name).to_string()];
    }
    let mut reports: Vec<String> = l
        .fields
        .iter()
        .filter(|f| {
            (!f.target.is_empty() || unmapped(&key, &f.name) != Unmapped::Structure)
                && key != "multiband delay"
        })
        .map(|f| match f.read(bytes) {
            None => {
                windfall_core::notice!("flp.native.fieldTruncated", device = name, field = f.name)
                    .to_string()
            }
            Some(v) => match descriptors.iter().find(|p| p.id == f.target) {
                Some(info) => windfall_core::notice!(
                    "flp.native.fieldMapped",
                    device = name,
                    field = f.name,
                    value = v,
                    target = info.id,
                    mapped = f.value(v, info),
                    rule = windfall_core::notice::term(if f.formula.is_some() {
                        "stated numerical rule"
                    } else {
                        "linear range fit"
                    }),
                    min = f.range.0,
                    max = f.range.1
                )
                .to_string(),
                None if key == "fruity phase inverter" && f.name == "state" => {
                    windfall_core::notice!(
                        "flp.native.phaseInverterState",
                        device = name,
                        value = v
                    )
                    .to_string()
                }
                None if matches!(f.name.as_str(), "on" | "enabled" | "effect_on") => {
                    windfall_core::notice!(
                        "flp.native.enableState",
                        device = name,
                        field = f.name,
                        value = v
                    )
                    .to_string()
                }
                None if matches!(key.as_str(), "slicex" | "fruity slicer")
                    && matches!(f.name.as_str(), "pitch" | "master_pitch") =>
                {
                    windfall_core::notice!(
                        "flp.native.slicePitch",
                        device = name,
                        field = f.name,
                        value = v
                    )
                    .to_string()
                }
                None if key == "fruity dx10"
                    && matches!(f.name.as_str(), "mod_fine" | "mod2_fine") =>
                {
                    windfall_core::notice!(
                        "flp.native.dx10Fine",
                        device = name,
                        field = f.name,
                        value = v
                    )
                    .to_string()
                }
                None => match unmapped(&key, &f.name) {
                    Unmapped::Structure => windfall_core::notice!(
                        "flp.native.fieldStructure",
                        device = name,
                        field = f.name,
                        value = v
                    )
                    .to_string(),
                    Unmapped::Display => windfall_core::notice!(
                        "flp.native.fieldDisplay",
                        device = name,
                        field = f.name,
                        value = v
                    )
                    .to_string(),
                    Unmapped::NoMeaning => windfall_core::notice!(
                        "flp.native.fieldNoMeaning",
                        device = name,
                        field = f.name,
                        value = v
                    )
                    .to_string(),
                    Unmapped::Elsewhere(rule) => windfall_core::notice!(
                        "flp.native.fieldElsewhere",
                        device = name,
                        field = f.name,
                        value = v,
                        rule = windfall_core::notice::term(rule)
                    )
                    .to_string(),
                    Unmapped::Open => windfall_core::notice!(
                        "flp.native.fieldOmitted",
                        device = name,
                        field = f.name,
                        value = v
                    )
                    .to_string(),
                },
            },
        })
        .collect();
    if key == "multiband delay" {
        // One line per bank and row keeps all 384 values inside the bounded
        // report: each names its source field, value and stand-in control.
        for bank in 1..=8 {
            for (row, unit) in [
                ("delay", "milliseconds, 1..1000"),
                ("vol", "gain, 0..1"),
                ("pan", "pan, -1..1"),
            ] {
                let prefix = format!("bank_{bank}/{row}.");
                let fields = l
                    .fields
                    .iter()
                    .filter(|f| f.name.starts_with(&prefix))
                    .map(
                        |f| match (f.read(bytes), descriptors.iter().find(|p| p.id == f.target)) {
                            (Some(v), Some(info)) => {
                                format!(
                                    "{}={v}->{}={}",
                                    f.name,
                                    info.id,
                                    v.clamp(f64::from(info.min), f64::from(info.max))
                                )
                            }
                            _ => windfall_core::notice!(
                                "flp.native.bandDelayFieldTruncated",
                                field = f.name
                            )
                            .to_string(),
                        },
                    )
                    .collect::<Vec<_>>();
                reports.push(
                    windfall_core::notice!(
                        "flp.native.bandDelayRow",
                        device = name,
                        fields = fields.join(", "),
                        unit = windfall_core::notice::term(unit)
                    )
                    .to_string(),
                );
            }
        }
        reports
            .push(windfall_core::notice!("flp.native.bandDelayBanks", device = name).to_string());
    }
    if key == "pitcher"
        && let Some(keys) = pitcher_keys(&l, state)
    {
        let names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let listed = (0..12)
            .filter(|key| keys & (1 << key) != 0)
            .map(|key| names[key])
            .collect::<Vec<_>>()
            .join(" ");
        reports.push(match pitcher_scale(keys) {
            Some((root, scale)) => format!(
                "{name}: enabled keys [{listed}] -> root={} scale={} (the stand-in's {} scale on {}).",
                root,
                scale,
                ["chromatic", "major", "minor", "pentatonic"][scale],
                names[usize::from(root)]
            ),
            None => format!(
                "{name}: enabled keys [{listed}] form none of the stand-in's scales (chromatic, major, minor, pentatonic on any root); it corrects to the chromatic scale and the original state is kept."
            ),
        });
    }
    if key == "fruity free filter" {
        reports.push(windfall_core::notice!("flp.native.freeFilter", device = name).to_string());
    }
    if key == "effector" {
        reports.push(windfall_core::notice!("flp.native.effectorScope", device = name).to_string());
    }
    if key == "slicex" {
        reports.push(windfall_core::notice!("flp.native.slicexScope", device = name).to_string());
    }
    if key == "fruity waveshaper" && bytes.len() >= 34 {
        let count = u32::from_le_bytes(bytes[30..34].try_into().expect("bounded header"));
        if count > 256 {
            reports.push(
                windfall_core::notice!("flp.native.shapeCount", device = name, points = count)
                    .to_string(),
            );
        }
        for i in 0..count.min(256) as usize {
            let Some(p) = bytes.get(34 + i * 24..58 + i * 24) else {
                reports.push(
                    windfall_core::notice!(
                        "flp.native.shapePointIncomplete",
                        device = name,
                        point = i
                    )
                    .to_string(),
                );
                break;
            };
            let tension = f32::from_le_bytes(p[16..20].try_into().expect("bounded point"));
            let mode = p[20];
            reports.push(
                windfall_core::notice!(
                    "flp.native.shapePoint",
                    device = name,
                    point = i,
                    mode = mode,
                    tension = tension,
                    flags = format!("{:?}", &p[21..24])
                )
                .to_string(),
            );
        }
    }
    reports
}

/// Variable-length public converters, with independent size caps before any
/// loops. These offsets come from flp_dec_plugins.py, not a vendor blob.
fn dynamic_layout(name: &str, state: &[u8]) -> Option<(EffectKind, Layout)> {
    if name == "fruity waveshaper" {
        let mut l = Layout::words("");
        l.minimum = 34;
        for (offset, n, t, max, scalar) in [
            (4, "preamp", "inputDb", 65535.0, Scalar::Word),
            (8, "wet", "mix", 65536.0, Scalar::Unsigned),
            (12, "postgain", "outputDb", 65536.0, Scalar::Unsigned),
            (16, "bipolarmode", "bipolar", 1.0, Scalar::Byte),
            (17, "removedc", "", 1.0, Scalar::Byte),
        ] {
            l.fields.push(Field {
                offset,
                scalar,
                name: n.into(),
                target: t.into(),
                range: (0.0, max),
                number: None,
                formula: None,
            });
        }
        return Some((EffectKind::Waveshaper, l));
    }
    if name == "fruity vocoder" {
        let count = u32::from_le_bytes(state.get(4..8)?.try_into().ok()?) as usize;
        if count > 128 {
            return None;
        }
        let mut l = Layout::words("version num_bands filter reserved");
        // The source's band count, which may be up to 128, played with as
        // many bands as the stand-in has: 16 to 32.
        l.map("num_bands", "bandCount", 0.0, 128.0, None);
        l.formula("num_bands", |v| v.clamp(16.0, 32.0));
        l.fields.push(Field {
            offset: 16,
            scalar: Scalar::Byte,
            name: "left_right".into(),
            target: String::new(),
            range: (0.0, 1.0),
            number: None,
            formula: None,
        });
        for i in 0..count {
            l.fields.push(Field {
                offset: 17 + i * 4,
                scalar: Scalar::Float,
                name: format!("bands.{i}"),
                target: String::new(),
                range: (0.0, 1.0),
                number: None,
                formula: None,
            });
        }
        let base = 17 + count * 4;
        for (i,n) in "freq_min freq_max freq_scale freq_invert freq_formant freq_bandwidth env_att env_rel hold mix_mod mix_car mix_wet".split_whitespace().enumerate() {
            l.fields.push(Field{offset:base+i*4,scalar:Scalar::Unsigned,name:n.into(),target:match n {"env_att"=>"attackMs","env_rel"=>"releaseMs","mix_wet"=>"mix","freq_formant"=>"color",_=>""}.into(),range:(0.0,65536.0),number:None,formula:None});
        }
        l.minimum = base + 48;
        return Some((EffectKind::Vocoder, l));
    }
    None
}

fn waveshaper_curve(params: &mut windfall_dsp::EffectParams, state: &[u8]) {
    let Some(count) = state
        .get(30..34)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_le_bytes)
        .filter(|n| *n <= 256)
    else {
        return;
    };
    let mut position = 0.0;
    let mut points = Vec::new();
    for i in 0..count as usize {
        let Some(record) = state.get(34 + i * 24..58 + i * 24) else {
            return;
        };
        let offset = f64::from_le_bytes(record[..8].try_into().expect("bounded record"));
        let value = f64::from_le_bytes(record[8..16].try_into().expect("bounded record"));
        if !offset.is_finite() || offset < 0.0 || !value.is_finite() {
            return;
        }
        position += offset;
        if !position.is_finite() {
            return;
        }
        let tension = f32::from_le_bytes(record[16..20].try_into().expect("bounded record"));
        points.push((
            position,
            value,
            crate::units::automation_curve(tension),
            record[20],
        ));
    }
    let Some(&(span, _, _, _)) = points.last().filter(|p| p.0 > 0.0) else {
        return;
    };
    for i in 0..33 {
        let at = span * i as f64 / 32.0;
        let upper = points.partition_point(|p| p.0 <= at);
        let (x, y, _, _) = points[upper.saturating_sub(1)];
        let value =
            points
                .get(upper)
                .filter(|p| p.0 > x)
                .map_or(y, |(end, target, tension, mode)| {
                    if *mode == 2 {
                        y
                    } else {
                        y + (target - y)
                            * windfall_project::curve_shape(
                                ((at - x) / (end - x)).clamp(0.0, 1.0),
                                *tension,
                            )
                    }
                });
        if let Some(index) = params
            .kind()
            .descriptors()
            .iter()
            .position(|p| p.id == format!("curve.{i}"))
        {
            params.set(index, value as f32);
        }
    }
}

pub(super) fn enabled(name: &str, state: &[u8]) -> Option<bool> {
    let key = name.trim().to_ascii_lowercase();
    if key == "effector" {
        return state
            .get(8..12)
            .and_then(|b| b.try_into().ok())
            .map(f32::from_le_bytes)
            .filter(|v| v.is_finite())
            .map(|v| v <= 0.5);
    }
    let offset = match key.as_str() {
        "fruity center" => {
            if state.len() == 4 {
                0
            } else {
                4
            }
        }
        "spreader" => 20,
        "transient processor" => 36,
        _ => return None,
    };
    state
        .get(offset..offset + 4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_le_bytes)
        .map(|v| v != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_layouts_only_address_existing_append_only_descriptors() {
        for &(name, _) in super::super::synth::DEFAULT_INSTRUMENTS {
            if let Some((kind, l)) = instrument_layout(&name.to_ascii_lowercase(), 0) {
                for f in l.fields.iter().filter(|f| !f.target.is_empty()) {
                    assert!(
                        kind.descriptors().iter().any(|p| p.id == f.target),
                        "{name}.{} -> {}",
                        f.name,
                        f.target
                    );
                }
            }
        }
        for name in super::super::effects::DEFAULT_EFFECTS
            .iter()
            .map(|(n, _)| *n)
            .chain([
                "Fruity Limiter",
                "Fruity Delay 3",
                "Fruity Waveshaper",
                "Fruity Vocoder",
            ])
        {
            let state = vec![0; 100];
            if let Some((kind, l)) = effect_layout(&name.to_ascii_lowercase(), 0)
                .or_else(|| dynamic_layout(&name.to_ascii_lowercase(), &state))
            {
                for f in l.fields.iter().filter(|f| !f.target.is_empty()) {
                    assert!(
                        kind.descriptors().iter().any(|p| p.id == f.target),
                        "{name}.{} -> {}",
                        f.name,
                        f.target
                    );
                    if let Some(link) = f.link(kind.descriptors()) {
                        assert!(link.value(f.range.0).is_finite());
                        assert!(link.value(f.range.1).is_finite());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod open_list {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    /// Every layout the importer knows, with its stand-in's controls and a
    /// state long enough to build the variable ones.
    fn layouts() -> Vec<(String, &'static [ParamInfo], Layout)> {
        let names: Vec<String> = super::super::synth::DEFAULT_INSTRUMENTS
            .iter()
            .map(|(n, _)| n.to_string())
            .chain(
                super::super::effects::DEFAULT_EFFECTS
                    .iter()
                    .map(|(n, _)| n.to_string()),
            )
            .chain(
                [
                    "Fruity Limiter",
                    "Fruity Delay 3",
                    "Fruity Waveshaper",
                    "Fruity Vocoder",
                    "Fruit Kick",
                    "Slicex",
                    "Fruity Slicer",
                    "BassDrum",
                    "Effector",
                    "Pitcher",
                    "Multiband Delay",
                    "Pitch Shifter",
                    "Tuner",
                ]
                .map(String::from),
            )
            .collect();
        let mut seen = BTreeSet::new();
        let mut found = Vec::new();
        for name in names {
            let key = name.to_ascii_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            let mut state = vec![0_u8; 100];
            state[4] = 2;
            if let Some((descriptors, layout)) = instrument_layout(&key, 0)
                .map(|(kind, layout)| (kind.descriptors(), layout))
                .or_else(|| {
                    effect_layout(&key, 0)
                        .or_else(|| dynamic_layout(&key, &state))
                        .map(|(kind, layout)| (kind.descriptors(), layout))
                })
            {
                found.push((key, descriptors, layout));
            }
        }
        found
    }

    /// `bank_3_e/grain_div` and `bands.7` name one control each of several
    /// like it: the bank number and a trailing index are replaced so a row
    /// stands for all of them.
    fn family(field: &str) -> String {
        let field = match field.strip_prefix("bank_") {
            Some(rest) => format!(
                "bank_#{}",
                rest.trim_start_matches(|c: char| c.is_ascii_digit())
            ),
            None => field.to_string(),
        };
        match field.rsplit_once('.') {
            Some((stem, index)) if index.chars().all(|c| c.is_ascii_digit()) => format!("{stem}.#"),
            _ => field,
        }
    }

    /// A stand-in control with every index replaced, so a row stands for
    /// all of them: `units.3.filter.q` is `units.#.filter.q`.
    fn control(id: &str) -> String {
        id.split('.')
            .map(|part| {
                if !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) {
                    "#"
                } else {
                    part
                }
            })
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Every decoded field that is omitted, by device: the table's fields
    /// without a stand-in control and the ones filed outside the table.
    fn omitted() -> BTreeMap<String, BTreeSet<String>> {
        let mut omitted: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (device, _, layout) in layouts() {
            for field in layout.fields.iter().filter(|f| f.target.is_empty()) {
                if unmapped(&device, &field.name) == Unmapped::Open {
                    omitted
                        .entry(device.clone())
                        .or_default()
                        .insert(family(&field.name));
                }
            }
        }
        for (device, fields) in OMITTED_ELSEWHERE {
            for field in *fields {
                omitted
                    .entry(device.to_string())
                    .or_default()
                    .insert(family(field));
            }
        }
        omitted
    }

    /// The stand-in of a device outside the table, where it is one of the
    /// processors; the channel's own envelope, LFO, arpeggiator, note echo
    /// and time stretch are pinned field by field in the test below.
    fn stand_in_elsewhere(device: &str) -> Option<&'static [ParamInfo]> {
        Some(match device {
            "simsynth" => InstrumentKind::TripleOsc.descriptors(),
            "3x osc" => InstrumentKind::SubtractiveSynth.descriptors(),
            "fruity reeverb 2" | "fruity reeverb" => EffectKind::Reverb.descriptors(),
            "fruity delay 2" => EffectKind::Delay.descriptors(),
            "fruity parametric eq 2" => EffectKind::Eq.descriptors(),
            _ => return None,
        })
    }

    #[test]
    fn decoded_fields_without_a_stand_in_control_are_exactly_the_recorded_list() {
        let actual = omitted();
        for (device, fields) in &actual {
            println!(
                "OPEN|{device}|{}",
                fields.iter().cloned().collect::<Vec<_>>().join(" ")
            );
        }
        let mut expected: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (device, field, reason) in EXPECTED {
            assert!(
                !reason.trim().is_empty(),
                "{device}.{field} gives no reason"
            );
            assert!(
                expected
                    .entry(device.to_string())
                    .or_default()
                    .insert(field.to_string()),
                "{device}.{field} is listed twice"
            );
        }
        assert_eq!(actual, expected);
        // The two devices completed in round seven have nothing left, and
        // the six round eight maps have taken their fields off the list.
        assert!(!actual.contains_key("multiband delay"));
        assert!(!actual.contains_key("fruity panomatic"));
    }

    #[test]
    fn every_omitted_field_was_checked_against_the_controls_of_its_stand_in() {
        // The README's rule: a described field stays omitted only while the
        // stand-in has no control that could carry it. Each device with an
        // omitted field has a review of the stand-in controls none of its
        // decoded fields sets, saying why none of its omitted fields fits
        // them. In the table those controls are worked out here from the
        // stand-in's own descriptors, so a control added to a stand-in, or
        // a field that stops setting one, fails this test until reviewed.
        let omitted = omitted();
        let mut reviewed: BTreeMap<&str, (BTreeSet<String>, &str)> = BTreeMap::new();
        for &(device, controls, why) in UNSET {
            assert!(!why.trim().is_empty(), "{device} gives no reason");
            let controls = controls.split_whitespace().map(String::from).collect();
            assert!(
                reviewed.insert(device, (controls, why)).is_none(),
                "{device} is reviewed twice"
            );
        }
        assert_eq!(
            reviewed.keys().copied().collect::<BTreeSet<_>>(),
            omitted.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            "every device with an omitted field, and no other, is reviewed"
        );
        let mut in_table = BTreeSet::new();
        for (device, descriptors, layout) in layouts() {
            let Some((controls, _)) = reviewed.get(device.as_str()) else {
                continue;
            };
            in_table.insert(device.clone());
            let set: BTreeSet<String> = layout
                .fields
                .iter()
                .filter(|f| !f.target.is_empty())
                .map(|f| control(&f.target))
                .collect();
            let unset: BTreeSet<String> = descriptors
                .iter()
                .map(|p| control(p.id))
                .filter(|c| !set.contains(c))
                .collect();
            assert_eq!(&unset, controls, "{device}: the controls no field sets");
        }
        for (device, (controls, _)) in &reviewed {
            if in_table.contains(*device) {
                continue;
            }
            if let Some(descriptors) = stand_in_elsewhere(device) {
                let known: BTreeSet<String> = descriptors.iter().map(|p| control(p.id)).collect();
                for c in controls {
                    assert!(known.contains(c), "{device}: the stand-in has no {c}");
                }
            } else {
                assert!(device.starts_with("channel "), "{device} has no stand-in");
            }
        }
        // The channel's own stand-ins, every field named: one added later
        // stops this from compiling until its review below is updated.
        let windfall_project::ModulationEnvelope {
            enabled: _,
            attack_ms: _,
            decay_ms: _,
            sustain: _,
            release_ms: _,
            depth: _,
        } = Default::default();
        let windfall_project::TargetLfo {
            enabled: _,
            shape: _,
            rate_hz: _,
            depth: _,
            delay_ms: _,
            retrigger: _,
        } = Default::default();
        let windfall_project::ArpeggiatorSettings {
            mode: _,
            rate: _,
            gate: _,
            range_octaves: _,
        } = Default::default();
        let windfall_project::NoteEchoSettings {
            enabled: _,
            time: _,
            feedback: _,
            pitch_semitones: _,
            repeats: _,
        } = Default::default();
    }

    #[test]
    fn devices_outside_the_table_report_exactly_their_recorded_omissions() {
        // The ones whose state `reports` reads; the channel's own fields
        // are filed by the voice conversion and tested on whole projects.
        for name in [
            "SimSynth",
            "3x Osc",
            "Fruity Reeverb 2",
            "Fruity Reeverb",
            "Fruity Delay 2",
            "Fruity Parametric EQ 2",
        ] {
            let state = [0_u8; 600];
            let lines = reports(name, &state);
            let filed: Vec<String> = lines.iter().filter_map(|line| omitted_field(line)).collect();
            let recorded = OMITTED_ELSEWHERE
                .iter()
                .find(|(device, _)| device.eq_ignore_ascii_case(name))
                .map(|(_, fields)| fields.to_vec())
                .unwrap_or_default();
            assert_eq!(filed, recorded, "{name}");
            for line in &lines {
                assert_eq!(
                    outcome(line) == crate::report::Outcome::Dropped,
                    omitted_field(line).is_some(),
                    "{line}"
                );
            }
        }
    }

    const NO_GAIN: &str = "the stand-in has no output or input level of its own";
    const WARMTH: &str = "the public converter makes warmth a six-voice unison; the three oscillator stand-in has no unison, and its detune moves its one voice (carried by the fine tuning)";
    const NO_LFO_SHAPE: &str = "the stand-in's modulation is a fixed sine; it has no shape control";
    const NO_SECOND_STAGE: &str =
        "a second (s_) stage: the stand-in has one limiter, one compressor and one gate";
    const NO_CURVE: &str = "the stand-in's attack and release have no curve control";
    const NO_DELAY_SECTION: &str =
        "part of the device's own delay section; the stand-in phaser has no delay";
    const NO_NOISE: &str = "the stand-in adds no noise and has no gate";
    const NO_VELOCITY: &str = "the stand-in's operators have no velocity sensitivity";
    const ROUTING: &str =
        "where the device sits in the chain (pre/post): the stand-in has one place in its slot";
    const NO_VOICE: &str = "the stand-in corrects one voice; it has no harmony voice controls";
    const NO_PER_KEY: &str =
        "the stand-in corrects every key the same way; it has no per-key switch";
    const NO_SLICE_FX: &str = "the stand-in plays its slices without modulation sources";
    const NO_NOISE_LAYER: &str = "the stand-in's kick has no noise layer";
    const NO_RANDOM: &str = "the stand-in plays the same hit every time; it has no variation";
    const NO_CROSSOVER: &str =
        "a crossover or crossfeed between the voices or sides; the stand-in has none";
    const NO_DRY: &str = "a dry level of its own: the stand-in's mix crossfades the wet level, which is carried, with the dry signal";
    const NO_WAVE: &str = "the stand-in has one wave for its sweep and no shape control";
    const NO_GRAIN_CONTROL: &str = "a grain playback control the stand-in's cloud has no counterpart for (it reads from one position with spray and density)";
    const NO_TIME_STRETCH: &str = "the stand-in plays its slices at their pitch; it has no stretch";
    const NO_FEEDBACK_SHAPE: &str = "the stand-in's feedback has no distortion, bit depth, rate reduction, resonance or diffusion stage";
    const ENVELOPE_SHAPE: &str = "Windfall's channel envelope is attack, decay, sustain and release with a depth; it has no predelay, hold, curve tension or tempo sync";
    const LFO_SHAPE: &str = "Windfall's channel LFO has a shape, rate, depth, start delay and retrigger; it has no attack or tempo sync, and the public readers give the channel LFO's predelay no time rule (DawVert's conversion of it is commented out), so it cannot be read as the start delay in milliseconds";
    const ARPEGGIATOR: &str = "Windfall's arpeggiator has a mode, rate, gate and octave range; it has no chords, repeats or slide";
    const NOTE_ECHO: &str = "Windfall's note echo has a time, feedback, pitch and repeats; it has no ping-pong, fat mode or echo pan";

    /// Device, then one of its decoded sound controls that has no stand-in
    /// control (a family stands for its indexed siblings), and why.
    /// Removing a row is how a field gets finished.
    const EXPECTED: &[(&str, &str, &str)] = &[
        (
            "bassdrum",
            "attack",
            "the stand-in kick has no attack control; its onset is fixed",
        ),
        (
            "bassdrum",
            "butt",
            "named without a unit or rule; the stand-in has no body control it could set",
        ),
        (
            "bassdrum",
            "click_cut",
            "the stand-in's click has a level (click_amount) and no filter",
        ),
        (
            "bassdrum",
            "click_freq",
            "the stand-in's click has a level (click_amount) and no filter",
        ),
        ("bassdrum", "drive", "the stand-in kick has no drive"),
        ("bassdrum", "noise_a", NO_NOISE_LAYER),
        ("bassdrum", "noise_d", NO_NOISE_LAYER),
        ("bassdrum", "noise_filter", NO_NOISE_LAYER),
        ("bassdrum", "noise_mix", NO_NOISE_LAYER),
        ("bassdrum", "noise_pitch", NO_NOISE_LAYER),
        ("bassdrum", "noise_s", NO_NOISE_LAYER),
        ("bassdrum", "noise_start", NO_NOISE_LAYER),
        ("bassdrum", "var_base", NO_RANDOM),
        ("bassdrum", "var_peak", NO_RANDOM),
        ("bassdrum", "var_phase", NO_RANDOM),
        ("bassdrum", "var_slide_time", NO_RANDOM),
        ("effector", "input_level", NO_GAIN),
        (
            "effector",
            "mod_rate",
            "the stand-in rack has no modulation source",
        ),
        (
            "effector",
            "mod_shape",
            "the stand-in rack has no modulation source",
        ),
        ("effector", "output_gain", NO_GAIN),
        (
            "effector",
            "tempo",
            "\"Tempo Reduce\": the stand-in rack has no tempo divider",
        ),
        (
            "effector",
            "x_mod",
            "the stand-in's X and Y are set directly (x_param, y_param) and not modulated",
        ),
        (
            "effector",
            "y_mod",
            "the stand-in's X and Y are set directly (x_param, y_param) and not modulated",
        ),
        (
            "frequency shifter",
            "feedback",
            "the stand-in shifter has a shift and a mix only",
        ),
        (
            "frequency shifter",
            "lr_phase",
            "the stand-in shifter has a shift and a mix only",
        ),
        (
            "frequency shifter",
            "mixer_track",
            "a sidechain source track; the stand-in has no sidechain",
        ),
        (
            "frequency shifter",
            "shape_left",
            "the stand-in shifter has a shift and a mix only",
        ),
        (
            "frequency shifter",
            "shape_right",
            "the stand-in shifter has a shift and a mix only",
        ),
        (
            "frequency shifter",
            "start_phase",
            "the stand-in shifter has a shift and a mix only",
        ),
        (
            "frequency shifter",
            "stereo",
            "the stand-in shifter has a shift and a mix only",
        ),
        (
            "frequency shifter",
            "type",
            "the stand-in shifter has a shift and a mix only",
        ),
        (
            "fruit kick",
            "osc_dist",
            "the stand-in kick has no distortion; level is the output level",
        ),
        ("fruity chorus", "crosscutoff", NO_CROSSOVER),
        ("fruity chorus", "crosstype", NO_CROSSOVER),
        ("fruity chorus", "lfo1_wave", NO_LFO_SHAPE),
        ("fruity chorus", "lfo2_wave", NO_LFO_SHAPE),
        ("fruity chorus", "lfo3_wave", NO_LFO_SHAPE),
        (
            "fruity delay 3",
            "delay_type",
            "a type switch without values in the public dataset; the stand-in's mode (stereo, ping-pong, inverted) cannot be read from it",
        ),
        ("fruity delay 3", "diffusion_level", NO_FEEDBACK_SHAPE),
        ("fruity delay 3", "diffusion_spread", NO_FEEDBACK_SHAPE),
        ("fruity delay 3", "dry", NO_DRY),
        ("fruity delay 3", "feedback_bits", NO_FEEDBACK_SHAPE),
        ("fruity delay 3", "feedback_reso", NO_FEEDBACK_SHAPE),
        ("fruity delay 3", "feedback_sample_rate", NO_FEEDBACK_SHAPE),
        ("fruity delay 3", "feedbackdist_knee", NO_FEEDBACK_SHAPE),
        ("fruity delay 3", "feedbackdist_symmetry", NO_FEEDBACK_SHAPE),
        ("fruity delay 3", "feedbackdist_type", NO_FEEDBACK_SHAPE),
        (
            "fruity delay 3",
            "keep_pitch",
            "the stand-in's time changes are not pitch-preserving or pitch-following by choice",
        ),
        (
            "fruity delay 3",
            "mod_cutoff",
            "the stand-in's modulation moves the time only",
        ),
        (
            "fruity delay 3",
            "smoothing",
            "the stand-in's time glide is fixed",
        ),
        (
            "fruity delay 3",
            "stereo",
            "a stereo width (0..6000); the stand-in delay has no width control",
        ),
        (
            "fruity delay 3",
            "tone",
            "a tone tilt; the stand-in's cuts are carried by feedback_cutoff and its low cut has no public field",
        ),
        (
            "fruity delay 3",
            "wet_input",
            "the input level of the wet path; the stand-in has no input level",
        ),
        (
            "fruity delay bank",
            "2x_oversampling",
            "the stand-in runs at the host rate; it has no oversampling switch",
        ),
        (
            "fruity delay bank",
            "bank_#_e/feedback_leave_first_echo",
            "the stand-in's units have no first-echo exception in their feedback",
        ),
        (
            "fruity delay bank",
            "bank_#_e/grain_div",
            "a grain divider (0..4096) the public layout names without a note value; the stand-in's division is the synced echo time, which feedback_time carries",
        ),
        (
            "fruity delay bank",
            "bank_#_e/grain_sh",
            "a grain shape the stand-in's units have no counterpart for",
        ),
        ("fruity dx10", "mod2_velsen", NO_VELOCITY),
        (
            "fruity dx10",
            "mod_thru",
            "the stand-in's modulators are not heard directly",
        ),
        ("fruity dx10", "mod_velsen", NO_VELOCITY),
        (
            "fruity dx10",
            "waveform",
            "a carrier waveform with no public rule; the stand-in's operator-4 feedback belongs to another operator",
        ),
        (
            "fruity fast dist",
            "mix",
            "the stand-in distortion has no mix",
        ),
        (
            "fruity fast dist",
            "type",
            "the stand-in's shape is carried by threshold; it has no type switch",
        ),
        ("fruity flanger", "cross", NO_CROSSOVER),
        ("fruity flanger", "dry", NO_DRY),
        ("fruity flangus", "cross", NO_CROSSOVER),
        ("fruity flangus", "dry", NO_DRY),
        (
            "fruity free filter",
            "center",
            "the stand-in filter has no centering switch",
        ),
        ("fruity granulizer", "depth", NO_GRAIN_CONTROL),
        ("fruity granulizer", "hold", NO_GRAIN_CONTROL),
        ("fruity granulizer", "hold_2", NO_GRAIN_CONTROL),
        (
            "fruity granulizer",
            "key_to",
            "what the keyboard plays (0..3, values not named publicly); the stand-in plays pitch from the key around its root",
        ),
        ("fruity granulizer", "loop", NO_GRAIN_CONTROL),
        ("fruity granulizer", "pan", "the stand-in cloud has no pan"),
        (
            "fruity granulizer",
            "speed",
            "a scan speed through the sample; the stand-in reads from one position",
        ),
        ("fruity granulizer", "trans_hold", NO_GRAIN_CONTROL),
        ("fruity granulizer", "trans_mode", NO_GRAIN_CONTROL),
        ("fruity granulizer", "wave_spacing", NO_GRAIN_CONTROL),
        (
            "fruity kick",
            "osc_dist",
            "the stand-in kick has no distortion; level is the output level",
        ),
        ("fruity limiter", "comp_att_curve", NO_CURVE),
        (
            "fruity limiter",
            "comp_rel",
            "the stand-in has one release for limiter and compressor, carried by limiter_rel",
        ),
        (
            "fruity limiter",
            "comp_sus",
            "the stand-in compressor has no sustain",
        ),
        ("fruity limiter", "limiter_att_curve", NO_CURVE),
        ("fruity limiter", "limiter_rel_curve", NO_CURVE),
        (
            "fruity limiter",
            "limiter_safety",
            "the stand-in limiter has no safety switch",
        ),
        (
            "fruity limiter",
            "limiter_sus",
            "the stand-in limiter has no sustain",
        ),
        (
            "fruity limiter",
            "noise_gain",
            "the stand-in gate has a threshold and release only",
        ),
        ("fruity limiter", "s_comp_att", NO_SECOND_STAGE),
        ("fruity limiter", "s_comp_att_curve", NO_SECOND_STAGE),
        ("fruity limiter", "s_comp_knee", NO_SECOND_STAGE),
        ("fruity limiter", "s_comp_ratio", NO_SECOND_STAGE),
        ("fruity limiter", "s_comp_rel", NO_SECOND_STAGE),
        ("fruity limiter", "s_comp_sus", NO_SECOND_STAGE),
        ("fruity limiter", "s_comp_thres", NO_SECOND_STAGE),
        ("fruity limiter", "s_gain", NO_SECOND_STAGE),
        ("fruity limiter", "s_limiter_att", NO_SECOND_STAGE),
        ("fruity limiter", "s_limiter_att_curve", NO_SECOND_STAGE),
        ("fruity limiter", "s_limiter_ceil", NO_SECOND_STAGE),
        ("fruity limiter", "s_limiter_rel", NO_SECOND_STAGE),
        ("fruity limiter", "s_limiter_rel_curve", NO_SECOND_STAGE),
        ("fruity limiter", "s_limiter_sus", NO_SECOND_STAGE),
        ("fruity limiter", "s_noise_gain", NO_SECOND_STAGE),
        ("fruity limiter", "s_noise_rel", NO_SECOND_STAGE),
        ("fruity limiter", "s_noise_thres", NO_SECOND_STAGE),
        ("fruity limiter", "s_sat", NO_SECOND_STAGE),
        (
            "fruity limiter",
            "sat",
            "the stand-in limiter has no saturation",
        ),
        (
            "fruity limiter",
            "sidechain_number",
            "a sidechain source number; the stand-in limiter has no sidechain",
        ),
        (
            "fruity multiband compressor",
            "filter_type",
            "the stand-in's crossover filters have one fixed type",
        ),
        (
            "fruity multiband compressor",
            "gain",
            "the stand-in has a makeup gain per band (carried) and no overall gain",
        ),
        (
            "fruity multiband compressor",
            "hi_knee",
            "the stand-in's bands have no knee",
        ),
        (
            "fruity multiband compressor",
            "hi_state",
            "a band on/solo/bypass state; the stand-in's bands are always on",
        ),
        (
            "fruity multiband compressor",
            "limiter",
            "the stand-in has no output limiter; its sidechain switch is an external key",
        ),
        (
            "fruity multiband compressor",
            "low_knee",
            "the stand-in's bands have no knee",
        ),
        (
            "fruity multiband compressor",
            "low_state",
            "a band on/solo/bypass state; the stand-in's bands are always on",
        ),
        (
            "fruity multiband compressor",
            "mid_freq_h",
            "the stand-in's bands meet at its two crossovers, carried by low_freq and hi_freq; the mid band has no edges of its own",
        ),
        (
            "fruity multiband compressor",
            "mid_freq_l",
            "the stand-in's bands meet at its two crossovers, carried by low_freq and hi_freq; the mid band has no edges of its own",
        ),
        (
            "fruity multiband compressor",
            "mid_knee",
            "the stand-in's bands have no knee",
        ),
        (
            "fruity multiband compressor",
            "mid_state",
            "a band on/solo/bypass state; the stand-in's bands are always on",
        ),
        (
            "fruity phaser",
            "freq_range",
            "a range switch for the sweep; the stand-in's sweep is set by minHz and maxHz, which are carried",
        ),
        ("fruity phaser", "gain", NO_GAIN),
        (
            "fruity phaser",
            "num_stages",
            "the stand-in phaser has a fixed number of stages",
        ),
        ("fruity slicer", "fitlen", NO_TIME_STRETCH),
        ("fruity slicer", "stretchtype", NO_TIME_STRETCH),
        (
            "fruity soft clipper",
            "postgain",
            "the stand-in has no output gain; its knee is the curve's softness",
        ),
        ("fruity stereo enhancer", "prepost", ROUTING),
        ("fruity stereo enhancer", "vol", NO_GAIN),
        (
            "fruity stereo shaper",
            "dephase",
            "a phase offset between the sides; the stand-in's matrix and delays have no phase control",
        ),
        (
            "fruity stereo shaper",
            "iodiff",
            "the public dataset names it \"In/Out Difference\" with no range or rule; the stand-in has nothing it could set",
        ),
        ("fruity stereo shaper", "prepost", ROUTING),
        (
            "fruity vocoder",
            "bands.#",
            "per-band levels; the stand-in's bands are all at one level",
        ),
        (
            "fruity vocoder",
            "filter",
            "a filter type for the bands; the stand-in's band filters are fixed",
        ),
        (
            "fruity vocoder",
            "freq_bandwidth",
            "the stand-in's band widths follow its band count",
        ),
        (
            "fruity vocoder",
            "freq_invert",
            "the stand-in's bands are not inverted",
        ),
        (
            "fruity vocoder",
            "freq_max",
            "the stand-in's bands span a fixed range",
        ),
        (
            "fruity vocoder",
            "freq_min",
            "the stand-in's bands span a fixed range",
        ),
        (
            "fruity vocoder",
            "freq_scale",
            "the stand-in's bands span a fixed range",
        ),
        (
            "fruity vocoder",
            "hold",
            "the stand-in's envelopes have an attack and release (carried) and no hold",
        ),
        (
            "fruity vocoder",
            "left_right",
            "which input side is the modulator: the stand-in takes its second signal from the sidechain key, not one side of the input",
        ),
        (
            "fruity vocoder",
            "mix_car",
            "the carrier heard in the output; the stand-in's mix crossfades with the input only",
        ),
        (
            "fruity vocoder",
            "mix_mod",
            "the modulator heard in the output; the stand-in's mix crossfades with the input only",
        ),
        (
            "fruity waveshaper",
            "removedc",
            "the stand-in has no DC filter",
        ),
        (
            "pitch shifter",
            "channel",
            "a source channel; the stand-in has no sidechain",
        ),
        (
            "pitch shifter",
            "delay",
            "the stand-in shifter has no delay section",
        ),
        (
            "pitch shifter",
            "delay_mode",
            "the stand-in shifter has no delay section",
        ),
        (
            "pitch shifter",
            "density",
            "the stand-in's grains are fixed",
        ),
        (
            "pitch shifter",
            "duration",
            "the stand-in's grains are fixed",
        ),
        (
            "pitch shifter",
            "feedback",
            "the stand-in shifter has no delay section",
        ),
        ("pitch shifter", "jitter", "the stand-in's grains are fixed"),
        (
            "pitch shifter",
            "pitch_mode",
            "a mode switch with no values in the unfinished public definition; formant correction already chooses the stand-in's mode",
        ),
        ("pitch shifter", "rand", "the stand-in's grains are fixed"),
        ("pitcher", "bypass.#", NO_PER_KEY),
        (
            "pitcher",
            "bypass_all",
            "bypass switches the slot, not a control; the public layout gives no rule tying it to the slot",
        ),
        (
            "pitcher",
            "finetune",
            "the stand-in's tuning is its reference (hz, carried); it has no further fine tune",
        ),
        (
            "pitcher",
            "formant",
            "the stand-in corrector has no formant control",
        ),
        (
            "pitcher",
            "gender",
            "the stand-in corrector has no formant control",
        ),
        ("pitcher", "harmonize", NO_VOICE),
        ("pitcher", "harmonize_vel", NO_VOICE),
        ("pitcher", "min_freq", "the stand-in tracks a fixed range"),
        ("pitcher", "octaves", NO_VOICE),
        (
            "pitcher",
            "ref_chan",
            "a MIDI reference channel; the stand-in is not played from MIDI",
        ),
        (
            "pitcher",
            "selector_key",
            "a key selector (a float with no value list in the public layout); the scale is read from the twelve key_on switches",
        ),
        (
            "pitcher",
            "selector_type",
            "a scale selector (a float with no value list in the public layout); the scale is read from the twelve key_on switches",
        ),
        (
            "pitcher",
            "stereo",
            "the stand-in corrects both sides alike",
        ),
        (
            "pitcher",
            "use_midi",
            "the stand-in is not played from MIDI",
        ),
        ("pitcher", "voice1_pan", NO_VOICE),
        ("pitcher", "voice2_pan", NO_VOICE),
        ("pitcher", "voice3_pan", NO_VOICE),
        ("pitcher", "voice4_pan", NO_VOICE),
        (
            "plucked!",
            "norm_decay",
            "the stand-in's decay is in seconds for every key; it has no normalizing switch",
        ),
        (
            "plucked!",
            "release",
            "\"Short Release\": the stand-in's release follows its decay",
        ),
        ("plucked!", "wide", "\"Widen\": the stand-in pluck is mono"),
        (
            "slicex",
            "crossfade",
            "the stand-in deck has no crossfade between slices",
        ),
        ("slicex", "layering", NO_SLICE_FX),
        ("slicex", "master_lfo_level", NO_SLICE_FX),
        ("slicex", "master_random_level", NO_SLICE_FX),
        ("slicex", "mod_x", NO_SLICE_FX),
        ("slicex", "mod_y", NO_SLICE_FX),
        (
            "soundgoodizer",
            "mode",
            "which of four characters plays; the stand-in has one amount and no character switch",
        ),
        (
            "spreader",
            "low_bypass",
            "the stand-in spreads the whole range",
        ),
        (
            "spreader",
            "mono",
            "a mono check switch; the stand-in has none",
        ),
        ("spreader", "postfx", ROUTING),
        ("spreader", "prefx", ROUTING),
        (
            "transient processor",
            "attack_shape",
            "the stand-in's attack has no shape control",
        ),
        ("transient processor", "gain", NO_GAIN),
        (
            "transient processor",
            "release_shape",
            "the stand-in's sustain has no shape control",
        ),
        (
            "transient processor",
            "split_balance",
            "the stand-in shapes the whole range; it has no split",
        ),
        (
            "transient processor",
            "split_freq",
            "the stand-in shapes the whole range; it has no split",
        ),
        (
            "vintage chorus",
            "delay_feedback",
            "the stand-in bucket chorus has no feedback",
        ),
        (
            "vintage chorus",
            "delay_hp",
            "a high-pass in the delay; the stand-in's tone is a low-pass",
        ),
        (
            "vintage chorus",
            "delay_time2",
            "a second delay line's time; the stand-in has one line, whose sweep depth is not a second time",
        ),
        (
            "vintage chorus",
            "invert_wet",
            "the stand-in bucket chorus has no wet inversion",
        ),
        (
            "vintage chorus",
            "mod_phase_start",
            "the stand-in's sweep starts where it is; it has no start phase",
        ),
        ("vintage chorus", "mod_shape_sine", NO_WAVE),
        (
            "vintage chorus",
            "mono_input",
            "the stand-in takes both sides",
        ),
        ("vintage chorus", "noise", NO_NOISE),
        ("vintage chorus", "noise_gain", NO_NOISE),
        ("vintage chorus", "noise_gate", NO_NOISE),
        (
            "vintage phaser",
            "color_gain",
            "the level of the feedback color; the stand-in's color is a frequency (color_freq, carried)",
        ),
        ("vintage phaser", "delay_feedback", NO_DELAY_SECTION),
        ("vintage phaser", "delay_hp", NO_DELAY_SECTION),
        ("vintage phaser", "delay_keeppitch", NO_DELAY_SECTION),
        ("vintage phaser", "delay_lp", NO_DELAY_SECTION),
        ("vintage phaser", "delay_mix", NO_DELAY_SECTION),
        ("vintage phaser", "delay_on", NO_DELAY_SECTION),
        ("vintage phaser", "delay_stereo", NO_DELAY_SECTION),
        ("vintage phaser", "delay_tempo", NO_DELAY_SECTION),
        ("vintage phaser", "delay_time", NO_DELAY_SECTION),
        (
            "vintage phaser",
            "feedback_hp",
            "a high-pass in the feedback; the stand-in's feedback color is a low-pass",
        ),
        (
            "vintage phaser",
            "input_hp",
            "the stand-in has no input high-pass",
        ),
        (
            "vintage phaser",
            "levels_mono",
            "the stand-in takes both sides",
        ),
        ("vintage phaser", "levels_noise", NO_NOISE),
        ("vintage phaser", "levels_noise_gate", NO_NOISE),
        (
            "vintage phaser",
            "lfo_pull",
            "the stand-in's sweep is a sine with no pull",
        ),
        (
            "vintage phaser",
            "lfo_scale",
            "the stand-in's sweep spans minHz to maxHz, which are carried",
        ),
        (
            "vintage phaser",
            "mod_phase_start",
            "the stand-in's sweep starts where it is; it has no start phase",
        ),
        ("vintage phaser", "mod_shape_sine", NO_WAVE),
        (
            "vintage phaser",
            "mod_tension",
            "the stand-in's sweep is a sine with no tension",
        ),
        (
            "vintage phaser",
            "phaser_freq",
            "a manual frequency for a phaser that does not sweep; the stand-in always sweeps between minHz and maxHz",
        ),
        (
            "vintage phaser",
            "phaser_hq",
            "a quality switch the public layout gives no stage count for; the stand-in's stages are not set from it",
        ),
        (
            "vintage phaser",
            "phaser_manual",
            "a manual (no sweep) switch; the stand-in always sweeps",
        ),
        (
            "vintage phaser",
            "rate_freq1",
            "rate ranges of the device's own; the stand-in's rate is carried by mod_speed",
        ),
        (
            "vintage phaser",
            "rate_freq2",
            "rate ranges of the device's own; the stand-in's rate is carried by mod_speed",
        ),
        ("simsynth", "osc1_warm", WARMTH),
        ("simsynth", "osc2_warm", WARMTH),
        ("simsynth", "osc3_warm", WARMTH),
        (
            "simsynth",
            "lfo_trackamp",
            "the stand-in's LFO does not follow the amplitude",
        ),
        ("simsynth", "chorus_on", "the stand-in synth has no chorus"),
        (
            "3x osc",
            "osc1_ofs",
            "an oscillator phase offset; the stand-in's oscillators have no phase control",
        ),
        (
            "3x osc",
            "osc1_detune",
            "a per-oscillator stereo detune; the stand-in's unison detune and spread apply to all oscillators together",
        ),
        (
            "3x osc",
            "osc2_ofs",
            "an oscillator phase offset; the stand-in's oscillators have no phase control",
        ),
        (
            "3x osc",
            "osc2_detune",
            "a per-oscillator stereo detune; the stand-in's unison detune and spread apply to all oscillators together",
        ),
        (
            "3x osc",
            "osc3_ofs",
            "an oscillator phase offset; the stand-in's oscillators have no phase control",
        ),
        (
            "3x osc",
            "osc3_detune",
            "a per-oscillator stereo detune; the stand-in's unison detune and spread apply to all oscillators together",
        ),
        (
            "3x osc",
            "phase_rand",
            "the stand-in's oscillators start at a fixed phase",
        ),
        (
            "3x osc",
            "osc1_invert",
            "the stand-in's oscillator levels are not signed",
        ),
        (
            "3x osc",
            "osc2_invert",
            "the stand-in's oscillator levels are not signed",
        ),
        (
            "3x osc",
            "osc3_invert",
            "the stand-in's oscillator levels are not signed",
        ),
        (
            "3x osc",
            "osc3_am",
            "oscillator 3 modulating the others' level at audio rate; the stand-in's LFOs move the volume at low rates only",
        ),
        (
            "fruity reeverb 2",
            "bass",
            "\"Bass Multi\": the stand-in's decay is the same for every frequency",
        ),
        (
            "fruity reeverb 2",
            "cross",
            "the crossover for the bass multiplier; the stand-in has none",
        ),
        (
            "fruity reeverb 2",
            "stereo",
            "\"Stereo Separation\" with no range in the public dataset, so no width can be read from it",
        ),
        (
            "fruity reeverb 2",
            "mod_speed",
            "the stand-in's modulation has a depth (carried by mod) and no rate",
        ),
        (
            "fruity reeverb 2",
            "tempo_predelay",
            "the stand-in's pre-delay is in milliseconds and does not follow the tempo",
        ),
        (
            "fruity reeverb 2",
            "mid_side",
            "the stand-in takes left and right",
        ),
        (
            "fruity reeverb",
            "color",
            "the stand-in reverb has no color control",
        ),
        (
            "fruity delay 2",
            "input_pan",
            "the stand-in delay has no input pan",
        ),
        (
            "fruity parametric eq 2",
            "1_order",
            "a band order (-3..3) with no slope in dB per octave and no meaning for its sign in the public sources; the stand-in's cut slopes cannot be read from it",
        ),
        (
            "fruity parametric eq 2",
            "2_order",
            "a band order (-3..3) with no slope in dB per octave and no meaning for its sign in the public sources; the stand-in's cut slopes cannot be read from it",
        ),
        (
            "fruity parametric eq 2",
            "3_order",
            "a band order (-3..3) with no slope in dB per octave and no meaning for its sign in the public sources; the stand-in's cut slopes cannot be read from it",
        ),
        (
            "fruity parametric eq 2",
            "4_order",
            "a band order (-3..3) with no slope in dB per octave and no meaning for its sign in the public sources; the stand-in's cut slopes cannot be read from it",
        ),
        (
            "fruity parametric eq 2",
            "5_order",
            "a band order (-3..3) with no slope in dB per octave and no meaning for its sign in the public sources; the stand-in's cut slopes cannot be read from it",
        ),
        (
            "fruity parametric eq 2",
            "6_order",
            "a band order (-3..3) with no slope in dB per octave and no meaning for its sign in the public sources; the stand-in's cut slopes cannot be read from it",
        ),
        (
            "fruity parametric eq 2",
            "7_order",
            "a band order (-3..3) with no slope in dB per octave and no meaning for its sign in the public sources; the stand-in's cut slopes cannot be read from it",
        ),
        ("channel envelope", "predelay", ENVELOPE_SHAPE),
        ("channel envelope", "hold", ENVELOPE_SHAPE),
        ("channel envelope", "attack_tension", ENVELOPE_SHAPE),
        ("channel envelope", "decay_tension", ENVELOPE_SHAPE),
        ("channel envelope", "release_tension", ENVELOPE_SHAPE),
        ("channel envelope", "tempo_sync", ENVELOPE_SHAPE),
        ("channel lfo", "predelay", LFO_SHAPE),
        ("channel lfo", "attack", LFO_SHAPE),
        ("channel lfo", "tempo_sync", LFO_SHAPE),
        ("channel arpeggiator", "chord", ARPEGGIATOR),
        ("channel arpeggiator", "repeat", ARPEGGIATOR),
        ("channel arpeggiator", "slide", ARPEGGIATOR),
        ("channel note echo", "ping_pong", NOTE_ECHO),
        ("channel note echo", "fat_mode", NOTE_ECHO),
        ("channel note echo", "pan", NOTE_ECHO),
        (
            "channel time stretch",
            "formant",
            "a formant offset in semitones; the stand-in's stretch has a formant-preservation switch only",
        ),
    ];

    /// Device, the stand-in's controls none of its decoded fields sets
    /// (indexes as `#`), and why none of its omitted fields fits them. In
    /// the table the controls are checked to be exactly these; outside it,
    /// to be the stand-in's own.
    const UNSET: &[(&str, &str, &str)] = &[
        (
            "bassdrum",
            "pitchDropSemitones sampleStart sampleTune",
            "the pitch drop is set from peak and base by rule; the sample layer's tune and start have no BassDrum field (its sample is the file reference)",
        ),
        (
            "effector",
            "amount feedback loopBeats model pitchSemitones xyEnabled",
            "XY is switched on by rule; the treatment, its amount, feedback, slice length and pitch jump have no public field (the effect selector's order is not public)",
        ),
        ("frequency shifter", "", "every control is set"),
        (
            "fruit kick",
            "level pitchDropSemitones",
            "the drop is set from max_freq and min_freq by rule; no field is an output level (osc_dist is distortion)",
        ),
        ("fruity chorus", "", "every control is set"),
        (
            "fruity delay 3",
            "division lowCutHz mode sync",
            "sync and the time are set by the tempo_sync rule; the division is only used while synced; no field is a low cut or one of the stand-in's modes",
        ),
        (
            "fruity delay bank",
            "units.#.division units.#.feedbackFilter.gainDb units.#.filter.gainDb",
            "the division is only used while synced, where feedback_time carries the time; the shelf and peak gains have no field (filter_gain and fbfilter_gain are carried as the filters' gains)",
        ),
        (
            "fruity dx10",
            "algorithm feedback gain",
            "the algorithm is set by rule; no field is operator 4's feedback or an output gain",
        ),
        ("fruity fast dist", "", "every control is set"),
        ("fruity flanger", "", "every control is set"),
        (
            "fruity flangus",
            "cycleBeats feedback rate2Hz sync toneHz",
            "the device has one speed (carried as the first rate), no feedback, tone or tempo sync field, and cross is a crossfeed, not feedback",
        ),
        ("fruity free filter", "", "every control is set"),
        (
            "fruity granulizer",
            "level pitch releaseMs rootKey seed",
            "no field is a level, pitch offset, release, root key or seed; key_to selects what the keyboard plays, not a root",
        ),
        (
            "fruity kick",
            "level pitchDropSemitones",
            "the drop is set from max_freq and min_freq by rule; no field is an output level (osc_dist is distortion)",
        ),
        ("fruity limiter", "", "every control is set"),
        (
            "fruity multiband compressor",
            "sidechain",
            "the stand-in's sidechain switch selects an external key; the device's limiter field is an output limiter",
        ),
        ("fruity phaser", "", "every control is set"),
        (
            "fruity slicer",
            "baseKey level sliceStarts.#",
            "the slice starts are read by the slice reader; no field is a base key or level",
        ),
        (
            "fruity soft clipper",
            "knee",
            "the knee is the curve's softness; postgain is an output level",
        ),
        (
            "fruity stereo enhancer",
            "bassHz bassLock mix",
            "no field is a mono-bass crossover or a mix; vol is an output level",
        ),
        (
            "fruity stereo shaper",
            "leftDelayMs mode rightDelayMs",
            "the delays are set from the delay field by rule; the mode stays stereo, the coordinates of the four gains",
        ),
        (
            "fruity vocoder",
            "carrierHz gain keyIsCarrier synthCarrier",
            "no field selects a synth carrier or its pitch, and gain scales the vocoded signal, not the carrier or modulator heard beside it; left_right picks sides of one input, not the sidechain key",
        ),
        (
            "fruity waveshaper",
            "curve.# oversampling",
            "the curve is set from the drawn points by rule; no field is an oversampling choice",
        ),
        ("pitch shifter", "", "every control is set"),
        (
            "pitcher",
            "amount mix root scale",
            "root and scale are set from key_on by rule; no field is a correction amount or a mix",
        ),
        (
            "plucked!",
            "level pickPosition",
            "no field is a level or a pick position",
        ),
        (
            "slicex",
            "baseKey releaseMs slices.#.gain slices.#.pitch slices.#.reverse sliceStarts.#",
            "the slices and their starts are read by the slice reader; no header field is a base key or release",
        ),
        ("soundgoodizer", "", "every control is set"),
        (
            "spreader",
            "haasMs mix",
            "no field is a Haas delay or a mix",
        ),
        (
            "transient processor",
            "listen",
            "listen chooses what is heard (attack, body); no field is that",
        ),
        (
            "vintage chorus",
            "cycleBeats depthMs toneHz",
            "the cycle is set from the rate when synced by rule; no field is a sweep depth or a low-pass tone (delay_time2 is a second line, delay_hp a high-pass)",
        ),
        (
            "vintage phaser",
            "cycleBeats stages stagger",
            "the cycle is set from the rate when synced by rule; no field gives a stage count or stagger (phaser_hq has no count)",
        ),
        ("simsynth", "", "every control is set"),
        (
            "3x osc",
            "ampVelocity filter.cutoffHz filter.drive filter.envelopeOctaves filter.keyTracking filter.mode filter.resonance filter.slope filter.velocity filterEnvelope.attackMs filterEnvelope.decayMs filterEnvelope.releaseMs filterEnvelope.sustain gain glideMs lfos.#.amp lfos.#.cutoffOctaves lfos.#.pitchSemitones lfos.#.pulseWidth lfos.#.rateHz lfos.#.shape oscillators.#.pulseWidth pan unisonDetuneCents unisonSpread unisonVoices",
            "the device has no filter, LFO, pulse width, glide or velocity field of its own (the channel's are converted with the channel); unison detune and spread apply to all oscillators while each stereo detune is per oscillator; the LFOs' volume depth is a low-rate movement, not oscillator 3's audio-rate modulation",
        ),
        (
            "fruity reeverb 2",
            "width",
            "Stereo Separation would be the width, but the public dataset gives it no range",
        ),
        (
            "fruity reeverb",
            "earlyLevel modulation width",
            "the device has no early level, modulation or width field; color is a tone",
        ),
        (
            "fruity delay 2",
            "lowCutHz modDepthMs modRateHz saturation",
            "no field is a low cut, modulation or saturation; input_pan is a pan",
        ),
        (
            "fruity parametric eq 2",
            "highCut.slope lowCut.slope",
            "the cut slopes would take the band orders, but the public sources give the orders no slope in dB per octave or meaning for their sign",
        ),
        (
            "channel envelope",
            "",
            "every field of the channel envelope is set",
        ),
        (
            "channel lfo",
            "delayMs",
            "the start delay would take the predelay, but the public readers give the channel LFO's predelay no time rule",
        ),
        (
            "channel arpeggiator",
            "",
            "every field of the arpeggiator is set",
        ),
        (
            "channel note echo",
            "",
            "every field of the note echo is set",
        ),
        (
            "channel time stretch",
            "formants",
            "the formant-preservation switch is a switch; a formant offset in semitones cannot be read as one",
        ),
    ];
}
