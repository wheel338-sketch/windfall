//! The bounded expression a controller link may shape its input with.
//!
//! A formula is text such as `1 - x^2` or `clamp(sin(x * pi), 0, 1)`. It is
//! parsed once, away from any time-critical thread, into a fixed-size program
//! of at most [`MAX_FORMULA_OPS`] steps. Running the program touches only a
//! fixed stack: it never allocates, loops a bounded number of times and
//! cannot call anything but the functions listed here. A compiled formula is
//! `Copy`, so a later realtime consumer can carry it by value.
//!
//! `x` is the source value, 0 to 1. A result that is not a number counts as
//! the unshaped input, and every result is held to 0 to 1.
//!
//! Grammar, loosest binding first:
//!
//! ```text
//! sum     = product (("+" | "-") product)*
//! product = unary (("*" | "/") unary)*
//! unary   = "-" unary | power
//! power   = atom ("^" unary)?
//! atom    = number | "x" | "pi" | "e" | name "(" sum ("," sum)* ")" | "(" sum ")"
//! ```
//!
//! Functions: `sin cos tan abs sqrt exp ln log floor ceil round` take one
//! value, `min max pow step` take two (`step(edge, v)` is 0 below the edge
//! and 1 from it up) and `clamp(v, low, high)` and `lerp(a, b, t)` take three.

/// The longest formula text a link stores, in bytes.
pub const MAX_FORMULA_BYTES: usize = 128;
/// The most steps a compiled formula has.
pub const MAX_FORMULA_OPS: usize = 48;
/// The deepest the value stack gets. Parsing refuses anything deeper.
const MAX_STACK: usize = 16;
/// How deeply parentheses and function calls may nest.
const MAX_DEPTH: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Push(f32),
    Input,
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Neg,
    Sin,
    Cos,
    Tan,
    Abs,
    Sqrt,
    Exp,
    Ln,
    Log10,
    Floor,
    Ceil,
    Round,
    Min,
    Max,
    Step,
    Clamp,
    Lerp,
}

impl Op {
    /// How many values the step takes off the stack. Each leaves one.
    fn takes(self) -> usize {
        match self {
            Op::Push(_) | Op::Input => 0,
            Op::Neg
            | Op::Sin
            | Op::Cos
            | Op::Tan
            | Op::Abs
            | Op::Sqrt
            | Op::Exp
            | Op::Ln
            | Op::Log10
            | Op::Floor
            | Op::Ceil
            | Op::Round => 1,
            Op::Add | Op::Sub | Op::Mul | Op::Div | Op::Pow | Op::Min | Op::Max | Op::Step => 2,
            Op::Clamp | Op::Lerp => 3,
        }
    }
}

/// A parsed formula: a fixed program that runs without allocating.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Formula {
    ops: [Op; MAX_FORMULA_OPS],
    len: u8,
}

impl Formula {
    /// Parses `text`. The message of a failure can be shown to the user.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_FORMULA_BYTES {
            return Err(format!(
                "A mapping formula is at most {MAX_FORMULA_BYTES} characters."
            ));
        }
        let mut parser = Parser {
            text: text.as_bytes(),
            at: 0,
            formula: Formula {
                ops: [Op::Input; MAX_FORMULA_OPS],
                len: 0,
            },
            depth: 0,
            stack: 0,
        };
        parser.sum()?;
        parser.space();
        if parser.at != parser.text.len() {
            return Err(format!(
                "Unexpected `{}` in the mapping formula.",
                parser.rest()
            ));
        }
        Ok(parser.formula)
    }

    /// The formula's value for the source value `x`, held to 0 to 1.
    pub fn eval(&self, x: f32) -> f32 {
        let x = if x.is_finite() {
            x.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut stack = [0.0_f32; MAX_STACK];
        let mut top = 0;
        for op in &self.ops[..usize::from(self.len)] {
            // The parser proved the stack never under- or overflows.
            let takes = op.takes();
            let args = &stack[top - takes..top];
            let value = match *op {
                Op::Push(value) => value,
                Op::Input => x,
                Op::Add => args[0] + args[1],
                Op::Sub => args[0] - args[1],
                Op::Mul => args[0] * args[1],
                Op::Div => args[0] / args[1],
                Op::Pow => args[0].powf(args[1]),
                Op::Neg => -args[0],
                Op::Sin => args[0].sin(),
                Op::Cos => args[0].cos(),
                Op::Tan => args[0].tan(),
                Op::Abs => args[0].abs(),
                Op::Sqrt => args[0].sqrt(),
                Op::Exp => args[0].exp(),
                Op::Ln => args[0].ln(),
                Op::Log10 => args[0].log10(),
                Op::Floor => args[0].floor(),
                Op::Ceil => args[0].ceil(),
                Op::Round => args[0].round(),
                Op::Min => args[0].min(args[1]),
                Op::Max => args[0].max(args[1]),
                Op::Step => f32::from(args[1] >= args[0]),
                Op::Clamp => {
                    let (low, high) = (args[1].min(args[2]), args[1].max(args[2]));
                    args[0].max(low).min(high)
                }
                Op::Lerp => args[0] + (args[1] - args[0]) * args[2],
            };
            top -= takes;
            stack[top] = value;
            top += 1;
        }
        let result = stack[0];
        if result.is_finite() {
            result.clamp(0.0, 1.0)
        } else {
            x
        }
    }
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
    formula: Formula,
    depth: usize,
    /// The stack depth the program has reached so far.
    stack: usize,
}

impl Parser<'_> {
    fn rest(&self) -> String {
        String::from_utf8_lossy(&self.text[self.at..]).into_owned()
    }

    fn space(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.space();
        if self.text.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn emit(&mut self, op: Op) -> Result<(), String> {
        let len = usize::from(self.formula.len);
        if len == MAX_FORMULA_OPS {
            return Err("The mapping formula is too long.".to_owned());
        }
        self.stack = self.stack - op.takes() + 1;
        if self.stack > MAX_STACK {
            return Err("The mapping formula is nested too deeply.".to_owned());
        }
        self.formula.ops[len] = op;
        self.formula.len += 1;
        Ok(())
    }

    fn nested<T>(
        &mut self,
        inner: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err("The mapping formula is nested too deeply.".to_owned());
        }
        let result = inner(self);
        self.depth -= 1;
        result
    }

    fn sum(&mut self) -> Result<(), String> {
        self.product()?;
        loop {
            if self.eat(b'+') {
                self.product()?;
                self.emit(Op::Add)?;
            } else if self.eat(b'-') {
                self.product()?;
                self.emit(Op::Sub)?;
            } else {
                return Ok(());
            }
        }
    }

    fn product(&mut self) -> Result<(), String> {
        self.unary()?;
        loop {
            if self.eat(b'*') {
                self.unary()?;
                self.emit(Op::Mul)?;
            } else if self.eat(b'/') {
                self.unary()?;
                self.emit(Op::Div)?;
            } else {
                return Ok(());
            }
        }
    }

    fn unary(&mut self) -> Result<(), String> {
        if self.eat(b'-') {
            self.nested(Self::unary)?;
            return self.emit(Op::Neg);
        }
        self.atom()?;
        if self.eat(b'^') {
            self.nested(Self::unary)?;
            self.emit(Op::Pow)?;
        }
        Ok(())
    }

    fn atom(&mut self) -> Result<(), String> {
        self.space();
        let Some(&first) = self.text.get(self.at) else {
            return Err("The mapping formula ends too early.".to_owned());
        };
        if first == b'(' {
            self.at += 1;
            self.nested(Self::sum)?;
            return self.close();
        }
        if first.is_ascii_digit() || first == b'.' {
            return self.number();
        }
        if first.is_ascii_alphabetic() {
            let start = self.at;
            while self
                .text
                .get(self.at)
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            {
                self.at += 1;
            }
            let name = std::str::from_utf8(&self.text[start..self.at])
                .unwrap_or_default()
                .to_ascii_lowercase();
            return self.named(&name);
        }
        Err(format!(
            "Unexpected `{}` in the mapping formula.",
            self.rest()
        ))
    }

    fn close(&mut self) -> Result<(), String> {
        if self.eat(b')') {
            Ok(())
        } else {
            Err("A `)` is missing in the mapping formula.".to_owned())
        }
    }

    fn number(&mut self) -> Result<(), String> {
        let start = self.at;
        while self
            .text
            .get(self.at)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            self.at += 1;
        }
        let text = std::str::from_utf8(&self.text[start..self.at]).unwrap_or_default();
        let value: f32 = text
            .parse()
            .map_err(|_| format!("`{text}` is not a number."))?;
        if !value.is_finite() {
            return Err(format!("`{text}` is too large."));
        }
        self.emit(Op::Push(value))
    }

    fn named(&mut self, name: &str) -> Result<(), String> {
        match name {
            "x" => return self.emit(Op::Input),
            "pi" => return self.emit(Op::Push(std::f32::consts::PI)),
            "e" => return self.emit(Op::Push(std::f32::consts::E)),
            _ => {}
        }
        let op = match name {
            "sin" => Op::Sin,
            "cos" => Op::Cos,
            "tan" => Op::Tan,
            "abs" => Op::Abs,
            "sqrt" => Op::Sqrt,
            "exp" => Op::Exp,
            "ln" => Op::Ln,
            "log" => Op::Log10,
            "floor" => Op::Floor,
            "ceil" => Op::Ceil,
            "round" => Op::Round,
            "min" => Op::Min,
            "max" => Op::Max,
            "pow" => Op::Pow,
            "step" => Op::Step,
            "clamp" => Op::Clamp,
            "lerp" => Op::Lerp,
            _ => return Err(format!("`{name}` is not known to the mapping formula.")),
        };
        if !self.eat(b'(') {
            return Err(format!("`{name}` needs its values in parentheses."));
        }
        let wanted = op.takes();
        for index in 0..wanted {
            if index > 0 && !self.eat(b',') {
                return Err(format!("`{name}` takes {wanted} values."));
            }
            self.nested(Self::sum)?;
        }
        self.space();
        if self.text.get(self.at) == Some(&b',') {
            return Err(format!("`{name}` takes {wanted} values."));
        }
        self.close()?;
        self.emit(op)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(text: &str, x: f32) -> f32 {
        Formula::parse(text).unwrap().eval(x)
    }

    #[test]
    fn arithmetic_follows_precedence_and_associativity() {
        assert_eq!(eval("x", 0.25), 0.25);
        assert_eq!(eval("1 - x", 0.25), 0.75);
        assert_eq!(eval("0.1 + 0.2 * 2", 0.0), 0.5);
        assert_eq!(eval("(0.1 + 0.2) * 2", 0.0), 0.6);
        assert_eq!(eval("1 - 0.5 - 0.25", 0.0), 0.25);
        assert_eq!(eval("x ^ 2", 0.5), 0.25);
        // Unary minus binds looser than a power and applies to its result.
        assert_eq!(eval("1 + -x^2", 0.5), 0.75);
        // A power is right associative: 0.5^(2^0.5).
        assert_eq!(eval("0.5 ^ 2 ^ 0.5", 0.0), 0.5_f32.powf(2.0_f32.powf(0.5)));
        assert_eq!(eval("1 / 4", 0.0), 0.25);
    }

    #[test]
    fn functions_and_constants_evaluate() {
        assert!((eval("sin(x * pi)", 0.5) - 1.0).abs() < 1e-6);
        assert!((eval("cos(0)", 0.0) - 1.0).abs() < 1e-6);
        assert_eq!(eval("abs(x - 1)", 0.25), 0.75);
        assert_eq!(eval("sqrt(x)", 0.25), 0.5);
        assert_eq!(eval("min(x, 0.3)", 0.9), 0.3);
        assert_eq!(eval("max(x, 0.3)", 0.1), 0.3);
        assert_eq!(eval("pow(x, 2)", 0.5), 0.25);
        assert_eq!(eval("step(0.5, x)", 0.49), 0.0);
        assert_eq!(eval("step(0.5, x)", 0.5), 1.0);
        assert_eq!(eval("clamp(x, 0.2, 0.4)", 0.9), 0.4);
        assert_eq!(eval("clamp(x, 0.4, 0.2)", 0.0), 0.2);
        assert_eq!(eval("lerp(0.2, 0.8, x)", 0.5), 0.5);
        assert_eq!(eval("floor(x * 4) / 4", 0.6), 0.5);
        assert_eq!(eval("round(x)", 0.6), 1.0);
        assert_eq!(eval("ceil(x)", 0.1), 1.0);
        assert!((eval("ln(e)", 0.0) - 1.0).abs() < 1e-6);
        assert!((eval("log(10)", 0.0) - 1.0).abs() < 1e-6);
        assert!((eval("exp(0) * 0.5", 0.0) - 0.5).abs() < 1e-6);
        assert!((eval("tan(0)", 0.0)).abs() < 1e-6);
        assert_eq!(eval("SIN(X * PI)", 0.5), eval("sin(x * pi)", 0.5));
    }

    #[test]
    fn results_are_bounded_and_non_numbers_fall_back_to_the_input() {
        assert_eq!(eval("x * 10", 0.5), 1.0);
        assert_eq!(eval("x - 10", 0.5), 0.0);
        // Division by zero and the square root of a negative are not numbers.
        assert_eq!(eval("0 / 0", 0.4), 0.4);
        assert_eq!(eval("sqrt(0 - 1)", 0.3), 0.3);
        assert_eq!(eval("1 / 0", 0.3), 0.3);
        // The input is held to its own range first.
        assert_eq!(eval("x", 7.0), 1.0);
        assert_eq!(eval("x", f32::NAN), 0.0);
    }

    #[test]
    fn malformed_text_is_refused_with_a_reason() {
        for text in [
            "",
            "x +",
            "(x",
            "x)",
            "sin x",
            "sin(x, 1)",
            "min(x)",
            "clamp(x, 1)",
            "y",
            "2 x",
            "1..2",
            "x $ 2",
            "system(1)",
        ] {
            let error = Formula::parse(text).unwrap_err();
            assert!(!error.is_empty(), "{text}");
        }
    }

    #[test]
    fn length_depth_and_program_size_are_bounded() {
        let long = "x".to_owned() + &" + x".repeat(MAX_FORMULA_BYTES);
        assert!(Formula::parse(&long).unwrap_err().contains("at most"));
        // Fits the text limit but not the program.
        let many = "x".to_owned() + &"+x".repeat(40);
        assert!(many.len() <= MAX_FORMULA_BYTES);
        assert!(Formula::parse(&many).unwrap_err().contains("too long"));
        let deep = "(".repeat(MAX_DEPTH + 1) + "x" + &")".repeat(MAX_DEPTH + 1);
        assert!(Formula::parse(&deep).unwrap_err().contains("nested"));
        let allowed = "(".repeat(MAX_DEPTH) + "x" + &")".repeat(MAX_DEPTH);
        assert_eq!(Formula::parse(&allowed).unwrap().eval(0.5), 0.5);
        // Right-nested operands grow the value stack; that is bounded too.
        let stacked = "x+(".repeat(11) + "x" + &")".repeat(11);
        assert!((Formula::parse(&stacked).unwrap().eval(0.05) - 0.6).abs() < 1e-5);
    }

    #[test]
    fn a_compiled_formula_is_copy_and_gives_the_same_values() {
        let formula = Formula::parse("1 - x ^ 2").unwrap();
        let copy = formula;
        for step in 0..=10 {
            let x = step as f32 / 10.0;
            assert_eq!(formula.eval(x), copy.eval(x));
            assert_eq!(copy.eval(x), (1.0 - x * x).clamp(0.0, 1.0));
        }
    }
}
