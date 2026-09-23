//! Minimal JSON writing shared by the glTF manifest and the node `extras`.
//!
//! The GLB's JSON chunk is written by hand — it is a few flat arrays, and a
//! serializer would add a dependency and an intermediate tree for nothing. The
//! price is that every string and number has to be made valid JSON here: names
//! come straight out of an untrusted file, and one stray control character or
//! `NaN` makes the whole model unreadable, not just one field.

use std::fmt::Write as _;

/// A JSON string literal, quotes included.
///
/// Escapes exactly what RFC 8259 requires — `"`, `\` and U+0000..U+001F — and
/// passes all other characters through as UTF-8.
pub(crate) fn string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A JSON number, never `NaN` / `Infinity` — neither is valid JSON, so a
/// non-finite value is written as `0`. Integral values drop the fraction.
pub(crate) fn num(v: f32) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    if v.fract() == 0.0 && v.abs() < 1e9 {
        #[expect(clippy::cast_possible_truncation, reason = "integral and below 1e9: exact in i64")]
        return format!("{}", v as i64);
    }
    format!("{v}")
}

/// [`num`] for the `f64` accessor bounds.
pub(crate) fn num64(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_owned();
    }
    if v.fract() == 0.0 && v.abs() < 1e15 {
        #[expect(clippy::cast_possible_truncation, reason = "integral and below 1e15: exact in i64")]
        return format!("{}", v as i64);
    }
    format!("{v}")
}

/// A JSON array of numbers.
pub(crate) fn nums(vs: &[f32]) -> String {
    let mut s = String::from("[");
    for (i, v) in vs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&num(*v));
    }
    s.push(']');
    s
}

/// An incrementally built JSON object.
pub(crate) struct Obj {
    s:     String,
    first: bool,
}

impl Obj {
    pub(crate) fn new() -> Self {
        Self { s: String::from("{"), first: true }
    }

    fn key(&mut self, k: &str) {
        if !self.first {
            self.s.push(',');
        }
        self.first = false;
        self.s.push_str(&string(k));
        self.s.push(':');
    }

    /// A value that is already JSON.
    pub(crate) fn raw(&mut self, k: &str, v: &str) {
        self.key(k);
        self.s.push_str(v);
    }

    pub(crate) fn num(&mut self, k: &str, v: f32) {
        self.raw(k, &num(v));
    }

    pub(crate) fn int(&mut self, k: &str, v: u64) {
        self.raw(k, &v.to_string());
    }

    pub(crate) fn bool(&mut self, k: &str, v: bool) {
        self.raw(k, if v { "true" } else { "false" });
    }

    pub(crate) fn string(&mut self, k: &str, v: &str) {
        self.raw(k, &string(v));
    }

    pub(crate) fn vec2(&mut self, k: &str, v: [f32; 2]) {
        self.raw(k, &nums(&v));
    }

    pub(crate) fn vec2i(&mut self, k: &str, v: [u8; 2]) {
        self.raw(k, &format!("[{},{}]", v[0], v[1]));
    }

    pub(crate) fn vec3(&mut self, k: &str, v: [f32; 3]) {
        self.raw(k, &nums(&v));
    }

    pub(crate) fn vec4(&mut self, k: &str, v: [f32; 4]) {
        self.raw(k, &nums(&v));
    }

    pub(crate) fn finish(mut self) -> String {
        self.s.push('}');
        self.s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_escape_what_json_requires() {
        assert_eq!(string("plain"), r#""plain""#);
        assert_eq!(string("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(string("\n\r\t"), r#""\n\r\t""#);
        assert_eq!(string("\u{1}\u{1f}"), r#""\u0001\u001f""#);
        // Printable non-ASCII passes through untouched.
        assert_eq!(string("Кель'Тузад ☃"), "\"Кель'Тузад ☃\"");
        // DEL and other non-control codepoints are legal unescaped.
        assert_eq!(string("\u{7f}\u{200b}"), "\"\u{7f}\u{200b}\"");
    }

    #[test]
    fn numbers_are_always_valid_json() {
        assert_eq!(num(f32::NAN), "0");
        assert_eq!(num(f32::INFINITY), "0");
        assert_eq!(num(f32::NEG_INFINITY), "0");
        assert_eq!(num(3.0), "3");
        assert_eq!(num(-0.5), "-0.5");
        assert_eq!(num(1e20), "100000000000000000000");
        assert_eq!(num64(f64::NAN), "0");
        assert_eq!(num64(2.0), "2");
        assert_eq!(num64(0.25), "0.25");
        assert_eq!(num64(1e300), format!("{}", 1e300));
    }

    #[test]
    fn object_writer_round_trips_through_a_real_parser() {
        let mut o = Obj::new();
        o.string("na\"me", "v\u{0}");
        o.num("n", f32::NAN);
        o.int("i", 7);
        o.bool("t", true);
        o.bool("f", false);
        o.vec2("v2", [1.0, 0.5]);
        o.vec2i("v2i", [3, 4]);
        o.vec3("v3", [1.0, 2.0, 3.0]);
        o.vec4("v4", [0.0; 4]);
        o.raw("raw", "[]");
        let parsed: serde_json::Value = serde_json::from_str(&o.finish()).unwrap();
        assert_eq!(parsed["na\"me"], "v\u{0}");
        assert_eq!(parsed["n"], 0);
        assert_eq!(parsed["v2i"][1], 4);
        assert_eq!(parsed["t"], true);
        assert_eq!(parsed["f"], false);
        assert_eq!(Obj::new().finish(), "{}");
    }
}
