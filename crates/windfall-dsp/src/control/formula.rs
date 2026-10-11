use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blocks::math::clean;
use crate::param::{ParamSet, param_set};

use super::{Controls, peak};

const MAX_BYTES: usize = 256;
const MAX_OPS: usize = 64;
const MAX_DEPTH: usize = 16;
const LIMIT: f32 = 1.0e6;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct FormulaSourceParams {
    pub a: f32,
    pub b: f32,
    pub base: f32,
    pub amount: f32,
    pub c: f32,
}

impl Default for FormulaSourceParams {
    fn default() -> Self {
        Self {
            a: 0.5,
            b: 0.5,
            base: 0.0,
            amount: 1.0,
            c: 0.0,
        }
    }
}

param_set!(FormulaSourceParams, "Formula", {
    float [a] "a" "A" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [b] "b" "B" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [base] "base" "Base" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [amount] "amount" "Amount" { Gain, Linear, -4.0, 4.0, 1.0 }
    float [c] "c" "C" { Fraction, Linear, 0.0, 1.0, 0.0 }
});

/// Rejected formulas leave the currently installed expression unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormulaError {
    TooLong,
    TooComplex,
    Syntax,
    InvalidNumber,
}

impl std::fmt::Display for FormulaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLong => "formula exceeds 256 bytes",
            Self::TooComplex => "formula exceeds 64 operations or 16 levels",
            Self::Syntax => "invalid formula syntax",
            Self::InvalidNumber => "formula contains an invalid numeric literal",
        })
    }
}

impl std::error::Error for FormulaError {}

#[derive(Clone, Copy)]
enum Op {
    Constant(f32),
    A,
    B,
    Peak,
    Add,
    Multiply,
    Negate,
    Sine,
    Abs,
    Min,
    Max,
}

#[derive(Clone, Copy)]
struct Program {
    ops: [Op; MAX_OPS],
    len: usize,
}

impl Program {
    fn evaluate(&self, a: f32, b: f32, peak: f32) -> f32 {
        let mut stack = [0.0; MAX_OPS];
        let mut used = 0;
        for op in &self.ops[..self.len] {
            match *op {
                Op::Constant(value) => {
                    stack[used] = value;
                    used += 1;
                }
                Op::A => {
                    stack[used] = a;
                    used += 1;
                }
                Op::B => {
                    stack[used] = b;
                    used += 1;
                }
                Op::Peak => {
                    stack[used] = peak;
                    used += 1;
                }
                Op::Negate | Op::Sine | Op::Abs => {
                    let value = stack[used - 1];
                    stack[used - 1] = match op {
                        Op::Negate => -value,
                        Op::Sine => value.sin(),
                        _ => value.abs(),
                    };
                }
                Op::Add | Op::Multiply | Op::Min | Op::Max => {
                    let right = stack[used - 1];
                    let left = stack[used - 2];
                    used -= 1;
                    let value = match op {
                        Op::Add => left + right,
                        Op::Multiply => left * right,
                        Op::Min => left.min(right),
                        _ => left.max(right),
                    };
                    stack[used - 1] = clean(value, -LIMIT, LIMIT, 0.0);
                }
            }
        }
        stack[0]
    }
}

struct Parser<'a> {
    input: &'a [u8],
    at: usize,
    program: Program,
}

impl<'a> Parser<'a> {
    fn compile(input: &'a str) -> Result<Program, FormulaError> {
        if input.len() > MAX_BYTES {
            return Err(FormulaError::TooLong);
        }
        let mut parser = Self {
            input: input.as_bytes(),
            at: 0,
            program: Program {
                ops: [Op::Constant(0.0); MAX_OPS],
                len: 0,
            },
        };
        parser.sum(0)?;
        parser.whitespace();
        if parser.at != parser.input.len() {
            return Err(FormulaError::Syntax);
        }
        Ok(parser.program)
    }

    fn emit(&mut self, op: Op) -> Result<(), FormulaError> {
        if self.program.len == MAX_OPS {
            return Err(FormulaError::TooComplex);
        }
        self.program.ops[self.program.len] = op;
        self.program.len += 1;
        Ok(())
    }

    fn whitespace(&mut self) {
        while self.input.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn take(&mut self, byte: u8) -> bool {
        self.whitespace();
        if self.input.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), FormulaError> {
        if self.take(byte) {
            Ok(())
        } else {
            Err(FormulaError::Syntax)
        }
    }

    fn sum(&mut self, depth: usize) -> Result<(), FormulaError> {
        self.product(depth)?;
        while self.take(b'+') {
            self.product(depth)?;
            self.emit(Op::Add)?;
        }
        Ok(())
    }

    fn product(&mut self, depth: usize) -> Result<(), FormulaError> {
        self.atom(depth)?;
        while self.take(b'*') {
            self.atom(depth)?;
            self.emit(Op::Multiply)?;
        }
        Ok(())
    }

    fn atom(&mut self, depth: usize) -> Result<(), FormulaError> {
        if depth >= MAX_DEPTH {
            return Err(FormulaError::TooComplex);
        }
        if self.take(b'-') {
            self.atom(depth + 1)?;
            return self.emit(Op::Negate);
        }
        if self.take(b'(') {
            self.sum(depth + 1)?;
            return self.expect(b')');
        }
        self.whitespace();
        let start = self.at;
        while self.input.get(self.at).is_some_and(u8::is_ascii_alphabetic) {
            self.at += 1;
        }
        if self.at != start {
            let name = &self.input[start..self.at];
            return match name {
                b"a" => self.emit(Op::A),
                b"b" => self.emit(Op::B),
                b"peak" | b"c" => self.emit(Op::Peak),
                b"sin" | b"abs" | b"min" | b"max" => {
                    self.expect(b'(')?;
                    self.sum(depth + 1)?;
                    if name == b"min" || name == b"max" {
                        self.expect(b',')?;
                        self.sum(depth + 1)?;
                    }
                    self.expect(b')')?;
                    self.emit(match name {
                        b"sin" => Op::Sine,
                        b"abs" => Op::Abs,
                        b"min" => Op::Min,
                        _ => Op::Max,
                    })
                }
                _ => Err(FormulaError::Syntax),
            };
        }
        // Decimal literals, optionally with a signed exponent. Parsing the
        // bounded borrowed substring allocates no memory.
        while self
            .input
            .get(self.at)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
        {
            self.at += 1;
        }
        if self
            .input
            .get(self.at)
            .is_some_and(|c| *c == b'e' || *c == b'E')
        {
            self.at += 1;
            if self
                .input
                .get(self.at)
                .is_some_and(|c| *c == b'+' || *c == b'-')
            {
                self.at += 1;
            }
            while self.input.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
        }
        if self.at == start {
            return Err(FormulaError::Syntax);
        }
        let literal =
            std::str::from_utf8(&self.input[start..self.at]).map_err(|_| FormulaError::Syntax)?;
        let value = literal
            .parse::<f32>()
            .map_err(|_| FormulaError::InvalidNumber)?;
        if !value.is_finite() || value.abs() > LIMIT {
            return Err(FormulaError::InvalidNumber);
        }
        self.emit(Op::Constant(value))
    }
}

/// A control-only source; default expression is `a * b + peak`.
/// Expression text is a separately persisted, bounded configuration asset.
pub struct FormulaSource {
    controls: Controls<5>,
    program: Program,
}

impl Default for FormulaSource {
    fn default() -> Self {
        let mut program = Program {
            ops: [Op::Constant(0.0); MAX_OPS],
            len: 5,
        };
        program.ops[..5].copy_from_slice(&[Op::A, Op::B, Op::Multiply, Op::Peak, Op::Add]);
        Self {
            controls: Controls::new([0.5, 0.5, 0.0, 1.0, 0.0]),
            program,
        }
    }
}

impl FormulaSource {
    pub fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.controls.prepare(sample_rate);
    }

    pub fn reset(&mut self) {
        self.controls.reset();
    }

    pub fn set_params(&mut self, params: &FormulaSourceParams) {
        let p = params.sanitized();
        self.controls.set([p.a, p.b, p.base, p.amount, p.c]);
    }

    /// Compiles at most 256 bytes to at most 64 stack operations. No allocation.
    /// Invalid input leaves the previous program installed.
    pub fn set_expression(&mut self, expression: &str) -> Result<(), FormulaError> {
        self.program = Parser::compile(expression)?;
        Ok(())
    }

    /// Inputs and output have the same compile-time length. `peak` is the
    /// instantaneous maximum absolute stereo sample, clamped to 0..1.
    pub fn process_control<const N: usize>(
        &mut self,
        left: &[f32; N],
        right: &[f32; N],
        output: &mut [f32; N],
    ) {
        for ((l, r), value) in left.iter().zip(right).zip(output) {
            let [a, b, base, amount, _c] = self.controls.tick();
            *value = (base + amount * self.program.evaluate(a, b, peak(*l, *r))).clamp(0.0, 1.0);
        }
    }
}

impl FormulaSource {
    /// Evaluate the three scalar inputs, with no audio-derived fourth input.
    pub fn next_value(&mut self) -> f32 {
        let [a, b, base, amount, c] = self.controls.tick();
        (base + amount * self.program.evaluate(a, b, c)).clamp(0.0, 1.0)
    }
}

impl crate::Effect for FormulaSource {
    type Params = FormulaSourceParams;
    fn prepare(&mut self, rate: f32, block: usize) { FormulaSource::prepare(self, rate, block); }
    fn reset(&mut self) { FormulaSource::reset(self); }
    fn set_params(&mut self, params: &Self::Params) { FormulaSource::set_params(self, params); }
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
}
