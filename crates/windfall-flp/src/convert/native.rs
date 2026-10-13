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
            ] {
                l.map(s, t, 0.0, max, None);
            }
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
    if name == "fruity waveshaper" {
        waveshaper_curve(&mut params, state);
    }
    Some(Translated {
        params: params.sanitized(),
        notes: vec![
            "public native state translated; range-only controls use a disclosed linear fit to stand-in ranges, not acoustic equivalence; original blob retained",
        ],
        links: l
            .fields
            .iter()
            .filter_map(|f| f.link(kind.descriptors()))
            .collect(),
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
        _ => Open,
    }
}

/// How the import review words a decoded sound control that has no stand-in
/// control. Lines that end with it are filed as dropped, not approximated.
pub(super) const OMITTED: &str =
    "omitted: decoded public field with no corresponding stand-in control (state retained).";

/// The review outcome of one line of [`reports`]: an omitted control is not
/// in the Windfall project; everything else is there in some form.
pub(super) fn outcome(line: &str) -> crate::report::Outcome {
    if line.ends_with(OMITTED) {
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
            lines.push(format!("{name}.{field}={raw:?}: public f64 at {}; oscillator tuning/shape/level, pulse width, cutoff/Q, band/high filter levels, ADSR, LFO delay and shared modulation use the reported numerical rules; unk fields have no public meaning and remain opaque.",i*8));
        }
        for (i,field) in "osc1_on osc1_o1 osc1_o2 osc1_warm osc2_on osc2_o1 osc2_o2 osc2_warm osc3_on osc3_o1 osc3_o2 osc3_warm lfo_on lfo_retrigger svf_on unk_6 lfo_trackamp unk_7 chorus_on unk_8".split_whitespace().enumerate(){
            let raw=state.get(436+i*4..440+i*4).and_then(|b|b.try_into().ok()).map(u32::from_le_bytes);
            lines.push(format!("{name}.{field}={raw:?}: public flag at {}; oscillator/LFO/filter enable, octave layers and LFO retrigger mapped; warmth, trackamp and chorus are reported per channel; unknown words remain opaque.",436+i*4));
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
            return vec![format!(
                "{name}: public variable layout is truncated or has a band count above 128; original state retained, safe defaults used."
            )];
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
            return vec![format!(
                "SCOPE: {name}: DawVert fl_studio.ddef/flp_dec_plugins.py and PyFLP plugin.py define no native state layout for this device; LMMS importer supplies no decoder. fl_studio.dset anonymous param_N ranges do not establish state offsets or knob meanings. Blob retained; registered stand-in defaults used."
            )];
        }
        return Vec::new();
    };
    let Some(bytes) = l.payload(state) else {
        return vec![format!(
            "{name}: invalid SSLF header/length; state retained, defaults used."
        )];
    };
    if bytes.len() < l.minimum {
        return vec![format!(
            "{name}: public layout requires {} bytes, has {}; defaults used, blob retained.",
            l.minimum,
            bytes.len()
        )];
    }
    if !l.usable(state) {
        return vec![format!(
            "{name}: a documented field is nonfinite or outside the public source range; state retained, safe defaults used."
        )];
    }
    let mut reports:Vec<String>=l.fields.iter().filter(|f|(!f.target.is_empty() || unmapped(&key, &f.name) != Unmapped::Structure) && key != "multiband delay").map(|f| {
        match f.read(bytes) {
            None=>format!("{name}.{}: truncated/nonfinite public field; safe default retained.",f.name),
            Some(v)=>match descriptors.iter().find(|p|p.id==f.target) {
                Some(info)=>format!("{name}.{}={v} -> {}={} ({}, range {}..{}); stand-in bounds apply.",f.name,info.id,f.value(v,info),if f.formula.is_some(){"stated numerical rule"}else{"linear range fit"},f.range.0,f.range.1),
                None if key=="fruity phase inverter" && f.name=="state"=>format!("{name}.state={v}: 0..1024 fitted to binary inversion of both sides (>512); public sources give no left/right bitmask."),
                None if matches!(f.name.as_str(),"on"|"enabled"|"effect_on")=>format!("{name}.{}={v}: native enable state controls the imported effect slot.",f.name),
                None if matches!(key.as_str(),"slicex"|"fruity slicer") && matches!(f.name.as_str(),"pitch"|"master_pitch")=>format!("{name}.{}={v}: pitch/100 mapped to sample playback slice pitch.",f.name),
                None if key=="fruity dx10" && matches!(f.name.as_str(),"mod_fine"|"mod2_fine")=>format!("{name}.{}={v}: DawVert fine=(raw/65536)^4*180 cents folded into operator ratio by 2^(cents/1200); zero coarse/fine disables its branch.",f.name),
                None=>match unmapped(&key, &f.name) {
                    Unmapped::Structure=>format!("{name}.{}={v}: layout structure, not a control.",f.name),
                    Unmapped::Display=>format!("{name}.{}={v}: editor display state; nothing heard depends on it (state retained).",f.name),
                    Unmapped::NoMeaning=>format!("{name}.{}={v}: named in the public layout without a meaning; kept opaque (state retained).",f.name),
                    Unmapped::Elsewhere(rule)=>format!("{name}.{}={v}: {rule}.",f.name),
                    Unmapped::Open=>format!("{name}.{}={v}: {OMITTED}",f.name),
                },
            },
        }
    }).collect();
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
                            _ => format!("{}: truncated/nonfinite, default kept", f.name),
                        },
                    )
                    .collect::<Vec<_>>();
                reports.push(format!(
                    "{name}: {} (raw float taken as {unit}; the public definition gives no unit).",
                    fields.join(", ")
                ));
            }
        }
        reports.push(format!("{name}: the public definition names no playing bank, band frequencies, feedback or wet level. Bank 1 plays; the other seven are kept as selectable banks; band edges are Windfall's own (40 Hz to 16 kHz, equal ratios); feedback and mix stay at the stand-in defaults."));
    }
    if key == "fruity free filter" {
        reports.push(format!("{name}: DawVert dataset numbers/ranges and universal converter disagree on frequency/type ranges; explicit dataset numbering is retained, but converter type/700 and freq/1024 ranges supply the seven named responses and Hz. The converter's negative index at type=0 is treated as lowpass safe fallback; source range disagreement is an approximation, not established acoustic parity."));
    }
    if key == "effector" {
        reports.push(format!("SCOPE: {name}: fl_studio.ddef contains only a commented thirteen-field prefix. Its wet/X/Y floats use an explicit 0..1 stand-in fit and bypass controls slot enable; effect selector ordering, modulation units and parameter numbering are absent from the public converter/PyFLP/LMMS. Every prefix field is decoded/reported, trailing bytes remain opaque."));
    }
    if key == "slicex" {
        reports.push(format!("{name}: public filename, data_exists/header, embedded WAV length/PCM and cue.sampleoffset are decoded by the sample reader. SCOPE: DawVert flp_dec_plugins.py, PyFLP and LMMS define no second-deck/custom-region mapping after this WAV payload; no additional region schema is inferred."));
    }
    if key == "fruity waveshaper" && bytes.len() >= 34 {
        let count = u32::from_le_bytes(bytes[30..34].try_into().expect("bounded header"));
        if count > 256 {
            reports.push(format!(
                "{name}.shape: count {count} exceeds 256; identity curve retained."
            ));
        }
        for i in 0..count.min(256) as usize {
            let Some(p) = bytes.get(34 + i * 24..58 + i * 24) else {
                reports.push(format!(
                    "{name}.shape.point{i}: incomplete public record; identity curve retained."
                ));
                break;
            };
            let tension = f32::from_le_bytes(p[16..20].try_into().expect("bounded point"));
            let mode = p[20];
            reports.push(format!("{name}.shape.point{i}: mode={mode}, tension={tension}, flags={:?}; shape sampled onto 33 stand-in knots; normal/tension uses the shared exponential curve fit, instant uses hold. Other source mode labels lack an acoustic curve formula in flp_dec_plugins.py/PyFLP and use the disclosed curve fit; original bytes retained.",&p[21..24]));
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

    /// Every layout the importer knows, with a state long enough to build
    /// the variable ones.
    fn layouts() -> Vec<(String, Layout)> {
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
            if let Some(layout) = instrument_layout(&key, 0)
                .map(|(_, layout)| layout)
                .or_else(|| {
                    effect_layout(&key, 0)
                        .or_else(|| dynamic_layout(&key, &state))
                        .map(|(_, layout)| layout)
                })
            {
                found.push((key, layout));
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

    fn open() -> BTreeMap<String, BTreeSet<String>> {
        let mut open: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (device, layout) in layouts() {
            for field in layout.fields.iter().filter(|f| f.target.is_empty()) {
                if unmapped(&device, &field.name) == Unmapped::Open {
                    open.entry(device.clone())
                        .or_default()
                        .insert(family(&field.name));
                }
            }
        }
        open
    }

    #[test]
    fn decoded_fields_without_a_stand_in_control_are_exactly_the_recorded_list() {
        let actual = open();
        for (device, fields) in &actual {
            println!(
                "OPEN|{device}|{}",
                fields.iter().cloned().collect::<Vec<_>>().join(" ")
            );
        }
        let expected: BTreeMap<String, BTreeSet<String>> = EXPECTED
            .iter()
            .map(|(device, fields)| {
                (
                    device.to_string(),
                    fields.split_whitespace().map(String::from).collect(),
                )
            })
            .collect();
        assert_eq!(actual, expected);
        // The two devices completed in round seven have nothing left.
        assert!(!actual.contains_key("multiband delay"));
    }

    /// Device, then its decoded sound controls that still have no stand-in
    /// control. Removing a field from this table is how one gets finished.
    const EXPECTED: &[(&str, &str)] = &[
        (
            "bassdrum",
            "attack butt click_cut click_freq drive noise_a noise_d noise_filter noise_mix noise_pitch noise_s noise_start peak var_base var_peak var_phase var_slide_time",
        ),
        (
            "effector",
            "input_level mod_rate mod_shape output_gain tempo x_mod y_mod",
        ),
        (
            "frequency shifter",
            "feedback lr_phase mixer_track shape_left shape_right start_phase stereo type",
        ),
        ("fruit kick", "osc_dist"),
        (
            "fruity chorus",
            "crosscutoff crosstype lfo1_wave lfo2_wave lfo3_wave wetonly",
        ),
        (
            "fruity delay 3",
            "delay_type diffusion_level diffusion_spread dry feedback_bits feedback_reso feedback_sample_rate feedbackdist_knee feedbackdist_symmetry feedbackdist_type keep_pitch mod_cutoff smoothing stereo tone wet_input",
        ),
        (
            "fruity delay bank",
            "2x_oversampling bank_#_e/feedback_leave_first_echo bank_#_e/grain_div bank_#_e/grain_sh",
        ),
        ("fruity dx10", "mod2_velsen mod_thru mod_velsen waveform"),
        ("fruity fast dist", "mix type"),
        ("fruity flanger", "cross dry"),
        ("fruity flangus", "cross dry"),
        ("fruity free filter", "center"),
        (
            "fruity granulizer",
            "depth hold hold_2 key_to loop pan speed trans_hold trans_mode wave_spacing",
        ),
        ("fruity kick", "osc_dist"),
        (
            "fruity limiter",
            "comp_att_curve comp_rel comp_sus limiter_att_curve limiter_rel_curve limiter_safety limiter_sus noise_gain s_comp_att s_comp_att_curve s_comp_knee s_comp_ratio s_comp_rel s_comp_sus s_comp_thres s_gain s_limiter_att s_limiter_att_curve s_limiter_ceil s_limiter_rel s_limiter_rel_curve s_limiter_sus s_noise_gain s_noise_rel s_noise_thres s_sat sat sidechain_number",
        ),
        (
            "fruity multiband compressor",
            "filter_type gain hi_knee hi_state limiter low_knee low_state mid_freq_h mid_freq_l mid_knee mid_state",
        ),
        ("fruity panomatic", "lfo_target"),
        ("fruity phaser", "freq_range gain num_stages"),
        ("fruity slicer", "fitlen stretchtype"),
        ("fruity soft clipper", "postgain"),
        ("fruity stereo enhancer", "prepost vol"),
        ("fruity stereo shaper", "delay dephase iodiff prepost"),
        (
            "fruity vocoder",
            "bands.# filter freq_bandwidth freq_invert freq_max freq_min freq_scale hold left_right mix_car mix_mod num_bands",
        ),
        ("fruity waveshaper", "removedc"),
        (
            "pitch shifter",
            "channel delay delay_mode density duration feedback formant_correction jitter pitch_mode rand wet",
        ),
        (
            "pitcher",
            "bypass.# bypass_all finetune formant gender harmonize harmonize_vel key_on.# min_freq octaves ref_chan selector_key selector_type stereo use_midi voice1_pan voice2_pan voice3_pan voice4_pan",
        ),
        ("plucked!", "norm_decay release wide"),
        (
            "slicex",
            "crossfade layering master_lfo_level master_random_level mod_x mod_y",
        ),
        ("soundgoodizer", "mode"),
        ("spreader", "low_bypass mono postfx prefx"),
        (
            "transient processor",
            "attack_shape gain release_shape split_balance split_freq",
        ),
        (
            "vintage chorus",
            "delay_feedback delay_hp delay_time2 invert_wet mod_phase_start mod_shape_sine mod_tempo_sync mono_input noise noise_gain noise_gate",
        ),
        (
            "vintage phaser",
            "color_freq color_gain delay_feedback delay_hp delay_keeppitch delay_lp delay_mix delay_on delay_stereo delay_tempo delay_time feedback_hp input_hp levels_mono levels_noise levels_noise_gate lfo_pull lfo_scale mod_phase_start mod_shape_sine mod_tempo mod_tension phaser_freq phaser_hq phaser_manual rate_freq1 rate_freq2",
        ),
    ];
}
