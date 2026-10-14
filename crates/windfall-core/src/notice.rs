//! Words for a person, kept as a stable code and named values.
//!
//! A crate that has something to tell the user (why a file was refused,
//! what an import changed) says it as a [`Notice`]: a code from the
//! append-only registry in `windfall-ipc` and the values the sentence is made
//! of. A notice holds no words. The interface has a sentence for every code
//! in every language it speaks, so the same notice reads as English or
//! Spanish wherever it is shown, and a kept one follows a language change.
//!
//! The text form of a notice, which its `Display` writes and which travels
//! wherever a `String` is expected (an error string, a report line, a
//! warning), is [`NOTICE_PREFIX`] followed by the JSON object
//! `{"code":…,"params":{…}}`, parameters in name order. This is the form
//! `windfall_ipc::ui_message` writes too. A value that is itself a notice is
//! carried in its text form, and the interface puts it into words as well;
//! any other text, such as what the operating system said, is shown as it is.

use std::fmt;
use std::fmt::Write as _;

/// What the text form of every notice starts with.
pub const NOTICE_PREFIX: &str = "windfall-message:";

/// One value of a notice.
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Int(i64),
    UInt(u64),
    /// A number that is not finite is carried as text, since JSON has no
    /// word for it.
    Float(f64),
    Text(String),
}

macro_rules! arg_from {
    ($variant:ident as $target:ty: $($source:ty),*) => {
        $(impl From<$source> for Arg {
            fn from(value: $source) -> Self {
                Arg::$variant(value as $target)
            }
        })*
    };
}
arg_from!(UInt as u64: u8, u16, u32, u64, usize);
arg_from!(Int as i64: i8, i16, i32, i64, isize);
arg_from!(Float as f64: f32, f64);

impl From<&str> for Arg {
    fn from(value: &str) -> Self {
        Arg::Text(value.to_owned())
    }
}
impl From<String> for Arg {
    fn from(value: String) -> Self {
        Arg::Text(value)
    }
}
impl From<Notice> for Arg {
    fn from(value: Notice) -> Self {
        Arg::Text(value.to_string())
    }
}

/// A value [`notice!`](crate::notice!) can take, by reference as `format!`
/// does: numbers, text, a path's display form and other notices.
pub trait ToArg {
    fn to_arg(&self) -> Arg;
}

macro_rules! number_to_arg {
    ($($source:ty),*) => {
        $(impl ToArg for $source {
            fn to_arg(&self) -> Arg {
                (*self).into()
            }
        })*
    };
}
number_to_arg!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize, f32, f64);

impl ToArg for str {
    fn to_arg(&self) -> Arg {
        Arg::Text(self.to_owned())
    }
}
impl ToArg for String {
    fn to_arg(&self) -> Arg {
        Arg::Text(self.clone())
    }
}
impl ToArg for std::borrow::Cow<'_, str> {
    fn to_arg(&self) -> Arg {
        Arg::Text(self.to_string())
    }
}
impl ToArg for bool {
    /// As `format!` writes it: `true` or `false`.
    fn to_arg(&self) -> Arg {
        Arg::Text(self.to_string())
    }
}
impl ToArg for char {
    fn to_arg(&self) -> Arg {
        Arg::Text(self.to_string())
    }
}
impl ToArg for std::path::Display<'_> {
    fn to_arg(&self) -> Arg {
        Arg::Text(self.to_string())
    }
}
impl ToArg for Notice {
    fn to_arg(&self) -> Arg {
        Arg::Text(self.to_string())
    }
}
impl ToArg for Arg {
    fn to_arg(&self) -> Arg {
        self.clone()
    }
}
impl<T: ToArg + ?Sized> ToArg for &T {
    fn to_arg(&self) -> Arg {
        (**self).to_arg()
    }
}

/// Something to tell the user: a registered code and its named values.
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub code: &'static str,
    pub params: Vec<(&'static str, Arg)>,
    /// The text form of a coded message given to [`term`], kept as it is.
    verbatim: Option<String>,
}

impl Notice {
    pub fn new(code: &'static str) -> Self {
        Self {
            code,
            params: Vec::new(),
            verbatim: None,
        }
    }

    /// The notice with one more value. A name given twice keeps the last.
    pub fn with(mut self, name: &'static str, value: impl Into<Arg>) -> Self {
        let value = value.into();
        match self.params.iter_mut().find(|(known, _)| *known == name) {
            Some(slot) => slot.1 = value,
            None => self.params.push((name, value)),
        }
        self
    }

    /// The value named `name`, if the notice has one.
    pub fn param(&self, name: &str) -> Option<&Arg> {
        self.params
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, value)| value)
    }
}

/// A word or name the interface shows in its own language when it has one
/// for it (a setting's name, an operation, a built-in device), and as it is
/// otherwise. Use it for a value that is one of Windfall's own English terms,
/// never for a name somebody typed. A text that is already a coded message
/// (a reason passed on from deeper down) stays that message.
pub fn term(text: impl AsRef<str>) -> Notice {
    let text = text.as_ref();
    if decode(text).is_some() {
        return Notice {
            verbatim: Some(text.to_owned()),
            ..Notice::new("text.term")
        };
    }
    Notice::new("text.term").with("text", text)
}

/// Builds a [`Notice`] from a literal code and `name = value` pairs. Values
/// are borrowed, as `format!` borrows them. Names are the parameter names
/// the registry declares for the code, which the source guard checks.
#[macro_export]
macro_rules! notice {
    ($code:literal $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::notice::Notice::new($code)
            $(.with(stringify!($name), $crate::notice::ToArg::to_arg(&$value)))*
    };
}

fn write_json_string(out: &mut impl fmt::Write, text: &str) -> fmt::Result {
    out.write_char('"')?;
    for c in text.chars() {
        match c {
            '"' => out.write_str("\\\"")?,
            '\\' => out.write_str("\\\\")?,
            '\n' => out.write_str("\\n")?,
            '\r' => out.write_str("\\r")?,
            '\t' => out.write_str("\\t")?,
            '\u{08}' => out.write_str("\\b")?,
            '\u{0c}' => out.write_str("\\f")?,
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32)?,
            c => out.write_char(c)?,
        }
    }
    out.write_char('"')
}

impl fmt::Display for Arg {
    /// The value as JSON.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Arg::Int(value) => write!(f, "{value}"),
            Arg::UInt(value) => write!(f, "{value}"),
            Arg::Float(value) if value.is_finite() => {
                // Rust writes every finite float as plain decimals, which is
                // valid JSON; a negative zero is a zero.
                if *value == 0.0 {
                    f.write_str("0")
                } else {
                    write!(f, "{value}")
                }
            }
            Arg::Float(value) => write_json_string(f, &value.to_string()),
            Arg::Text(text) => write_json_string(f, text),
        }
    }
}

impl fmt::Display for Notice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(text) = &self.verbatim {
            return f.write_str(text);
        }
        f.write_str(NOTICE_PREFIX)?;
        f.write_str("{\"code\":")?;
        write_json_string(f, self.code)?;
        f.write_str(",\"params\":{")?;
        let mut params: Vec<_> = self.params.iter().collect();
        params.sort_by(|a, b| a.0.cmp(b.0));
        for (index, (name, value)) in params.into_iter().enumerate() {
            if index > 0 {
                f.write_char(',')?;
            }
            write_json_string(f, name)?;
            write!(f, ":{value}")?;
        }
        f.write_str("}}")
    }
}

impl From<Notice> for String {
    fn from(notice: Notice) -> Self {
        notice.to_string()
    }
}

/// A notice read back from its text form, for tests and for code that has
/// to tell one notice from another.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub code: String,
    pub params: Vec<(String, Arg)>,
}

impl Decoded {
    /// The value named `name`.
    pub fn param(&self, name: &str) -> Option<&Arg> {
        self.params
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value)
    }

    /// The value named `name` as text, numbers written as JSON writes them.
    pub fn text(&self, name: &str) -> Option<String> {
        self.param(name).map(|value| match value {
            Arg::Text(text) => text.clone(),
            other => other.to_string(),
        })
    }
}

/// The code of a notice in its text form, or `None` for any other text.
pub fn code_of(text: &str) -> Option<String> {
    decode(text).map(|decoded| decoded.code)
}

/// Reads the text form of a notice; `None` for any other text.
pub fn decode(text: &str) -> Option<Decoded> {
    let mut reader = Reader {
        text: text.strip_prefix(NOTICE_PREFIX)?.as_bytes(),
        at: 0,
    };
    reader.expect(b'{')?;
    let mut code = None;
    let mut params = Vec::new();
    loop {
        let key = reader.string()?;
        reader.expect(b':')?;
        match key.as_str() {
            "code" => code = Some(reader.string()?),
            "params" => {
                reader.expect(b'{')?;
                if !reader.eat(b'}') {
                    loop {
                        let name = reader.string()?;
                        reader.expect(b':')?;
                        params.push((name, reader.value()?));
                        if reader.eat(b'}') {
                            break;
                        }
                        reader.expect(b',')?;
                    }
                }
            }
            _ => return None,
        }
        if reader.eat(b'}') {
            break;
        }
        reader.expect(b',')?;
    }
    (reader.at == reader.text.len()).then_some(())?;
    Some(Decoded {
        code: code?,
        params,
    })
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn eat(&mut self, byte: u8) -> bool {
        if self.text.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        self.eat(byte).then_some(())
    }

    fn string(&mut self) -> Option<String> {
        self.expect(b'"')?;
        let mut bytes = Vec::new();
        loop {
            let byte = *self.text.get(self.at)?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(bytes).ok(),
                b'\\' => {
                    let escaped = *self.text.get(self.at)?;
                    self.at += 1;
                    let c = match escaped {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'b' => '\u{08}',
                        b'f' => '\u{0c}',
                        b'u' => {
                            let hex =
                                std::str::from_utf8(self.text.get(self.at..self.at + 4)?).ok()?;
                            self.at += 4;
                            char::from_u32(u32::from_str_radix(hex, 16).ok()?)?
                        }
                        _ => return None,
                    };
                    let mut buffer = [0; 4];
                    bytes.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                }
                other => bytes.push(other),
            }
        }
    }

    fn value(&mut self) -> Option<Arg> {
        if self.text.get(self.at) == Some(&b'"') {
            return self.string().map(Arg::Text);
        }
        let start = self.at;
        while self
            .text
            .get(self.at)
            .is_some_and(|b| b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E'))
        {
            self.at += 1;
        }
        let number = std::str::from_utf8(&self.text[start..self.at]).ok()?;
        if let Ok(value) = number.parse::<u64>() {
            Some(Arg::UInt(value))
        } else if let Ok(value) = number.parse::<i64>() {
            Some(Arg::Int(value))
        } else {
            number.parse::<f64>().ok().map(Arg::Float)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notice_is_its_code_and_values_in_name_order_and_reads_back() {
        let owner = crate::notice!("project.owner.channel", id = 3_u32);
        let notice = crate::notice!(
            "project.check.volumeOutOfRange",
            value = 2.5_f64,
            owner = owner.clone(),
            max = 4_u32,
            name = "Kick \"one\"\n\\",
        );
        let text = notice.to_string();
        assert_eq!(
            text,
            "windfall-message:{\"code\":\"project.check.volumeOutOfRange\",\"params\":{\"max\":4,\"name\":\"Kick \\\"one\\\"\\n\\\\\",\"owner\":\"windfall-message:{\\\"code\\\":\\\"project.owner.channel\\\",\\\"params\\\":{\\\"id\\\":3}}\",\"value\":2.5}}"
        );
        let decoded = decode(&text).unwrap();
        assert_eq!(decoded.code, "project.check.volumeOutOfRange");
        assert_eq!(decoded.param("max"), Some(&Arg::UInt(4)));
        assert_eq!(decoded.param("value"), Some(&Arg::Float(2.5)));
        assert_eq!(decoded.text("name").unwrap(), "Kick \"one\"\n\\");
        assert_eq!(decoded.text("owner").unwrap(), owner.to_string());
        assert_eq!(
            code_of(&decoded.text("owner").unwrap()).unwrap(),
            "project.owner.channel"
        );
        assert_eq!(code_of("plain words"), None);
        assert_eq!(String::from(notice.clone()), text);
    }

    #[test]
    fn a_term_is_a_word_to_translate_unless_it_is_a_coded_message_already() {
        let word = term("Cutoff").to_string();
        let decoded = decode(&word).unwrap();
        assert_eq!(decoded.code, "text.term");
        assert_eq!(decoded.text("text").unwrap(), "Cutoff");
        let reason = crate::notice!("codec.channelCount", channels = 65_u32).to_string();
        assert_eq!(term(&reason).to_string(), reason);
        let outer = crate::notice!("codec.unsupported", reason = term(&reason)).to_string();
        assert_eq!(decode(&outer).unwrap().text("reason").unwrap(), reason);
    }

    #[test]
    fn numbers_that_json_cannot_hold_are_text_and_negative_numbers_read_back() {
        let text = crate::notice!(
            "x.y",
            a = f64::NAN,
            b = f64::INFINITY,
            c = -3_i32,
            d = -0.0_f64,
            e = 1e-7_f64
        )
        .to_string();
        let decoded = decode(&text).unwrap();
        assert_eq!(decoded.text("a").unwrap(), "NaN");
        assert_eq!(decoded.text("b").unwrap(), "inf");
        assert_eq!(decoded.param("c"), Some(&Arg::Int(-3)));
        assert_eq!(decoded.param("d"), Some(&Arg::UInt(0)));
        assert_eq!(decoded.param("e"), Some(&Arg::Float(1e-7)));
        assert!(decode("windfall-message:{").is_none());
        assert!(decode("windfall-message:{\"code\":\"a.b\",\"params\":{}}x").is_none());
    }
}
