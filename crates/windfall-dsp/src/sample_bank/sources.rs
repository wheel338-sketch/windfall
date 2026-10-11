//! Newly authored mathematical sources. No recordings, voice models or
//! third-party presets are used. Render only on a worker/control thread.
use super::{LoadedZone, SampleBank, SampleZone};
use windfall_core::AudioBuffer;

/// Deterministic struck-string/electric-keyboard source material, authored
/// for Windfall. Source version 1 is part of the persisted generation recipe.
pub fn piano_bank(electric: bool) -> SampleBank {
    let rate = 24000u32;
    let mut zones = Vec::new();
    for root in (24..=108u8).step_by(6) {
        for layer in 0..2 {
            let mut pcm = Vec::with_capacity(rate as usize * 3);
            let frequency = 440.0 * 2.0_f64.powf((f64::from(root) - 69.0) / 12.0);
            for frame in 0..rate * 3 {
                let time = f64::from(frame) / f64::from(rate);
                let mut value = 0.0;
                for partial in 1..=12 {
                    let n = f64::from(partial);
                    let hz = frequency * n * (1.0 + 0.00004 * n * n).sqrt();
                    if hz >= f64::from(rate) * 0.45 {
                        break;
                    }
                    let strength = if electric {
                        1.0 / n.powf(2.2)
                    } else {
                        1.0 / n.powf(if layer == 0 { 2.0 } else { 1.35 })
                    };
                    let decay = if electric { 1.6 } else { 0.8 + n * 0.19 };
                    value += (std::f64::consts::TAU * hz * time).sin()
                        * strength
                        * (-decay * time).exp();
                }
                let edge = (time / 0.003).min(1.0) * ((3.0 - time) / 0.03).clamp(0.0, 1.0);
                pcm.push((value * 0.32 * edge) as f32);
            }
            let mapping = SampleZone {
                root_key: root,
                key_low: if root == 24 { 0 } else { root - 3 },
                key_high: if root == 108 { 127 } else { root + 2 },
                velocity_low: if layer == 0 { 0 } else { 64 },
                velocity_high: if layer == 0 { 63 } else { 127 },
                release_ms: 180.0,
                ..Default::default()
            };
            zones.push(LoadedZone {
                mapping,
                audio: AudioBuffer::from_interleaved(rate, 1, pcm),
            });
        }
    }
    SampleBank::new(zones).expect("generated piano satisfies bank bounds")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpeechError {
    Empty,
    TooLong,
    UnsupportedCharacter(char),
    InvalidControls,
    Cancelled,
}
impl std::fmt::Display for SpeechError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "speech render failed: {self:?}")
    }
}
impl std::error::Error for SpeechError {}

/// A deliberately robotic English grapheme/rule voice. Digraph rules and
/// vowel formants are our own code; unsupported alphabets fail explicitly.
/// Pitch sets the excitation frequency independently from phrase duration.
pub fn render_text(
    text: &str,
    pitch: f32,
    duration: f32,
    keep_going: &mut dyn FnMut() -> bool,
) -> Result<AudioBuffer, SpeechError> {
    if text.trim().is_empty() {
        return Err(SpeechError::Empty);
    }
    if text.len() > 512 {
        return Err(SpeechError::TooLong);
    }
    if !pitch.is_finite()
        || !(24.0..=84.0).contains(&pitch)
        || !duration.is_finite()
        || !(0.1..=60.0).contains(&duration)
    {
        return Err(SpeechError::InvalidControls);
    }
    for c in text.chars() {
        if !c.is_ascii_alphabetic() && !c.is_ascii_whitespace() && !".,!?'-".contains(c) {
            return Err(SpeechError::UnsupportedCharacter(c));
        }
    }
    let lower = text.to_ascii_lowercase();
    let mut phones = Vec::new();
    let mut chars = lower.chars().peekable();
    while let Some(c) = chars.next() {
        let next = chars.peek().copied();
        let phone = match (c, next) {
            ('e', Some('e')) | ('e', Some('a')) => {
                chars.next();
                'i'
            }
            ('o', Some('o')) => {
                chars.next();
                'u'
            }
            ('s', Some('h')) | ('c', Some('h')) | ('t', Some('h')) => {
                chars.next();
                's'
            }
            ('p' | 'g' | 'd' | 'k' | 'b' | 't', _) => 't',
            ('f' | 'v' | 's' | 'z' | 'h' | 'x' | 'c' | 'q', _) => 's',
            ('m' | 'n' | 'l' | 'r' | 'w' | 'y', _) => 'n',
            ('a' | 'e' | 'i' | 'o' | 'u', _) => c,
            _ => ' ',
        };
        phones.push(phone);
    }
    let rate = 24000u32;
    let frames = (duration * rate as f32).round() as usize;
    let frequency = 440.0 * 2.0_f64.powf((f64::from(pitch) - 69.0) / 12.0);
    let mut pcm = vec![0.0; frames];
    let mut poles = [[0.0f64; 2]; 3];
    let mut noise = 0x71a9_5e3du32;
    for (frame, output) in pcm.iter_mut().enumerate() {
        if frame % 1024 == 0 && !keep_going() {
            return Err(SpeechError::Cancelled);
        }
        let position = frame * phones.len() / frames;
        let phone = phones[position];
        let formants = match phone {
            'a' => [750.0, 1150.0, 2500.0],
            'e' => [500.0, 1800.0, 2600.0],
            'i' => [280.0, 2200.0, 3000.0],
            'o' => [450.0, 850.0, 2500.0],
            'u' => [320.0, 650.0, 2400.0],
            'n' => [250.0, 1200.0, 2200.0],
            _ => [1800.0, 3600.0, 5500.0],
        };
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        let phase = (frame as f64 * frequency / f64::from(rate)).fract();
        let excitation = if phone == ' ' {
            0.0
        } else if phone == 's' || phone == 't' {
            f64::from(noise >> 8) / 8388608.0 - 1.0
        } else if phase < 0.15 {
            0.85
        } else {
            -0.15
        };
        let mut value = 0.0;
        for (index, hz) in formants.iter().enumerate() {
            let radius = (-std::f64::consts::PI * 120.0 / f64::from(rate)).exp();
            let angle = std::f64::consts::TAU * hz / f64::from(rate);
            let next = excitation * (1.0 - radius) + 2.0 * radius * angle.cos() * poles[index][0]
                - radius * radius * poles[index][1];
            poles[index][1] = poles[index][0];
            poles[index][0] = next;
            value += next;
        }
        let progress = (frame as f64 * phones.len() as f64 / frames as f64).fract();
        let envelope = (progress / 0.08).min(1.0) * ((1.0 - progress) / 0.12).min(1.0);
        let articulation = if phone == 't' {
            (-18.0 * progress).exp()
        } else {
            1.0
        };
        *output = if phone == ' ' {
            0.0
        } else {
            (value * 2.0 * envelope * articulation).tanh() as f32
        };
    }
    Ok(AudioBuffer::from_interleaved(rate, 1, pcm))
}
