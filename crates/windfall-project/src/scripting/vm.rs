use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const SCRIPT_VERSION: u32 = 1;
pub const MAX_SOURCE_BYTES: usize = 16_384;
pub const MAX_OPS: usize = 512;
pub const MAX_NOTES: usize = 4096;
pub const MAX_PARAMETERS: usize = 16;
const STACK: usize = 32;
const REGISTERS: usize = 24;
const FIELDS: [&str; 8] = [
    "start", "length", "key", "velocity", "pan", "index", "value", "number",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptParameter {
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptDiagnostic {
    /// One-based source line; zero means input/admission, rather than source.
    pub line: u32,
    pub message: String,
}

impl std::fmt::Display for ScriptDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}
impl std::error::Error for ScriptDiagnostic {}

fn error(line: usize, message: impl Into<String>) -> ScriptDiagnostic {
    ScriptDiagnostic {
        line: line as u32,
        message: message.into(),
    }
}

/// Plain note value, independent of project IDs. Expression/articulation remains
/// on the original note in the realtime path; piano replacement emits default expression.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ScriptNote {
    pub start: u32,
    pub length: u32,
    pub key: u8,
    pub velocity: f32,
    pub pan: f32,
}

impl Default for ScriptNote {
    fn default() -> Self {
        Self {
            start: 0,
            length: crate::TICKS_PER_STEP,
            key: 60,
            velocity: 0.8,
            pan: 0.0,
        }
    }
}

impl From<&crate::Note> for ScriptNote {
    fn from(n: &crate::Note) -> Self {
        Self {
            start: n.start,
            length: n.length,
            key: n.key,
            velocity: n.velocity,
            pan: n.pan,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Nop,
    Constant(f64),
    Load(u8),
    Store(u8),
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
    Clamp,
    Emit,
    Action,
    Map,
    Feedback(u8),
    Selected(u16),
    Repeat(u16, u16),
    End(u16),
    AudioGain,
    AudioReverse,
    AudioNormalize,
    Copies(u8, i8, u32),
}

#[derive(Debug, Clone, Copy)]
struct Instruction {
    op: Op,
    line: u16,
}
const EMPTY: Instruction = Instruction {
    op: Op::Nop,
    line: 0,
};

#[derive(Debug, Clone)]
pub struct Program {
    code: [Instruction; MAX_OPS],
    len: usize,
    pub parameters: Vec<ScriptParameter>,
}

impl Program {
    pub fn compile(source: &str) -> Result<Self, ScriptDiagnostic> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(error(0, "source byte limit exceeded"));
        }
        let mut program = Self {
            code: [EMPTY; MAX_OPS],
            len: 0,
            parameters: Vec::new(),
        };
        let mut block: Option<usize> = None;
        let mut started = false;
        for (i, line) in source.lines().enumerate() {
            let line_no = i + 1;
            let words: Vec<_> = line
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            if words.is_empty() {
                continue;
            }
            match words[0] {
                "param" => {
                    if started || words.len() != 5 || program.parameters.len() == MAX_PARAMETERS {
                        return Err(error(
                            line_no,
                            "use param NAME MIN MAX DEFAULT before statements (at most 16)",
                        ));
                    }
                    let name = words[1];
                    if name.len() > 32
                        || !name.bytes().all(|c| c.is_ascii_alphabetic() || c == b'_')
                        || FIELDS.contains(&name)
                        || program.parameters.iter().any(|p| p.name == name)
                    {
                        return Err(error(line_no, "invalid or duplicate parameter name"));
                    }
                    let min = finite(words[2], line_no)?;
                    let max = finite(words[3], line_no)?;
                    let default = finite(words[4], line_no)?;
                    if min > max || default < min || default > max {
                        return Err(error(line_no, "parameter default must be within min/max"));
                    }
                    program.parameters.push(ScriptParameter {
                        name: name.into(),
                        min,
                        max,
                        default,
                    });
                }
                "selected" | "repeat" => {
                    started = true;
                    if block.is_some() {
                        return Err(error(line_no, "nested loops are not supported"));
                    }
                    let op = if words[0] == "selected" && words.len() == 1 {
                        Op::Selected(0)
                    } else if words[0] == "repeat" && words.len() == 2 {
                        let count = words[1]
                            .parse::<u16>()
                            .map_err(|_| error(line_no, "repeat needs an integer count"))?;
                        if count == 0 || usize::from(count) > MAX_NOTES {
                            return Err(error(line_no, "repeat count must be 1..4096"));
                        }
                        Op::Repeat(count, 0)
                    } else {
                        return Err(error(line_no, "use selected or repeat COUNT"));
                    };
                    block = Some(program.len);
                    program.push(op, line_no)?;
                }
                "end" if words.len() == 1 => {
                    let begin = block
                        .take()
                        .ok_or_else(|| error(line_no, "end without loop"))?;
                    program.push(Op::End(begin as u16), line_no)?;
                    let end = program.len as u16;
                    program.code[begin].op = match program.code[begin].op {
                        Op::Selected(_) => Op::Selected(end),
                        Op::Repeat(count, _) => Op::Repeat(count, end),
                        _ => unreachable!(),
                    };
                }
                "set" if words.len() >= 3 => {
                    started = true;
                    let field = FIELDS
                        .iter()
                        .position(|name| *name == words[1])
                        .filter(|i| *i < 5)
                        .ok_or_else(|| {
                            error(
                                line_no,
                                "only start, length, key, velocity and pan are writable",
                            )
                        })?;
                    program.expression(&words[2..], line_no)?;
                    program.push(Op::Store(field as u8), line_no)?;
                }
                "emit" if words.len() == 1 => {
                    started = true;
                    program.push(Op::Emit, line_no)?;
                }
                "tempo" if words.len() >= 2 => {
                    started = true;
                    program.expression(&words[1..], line_no)?;
                    program.push(Op::Action, line_no)?;
                }
                "map" if words.len() >= 2 => {
                    started = true;
                    program.expression(&words[1..], line_no)?;
                    program.push(Op::Map, line_no)?;
                }
                "gain" | "normalize" if words.len() >= 2 => {
                    started = true;
                    program.expression(&words[1..], line_no)?;
                    program.push(if words[0] == "gain" { Op::AudioGain } else { Op::AudioNormalize }, line_no)?;
                }
                "reverse" if words.len() == 1 => {
                    started = true;
                    program.push(Op::AudioReverse, line_no)?;
                }
                "copies" if words.len() == 4 => {
                    started = true;
                    let count = words[1].parse::<u8>().ok().filter(|n| (1..=16).contains(n))
                        .ok_or_else(|| error(line_no, "copies count must be 1..16"))?;
                    let key_step = words[2].parse::<i8>().map_err(|_| error(line_no, "copies key step must be -128..127"))?;
                    let tick_step = words[3].parse::<u32>().ok().filter(|n| *n <= crate::MAX_PATTERN_TICKS)
                        .ok_or_else(|| error(line_no, "copies tick step exceeds pattern bound"))?;
                    program.push(Op::Copies(count, key_step, tick_step), line_no)?;
                }
                "feedback" if words.len() >= 3 => {
                    started = true;
                    let number = words[1]
                        .parse::<u8>()
                        .ok()
                        .filter(|n| *n <= 127)
                        .ok_or_else(|| error(line_no, "feedback needs a CC number 0..127"))?;
                    program.expression(&words[2..], line_no)?;
                    program.push(Op::Feedback(number), line_no)?;
                }
                _ => return Err(error(line_no, "unknown statement; see docs/SCRIPTING.md")),
            }
        }
        if block.is_some() {
            return Err(error(source.lines().count(), "loop is missing end"));
        }
        if program.len == 0 {
            return Err(error(0, "script has no statements"));
        }
        Ok(program)
    }

    fn push(&mut self, op: Op, line: usize) -> Result<(), ScriptDiagnostic> {
        if self.len == MAX_OPS {
            return Err(error(line, "compiled instruction limit exceeded"));
        }
        self.code[self.len] = Instruction {
            op,
            line: line as u16,
        };
        self.len += 1;
        Ok(())
    }

    fn expression(&mut self, words: &[&str], line: usize) -> Result<(), ScriptDiagnostic> {
        let mut depth = 0usize;
        for word in words {
            let (op, inputs) = match *word {
                "+" => (Op::Add, 2),
                "-" => (Op::Sub, 2),
                "*" => (Op::Mul, 2),
                "/" => (Op::Div, 2),
                "min" => (Op::Min, 2),
                "max" => (Op::Max, 2),
                "clamp" => (Op::Clamp, 3),
                _ => {
                    let register = FIELDS.iter().position(|name| *name == *word).or_else(|| {
                        self.parameters
                            .iter()
                            .position(|p| p.name == *word)
                            .map(|i| i + FIELDS.len())
                    });
                    (
                        match register {
                            Some(r) => Op::Load(r as u8),
                            None => Op::Constant(finite(word, line)?),
                        },
                        0,
                    )
                }
            };
            if depth < inputs {
                return Err(error(
                    line,
                    "expression stack underflow (expressions use postfix notation)",
                ));
            }
            depth = depth - inputs + 1;
            if depth > STACK {
                return Err(error(line, "expression stack limit exceeded"));
            }
            self.push(op, line)?;
        }
        if depth != 1 {
            return Err(error(line, "expression must produce exactly one value"));
        }
        Ok(())
    }

    fn registers(&self, parameters: &[f64]) -> Result<[f64; REGISTERS], ScriptDiagnostic> {
        if !parameters.is_empty() && parameters.len() != self.parameters.len() {
            return Err(error(0, "parameter count mismatch"));
        }
        let mut registers = [0.0; REGISTERS];
        set_note(&mut registers, ScriptNote::default());
        for (i, param) in self.parameters.iter().enumerate() {
            let value = parameters.get(i).copied().unwrap_or(param.default);
            if !value.is_finite() || value < param.min || value > param.max {
                return Err(error(
                    0,
                    format!("parameter {} is outside its bounds", param.name),
                ));
            }
            registers[FIELDS.len() + i] = value;
        }
        Ok(registers)
    }

    pub fn validate_parameters(&self, parameters: &[f64]) -> Result<(), ScriptDiagnostic> {
        self.registers(parameters).map(|_| ())
    }

    /// Cooperative work/time/memory budget. No unbounded jump, recursion, string
    /// allocation or external operation exists in the language. The caller runs
    /// this on a worker and can supply a cancellation/deadline probe.
    pub fn run(
        &self,
        input: ScriptInput<'_>,
        limits: RunLimits,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<ScriptOutput, ScriptDiagnostic> {
        if input.selected.len() > MAX_NOTES
            || limits.steps > 1_000_000
            || limits.notes > MAX_NOTES
            || limits.messages > 128
        {
            return Err(error(0, "input or budget exceeds hard memory/work limits"));
        }
        let mut registers = self.registers(input.parameters)?;
        if !input.value.is_finite() || !(0.0..=1.0).contains(&input.value) || input.number > 127 {
            return Err(error(0, "invalid MIDI input"));
        }
        registers[6] = input.value;
        registers[7] = f64::from(input.number);
        let mut stack = [0.0; STACK];
        let mut sp = 0;
        let mut pc = 0;
        let mut steps = 0;
        let mut iteration = 0;
        let mut output = ScriptOutput::default();
        while pc < self.len {
            let instruction = self.code[pc];
            if steps >= limits.steps {
                return Err(error(usize::from(instruction.line), "step limit exceeded"));
            }
            if cancelled() {
                return Err(error(
                    usize::from(instruction.line),
                    "cancelled or time limit exceeded",
                ));
            }
            steps += 1;
            pc += 1;
            match instruction.op {
                Op::Copies(_, _, _) => {
                    return Err(error(usize::from(instruction.line), "copies require the compiled channel note host"));
                }
                Op::AudioGain | Op::AudioReverse | Op::AudioNormalize => {
                    return Err(error(usize::from(instruction.line), "audio statements require the audio selection worker"));
                }
                Op::Selected(end) => {
                    iteration = 0;
                    if input.selected.is_empty() {
                        pc = usize::from(end);
                    } else {
                        set_note(&mut registers, input.selected[0]);
                        registers[5] = 0.0;
                    }
                }
                Op::Repeat(_, _) => {
                    iteration = 0;
                    set_note(&mut registers, ScriptNote::default());
                    registers[5] = 0.0;
                }
                Op::End(begin) => {
                    iteration += 1;
                    let next = match self.code[usize::from(begin)].op {
                        Op::Selected(_) => {
                            if let Some(note) = input.selected.get(iteration) {
                                set_note(&mut registers, *note);
                                true
                            } else {
                                false
                            }
                        }
                        Op::Repeat(count, _) => {
                            set_note(&mut registers, ScriptNote::default());
                            iteration < usize::from(count)
                        }
                        _ => false,
                    };
                    if next {
                        registers[5] = iteration as f64;
                        pc = usize::from(begin) + 1;
                    }
                }
                Op::Emit => {
                    if output.notes.len() >= limits.notes {
                        return Err(error(
                            usize::from(instruction.line),
                            "note memory limit exceeded",
                        ));
                    }
                    output.notes.push(
                        read_note(&registers)
                            .map_err(|message| error(usize::from(instruction.line), message))?,
                    );
                }
                Op::Action | Op::Map | Op::Feedback(_) => {
                    if output.tempos.len() + output.feedback.len() >= limits.messages {
                        return Err(error(
                            usize::from(instruction.line),
                            "message memory limit exceeded",
                        ));
                    }
                    sp -= 1;
                    let value = stack[sp];
                    match instruction.op {
                        Op::Map => {
                            if !(0.0..=1.0).contains(&value) {
                                return Err(error(
                                    usize::from(instruction.line),
                                    "mapped value is outside 0..1",
                                ));
                            }
                            output.mapped = Some(value as f32);
                        }
                        Op::Action => {
                            if !(crate::MIN_TEMPO_BPM..=crate::MAX_TEMPO_BPM).contains(&value) {
                                return Err(error(
                                    usize::from(instruction.line),
                                    "tempo is outside project bounds",
                                ));
                            }
                            output.tempos.push(value);
                        }
                        Op::Feedback(number) => {
                            if !(0.0..=127.0).contains(&value) {
                                return Err(error(
                                    usize::from(instruction.line),
                                    "feedback value is outside 0..127",
                                ));
                            }
                            output.feedback.push([0xb0, number, value.round() as u8]);
                        }
                        _ => unreachable!(),
                    }
                }
                op => arithmetic(op, &mut registers, &mut stack, &mut sp)
                    .map_err(|message| error(usize::from(instruction.line), message))?,
            }
        }
        Ok(output)
    }

    /// Restricts a program to straight-line note operations with exactly one
    /// emit, last. The stored fixed bytecode owns no heap allocations and cannot
    /// grow output. An invalid runtime arithmetic result reports an error.
    pub fn note_transform(&self, parameters: &[f64]) -> Result<NoteTransform, ScriptDiagnostic> {
        let mut emits = 0;
        let mut copies = None;
        for (i, instruction) in self.code[..self.len].iter().enumerate() {
            match instruction.op {
                Op::Selected(_)
                | Op::Repeat(_, _)
                | Op::End(_)
                | Op::Action
                | Op::Map
                | Op::Feedback(_)
                | Op::AudioGain
                | Op::AudioReverse
                | Op::AudioNormalize => {
                    return Err(error(
                        usize::from(instruction.line),
                        "realtime note transforms allow only set and one final emit",
                    ));
                }
                Op::Emit => {
                    emits += 1;
                    if i + 1 != self.len {
                        return Err(error(usize::from(instruction.line), "emit must be last"));
                    }
                }
                Op::Copies(count, key_step, tick_step) => {
                    if copies.replace((count, key_step, tick_step)).is_some() {
                        return Err(error(usize::from(instruction.line), "use copies once before the final emit"));
                    }
                }
                _ => (),
            }
        }
        if emits != 1 {
            return Err(error(0, "realtime note transform needs exactly one emit"));
        }
        Ok(NoteTransform {
            code: self.code,
            len: self.len,
            registers: self.registers(parameters)?,
            copies: copies.unwrap_or((1, 0, 0)),
        })
    }
}

#[path = "audio.rs"]
mod audio;

fn finite(word: &str, line: usize) -> Result<f64, ScriptDiagnostic> {
    word.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .ok_or_else(|| error(line, format!("unknown name or non-finite number: {word}")))
}

fn set_note(registers: &mut [f64; REGISTERS], note: ScriptNote) {
    registers[0] = f64::from(note.start);
    registers[1] = f64::from(note.length);
    registers[2] = f64::from(note.key);
    registers[3] = f64::from(note.velocity);
    registers[4] = f64::from(note.pan);
}

fn read_note(r: &[f64; REGISTERS]) -> Result<ScriptNote, &'static str> {
    if !r[..5].iter().all(|n| n.is_finite())
        || r[0] < 0.0
        || r[0] >= f64::from(crate::MAX_PATTERN_TICKS)
        || r[1] < 1.0
        || r[1] > f64::from(crate::MAX_PATTERN_TICKS)
        || !(0.0..=127.0).contains(&r[2])
        || !(0.0..=1.0).contains(&r[3])
        || !(-1.0..=1.0).contains(&r[4])
    {
        return Err("emitted note is outside musical bounds");
    }
    let note = ScriptNote {
        start: r[0].round() as u32,
        length: r[1].round() as u32,
        key: r[2].round() as u8,
        velocity: r[3] as f32,
        pan: r[4] as f32,
    };
    if note
        .start
        .checked_add(note.length)
        .is_none_or(|end| end > crate::MAX_PATTERN_TICKS)
    {
        return Err("note end exceeds pattern bound");
    }
    Ok(note)
}

fn arithmetic(
    op: Op,
    registers: &mut [f64; REGISTERS],
    stack: &mut [f64; STACK],
    sp: &mut usize,
) -> Result<(), &'static str> {
    match op {
        Op::Nop => (),
        Op::Constant(value) => {
            stack[*sp] = value;
            *sp += 1;
        }
        Op::Load(index) => {
            stack[*sp] = registers[usize::from(index)];
            *sp += 1;
        }
        Op::Store(index) => {
            *sp -= 1;
            registers[usize::from(index)] = stack[*sp];
        }
        Op::Clamp => {
            *sp -= 2;
            let min = stack[*sp];
            let max = stack[*sp + 1];
            if min > max {
                return Err("clamp minimum exceeds maximum");
            }
            stack[*sp - 1] = stack[*sp - 1].clamp(min, max);
        }
        Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Min | Op::Max => {
            *sp -= 1;
            let a = stack[*sp - 1];
            let b = stack[*sp];
            stack[*sp - 1] = match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div if b == 0.0 => return Err("division by zero"),
                Op::Div => a / b,
                Op::Min => a.min(b),
                Op::Max => a.max(b),
                _ => unreachable!(),
            };
        }
        _ => return Err("invalid arithmetic opcode"),
    }
    if stack[..*sp].iter().any(|n| !n.is_finite()) {
        return Err("non-finite arithmetic result");
    }
    Ok(())
}

pub struct ScriptInput<'a> {
    pub selected: &'a [ScriptNote],
    pub parameters: &'a [f64],
    pub value: f64,
    pub number: u8,
}

impl Default for ScriptInput<'_> {
    fn default() -> Self {
        Self {
            selected: &[],
            parameters: &[],
            value: 0.0,
            number: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RunLimits {
    pub steps: usize,
    pub notes: usize,
    pub messages: usize,
}
impl Default for RunLimits {
    fn default() -> Self {
        Self {
            steps: 100_000,
            notes: MAX_NOTES,
            messages: 128,
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct ScriptOutput {
    pub notes: Vec<ScriptNote>,
    pub tempos: Vec<f64>,
    pub feedback: Vec<[u8; 3]>,
    pub mapped: Option<f32>,
}

#[derive(Debug, Clone)]
pub struct NoteTransform {
    code: [Instruction; MAX_OPS],
    len: usize,
    registers: [f64; REGISTERS],
    copies: (u8, i8, u32),
}

/// Tiny nonallocating runtime error. Format it on the control thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransformError {
    Arithmetic,
    NoteBounds,
}

impl NoteTransform {
    /// Fixed stack/register arrays, straight-line bytecode, one input/output.
    /// No allocation, deallocation, clock, lock or IO on success or failure.
    pub fn evaluate(&self, input: ScriptNote) -> Result<ScriptNote, TransformError> {
        let mut registers = self.registers;
        set_note(&mut registers, input);
        let mut stack = [0.0; STACK];
        let mut sp = 0;
        for instruction in &self.code[..self.len] {
            if matches!(instruction.op, Op::Copies(_, _, _)) { continue; }
            if instruction.op == Op::Emit {
                return read_note(&registers).map_err(|_| TransformError::NoteBounds);
            }
            arithmetic(instruction.op, &mut registers, &mut stack, &mut sp)
                .map_err(|_| TransformError::Arithmetic)?;
        }
        Err(TransformError::Arithmetic)
    }

    /// Bounded duplication, with every end/key checked before returning any
    /// output. Owners/instance IDs belong to the host, not the script VM.
    pub fn evaluate_notes(&self, input: ScriptNote) -> Result<TransformedNotes, TransformError> {
        let note = self.evaluate(input)?;
        let mut output = TransformedNotes { notes: [note; 16], len: usize::from(self.copies.0) };
        for index in 0..output.len {
            let key = i32::from(note.key) + index as i32 * i32::from(self.copies.1);
            let start = note.start.checked_add((index as u32).checked_mul(self.copies.2).ok_or(TransformError::NoteBounds)?)
                .ok_or(TransformError::NoteBounds)?;
            if !(0..=127).contains(&key) || start.checked_add(note.length).is_none_or(|end| end > crate::MAX_PATTERN_TICKS) {
                return Err(TransformError::NoteBounds);
            }
            output.notes[index].key = key as u8;
            output.notes[index].start = start;
        }
        Ok(output)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TransformedNotes {
    notes: [ScriptNote; 16],
    len: usize,
}
impl TransformedNotes {
    pub fn as_slice(&self) -> &[ScriptNote] { &self.notes[..self.len] }
    pub fn single(note: ScriptNote) -> Self { Self { notes: [note; 16], len: 1 } }
}
