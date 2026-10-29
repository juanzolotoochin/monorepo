//! A RON syntax tree that keeps what `ron::Value` drops: struct and enum
//! variant names (`Sphere(...)`, `SingleCurve`, `Some(...)`). Effect files
//! merge over their `base` as trees (`catalog.rs`); the merged tree is printed
//! back and parsed strictly by `vfx::parse`.
//!
//! Numbers, bools, strings and chars keep their source text; comments are
//! dropped. Raw strings, byte strings, raw identifiers and the
//! `#![enable(...)]` extensions header are errors (no effect file uses them).
use std::fmt::Display;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A number, bool, string or char, as written.
    Scalar(String),
    /// `Name`, `Name(...)` or `(...)`: a struct, tuple or enum variant,
    /// `Some(...)` and `None` included.
    Struct { name: Option<String>, fields: Fields },
    List(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fields {
    /// A bare name: `None`, `SingleCurve`.
    Unit,
    /// `(0.0, 1.0)`, `Some(x)`, `Standard((...))`. Also `()`.
    Tuple(Vec<Value>),
    /// `(a: 1, b: 2)`: a section.
    Named(Vec<(String, Value)>),
}

impl Value {
    /// The value of field `name`, if this is a section that has one.
    pub fn field(&self, name: &str) -> Option<&Value> {
        match self {
            Value::Struct { fields: Fields::Named(fields), .. } => fields.iter().find(|(n, _)| n == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Removes field `name` from a section and returns it.
    pub fn take_field(&mut self, name: &str) -> Option<Value> {
        let Value::Struct { fields: Fields::Named(fields), .. } = self else { return None };
        let index = fields.iter().position(|(n, _)| n == name)?;
        Some(fields.remove(index).1)
    }

    /// The text of a string, escapes resolved.
    pub fn as_str(&self) -> Option<String> {
        let Value::Scalar(text) = self else { return None };
        unescape(text.strip_prefix('"')?.strip_suffix('"')?).ok()
    }

    /// The value as RON text, one field or item per line.
    pub fn print(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    /// Whether the value prints on one line: no sections, lists or maps in it.
    fn flat(&self) -> bool {
        match self {
            Value::Scalar(_) | Value::Struct { fields: Fields::Unit, .. } => true,
            Value::Struct { fields: Fields::Tuple(items), .. } => items.iter().all(Value::flat),
            _ => false,
        }
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Value::Scalar(text) => out.push_str(text),
            Value::Struct { name, fields } => {
                out.push_str(name.as_deref().unwrap_or(""));
                match fields {
                    Fields::Unit => {}
                    Fields::Tuple(items) if self.flat() => {
                        out.push('(');
                        for (i, item) in items.iter().enumerate() {
                            if i > 0 {
                                out.push_str(", ");
                            }
                            item.write(out, indent);
                        }
                        out.push(')');
                    }
                    Fields::Tuple(items) => block(out, indent, ('(', ')'), items, |out, item, indent| item.write(out, indent)),
                    Fields::Named(fields) => block(out, indent, ('(', ')'), fields, |out, (name, value), indent| {
                        out.push_str(name);
                        out.push_str(": ");
                        value.write(out, indent);
                    }),
                }
            }
            Value::List(items) => block(out, indent, ('[', ']'), items, |out, item, indent| item.write(out, indent)),
            Value::Map(entries) => block(out, indent, ('{', '}'), entries, |out, (key, value), indent| {
                key.write(out, indent);
                out.push_str(": ");
                value.write(out, indent);
            }),
        }
    }
}

/// `items` between `open` and `close`, one per line, each followed by a comma.
fn block<T>(out: &mut String, indent: usize, (open, close): (char, char), items: &[T], each: impl Fn(&mut String, &T, usize)) {
    out.push(open);
    if !items.is_empty() {
        for item in items {
            out.push('\n');
            out.push_str(&"    ".repeat(indent + 1));
            each(out, item, indent + 1);
            out.push(',');
        }
        out.push('\n');
        out.push_str(&"    ".repeat(indent));
    }
    out.push(close);
}

/// Parses one RON value. Errors start with `line:column: `.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut parser = Parser { text, pos: 0 };
    parser.skip()?;
    if parser.rest().starts_with("#!") {
        return Err(parser.error("the #![enable(...)] extensions header isn't supported"));
    }
    let value = parser.value()?;
    parser.skip()?;
    if parser.pos < text.len() {
        return Err(parser.error("expected the end of the file"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    /// A byte offset into `text`.
    pos: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn error(&self, what: impl Display) -> String {
        let before = &self.text[..self.pos];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        format!("{line}:{column}: {what}")
    }

    /// Skips whitespace and comments (`/* */` nest, as in RON).
    fn skip(&mut self) -> Result<(), String> {
        loop {
            let rest = self.rest();
            self.pos += rest.len() - rest.trim_start().len();
            let rest = self.rest();
            if rest.starts_with("//") {
                self.pos += rest.find('\n').unwrap_or(rest.len());
            } else if rest.starts_with("/*") {
                let start = self.pos;
                let mut depth = 0;
                loop {
                    let rest = self.rest();
                    if rest.starts_with("/*") {
                        depth += 1;
                        self.pos += 2;
                    } else if rest.starts_with("*/") {
                        depth -= 1;
                        self.pos += 2;
                        if depth == 0 {
                            break;
                        }
                    } else if let Some(c) = rest.chars().next() {
                        self.pos += c.len_utf8();
                    } else {
                        self.pos = start;
                        return Err(self.error("unterminated /* comment"));
                    }
                }
            } else {
                return Ok(());
            }
        }
    }

    /// Skips whitespace, then consumes `c` if it's next.
    fn eat(&mut self, c: char) -> Result<bool, String> {
        self.skip()?;
        let found = self.peek() == Some(c);
        if found {
            self.pos += 1;
        }
        Ok(found)
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        if self.eat(c)? {
            Ok(())
        } else {
            Err(self.error(format!("expected `{c}`")))
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip()?;
        let Some(c) = self.peek() else { return Err(self.error("expected a value")) };
        match c {
            '[' => {
                self.pos += 1;
                Ok(Value::List(self.items(']', Self::value)?))
            }
            '{' => {
                self.pos += 1;
                let entries = self.items('}', |p| {
                    let key = p.value()?;
                    p.expect(':')?;
                    Ok((key, p.value()?))
                })?;
                Ok(Value::Map(entries))
            }
            '(' => Ok(Value::Struct { name: None, fields: self.body()? }),
            '"' => self.quoted('"', "string"),
            '\'' => self.quoted('\'', "char"),
            c if c.is_ascii_digit() || matches!(c, '-' | '+' | '.') => self.number(),
            c if c.is_ascii_alphabetic() || c == '_' => {
                let name = self.ident()?;
                if name == "true" || name == "false" {
                    return Ok(Value::Scalar(name));
                }
                self.skip()?;
                let fields = if self.peek() == Some('(') { self.body()? } else { Fields::Unit };
                Ok(Value::Struct { name: Some(name), fields })
            }
            _ => Err(self.error(format!("unexpected `{c}`"))),
        }
    }

    /// Comma-separated items up to `close` (a trailing comma is fine).
    fn items<T>(&mut self, close: char, mut item: impl FnMut(&mut Self) -> Result<T, String>) -> Result<Vec<T>, String> {
        let mut items = Vec::new();
        loop {
            if self.eat(close)? {
                return Ok(items);
            }
            items.push(item(self)?);
            if !self.eat(',')? {
                self.expect(close)?;
                return Ok(items);
            }
        }
    }

    /// `(...)`: named fields if it starts with `name:`, else a tuple.
    fn body(&mut self) -> Result<Fields, String> {
        self.expect('(')?;
        self.skip()?;
        let start = self.pos;
        let named = self.peek().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && {
            self.word();
            self.skip()?;
            self.rest().starts_with(':')
        };
        self.pos = start;
        if !named {
            return Ok(Fields::Tuple(self.items(')', Self::value)?));
        }
        let mut fields: Vec<(String, Value)> = Vec::new();
        self.items(')', |p| {
            p.skip()?;
            let at = p.pos;
            let name = p.ident()?;
            if fields.iter().any(|(n, _)| *n == name) {
                p.pos = at;
                return Err(p.error(format!("field `{name}` appears twice")));
            }
            p.expect(':')?;
            let value = p.value()?;
            fields.push((name, value));
            Ok(())
        })?;
        Ok(Fields::Named(fields))
    }

    /// The identifier chars from here on, consumed.
    fn word(&mut self) -> &'a str {
        let rest = self.rest();
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
        self.pos += end;
        &rest[..end]
    }

    fn ident(&mut self) -> Result<String, String> {
        let start = self.pos;
        let word = self.word();
        if word.is_empty() {
            return Err(self.error("expected a name"));
        }
        if matches!(word, "r" | "b" | "br") && matches!(self.peek(), Some('"' | '#' | '\'')) {
            self.pos = start;
            return Err(self.error("raw strings, raw identifiers and byte strings aren't supported"));
        }
        Ok(word.to_string())
    }

    /// A number as written: digits, `.`, `_`, exponents, `0x`, `inf`, `NaN`.
    fn number(&mut self) -> Result<Value, String> {
        let start = self.pos;
        let bytes = self.text.as_bytes();
        let mut end = start + usize::from(matches!(bytes[start], b'+' | b'-'));
        let digits = end;
        while let Some(&b) = bytes.get(end) {
            let exponent_sign = matches!(b, b'+' | b'-')
                && matches!(bytes[end - 1], b'e' | b'E')
                && !self.text[digits..end].starts_with("0x");
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || exponent_sign {
                end += 1;
            } else {
                break;
            }
        }
        if end == digits {
            return Err(self.error("expected a number"));
        }
        self.pos = end;
        Ok(Value::Scalar(self.text[start..end].to_string()))
    }

    /// A string or char, kept as written; its escapes must be valid.
    fn quoted(&mut self, quote: char, what: &str) -> Result<Value, String> {
        let start = self.pos;
        let mut chars = self.rest().char_indices().skip(1);
        let end = loop {
            match chars.next() {
                Some((_, '\\')) => {
                    chars.next();
                }
                Some((i, c)) if c == quote => break start + i + 1,
                Some(_) => {}
                None => return Err(self.error(format!("unterminated {what}"))),
            }
        };
        let text = &self.text[start..end];
        let body = unescape(&text[1..text.len() - 1]).map_err(|e| self.error(e))?;
        if quote == '\'' && body.chars().count() != 1 {
            return Err(self.error("a char holds one character"));
        }
        self.pos = end;
        Ok(Value::Scalar(text.to_string()))
    }
}

/// Resolves a string's or char's escapes.
fn unescape(body: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next() {
            Some('n') => '\n',
            Some('r') => '\r',
            Some('t') => '\t',
            Some('0') => '\0',
            Some(c @ ('\\' | '"' | '\'' | '/')) => c,
            Some('u') => {
                let rest = chars.as_str();
                let hex = rest.strip_prefix('{').and_then(|r| r.split_once('}')).map(|(hex, _)| hex);
                let Some(hex) = hex else { return Err("expected \\u{...}".into()) };
                let code = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                chars = rest[hex.len() + 2..].chars();
                code.ok_or_else(|| format!("bad escape \\u{{{hex}}}"))?
            }
            Some(c) => return Err(format!("unsupported escape \\{c}")),
            None => return Err("a string can't end in \\".into()),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_names_and_source_text() {
        let tree = parse(
            "// a comment\n(a: Sphere(radius: 1.5e-1), b: [Some((1, -2.0)), None], c: SingleCurve, /* x /* y */ */ d: \"q\\\"\\u{41}\", e: {'k': true})",
        )
        .unwrap();
        let printed = tree.print();
        assert_eq!(parse(&printed).unwrap(), tree);
        assert_eq!(tree.field("d").unwrap().as_str().unwrap(), "q\"A");
        let Some(Value::Struct { name, fields: Fields::Named(fields) }) = tree.field("a") else { panic!() };
        assert_eq!(name.as_deref(), Some("Sphere"));
        assert_eq!(fields[0].1, Value::Scalar("1.5e-1".into()));
        assert!(printed.contains("Some((1, -2.0))") && printed.contains("c: SingleCurve"), "{printed}");
    }

    #[test]
    fn unsupported_syntax_names_the_place() {
        assert_eq!(parse("(\n  a: r\"x\")").unwrap_err(), "2:6: raw strings, raw identifiers and byte strings aren't supported");
        assert!(parse("(a: b\"x\")").unwrap_err().starts_with("1:5: "));
        assert!(parse("#![enable(implicit_some)]\n(a: 1)").unwrap_err().contains("extensions"));
        assert!(parse("(a: 1, a: 2)").unwrap_err().contains("twice"));
        assert!(parse("(a: \"x)").unwrap_err().contains("unterminated"));
        assert!(parse("(a: 1) 2").is_err());
    }
}
