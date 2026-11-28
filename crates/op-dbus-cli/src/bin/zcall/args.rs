//! `--flag value` tokens to the method's JSON arguments, typed by the sealed
//! args schema. The mount validates the result again before any effect.

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value};

use crate::catalog::{Flag, Method, item_schema, is_switch, value_type};

#[derive(Default)]
pub struct CallOptions {
    pub print: bool,
    pub confirm: bool,
}

/// zcall's own call options. A method property with the same spelling wins,
/// so no sealed argument is ever shadowed.
pub const CALL_OPTIONS: [(&str, &str); 4] = [
    ("--json", "whole argument object as JSON"),
    ("--yes", "confirm a mutating method"),
    ("--print", "show the operation path and arguments, do not call"),
    ("--dry-run", "same as --print"),
];

pub fn build(method: &Method, tokens: &[String]) -> Result<(Value, CallOptions)> {
    let mut options = CallOptions::default();
    let mut object = Map::new();
    let mut raw_json: Option<Value> = None;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        index += 1;
        let (name, inline) = match token.split_once('=') {
            Some((name, value)) if name.starts_with('-') => (name, Some(value.to_owned())),
            _ => (token.as_str(), None),
        };
        let flag = name.starts_with('-').then(|| method.find_flag(name)).flatten();
        if flag.is_none() {
            match name {
                "--json" | "-a" | "--arguments" => {
                    let raw = take_value(name, inline, tokens, &mut index)?;
                    raw_json = Some(serde_json::from_str(&raw).context("--json is not valid JSON")?);
                    continue;
                }
                "--print" | "--dry-run" => {
                    options.print = true;
                    continue;
                }
                "--yes" | "--confirm-side-effects" => {
                    options.confirm = true;
                    continue;
                }
                _ => {}
            }
        }
        let Some(flag) = flag else {
            if !token.starts_with('-') {
                positional(method, &mut object, token)?;
                continue;
            }
            let known: Vec<String> = method.flags().into_iter().map(|f| f.flag).collect();
            bail!(
                "{} has no flag {name}; its flags: {}",
                method.name,
                if known.is_empty() { "(none)".into() } else { known.join(" ") }
            );
        };
        let value = if is_switch(&flag.schema) {
            match inline {
                Some(raw) => parse_bool(&raw).with_context(|| format!("{name} expects true/false"))?,
                None => match tokens.get(index).and_then(|next| parse_bool(next).ok()) {
                    Some(value) => {
                        index += 1;
                        value
                    }
                    None => Value::Bool(true),
                },
            }
        } else {
            let raw = take_value(name, inline, tokens, &mut index)?;
            convert(&raw, &flag)?
        };
        insert(&mut object, &flag, value);
    }
    let arguments = match raw_json {
        Some(json) => {
            ensure!(object.is_empty(), "use either --json or flags, not both");
            json
        }
        None => Value::Object(object),
    };
    let missing: Vec<String> = method
        .flags()
        .into_iter()
        .filter(|flag| flag.required && arguments.get(&flag.key).is_none())
        .map(|flag| flag.flag)
        .collect();
    ensure!(missing.is_empty(), "{} requires {}", method.name, missing.join(" "));
    Ok((arguments, options))
}

fn take_value(name: &str, inline: Option<String>, tokens: &[String], index: &mut usize) -> Result<String> {
    if let Some(value) = inline {
        return Ok(value);
    }
    let value = tokens.get(*index).with_context(|| format!("{name} needs a value"))?;
    *index += 1;
    Ok(value.clone())
}

/// Bare words go to an `arguments` array property when the method has one
/// (captured CLIs use it for positionals).
fn positional(method: &Method, object: &mut Map<String, Value>, token: &str) -> Result<()> {
    let flag = method
        .flags()
        .into_iter()
        .find(|flag| flag.key == "arguments" && value_type(&flag.schema).as_deref() == Some("array"))
        .with_context(|| format!("unexpected word {token:?}; {} takes only flags", method.name))?;
    insert(object, &flag, Value::Array(vec![Value::String(token.to_owned())]));
    Ok(())
}

fn insert(object: &mut Map<String, Value>, flag: &Flag, value: Value) {
    match (object.get_mut(&flag.key), value) {
        (Some(Value::Array(existing)), Value::Array(more)) => existing.extend(more),
        (_, value) => {
            object.insert(flag.key.clone(), value);
        }
    }
}

pub fn parse_bool(raw: &str) -> Result<Value> {
    Ok(Value::Bool(match raw.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => true,
        "false" | "0" | "no" | "off" => false,
        _ => bail!("not a boolean: {raw}"),
    }))
}

pub fn convert(raw: &str, flag: &Flag) -> Result<Value> {
    convert_schema(raw, &flag.schema).with_context(|| format!("{} {raw:?}", flag.flag))
}

fn convert_schema(raw: &str, schema: &Value) -> Result<Value> {
    Ok(match value_type(schema).as_deref() {
        Some("integer") => Value::from(raw.parse::<i64>().context("expects an integer")?),
        Some("number") => {
            serde_json::Number::from_f64(raw.parse::<f64>().context("expects a number")?)
                .map(Value::Number)
                .context("expects a finite number")?
        }
        Some("boolean") => parse_bool(raw)?,
        Some("object") | None if raw.starts_with(['{', '[']) || value_type(schema).is_some() => {
            serde_json::from_str(raw).context("expects JSON")?
        }
        Some("array") => {
            if raw.starts_with('[') {
                serde_json::from_str(raw).context("expects a JSON array")?
            } else {
                let item = item_schema(schema).cloned().unwrap_or(Value::Null);
                match value_type(&item).as_deref() {
                    // Comma lists only for numbers/booleans; strings may contain
                    // commas, so repeat the flag to add more string items.
                    Some("integer" | "number" | "boolean") => Value::Array(
                        raw.split(',')
                            .filter(|part| !part.is_empty())
                            .map(|part| convert_schema(part, &item))
                            .collect::<Result<_>>()?,
                    ),
                    _ => Value::Array(vec![convert_schema(raw, &item)?]),
                }
            }
        }
        _ => Value::String(raw.to_owned()),
    })
}
