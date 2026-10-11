//! Windfall-authored CC0 two-bar arrangements, rendered from our oscillators
//! and drum generators. No recorded or third-party material enters this path.
use crate::{
    Rendered, SAMPLE_RATE,
    math::{exp, sin_turns},
};

pub const PROVENANCE: &str = "Windfall factory generator loops-v1; original code arrangements and synthesized oscillators/drums; CC0-1.0; no external audio";
#[derive(Debug, Clone, Copy)]
pub struct StarterLoop {
    pub id: &'static str,
    pub name: &'static str,
    pub genre: &'static str,
    pub tempo: u16,
    /// Pitch class C=0; all v1 melodic arrangements use the minor scale.
    pub key: u8,
    pub bars: u8,
    pub path: &'static str,
    rhythm: u8,
}
pub static LOOPS: [StarterLoop; 8] = [
    StarterLoop {
        id: "house-a",
        name: "Afterglow",
        genre: "House",
        tempo: 124,
        key: 9,
        bars: 2,
        path: "Loops/House/Afterglow.wav",
        rhythm: 0,
    },
    StarterLoop {
        id: "techno-e",
        name: "Pulse Grid",
        genre: "Techno",
        tempo: 132,
        key: 4,
        bars: 2,
        path: "Loops/Techno/Pulse Grid.wav",
        rhythm: 1,
    },
    StarterLoop {
        id: "hiphop-d",
        name: "Corner Steps",
        genre: "Hip hop",
        tempo: 90,
        key: 2,
        bars: 2,
        path: "Loops/Hip hop/Corner Steps.wav",
        rhythm: 2,
    },
    StarterLoop {
        id: "breakbeat-g",
        name: "Broken Orbit",
        genre: "Breakbeat",
        tempo: 140,
        key: 7,
        bars: 2,
        path: "Loops/Breakbeat/Broken Orbit.wav",
        rhythm: 3,
    },
    StarterLoop {
        id: "ambient-c",
        name: "Slow Horizon",
        genre: "Ambient",
        tempo: 75,
        key: 0,
        bars: 2,
        path: "Loops/Ambient/Slow Horizon.wav",
        rhythm: 4,
    },
    StarterLoop {
        id: "funk-f",
        name: "Pocket Circuit",
        genre: "Funk",
        tempo: 108,
        key: 5,
        bars: 2,
        path: "Loops/Funk/Pocket Circuit.wav",
        rhythm: 5,
    },
    StarterLoop {
        id: "dnb-b",
        name: "Night Run",
        genre: "Drum and bass",
        tempo: 174,
        key: 11,
        bars: 2,
        path: "Loops/Drum and bass/Night Run.wav",
        rhythm: 6,
    },
    StarterLoop {
        id: "pop-c",
        name: "Glass Avenue",
        genre: "Synth pop",
        tempo: 112,
        key: 0,
        bars: 2,
        path: "Loops/Synth pop/Glass Avenue.wav",
        rhythm: 7,
    },
];
pub fn manifest() -> &'static [StarterLoop] {
    &LOOPS
}
impl StarterLoop {
    pub fn render(&self) -> Rendered {
        self.render_fit(f64::from(self.tempo), self.key)
            .expect("valid factory metadata")
    }
    /// Resynthesis preserves note frequencies independently of tempo. Bounds
    /// keep one stereo loop below 12 MiB of floats at the slowest allowed tempo.
    pub fn render_fit(&self, tempo: f64, key: u8) -> Result<Rendered, &'static str> {
        if !tempo.is_finite() || !(40.0..=300.0).contains(&tempo) || key > 11 {
            return Err("Loop fit requires 40..300 BPM and a pitch class 0..11");
        }
        let beats = usize::from(self.bars) * 4;
        let frames = (SAMPLE_RATE as f64 * beats as f64 * 60.0 / tempo).round() as usize;
        let mut mix = vec![0.0f64; frames * 2];
        let drums: Vec<_> = crate::manifest()
            .iter()
            .filter(|s| {
                matches!(
                    s.name,
                    "Kick Punch" | "Snare Tight" | "Hat Closed 1" | "Clap Wide"
                )
            })
            .map(|s| (s.name, s.render()))
            .collect();
        for step in 0..beats * 4 {
            let beat = step / 4;
            let sub = step % 4;
            let kick = match self.rhythm {
                0 | 1 | 7 => sub == 0,
                2 | 5 => step % 16 == 0 || step % 16 == 7 || step % 16 == 10,
                3 | 6 => step % 16 == 0 || step % 16 == 6 || step % 16 == 11,
                _ => step == 0,
            };
            let snare = (beat % 4 == 1 || beat % 4 == 3) && sub == 0 && self.rhythm != 4;
            let hat = sub.is_multiple_of(2) && self.rhythm != 4;
            let swing = if matches!(self.rhythm, 2 | 5) && sub % 2 == 1 {
                0.15
            } else {
                0.0
            };
            let start = (((step as f64 + swing) / 4.0) * 60.0 / tempo * SAMPLE_RATE as f64).round()
                as usize;
            for (name, sound) in &drums {
                let gain = match *name {
                    "Kick Punch" if kick => 0.52,
                    "Snare Tight" if snare => 0.32,
                    "Clap Wide" if snare && self.rhythm == 0 => 0.2,
                    "Hat Closed 1" if hat => {
                        if sub == 0 {
                            0.10
                        } else {
                            0.07
                        }
                    }
                    _ => 0.0,
                };
                if gain == 0.0 {
                    continue;
                }
                for frame in 0..sound.frames() {
                    for channel in 0..2 {
                        let source = frame * sound.channels as usize
                            + channel.min(sound.channels as usize - 1);
                        mix[((start + frame) % frames) * 2 + channel] +=
                            sound.samples[source] as f64 / 8_388_608.0 * gain;
                    }
                }
            }
            if sub != 0 && !(self.rhythm == 5 && sub == 3) {
                continue;
            }
            let scale = [0i32, 3, 7, 10, 7, 5, 3, 7];
            let midi = 36 + i32::from(key) + scale[beat % 8];
            let frequency = 440.0 * exp((midi as f64 - 69.0) / 12.0 * std::f64::consts::LN_2);
            let duration = (SAMPLE_RATE as f64 * 60.0 / tempo
                * if self.rhythm == 4 { 2.0 } else { 0.65 })
            .round() as usize;
            for frame in 0..duration {
                let t = frame as f64 / SAMPLE_RATE as f64;
                let attack = (t / 0.008).min(1.0);
                let release = ((duration - frame) as f64 / (SAMPLE_RATE as f64 * 0.012)).min(1.0);
                let tone = (sin_turns(t * frequency) + 0.22 * sin_turns(t * frequency * 2.0))
                    * exp(-t * if self.rhythm == 4 { 1.5 } else { 5.0 })
                    * attack
                    * release
                    * 0.28;
                mix[((start + frame) % frames) * 2] += tone;
                mix[((start + frame) % frames) * 2 + 1] += tone * 0.92;
            }
        }
        let peak = mix.iter().fold(0.0f64, |p, s| p.max(s.abs()));
        let gain = if peak > 0.85 { 0.85 / peak } else { 1.0 };
        Ok(Rendered {
            channels: 2,
            samples: mix
                .into_iter()
                .map(|s| (s * gain * 8_388_607.0).round() as i32)
                .collect(),
        })
    }
}
/// Per-asset metadata, including the exact generator/provenance revision.
pub fn metadata_json() -> String {
    let rows:Vec<_>=LOOPS.iter().map(|l|format!("{{\"id\":\"{}\",\"name\":\"{}\",\"path\":\"{}\",\"genre\":\"{}\",\"tempo\":{},\"key\":{},\"mode\":\"minor\",\"bars\":{},\"provenance\":\"{}\"}}",l.id,l.name,l.path,l.genre,l.tempo,l.key,l.bars,PROVENANCE)).collect();
    format!("{{\"schema\":1,\"loops\":[{}]}}\n", rows.join(","))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_wav_decode_and_pitch_measurement() {
        let source = LOOPS[4].render_fit(100.0, 0).unwrap();
        let fit = LOOPS[4].render_fit(100.0, 2).unwrap();
        let bytes = fit.to_wav();
        let mut reader = hound::WavReader::new(bytes.as_slice()).unwrap();
        assert_eq!(reader.spec().sample_rate, 48000);
        assert_eq!(reader.spec().channels, 2);
        let samples: Vec<i32> = reader.samples::<i32>().map(Result::unwrap).collect();
        assert_eq!(samples, fit.samples());
        let spectrum = |audio: &Rendered| {
            let mono: Vec<_> = (12000..24000)
                .map(|frame| audio.samples()[frame * 2] as f64 / 8_388_608.0)
                .collect();
            crate::analysis::Spectrum::of(&mono, 48000.0)
        };
        // Independent A440 equal-temperament C2=65.406 and D2=73.416 Hz.
        let original = spectrum(&source);
        let shifted = spectrum(&fit);
        assert!(original.share(61.0, 69.0) > original.share(70.0, 78.0) * 2.0);
        assert!(shifted.share(70.0, 78.0) > shifted.share(61.0, 69.0) * 2.0);
    }
    #[test]
    fn byte_stable_generation_and_metadata() {
        for asset in manifest() {
            assert_eq!(asset.render().to_wav(), asset.render().to_wav());
            assert!(metadata_json().contains(asset.id));
            assert!(asset.key < 12 && asset.bars == 2 && asset.tempo >= 40);
            assert_eq!(
                asset.render().frames(),
                (48000.0 * 8.0 * 60.0 / f64::from(asset.tempo)).round() as usize
            );
        }
        assert!(metadata_json().contains("CC0-1.0"));
    }
    #[test]
    fn tempo_fit_preserves_bars_and_key_fit_changes_pitch() {
        let asset = &LOOPS[0];
        let fit = asset.render_fit(100.0, 2).unwrap();
        assert_eq!(fit.frames(), 230400);
        assert_ne!(fit.samples(), asset.render_fit(100.0, 9).unwrap().samples());
        assert!(asset.render_fit(f64::NAN, 0).is_err());
        assert!(asset.render_fit(120.0, 12).is_err());
    }
}
