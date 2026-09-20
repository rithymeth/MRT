//! Saving and loading game state: a plain MRT value, written to a file and
//! read back.
//!
//! -- Why this exists, given MRT already has a JSON reader and writer --
//!
//! `examples/lib/json.mrt` turns any plain MRT value into JSON text and
//! back, entirely in MRT itself, and a program that wants a different file
//! layout should keep using it. What it cannot do is touch a file at all:
//! MRT has no general file I/O, on purpose -- letting an interpreted
//! program read and write arbitrary paths is a much bigger door to open
//! than a game asking to keep its own save file. `gameSaveData`/
//! `gameLoadData` open exactly that one door, in exactly the shape a save
//! file needs: a value in, a value back out, the file format itself not a
//! program's concern -- the save-file equivalent of `gameSave`/`gameLoad`
//! already doing that for a picture instead of a value.
//!
//! -- What "plain" means here --
//!
//! Null, booleans, numbers, strings, arrays and objects -- the same line
//! `examples/lib/json.mrt`'s own `write` draws for a function ("a function
//! has no JSON form"), extended to everything else this crate does not
//! consider data: a struct type, an instance, a builtin, a generator. All
//! of them are refused by name rather than silently dropped or turned into
//! something that cannot be read back as what it was.

use std::fs;
use std::rc::Rc;

use crate::error::{value_error, Signal};
use crate::value::{format_number, type_name, ObjKey, ObjMap, Value};

/// Write `value` to `path` as JSON text.
pub fn save(path: &str, value: &Value) -> Result<Value, Signal> {
    let mut out = String::new();
    encode(value, &mut out).map_err(|what| value_error(format!("gameSaveData(): {what}")))?;
    fs::write(path, out)
        .map_err(|e| value_error(format!("gameSaveData() could not write {path}: {e}")))?;
    Ok(Value::Null)
}

/// Read `path` back into the value it was saved from.
///
/// A missing file or malformed contents are refused rather than answered
/// with some default: a program checking its own save file for the first
/// time should decide what "no save yet" means for itself, in a `catch`,
/// rather than have this guess on its behalf.
pub fn load(path: &str) -> Result<Value, Signal> {
    let text = fs::read_to_string(path)
        .map_err(|e| value_error(format!("gameLoadData() could not read {path}: {e}")))?;
    let mut cursor = Cursor::new(&text);
    let value =
        decode(&mut cursor).map_err(|e| value_error(format!("gameLoadData(): {path}: {e}")))?;
    cursor.skip_space();
    if !cursor.done() {
        return Err(value_error(format!(
            "gameLoadData(): {path}: unexpected {} after the document.",
            cursor.here()
        )));
    }
    Ok(value)
}

// -- encoding -----------------------------------------------------------

fn encode(value: &Value, out: &mut String) -> Result<(), String> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if !n.is_finite() {
                return Err("a number that is NaN or infinite has no save-file form.".to_string());
            }
            out.push_str(&format_number(*n));
        }
        Value::Str(s) => encode_str(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.borrow().iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (key, val)) in map.borrow().iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode_str(&key.to_string(), out);
                out.push(':');
                encode(val, out)?;
            }
            out.push('}');
        }
        other => return Err(format!("a {} has no save-file form.", type_name(other))),
    }
    Ok(())
}

fn encode_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

// -- decoding -------------------------------------------------------------

/// A cursor over the source text that tracks where it is, the same shape
/// `examples/lib/json.mrt`'s own `Reader` takes, so that every failure can
/// name a line and a column.
struct Cursor {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
}

impl Cursor {
    fn new(text: &str) -> Cursor {
        Cursor {
            chars: text.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
        }
    }

    fn done(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.bump();
        }
    }

    /// What the cursor is looking at, phrased for an error message.
    fn here(&self) -> String {
        match self.peek() {
            None => "end of input".to_string(),
            Some('\n') => "a line break".to_string(),
            Some(c) => format!("'{c}'"),
        }
    }

    fn fail(&self, message: impl Into<String>) -> String {
        format!(
            "{} at line {}, column {}.",
            message.into(),
            self.line,
            self.column
        )
    }
}

fn decode(cur: &mut Cursor) -> Result<Value, String> {
    cur.skip_space();
    match cur.peek() {
        Some('{') => decode_object(cur),
        Some('[') => decode_array(cur),
        Some('"') => decode_string(cur).map(Value::str),
        Some('t') | Some('f') | Some('n') => decode_keyword(cur),
        Some(c) if c == '-' || c.is_ascii_digit() => decode_number(cur),
        _ => Err(cur.fail(format!("expected a value, found {}", cur.here()))),
    }
}

fn decode_object(cur: &mut Cursor) -> Result<Value, String> {
    cur.bump(); // the '{'
    let mut map = ObjMap::new();
    cur.skip_space();
    if cur.peek() == Some('}') {
        cur.bump();
        return Ok(Value::object(map));
    }
    loop {
        cur.skip_space();
        if cur.peek() != Some('"') {
            return Err(cur.fail(format!("expected a key, found {}", cur.here())));
        }
        let key = decode_string(cur)?;
        cur.skip_space();
        if cur.peek() != Some(':') {
            return Err(cur.fail(format!(
                "expected ':' after the key \"{key}\", found {}",
                cur.here()
            )));
        }
        cur.bump();
        let value = decode(cur)?;
        map.insert(ObjKey::Str(Rc::from(key.as_str())), value);

        cur.skip_space();
        match cur.peek() {
            Some('}') => {
                cur.bump();
                return Ok(Value::object(map));
            }
            Some(',') => {
                cur.bump();
            }
            _ => {
                return Err(cur.fail(format!(
                    "expected ',' or '}}' in the object, found {}",
                    cur.here()
                )))
            }
        }
    }
}

fn decode_array(cur: &mut Cursor) -> Result<Value, String> {
    cur.bump(); // the '['
    let mut items = Vec::new();
    cur.skip_space();
    if cur.peek() == Some(']') {
        cur.bump();
        return Ok(Value::array(items));
    }
    loop {
        items.push(decode(cur)?);
        cur.skip_space();
        match cur.peek() {
            Some(']') => {
                cur.bump();
                return Ok(Value::array(items));
            }
            Some(',') => {
                cur.bump();
            }
            _ => {
                return Err(cur.fail(format!(
                    "expected ',' or ']' in the array, found {}",
                    cur.here()
                )))
            }
        }
    }
}

fn decode_string(cur: &mut Cursor) -> Result<String, String> {
    cur.bump(); // the opening quote
    let mut out = String::new();
    loop {
        match cur.bump() {
            None => return Err(cur.fail("unterminated string")),
            Some('"') => return Ok(out),
            Some('\\') => out.push(decode_escape(cur)?),
            Some(c) if (c as u32) < 0x20 => {
                return Err(cur.fail("a control character must be escaped inside a string"))
            }
            Some(c) => out.push(c),
        }
    }
}

fn decode_escape(cur: &mut Cursor) -> Result<char, String> {
    match cur.bump() {
        None => Err(cur.fail("unterminated escape")),
        Some('"') => Ok('"'),
        Some('\\') => Ok('\\'),
        Some('/') => Ok('/'),
        Some('n') => Ok('\n'),
        Some('t') => Ok('\t'),
        Some('r') => Ok('\r'),
        Some('b') => Ok('\u{8}'),
        Some('f') => Ok('\u{c}'),
        Some('u') => decode_unicode_escape(cur),
        Some(c) => Err(cur.fail(format!("unknown escape \\{c}"))),
    }
}

/// One `\uXXXX` escape, as the single character it names.
///
/// Limited to the Basic Multilingual Plane, the same simplification
/// `examples/lib/json.mrt` makes for its own reasons: a surrogate pair
/// spanning two escapes (needed only for characters outside it, such as
/// most emoji) is refused by name rather than decoded partially or wrongly.
fn decode_unicode_escape(cur: &mut Cursor) -> Result<char, String> {
    let mut code = 0u32;
    for _ in 0..4 {
        let c = cur
            .bump()
            .ok_or_else(|| cur.fail("\\u needs four hex digits"))?;
        let digit = c
            .to_digit(16)
            .ok_or_else(|| cur.fail(format!("'{c}' is not a hex digit")))?;
        code = code * 16 + digit;
    }
    char::from_u32(code).ok_or_else(|| {
        cur.fail("this \\u escape is half of a surrogate pair, which save files do not support")
    })
}

fn decode_keyword(cur: &mut Cursor) -> Result<Value, String> {
    if read_word(cur, "true") {
        return Ok(Value::Bool(true));
    }
    if read_word(cur, "false") {
        return Ok(Value::Bool(false));
    }
    if read_word(cur, "null") {
        return Ok(Value::Null);
    }
    Err(cur.fail(format!(
        "expected true, false or null, found {}",
        cur.here()
    )))
}

/// Consumes `word` if the cursor is sitting on it, and reports whether it did.
fn read_word(cur: &mut Cursor, word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    let end = cur.pos + chars.len();
    if end > cur.chars.len() || cur.chars[cur.pos..end] != chars[..] {
        return false;
    }
    for _ in 0..chars.len() {
        cur.bump();
    }
    true
}

/// JSON numbers are a strict subset of what Rust's own float parser
/// accepts, so the shape is checked here and the text itself is handed
/// over once it is known good.
fn decode_number(cur: &mut Cursor) -> Result<Value, String> {
    let mut text = String::new();
    if cur.peek() == Some('-') {
        text.push(cur.bump().expect("just peeked"));
    }
    read_int(cur, &mut text)?;

    if cur.peek() == Some('.') {
        text.push(cur.bump().expect("just peeked"));
        read_digits(cur, &mut text, "a digit after the decimal point")?;
    }
    if matches!(cur.peek(), Some('e' | 'E')) {
        text.push(cur.bump().expect("just peeked"));
        if matches!(cur.peek(), Some('+' | '-')) {
            text.push(cur.bump().expect("just peeked"));
        }
        read_digits(cur, &mut text, "a digit in the exponent")?;
    }

    text.parse::<f64>()
        .map(Value::Number)
        .map_err(|_| cur.fail("not a valid number"))
}

/// The integer part: either a single 0, or digits that do not start with
/// one. JSON rejects "01", and so does this.
fn read_int(cur: &mut Cursor, text: &mut String) -> Result<(), String> {
    if !matches!(cur.peek(), Some(c) if c.is_ascii_digit()) {
        return Err(cur.fail(format!("expected a digit, found {}", cur.here())));
    }
    let first = cur.bump().expect("just checked");
    text.push(first);
    if first == '0' {
        if matches!(cur.peek(), Some(c) if c.is_ascii_digit()) {
            return Err(cur.fail("a number may not have a leading zero"));
        }
        return Ok(());
    }
    while matches!(cur.peek(), Some(c) if c.is_ascii_digit()) {
        text.push(cur.bump().expect("just checked"));
    }
    Ok(())
}

fn read_digits(cur: &mut Cursor, text: &mut String, what: &str) -> Result<(), String> {
    if !matches!(cur.peek(), Some(c) if c.is_ascii_digit()) {
        return Err(cur.fail(format!("expected {what}, found {}", cur.here())));
    }
    while matches!(cur.peek(), Some(c) if c.is_ascii_digit()) {
        text.push(cur.bump().expect("just checked"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::stringify;

    fn round_trip(value: Value) -> Value {
        let mut text = String::new();
        encode(&value, &mut text).expect("encodes");
        let mut cursor = Cursor::new(&text);
        decode(&mut cursor).expect("decodes")
    }

    #[test]
    fn null_booleans_and_numbers_round_trip() {
        assert_eq!(stringify(&round_trip(Value::Null)), "null");
        assert_eq!(stringify(&round_trip(Value::Bool(true))), "true");
        assert_eq!(stringify(&round_trip(Value::Number(-12.5))), "-12.5");
        assert_eq!(stringify(&round_trip(Value::Number(0.0))), "0");
    }

    #[test]
    fn strings_round_trip_including_escapes() {
        let value = Value::str("a \"quoted\" line\nwith a tab\there");
        let Value::Str(s) = round_trip(value) else {
            panic!("expected a string back");
        };
        assert_eq!(&*s, "a \"quoted\" line\nwith a tab\there");
    }

    #[test]
    fn arrays_and_objects_round_trip() {
        let mut map = ObjMap::new();
        map.insert(ObjKey::Str(Rc::from("hp")), Value::Number(7.0));
        map.insert(
            ObjKey::Str(Rc::from("items")),
            Value::array(vec![Value::str("sword"), Value::str("shield")]),
        );
        let value = Value::object(map);
        assert_eq!(
            stringify(&round_trip(value)),
            r#"{hp: 7, items: [sword, shield]}"#
        );
    }

    #[test]
    fn a_function_has_no_save_file_form() {
        let map = ObjMap::new();
        let value = Value::object(map);
        let mut text = String::new();
        // Sanity: an empty object still encodes fine on its own.
        assert!(encode(&value, &mut text).is_ok());

        let function = crate::value::Builtin::named("test");
        let mut out = String::new();
        let err = encode(&function, &mut out).expect_err("a builtin is not data");
        assert_eq!(err, "a function has no save-file form.");
    }

    #[test]
    fn malformed_json_names_the_line_and_column() {
        let mut cursor = Cursor::new("{\n  \"a\": ,\n}");
        let err = decode(&mut cursor).expect_err("trailing comma-less value");
        assert_eq!(err, "expected a value, found ',' at line 2, column 8.");
    }

    #[test]
    fn a_non_positive_zero_still_rejects_leading_zeroes() {
        let mut cursor = Cursor::new("01");
        let err = decode(&mut cursor).expect_err("leading zero");
        assert_eq!(
            err,
            "a number may not have a leading zero at line 1, column 2."
        );
    }

    // -- from MRT source, both engines --------------------------------

    fn go(source: &str) -> String {
        let file = crate::SourceFile::new("test.mrt", source);
        let text = |outcome: crate::Outcome| {
            let mut lines = outcome.output;
            if let Some(error) = outcome.error {
                lines.push(error);
            }
            lines.join("\n")
        };
        let walked = text(crate::run(&file));
        let compiled = text(crate::run_vm(&file));
        assert_eq!(walked, compiled, "the engines disagree");
        walked
    }

    fn main_of(body: &str) -> String {
        go(&format!("func main() {{ {body} }}"))
    }

    #[test]
    fn save_and_load_need_no_screen_at_all() {
        // Persisting game state is arithmetic on a value and a file. A
        // program checking its own save file should not have to open a
        // framebuffer first.
        let path = std::env::temp_dir().join("mrt_save_data_test.json");
        let _ = std::fs::remove_file(&path);
        let escaped = path.to_string_lossy().replace('\\', "\\\\");

        assert_eq!(
            main_of(&format!(
                r#"var state = {{level: 3, name: "Nyx", items: ["map", "torch"]}};
                   gameSaveData("{escaped}", state);
                   var loaded = gameLoadData("{escaped}");
                   print(loaded.level, loaded.name, loaded.items);"#
            )),
            "3 Nyx [map, torch]"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_missing_save_file_is_a_catchable_error() {
        let path = std::env::temp_dir().join("mrt_save_data_never_written.json");
        let _ = std::fs::remove_file(&path);
        let escaped = path.to_string_lossy().replace('\\', "\\\\");

        assert_eq!(
            main_of(&format!(
                r#"try {{
                       gameLoadData("{escaped}");
                   }} catch (e) {{
                       print("no save yet:", e.kind);
                   }}"#
            )),
            "no save yet: ValueError"
        );
    }

    #[test]
    fn a_function_cannot_be_saved() {
        assert_eq!(
            go(r#"func f() {} func main() { gameSaveData("/tmp/mrt_save_data_unused.json", f); }"#),
            "Runtime Error: gameSaveData(): a function has no save-file form. [line 1]"
        );
    }

    #[test]
    fn malformed_save_data_is_a_catchable_error_naming_where() {
        let path = std::env::temp_dir().join("mrt_save_data_corrupt.json");
        std::fs::write(&path, "{not valid json").expect("wrote a broken file");
        let escaped = path.to_string_lossy().replace('\\', "\\\\");

        assert_eq!(
            main_of(&format!(
                r#"try {{
                       gameLoadData("{escaped}");
                   }} catch (e) {{
                       print("caught:", e.kind);
                   }}"#
            )),
            "caught: ValueError"
        );
        let _ = std::fs::remove_file(&path);
    }
}
