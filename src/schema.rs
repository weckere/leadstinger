//! JSON Schemas read from the registry's parameter summaries (`Spec.params`), so generated
//! clients and tool lists can't drift from the commands.
//!
//! A summary is figcraft-style: `{tile, fade? (seconds; …), event?: press|release (…),
//! tile | tiles (…)}  a note → {what it returns}`. Read from it: each top-level
//! parameter, `?` for optional, `a|b|c` after a colon as an enum, `bool` (or `true`) as a
//! boolean, a spaced ` | ` between parameters as alternatives (none of them required), and
//! the parenthesised text as the description. Types the summary doesn't state are left out.

use serde_json::{json, Map, Value};

use crate::commands::Spec;

/// A summary read: the input schema, the note after it and what it returns.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub input: Value,
    pub note: Option<String>,
    pub returns: Option<String>,
}

/// The schema of a command's parameters.
pub fn input_schema(spec: &Spec) -> Value {
    read(spec.params).map(|s| s.input).unwrap_or_else(|_| json!({"type": "object"}))
}

/// Read a summary; fails on one this reader can't take apart, which the registry test
/// turns into a failure for that command.
pub fn read(text: &str) -> Result<Summary, String> {
    let text = text.trim();
    let (inner, rest) = if text.starts_with('{') {
        let close = matching(text, 0).ok_or_else(|| "the { has no matching }".to_owned())?;
        (Some(&text[1..close]), &text[close + 1..])
    } else {
        (None, text)
    };
    let (note, returns) = match rest.split_once('→') {
        Some((n, r)) => (n, Some(r.trim().to_owned())),
        None => (rest, None),
    };
    let note = Some(note.trim().to_owned()).filter(|n| !n.is_empty());
    let mut properties = Map::new();
    let mut required = Vec::new();
    for item in inner.map(split_top).unwrap_or_default() {
        let group = alternatives(item)?;
        let alone = group.len() == 1;
        for p in group {
            if alone && !p.optional {
                required.push(Value::String(p.name.clone()));
            }
            properties.entry(p.name.clone()).or_insert_with(|| p.schema());
        }
    }
    let mut input = json!({"type": "object", "properties": properties});
    if !required.is_empty() {
        input["required"] = Value::Array(required);
    }
    Ok(Summary { input, note, returns })
}

struct Param {
    name: String,
    optional: bool,
    value: Option<String>,
    desc: Option<String>,
}

impl Param {
    fn schema(&self) -> Value {
        let mut s = Map::new();
        let mut desc = self.desc.clone();
        match self.value.as_deref() {
            Some("bool" | "true" | "false") => {
                s.insert("type".into(), json!("boolean"));
            }
            Some(v) if v.contains('|') && v.split('|').all(is_word) => {
                s.insert("type".into(), json!("string"));
                s.insert("enum".into(), json!(v.split('|').collect::<Vec<_>>()));
            }
            Some(v) => desc = Some(match desc {
                Some(d) => format!("{v}; {d}"),
                None => v.to_owned(),
            }),
            None => {}
        }
        if let Some(d) = &desc {
            if d.starts_with('[') {
                s.insert("type".into(), json!("array"));
            } else if ["default true", "default false", "default: toggle", "true:", "false:"].iter().any(|k| d.starts_with(k)) {
                s.insert("type".into(), json!("boolean"));
            }
            s.insert("description".into(), json!(d));
        }
        Value::Object(s)
    }
}

fn is_word(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

/// Track brackets: inside parentheses (prose) only parentheses count, so a key name
/// such as `]` in a description doesn't close anything.
fn bracket(stack: &mut Vec<char>, c: char) {
    let top = stack.last().copied();
    if top == Some('(') {
        match c {
            '(' => stack.push(c),
            ')' => drop(stack.pop()),
            _ => {}
        }
        return;
    }
    match c {
        '{' | '(' | '[' => stack.push(c),
        '}' if top == Some('{') => drop(stack.pop()),
        ']' if top == Some('[') => drop(stack.pop()),
        _ => {}
    }
}

/// The index of the bracket matching the one at `open`.
fn matching(text: &str, open: usize) -> Option<usize> {
    let mut stack = Vec::new();
    for (i, c) in text.char_indices().skip_while(|(i, _)| *i < open) {
        bracket(&mut stack, c);
        if stack.is_empty() {
            return Some(i);
        }
    }
    None
}

/// Split on commas outside any brackets.
fn split_top(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut stack, mut start) = (Vec::new(), 0);
    for (i, c) in text.char_indices() {
        if c == ',' && stack.is_empty() {
            out.push(&text[start..i]);
            start = i + 1;
        } else {
            bracket(&mut stack, c);
        }
    }
    out.push(&text[start..]);
    out.into_iter().map(str::trim).filter(|s| !s.is_empty()).collect()
}

/// One item: a parameter, or several separated by a spaced ` | `.
fn alternatives(item: &str) -> Result<Vec<Param>, String> {
    let mut out = Vec::new();
    let mut rest = item;
    loop {
        let (p, after) = param(rest).map_err(|e| format!("{e} in {item:?}"))?;
        out.push(p);
        let after = after.trim_start();
        if let Some(next) = after.strip_prefix('|') {
            rest = next.trim_start();
        } else if after.is_empty() {
            return Ok(out);
        } else {
            return Err(format!("can't read {after:?} in {item:?}"));
        }
    }
}

/// `name`, `?`, `: value`, `(description)`; returns what's left.
fn param(text: &str) -> Result<(Param, &str), String> {
    let end = text.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(text.len());
    if end == 0 {
        return Err("no parameter name".into());
    }
    let name = text[..end].to_owned();
    let mut rest = &text[end..];
    let optional = rest.starts_with('?');
    if optional {
        rest = &rest[1..];
    }
    let mut value = None;
    if let Some(v) = rest.trim_start().strip_prefix(':') {
        // An enum is written unspaced (a|b); a spaced | starts an alternative.
        let v = v.trim_start();
        let bytes = v.as_bytes();
        let stop = (0..v.len())
            .find(|&i| bytes[i] == b'(' || (bytes[i] == b'|' && i > 0 && bytes[i - 1] == b' '))
            .unwrap_or(v.len());
        value = Some(v[..stop].trim().to_owned()).filter(|s| !s.is_empty());
        rest = &v[stop..];
    }
    let mut desc = None;
    let trimmed = rest.trim_start();
    if trimmed.starts_with('(') {
        let close = matching(trimmed, 0).ok_or("a ( has no matching )")?;
        desc = Some(trimmed[1..close].trim().to_owned());
        rest = &trimmed[close + 1..];
    }
    Ok((Param { name, optional, value, desc }, rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::COMMANDS;

    #[test]
    fn every_command_summary_reads() {
        let mut wrong = Vec::new();
        for s in COMMANDS {
            match read(s.params) {
                Err(e) => wrong.push(format!("{}: {e}", s.id)),
                Ok(r) => {
                    if let Some(main) = s.main.filter(|m| r.input["properties"].get(*m).is_none()) {
                        wrong.push(format!("{}: its main parameter {main} isn't in {}", s.id, s.params));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "summaries to rewrite:\n{}", wrong.join("\n"));
    }

    #[test]
    fn parameters_enums_and_alternatives() {
        let r = read("{tile, event?: press|release (default press), toggle? (true: stops it)}  as its hotkey would → {tile, state}").unwrap();
        assert_eq!(r.input["required"], json!(["tile"]));
        assert_eq!(r.input["properties"]["event"]["enum"], json!(["press", "release"]));
        assert_eq!(r.input["properties"]["event"]["description"], "default press");
        assert_eq!(r.input["properties"]["toggle"]["type"], "boolean");
        assert_eq!(r.note.as_deref(), Some("as its hotkey would"));
        assert_eq!(r.returns.as_deref(), Some("{tile, state}"));

        let r = read("{set? (default current) | all: true}  the main window stays").unwrap();
        assert!(r.input.get("required").is_none());
        assert_eq!(r.input["properties"]["all"]["type"], "boolean");

        let r = read("{tile, line, word, at (its new start) | by (seconds), split?}").unwrap();
        assert_eq!(r.input["required"], json!(["tile", "line", "word"]));
        assert!(r.input["properties"]["by"].is_object());

        let r = read("{cues ([{start, end, text}] as tile.subtitles gives them), x11: bool}").unwrap();
        assert_eq!(r.input["properties"]["cues"]["type"], "array");
        assert_eq!(r.input["properties"]["x11"]["type"], "boolean");

        let r = read("→ {devices: [name], current}").unwrap();
        assert_eq!(r.input, json!({"type": "object", "properties": {}}));
        assert!(read("{tile, (oops)}").is_err());
    }
}
