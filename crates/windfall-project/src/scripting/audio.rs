//! Control-worker audio adapter for the same bounded postfix bytecode.
use super::*;
use windfall_core::AudioBuffer;

pub const MAX_AUDIO_SCRIPT_STEPS: usize = 100_000_000;
const MAX_BYTES: usize = 64 * 1024 * 1024;

impl Program {
    /// Audio scripts contain gain/normalize expressions and frame reverse only.
    /// `value` is frame position 0..1; `index` is the zero-based frame number.
    /// Each statement is a pass over the immutable selection's private copy.
    pub fn process_audio(
        &self,
        input: &AudioBuffer,
        parameters: &[f64],
        budget: usize,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<AudioBuffer, ScriptDiagnostic> {
        if input.frames() == 0 || !(1..=2).contains(&input.channels())
            || input.sample_rate() == 0
            || input.frames() > input.sample_rate() as usize * 120
            || input.samples().len() > MAX_BYTES / size_of::<f32>()
            || budget == 0 || budget > MAX_AUDIO_SCRIPT_STEPS
        {
            return Err(error(0, "audio input or memory/work budget exceeds limits"));
        }
        for instruction in &self.code[..self.len] {
            if matches!(instruction.op, Op::Store(_) | Op::Emit | Op::Selected(_) | Op::Repeat(_, _) | Op::End(_) | Op::Action | Op::Map | Op::Feedback(_) | Op::Copies(_, _, _)) {
                return Err(error(usize::from(instruction.line), "audio scripts allow gain, normalize and reverse only"));
            }
        }
        let initial = self.registers(parameters)?;
        let mut left = budget;
        let mut check = |line: usize| -> Result<(), ScriptDiagnostic> {
            if left == 0 { return Err(error(line, "audio instruction budget exceeded")); }
            left -= 1;
            if cancelled() { return Err(error(line, "audio script cancelled or deadline exceeded")); }
            Ok(())
        };
        // Validate before allocating the single output buffer. Never expose partial output.
        for sample in input.samples() {
            check(0)?;
            if !sample.is_finite() { return Err(error(0, "non-finite audio input")); }
        }
        let mut samples = input.samples().to_vec();
        let channels = usize::from(input.channels());
        let frames = input.frames();
        let mut begin = 0;
        for end in 0..self.len {
            let instruction = self.code[end];
            if !matches!(instruction.op, Op::AudioGain | Op::AudioNormalize | Op::AudioReverse) { continue; }
            let line = usize::from(instruction.line);
            let mut peak = 0.0_f64;
            if instruction.op == Op::AudioNormalize {
                for sample in &samples {
                    check(line)?;
                    peak = peak.max(f64::from(sample.abs()));
                }
                if peak == 0.0 { return Err(error(line, "cannot normalize a silent selection")); }
            }
            if instruction.op == Op::AudioReverse {
                if begin != end { return Err(error(line, "reverse takes no expression")); }
                for frame in 0..frames / 2 {
                    check(line)?;
                    for channel in 0..channels {
                        samples.swap(frame * channels + channel, (frames - 1 - frame) * channels + channel);
                    }
                }
            } else {
                for frame in 0..frames {
                    let mut registers = initial;
                    registers[5] = frame as f64;
                    registers[6] = if frames == 1 { 0.0 } else { frame as f64 / (frames - 1) as f64 };
                    let mut stack = [0.0; STACK];
                    let mut sp = 0;
                    for op in &self.code[begin..end] {
                        check(usize::from(op.line))?;
                        arithmetic(op.op, &mut registers, &mut stack, &mut sp)
                            .map_err(|message| error(usize::from(op.line), message))?;
                    }
                    check(line)?;
                    if sp != 1 { return Err(error(line, "audio expression must produce one value")); }
                    let mut gain = stack[0];
                    if instruction.op == Op::AudioNormalize {
                        if !(0.0..=1.0).contains(&gain) { return Err(error(line, "normalize target must be 0..1 amplitude")); }
                        gain /= peak;
                    }
                    for channel in 0..channels {
                        let offset = frame * channels + channel;
                        let sample = f64::from(samples[offset]) * gain;
                        if !sample.is_finite() || sample.abs() > 1.0 {
                            return Err(error(line, "audio result exceeds full scale; reduce gain"));
                        }
                        samples[offset] = sample as f32;
                    }
                }
            }
            begin = end + 1;
        }
        if begin != self.len { return Err(error(0, "audio expression has no gain/normalize statement")); }
        check(0)?;
        Ok(AudioBuffer::from_interleaved(input.sample_rate(), input.channels(), samples))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> AudioBuffer { AudioBuffer::from_interleaved(4, 2, vec![0.1, -0.2, 0.2, -0.4, 0.3, -0.6, 0.4, -0.8]) }
    #[test]
    fn audio_scripts_original_examples_have_exact_frames_and_stereo_pairing() {
        let source = input();
        let run = |text: &str| Program::compile(text).unwrap().process_audio(&source, &[], 10000, || false).unwrap();
        assert_eq!(run("reverse").samples(), &[0.4, -0.8, 0.3, -0.6, 0.2, -0.4, 0.1, -0.2]);
        assert_eq!(run("normalize 0.8").samples(), source.samples());
        let faded = run("gain value");
        assert_eq!(faded.samples()[0..2], [0.0, 0.0]);
        assert_eq!(faded.samples()[6..8], [0.4, -0.8]);
        assert!((faded.samples()[2] - 0.2 / 3.0).abs() < 1e-7);
        let squared = run("gain value value *");
        assert!((squared.samples()[2] - 0.2 / 9.0).abs() < 1e-7);
        assert_eq!(source.samples(), input().samples());
    }
    #[test]
    fn audio_scripts_budget_cancel_invalid_surface_and_clipping_return_no_result() {
        let program = Program::compile("gain value").unwrap();
        assert!(program.process_audio(&input(), &[], 2, || false).unwrap_err().message.contains("budget"));
        assert!(program.process_audio(&input(), &[], 10000, || true).unwrap_err().message.contains("cancelled"));
        assert!(Program::compile("gain 2").unwrap().process_audio(&input(), &[], 10000, || false).is_err());
        assert!(Program::compile("emit").unwrap().process_audio(&input(), &[], 10000, || false).is_err());
        assert!(program.note_transform(&[]).is_err());
        assert!(program.run(ScriptInput::default(), RunLimits::default(), || false).is_err());
    }
}
