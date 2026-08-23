// Copyright (c) The cargo-guppy Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A serializer that reproduces the output of `toml` 0.5's `Serializer::pretty`
//! with `pretty_array(false)`.
//!
//! This is an internal module that is public only because it is shared by
//! several guppy-related projects.
//!
//! The TOML output is part of the stable on-disk format for guppy-related
//! projects. If you're not bound by backwards compatibility concerns, you
//! probably want to use `toml` 1's serializer -- its output format is better.

// Specific details that are preserved here (from toml 0.5.11's src/ser.rs):
//
// * toml 0.5's preserve_order feature (tables are emitted in iteration order)
// * literal single-quoted strings
// * headers for empty tables
// * blank line placement around `[[...]]` entries
//
// The structure deliberately mirrors the original state machine in toml 0.5 so
// that the two can be compared side by side.

use std::{cell::Cell, fmt::Write};
use toml::{Table, Value, ser::Error};

/// Writes `table` to `out` in the toml 0.5 pretty format.
///
/// Returns an error if a non-table value follows a table or array of tables
/// within the same parent. This matches toml 0.5's `ValueAfterTable` error.
pub fn write_table(table: &Table, out: &mut String) -> Result<(), Error> {
    emit_table(out, table, &State::End)
}

/// Reorders values within `value` to conform to what toml 0.5 does.
///
/// This mirrors toml 0.5.11's `value.rs:412-439` (`impl Serialize for
/// Value::Table`), and reorders items as:
///
/// * plain values
/// * then arrays of tables
/// * then tables, recursively
///
/// toml 0.5 applies this reordering to `Value` trees only, not to structs or a
/// directly serialized `Table` field, so callers are expected to only apply it
/// in those cases.
pub fn reorder_value(value: &Value) -> Value {
    match value {
        Value::String(_)
        | Value::Integer(_)
        | Value::Float(_)
        | Value::Boolean(_)
        | Value::Datetime(_) => value.clone(),
        Value::Array(array) => Value::Array(array.iter().map(reorder_value).collect()),
        Value::Table(table) => {
            let mut reordered = Table::new();
            for (key, value) in table {
                if !value.is_table() && !is_array_of_tables(value) {
                    reordered.insert(key.clone(), reorder_value(value));
                }
            }
            for (key, value) in table {
                if is_array_of_tables(value) {
                    reordered.insert(key.clone(), reorder_value(value));
                }
            }
            for (key, value) in table {
                if value.is_table() {
                    reordered.insert(key.clone(), reorder_value(value));
                }
            }
            Value::Table(reordered)
        }
    }
}

// toml 0.5 treats an array as an array of tables if any element is a table.
fn is_array_of_tables(value: &Value) -> bool {
    match value {
        Value::Array(array) => array.iter().any(Value::is_table),
        Value::String(_)
        | Value::Integer(_)
        | Value::Float(_)
        | Value::Boolean(_)
        | Value::Datetime(_)
        | Value::Table(_) => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ArrayState {
    Started,
    StartedAsATable,
}

#[derive(Clone, Copy, Debug)]
enum State<'a> {
    End,
    Table {
        key: &'a str,
        parent: &'a State<'a>,
        first: &'a Cell<bool>,
        table_emitted: &'a Cell<bool>,
    },
    Array {
        parent: &'a State<'a>,
        first: &'a Cell<bool>,
        type_: &'a Cell<Option<ArrayState>>,
    },
}

fn value_after_table() -> Error {
    serde::ser::Error::custom("values must be emitted before tables")
}

fn emit_value(out: &mut String, value: &Value, state: &State<'_>) -> Result<(), Error> {
    match value {
        Value::String(s) => {
            emit_key(out, state, ArrayState::Started)?;
            emit_str(out, s, false);
            newline_if_table(out, state);
            Ok(())
        }
        Value::Integer(i) => display(out, i, state),
        Value::Float(f) => {
            emit_key(out, state, ArrayState::Started)?;
            emit_float(out, *f);
            newline_if_table(out, state);
            Ok(())
        }
        Value::Boolean(b) => display(out, b, state),
        // toml_datetime 1.x's Display matches toml 0.5 almost all the time,
        // with two exceptions:
        //
        // 1. An explicit zero fractional second is kept (`07:32:00.0`, where
        //    0.5 dropped it).
        // 2. Negative offsets under an hour keep their sign (`-00:30`, which
        //    0.5 mis-wrote as `+00:30`).
        //
        // We accept these divergences here in the interest of not
        // overengineering a solution, especially since this metadata is
        // unlikely to have toml datetime values in the first place.
        Value::Datetime(dt) => display(out, dt, state),
        Value::Array(array) => emit_array(out, array, state),
        Value::Table(table) => emit_table(out, table, state),
    }
}

fn display(
    out: &mut String,
    value: impl std::fmt::Display,
    state: &State<'_>,
) -> Result<(), Error> {
    emit_key(out, state, ArrayState::Started)?;
    write!(out, "{value}").expect("writing to a String cannot fail");
    newline_if_table(out, state);
    Ok(())
}

fn emit_float(out: &mut String, v: f64) {
    match (v.is_sign_negative(), v.is_nan(), v == 0.0) {
        (true, true, _) => out.push_str("-nan"),
        (false, true, _) => out.push_str("nan"),
        (true, false, true) => out.push_str("-0.0"),
        (false, false, true) => out.push_str("0.0"),
        (_, false, false) => {
            write!(out, "{v}").expect("writing to a String cannot fail");
            if v % 1.0 == 0.0 {
                out.push_str(".0");
            }
        }
    }
}

fn newline_if_table(out: &mut String, state: &State<'_>) {
    match state {
        State::Table { .. } => out.push('\n'),
        State::End | State::Array { .. } => {}
    }
}

fn emit_array(out: &mut String, array: &[Value], state: &State<'_>) -> Result<(), Error> {
    array_type(state, ArrayState::Started);
    let first = Cell::new(true);
    let type_ = Cell::new(None);
    for element in array {
        emit_value(
            out,
            element,
            &State::Array {
                parent: state,
                first: &first,
                type_: &type_,
            },
        )?;
        first.set(false);
    }

    match type_.get() {
        Some(ArrayState::StartedAsATable) => return Ok(()),
        Some(ArrayState::Started) => out.push(']'),
        None => {
            emit_key(out, state, ArrayState::Started)?;
            out.push_str("[]");
        }
    }
    newline_if_table(out, state);
    Ok(())
}

fn emit_table(out: &mut String, table: &Table, state: &State<'_>) -> Result<(), Error> {
    array_type(state, ArrayState::StartedAsATable);
    let first = Cell::new(true);
    let table_emitted = Cell::new(false);
    for (key, value) in table {
        emit_value(
            out,
            value,
            &State::Table {
                key,
                parent: state,
                first: &first,
                table_emitted: &table_emitted,
            },
        )?;
        first.set(false);
    }

    if first.get() {
        emit_table_header(out, state);
    }
    Ok(())
}

fn emit_key(out: &mut String, state: &State<'_>, type_: ArrayState) -> Result<(), Error> {
    array_type(state, type_);
    emit_key_inner(out, state)
}

fn emit_key_inner(out: &mut String, state: &State<'_>) -> Result<(), Error> {
    match *state {
        State::End => Ok(()),
        State::Array {
            parent,
            first,
            type_,
        } => {
            assert!(
                type_.get().is_some(),
                "array_type is always called before emit_key_inner"
            );
            if first.get() {
                emit_key_inner(out, parent)?;
            }
            if first.get() {
                out.push('[');
            } else {
                out.push_str(", ");
            }
            Ok(())
        }
        State::Table {
            key,
            parent,
            first,
            table_emitted,
        } => {
            if table_emitted.get() {
                return Err(value_after_table());
            }
            if first.get() {
                emit_table_header(out, parent);
                first.set(false);
            }
            escape_key(out, key);
            out.push_str(" = ");
            Ok(())
        }
    }
}

fn array_type(state: &State<'_>, type_: ArrayState) {
    if let State::Array { type_: prev, .. } = state
        && prev.get().is_none()
    {
        prev.set(Some(type_));
    }
}

fn escape_key(out: &mut String, key: &str) {
    let bare = !key.is_empty()
        && key
            .chars()
            .all(|c| matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_'));
    if bare {
        out.push_str(key);
    } else {
        emit_str(out, key, true);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StrType {
    NewlineTriple,
    OnelineTriple,
    OnelineSingle,
}

enum StrRepr {
    // A literal string using single quotes.
    Literal(String, StrType),
    // A basic string using double quotes and escapes.
    Std(StrType),
}

fn str_repr(value: &str) -> StrRepr {
    let mut out = String::with_capacity(value.len() * 2);
    let mut ty = StrType::OnelineSingle;
    let mut max_found_singles = 0;
    let mut found_singles = 0;
    let mut can_be_pretty = true;

    for ch in value.chars() {
        if can_be_pretty {
            if ch == '\'' {
                found_singles += 1;
                if found_singles >= 3 {
                    can_be_pretty = false;
                }
            } else {
                if found_singles > max_found_singles {
                    max_found_singles = found_singles;
                }
                found_singles = 0
            }
            match ch {
                '\t' => {}
                '\n' => ty = StrType::NewlineTriple,
                c if c <= '\u{1f}' || c == '\u{7f}' => can_be_pretty = false,
                _ => {}
            }
            out.push(ch);
        } else if ch == '\n' {
            ty = StrType::NewlineTriple;
        }
    }
    if can_be_pretty && found_singles > 0 && value.ends_with('\'') {
        can_be_pretty = false;
    }
    if !can_be_pretty {
        debug_assert!(ty != StrType::OnelineTriple);
        return StrRepr::Std(ty);
    }
    if found_singles > max_found_singles {
        max_found_singles = found_singles;
    }
    debug_assert!(max_found_singles < 3);
    if ty == StrType::OnelineSingle && max_found_singles >= 1 {
        ty = StrType::OnelineTriple;
    }
    StrRepr::Literal(out, ty)
}

fn emit_str(out: &mut String, value: &str, is_key: bool) {
    let repr = if is_key {
        StrRepr::Std(StrType::OnelineSingle)
    } else {
        str_repr(value)
    };
    match repr {
        StrRepr::Literal(literal, ty) => {
            match ty {
                StrType::NewlineTriple => out.push_str("'''\n"),
                StrType::OnelineTriple => out.push_str("'''"),
                StrType::OnelineSingle => out.push('\''),
            }
            out.push_str(&literal);
            match ty {
                StrType::OnelineSingle => out.push('\''),
                StrType::NewlineTriple | StrType::OnelineTriple => out.push_str("'''"),
            }
        }
        StrRepr::Std(ty) => {
            match ty {
                StrType::NewlineTriple => out.push_str("\"\"\"\n"),
                StrType::OnelineSingle | StrType::OnelineTriple => out.push('"'),
            }
            for ch in value.chars() {
                match ch {
                    '\u{8}' => out.push_str("\\b"),
                    '\u{9}' => out.push_str("\\t"),
                    '\u{a}' => match ty {
                        StrType::NewlineTriple => out.push('\n'),
                        StrType::OnelineSingle => out.push_str("\\n"),
                        StrType::OnelineTriple => {
                            unreachable!("newlines always produce NewlineTriple")
                        }
                    },
                    '\u{c}' => out.push_str("\\f"),
                    '\u{d}' => out.push_str("\\r"),
                    '\u{22}' => out.push_str("\\\""),
                    '\u{5c}' => out.push_str("\\\\"),
                    c if c <= '\u{1f}' || c == '\u{7f}' => {
                        write!(out, "\\u{:04X}", ch as u32)
                            .expect("writing to a String cannot fail");
                    }
                    ch => out.push(ch),
                }
            }
            match ty {
                StrType::NewlineTriple => out.push_str("\"\"\""),
                StrType::OnelineSingle | StrType::OnelineTriple => out.push('"'),
            }
        }
    }
}

fn emit_table_header(out: &mut String, state: &State<'_>) {
    let array_of_tables = match state {
        State::End => return,
        State::Array { .. } => true,
        State::Table { .. } => false,
    };

    // `[a.b]` headers can omit their `[a]` ancestor, but `[[a]]` ancestors
    // cannot be omitted, so emit those first.
    let mut p = state;
    if let State::Array { first, parent, .. } = *state
        && first.get()
    {
        p = parent;
    }
    while let State::Table { first, parent, .. } = *p {
        p = parent;
        if !first.get() {
            break;
        }
        if let State::Array {
            parent: State::Table { .. },
            ..
        } = *parent
        {
            emit_table_header(out, parent);
            break;
        }
    }

    match *state {
        State::Table { first, .. } => {
            if !first.get() {
                out.push('\n');
            }
        }
        State::Array { parent, first, .. } => {
            if !first.get() {
                out.push('\n');
            } else if let State::Table { first, .. } = *parent
                && !first.get()
            {
                out.push('\n');
            }
        }
        State::End => {}
    }
    out.push('[');
    if array_of_tables {
        out.push('[');
    }
    _ = emit_key_part(out, state);
    if array_of_tables {
        out.push(']');
    }
    out.push_str("]\n");
}

// Returns true if nothing was written, so that callers know whether to insert a
// `.` separator.
#[must_use]
fn emit_key_part(out: &mut String, state: &State<'_>) -> bool {
    match *state {
        State::Array { parent, .. } => emit_key_part(out, parent),
        State::End => true,
        State::Table {
            key,
            parent,
            table_emitted,
            ..
        } => {
            table_emitted.set(true);
            let first = emit_key_part(out, parent);
            if !first {
                out.push('.');
            }
            escape_key(out, key);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(input: &str) -> Result<String, Error> {
        let table: Table = toml::from_str(input).expect("valid TOML input");
        let mut out = String::new();
        write_table(&table, &mut out)?;
        Ok(out)
    }

    // Expected outputs in this module were captured from toml 0.5.11's
    // `Serializer::pretty` with `pretty_array(false)`.
    #[test]
    fn summary_shape() {
        let input = r#"
hakari-package = "workspace-hack"
resolver = "2"
output-single-feature = true
platforms = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"]
empty = []
[traversal-excludes]
workspace-members = ["a"]
[[traversal-excludes.ids]]
name = "cargo-compare"
version = "0.1.0"
workspace-path = "internal-tools/cargo-compare"
[[traversal-excludes.ids]]
name = "serde"
version = "1.0.0"
crates-io = true
[[traversal-excludes.third-party]]
name = "quote"
version = "1"
crates-io = true
[final-excludes]
[registries.alt]
index = "https://example.com/alt"
[registries."my.registry"]
index = "https://example.com/index"
"#;
        let expected = "\
hakari-package = 'workspace-hack'
resolver = '2'
output-single-feature = true
platforms = ['x86_64-unknown-linux-gnu', 'aarch64-apple-darwin']
empty = []

[traversal-excludes]
workspace-members = ['a']

[[traversal-excludes.ids]]
name = 'cargo-compare'
version = '0.1.0'
workspace-path = 'internal-tools/cargo-compare'

[[traversal-excludes.ids]]
name = 'serde'
version = '1.0.0'
crates-io = true

[[traversal-excludes.third-party]]
name = 'quote'
version = '1'
crates-io = true

[final-excludes]
[registries.alt]
index = 'https://example.com/alt'

[registries.\"my.registry\"]
index = 'https://example.com/index'
";
        assert_eq!(write(input).unwrap(), expected);
    }

    #[test]
    fn strings() {
        let input = r#"
plain = "abc"
apostrophe = "it's"
ends-with-apostrophe = "ends'"
triple = "'''"
double-quotes = "say \"hi\""
escapes = "tab\there\u0001\u007f"
multiline = "a\nb"
multiline-apostrophe = "a'\nb"
multiline-control = "a\r\nb"
backslash = "C:\\dir"
"#;
        let expected = "\
plain = 'abc'
apostrophe = '''it's'''
ends-with-apostrophe = \"ends'\"
triple = \"'''\"
double-quotes = 'say \"hi\"'
escapes = \"tab\\there\\u0001\\u007F\"
multiline = '''
a
b'''
multiline-apostrophe = '''
a'
b'''
multiline-control = \"\"\"
a\\r
b\"\"\"
backslash = 'C:\\dir'
";
        assert_eq!(write(input).unwrap(), expected);
    }

    #[test]
    fn scalars_and_nesting() {
        let input = r#"
int = -42
float = 1.0
float2 = 2.5
yes = false
nested = [[1, 2], []]
[a.b.c]
x = 1
[[aot]]
[[aot]]
y = 2
[aot.sub]
z = 3
"#;
        let expected = "\
int = -42
float = 1.0
float2 = 2.5
yes = false
nested = [[1, 2], []]
[a.b.c]
x = 1

[[aot]]

[[aot]]
y = 2

[aot.sub]
z = 3
";
        assert_eq!(write(input).unwrap(), expected);
    }

    #[test]
    fn reorder_value_matches_toml_05() {
        let input = r#"
plain = [1, 2]
v = true
[t]
x = 1
[[aot]]
[[aot]]
sub = { inner = { y = 1 }, z = 2 }
w = 3
"#;
        let table: Table = toml::from_str(input).expect("valid TOML input");
        let reordered = match reorder_value(&Value::Table(table)) {
            Value::Table(table) => table,
            other => panic!("reordered a table into {other:?}"),
        };
        let keys: Vec<_> = reordered.keys().map(String::as_str).collect();
        assert_eq!(keys, ["plain", "v", "aot", "t"]);

        let second = reordered["aot"][1]
            .as_table()
            .expect("array of tables element is a table");
        let keys: Vec<_> = second.keys().map(String::as_str).collect();
        assert_eq!(keys, ["w", "sub"], "array elements are reordered");
        let sub = second["sub"].as_table().expect("sub is a table");
        let keys: Vec<_> = sub.keys().map(String::as_str).collect();
        assert_eq!(keys, ["z", "inner"], "nested tables are reordered");

        let mut out = String::new();
        write_table(&reordered, &mut out).expect("reordered table serializes");
        assert_eq!(
            out,
            "\
plain = [1, 2]
v = true

[[aot]]

[[aot]]
w = 3

[aot.sub]
z = 2

[aot.sub.inner]
y = 1

[t]
x = 1
"
        );
    }

    #[test]
    fn value_after_table_is_an_error() {
        let mut table = Table::new();
        table.insert("t".to_owned(), Value::Table(Table::new()));
        table.insert("v".to_owned(), Value::Integer(1));
        let mut out = String::new();
        let err = write_table(&table, &mut out).unwrap_err();
        assert!(
            err.to_string()
                .contains("values must be emitted before tables"),
            "unexpected error: {err}"
        );
    }
}
