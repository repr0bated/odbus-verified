use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const CAPTURE_BYTES: usize = 16 * 1024 * 1024;
const CAPTURE_NODES: usize = 512;
const CAPTURE_DEPTH: usize = 16;
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(60);
const TOKEN_LIMIT: usize = 1024;

/// Raw threetched config schema plus a recursively discovered CLI token contract.
/// Token strings deliberately do not pretend to be threetched semantic value types.
#[derive(Debug, Clone)]
pub struct ThreetchedSurface {
    binary_path: PathBuf,
    binary_sha256: String,
    schema_bytes: Vec<u8>,
    schema_json: Value,
    schema_sha256: String,
    version: Option<String>,
    commands: BTreeMap<String, CommandSurface>,
}

/// Runtime-independent seal input. Executable paths are selected by the backend,
/// never trusted from a captured build artifact's location.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceSnapshot {
    pub format: String,
    pub binary_sha256: String,
    pub config_schema: String,
    pub config_schema_sha256: String,
    pub version: Option<String>,
    pub commands: BTreeMap<String, CapturedCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedCommand {
    pub argv: Vec<String>,
    pub summary: String,
    pub help: String,
    pub input_schema: Value,
    pub output_schema: Value,
}

impl SurfaceSnapshot {
    /// Enforce the complete serialized seal-input bound, including JSON escaping
    /// and the derived contracts, rather than only the raw captured help bytes.
    pub fn validate_size(&self) -> Result<()> {
        struct Ceiling(usize);
        impl std::io::Write for Ceiling {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0 = self.0.checked_add(bytes.len())
                    .filter(|size| *size <= CAPTURE_BYTES)
                    .ok_or_else(|| std::io::Error::other("threetched snapshot exceeds byte ceiling"))?;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        serde_json::to_writer(Ceiling(0), self)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct CommandSurface {
    argv: Vec<String>,
    summary: String,
    usage: String,
    help: String,
    options: Vec<OptionSpec>,
    positionals: Vec<Positional>,
}

#[derive(Debug, Clone)]
struct OptionSpec {
    spellings: Vec<String>,
    values: Vec<Positional>,
    required: bool,
    repeatable: bool,
    choices: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct Positional {
    name: String,
    required: bool,
    multiple: bool,
    choices: Option<Vec<String>>,
}

struct Capture<'a> {
    binary: &'a Path,
    config_dir: Option<&'a Path>,
    bytes: usize,
    nodes: usize,
    deadline: Instant,
    commands: BTreeMap<String, CommandSurface>,
}

impl Capture<'_> {
    fn run(&mut self, args: &[&str]) -> Result<String> {
        ensure!(
            Instant::now() < self.deadline,
            "threetched CLI discovery deadline exceeded"
        );
        let text = crate::fuse_process::capture_surface_with_config(self.binary, args, self.config_dir)?;
        self.bytes = self
            .bytes
            .checked_add(text.len())
            .context("capture size overflow")?;
        ensure!(
            self.bytes <= CAPTURE_BYTES,
            "threetched CLI capture exceeds byte ceiling"
        );
        Ok(text)
    }

    fn discover(&mut self, path: Vec<String>, summary: String) -> Result<()> {
        ensure!(
            path.len() <= CAPTURE_DEPTH,
            "threetched CLI exceeds discovery depth"
        );
        self.nodes += 1;
        ensure!(
            self.nodes <= CAPTURE_NODES,
            "threetched CLI exceeds discovery node ceiling"
        );
        let mut args: Vec<_> = path.iter().map(String::as_str).collect();
        args.push("--help");
        let help = self
            .run(&args)
            .with_context(|| format!("capture threetched help for {}", path.join(" ")))?;
        let children = command_words(&help)
            .with_context(|| format!("discover threetched children of {}", path.join(" ")))?;
        let usage =
            usage(&help).with_context(|| format!("parse threetched usage for {}", path.join(" ")))?;
        if children.is_empty() || optional_command(&usage) {
            let id = path.join(".");
            ensure!(!id.is_empty(), "empty threetched method ID");
            let surface = parse_surface(path.clone(), summary, usage, help.clone())
                .with_context(|| format!("parse threetched CLI help for {}", path.join(" ")))?;
            ensure!(
                self.commands.insert(id.clone(), surface).is_none(),
                "duplicate threetched method ID {id}"
            );
        } else {
            ensure!(
                usage.contains("<COMMAND>") || usage.contains("<SUBCOMMAND>"),
                "ambiguous threetched command-group usage: {usage}"
            );
        }
        for (name, description) in children {
            let mut child = path.clone();
            child.push(name);
            self.discover(child, description)?;
        }
        Ok(())
    }
}

impl ThreetchedSurface {
    pub fn capture(binary: impl AsRef<Path>) -> Result<Self> {
        Self::capture_selected(binary.as_ref(), None)
    }

    pub fn capture_with_config(binary: impl AsRef<Path>, config_dir: impl AsRef<Path>) -> Result<Self> {
        ensure!(config_dir.as_ref().is_absolute(), "capture config directory must be absolute");
        Self::capture_selected(binary.as_ref(), Some(config_dir.as_ref()))
    }

    fn capture_selected(binary: &Path, config_dir: Option<&Path>) -> Result<Self> {
        let binary_path = binary.canonicalize().with_context(|| {
            format!(
                "resolve selected threetched binary {}",
                binary.display()
            )
        })?;
        let binary_sha256 = binary_digest(&binary_path)?;
        let mut capture = Capture {
            binary: &binary_path,
            config_dir,
            bytes: 0,
            nodes: 0,
            deadline: Instant::now() + CAPTURE_TIMEOUT,
            commands: BTreeMap::new(),
        };
        let schema_bytes = capture.run(&["config", "schema"])?.into_bytes();
        let schema_json =
            serde_json::from_slice(&schema_bytes).context("threetched config schema is not JSON")?;
        let schema_sha256 = format!("{:x}", Sha256::digest(&schema_bytes));
        let root = capture.run(&["--help"])?;
        let children = command_words(&root)?;
        ensure!(!children.is_empty(), "threetched --help listed no commands");
        for (name, description) in children {
            capture.discover(vec![name], description)?;
        }
        let version = Some(capture.run(&["--version"])?.trim().to_owned());
        let commands = capture.commands;
        ensure!(
            binary_digest(&binary_path)? == binary_sha256,
            "threetched binary changed during discovery; capture again"
        );
        let surface = Self {
            binary_path,
            binary_sha256,
            schema_bytes,
            schema_json,
            schema_sha256,
            version,
            commands,
        };
        for command in surface.commands() {
            surface.input_schema(command)?;
        }
        surface.snapshot().validate_size()?;
        Ok(surface)
    }

    pub fn snapshot(&self) -> SurfaceSnapshot {
        SurfaceSnapshot {
            format: "threetched-cli/1".into(),
            binary_sha256: self.binary_sha256.clone(),
            config_schema: String::from_utf8(self.schema_bytes.clone()).expect("capture is UTF-8"),
            config_schema_sha256: self.schema_sha256.clone(),
            version: self.version.clone(),
            commands: self.commands.iter().map(|(name, command)| {
                (name.clone(), CapturedCommand {
                    argv: command.argv.clone(), summary: command.summary.clone(), help: command.help.clone(),
                    input_schema: self.input_schema(name).expect("captured input contract was checked"),
                    output_schema: self.output_schema(name).expect("captured command exists"),
                })
            }).collect(),
        }
    }

    /// Rebuild contracts from sealed help without executing discovery commands.
    /// Runtime selection must resolve to the exact captured binary content.
    pub fn from_snapshot(binary: impl AsRef<Path>, snapshot: &SurfaceSnapshot) -> Result<Self> {
        ensure!(snapshot.format == "threetched-cli/1", "unsupported threetched snapshot format");
        ensure!(!snapshot.commands.is_empty() && snapshot.commands.len() <= CAPTURE_NODES,
            "threetched snapshot exceeds command bounds");
        snapshot.validate_size()?;
        ensure!(snapshot.binary_sha256.len() == 64
            && snapshot.binary_sha256.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "invalid threetched binary digest");
        let binary_path = binary.as_ref().canonicalize().context("resolve threetched runtime binary")?;
        ensure!(binary_digest(&binary_path)? == snapshot.binary_sha256,
            "threetched runtime binary differs from sealed discovery");
        let schema_bytes = snapshot.config_schema.as_bytes().to_vec();
        let schema_json: Value = serde_json::from_slice(&schema_bytes)?;
        ensure!(schema_json.is_object(), "threetched config schema must be an object");
        let schema_sha256 = format!("{:x}", Sha256::digest(&schema_bytes));
        ensure!(schema_sha256 == snapshot.config_schema_sha256, "threetched config schema digest mismatch");
        let mut commands = BTreeMap::new();
        for (id, captured) in &snapshot.commands {
            ensure!(!captured.argv.is_empty() && captured.argv.len() <= CAPTURE_DEPTH
                && captured.argv.iter().all(|word| command_word_valid(word))
                && captured.argv.join(".") == *id,
                "invalid threetched command identity {id}");
            let usage = usage(&captured.help)?;
            ensure!(usage.split_whitespace().skip(1).take(captured.argv.len())
                .eq(captured.argv.iter().map(String::as_str)),
                "threetched command argv differs from captured usage for {id}");
            ensure!(command_words(&captured.help)?.is_empty() || optional_command(&usage),
                "threetched snapshot contains a non-runnable command group");
            let command = parse_surface(captured.argv.clone(), captured.summary.clone(), usage, captured.help.clone())?;
            commands.insert(id.clone(), command);
        }
        let surface = Self { binary_path, binary_sha256: snapshot.binary_sha256.clone(), schema_bytes,
            schema_json, schema_sha256, version: snapshot.version.clone(), commands };
        for (id, captured) in &snapshot.commands {
            ensure!(surface.input_schema(id)? == captured.input_schema
                && surface.output_schema(id)? == captured.output_schema,
                "threetched sealed CLI contract differs from captured help for {id}");
        }
        Ok(surface)
    }

    pub fn binary_path(&self) -> &Path {
        &self.binary_path
    }
    pub fn binary_sha256(&self) -> &str {
        &self.binary_sha256
    }
    pub fn schema_bytes(&self) -> &[u8] {
        &self.schema_bytes
    }
    pub fn schema_json(&self) -> &Value {
        &self.schema_json
    }
    pub fn schema_sha256(&self) -> &str {
        &self.schema_sha256
    }
    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }
    pub fn commands(&self) -> impl Iterator<Item = &str> {
        self.commands.keys().map(String::as_str)
    }

    fn command(&self, method: &str) -> Result<&CommandSurface> {
        self.commands
            .get(method)
            .with_context(|| format!("unknown threetched command '{method}'"))
    }

    /// All captured aliases for one option, for backend context restrictions.
    /// Missing options return an empty list; unknown methods remain errors.
    pub fn option_spellings(&self, method: &str, canonical_spelling: &str) -> Result<Vec<&str>> {
        Ok(self.command(method)?.options.iter()
            .find(|option| option.spellings.iter().any(|spelling| spelling == canonical_spelling))
            .map(|option| option.spellings.iter().map(String::as_str).collect())
            .unwrap_or_default())
    }

    pub fn input_schema(&self, method: &str) -> Result<Value> {
        let surface = self.command(method)?;
        let mut properties = serde_json::Map::new();
        let mut constraints = Vec::new();
        for option in &surface.options {
            let value = if option.values.is_empty() {
                if option.required {
                    json!({"const":true})
                } else {
                    json!({"type":"boolean"})
                }
            } else {
                let occurrence = token_schema(&option.values, false, option.choices.as_deref())?;
                json!({"oneOf":[occurrence, {
                    "type":"array", "minItems":1, "maxItems":TOKEN_LIMIT, "items":occurrence
                }]})
            };
            for spelling in &option.spellings {
                properties.insert(spelling.clone(), value.clone());
            }
            if option.required {
                constraints.push(json!({"anyOf": option.spellings.iter()
                    .map(|name| json!({"required":[name]})).collect::<Vec<_>>()}));
            }
            for (index, left) in option.spellings.iter().enumerate() {
                for right in &option.spellings[index + 1..] {
                    constraints.push(json!({"not":{"required":[left,right]}}));
                }
            }
        }
        properties.insert(
            "arguments".into(),
            token_schema(&surface.positionals, false, None)?,
        );
        let mut schema =
            json!({"type":"object","additionalProperties":false,"properties":properties});
        if surface.positionals.iter().any(|slot| slot.required) {
            schema["required"] = json!(["arguments"]);
        }
        if !constraints.is_empty() {
            schema["allOf"] = json!(constraints);
        }
        Ok(schema)
    }

    /// Reproduce the captured CLI path and literal tokens. No key renaming or
    /// semantic conversion occurs, and unknown/ambiguous inputs are rejected.
    pub fn arguments(&self, method: &str, input: &Value) -> Result<Vec<String>> {
        ensure!(
            binary_digest(&self.binary_path)? == self.binary_sha256,
            "threetched binary changed since discovery; recapture before dispatch"
        );
        let surface = self.command(method)?;
        jsonschema::Validator::new(&self.input_schema(method)?)?
            .validate(input)
            .map_err(|error| anyhow::anyhow!("threetched CLI tokens: {error}"))?;
        let object = input
            .as_object()
            .context("threetched arguments must be an object")?;
        let mut argv = surface.argv.clone();
        for (key, value) in object {
            if key == "arguments" {
                continue;
            }
            let option = surface
                .options
                .iter()
                .find(|option| option.spellings.contains(key))
                .context("argument was not discovered in threetched help")?;
            if option.values.is_empty() {
                if value.as_bool().context("switch must be boolean")? {
                    argv.push(key.clone());
                }
            } else {
                let values = value
                    .as_array()
                    .context("option values must be token arrays")?;
                if values.first().is_some_and(Value::is_array) {
                    // Repetition is an explicit token representation, never a
                    // guess that a threetched option is scalar or repeatable.
                    for occurrence in values {
                        append_option(
                            &mut argv,
                            key,
                            occurrence
                                .as_array()
                                .context("option occurrence must be a token array")?,
                        )?;
                    }
                } else {
                    append_option(&mut argv, key, values)?;
                }
            }
        }
        if let Some(values) = object.get("arguments") {
            let values = values
                .as_array()
                .context("positional values must be token arrays")?;
            if !values.is_empty() {
                argv.push("--".into());
                for value in values {
                    argv.push(token(value)?.to_owned());
                }
            }
        }
        Ok(argv)
    }

    pub fn output_schema(&self, method: &str) -> Result<Value> {
        self.command(method)?;
        Ok(json!({
            "type":"object","additionalProperties":false,
            "properties":{"stdout":{"type":"string"},"stderr":{"type":"string"},"exit":{"type":"integer"}},
            "required":["stdout","stderr","exit"]
        }))
    }

    pub fn metadata(&self, method: &str) -> Result<Value> {
        let surface = self.command(method)?;
        Ok(json!({
            "name":method,"usage":surface.summary,"cli_usage":surface.usage,
            "argv":surface.argv,"help":surface.help,
            "source_kind":"binary_cli_help", "config_schema_sha256":self.schema_sha256,
            "binary_sha256":self.binary_sha256,
            "argument_encoding":"literal CLI tokens; flat arrays are one occurrence, nested arrays explicitly repeat the option",
            "constraint_scope":"CLI token arity and explicit choices only; semantic threetched types, repetition validity and validators remain upstream",
            "options":surface.options.iter().map(|option| json!({
                "spellings":option.spellings,"required":option.required,
                "repeatable_explicit":option.repeatable,"choices":option.choices,
                "value_names":option.values.iter().map(|value| &value.name).collect::<Vec<_>>()
            })).collect::<Vec<_>>()
        }))
    }

    /// Compatibility discovery helper: only actual CLI descendants are returned.
    /// Config property names never invent methods or positional arguments.
    pub fn lookup_section_methods(&self, section: &str) -> Vec<String> {
        let prefix = format!("{section}.");
        self.commands
            .keys()
            .filter_map(|name| name.strip_prefix(&prefix).map(str::to_owned))
            .collect()
    }

    pub fn explode_section_methods(&self, section: &str) -> Vec<SectionMethod> {
        let prefix = format!("{section}.");
        self.commands
            .iter()
            .filter_map(|(id, surface)| {
                id.strip_prefix(&prefix).map(|name| SectionMethod {
                    name: name.to_owned(),
                    argv: surface.argv.clone(),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionMethod {
    pub name: String,
    pub argv: Vec<String>,
}

fn token(value: &Value) -> Result<&str> {
    let token = value.as_str().context("CLI token is not a string")?;
    ensure!(!token.contains('\0'), "CLI token contains NUL");
    Ok(token)
}

fn binary_digest(path: &Path) -> Result<String> {
    let mut file =
        File::open(path).with_context(|| format!("open threetched binary {}", path.display()))?;
    let maximum = 512 * 1024 * 1024_u64;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file(), "threetched binary is not a regular file");
    ensure!(
        metadata.len() <= maximum,
        "threetched binary exceeds bounded discovery input size"
    );
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .context("threetched binary size overflow")?;
        ensure!(
            total <= maximum,
            "threetched binary exceeds bounded discovery input size"
        );
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn append_option(argv: &mut Vec<String>, spelling: &str, values: &[Value]) -> Result<()> {
    let tokens: Vec<_> = values.iter().map(token).collect::<Result<_>>()?;
    if tokens.len() == 1 && tokens[0].starts_with('-') {
        // Clap's value-assignment spelling preserves a leading hyphen as
        // data instead of turning the value into another flag or --help.
        argv.push(format!("{spelling}={}", tokens[0]));
    } else {
        ensure!(
            !tokens.iter().any(|token| token.starts_with('-')),
            "leading-hyphen multi-value option cannot be represented unambiguously"
        );
        argv.push(spelling.to_owned());
        argv.extend(tokens.into_iter().map(str::to_owned));
    }
    Ok(())
}

fn scalar_schema(choices: Option<&[String]>) -> Value {
    let mut value = json!({"type":"string","pattern":"^[^\\u0000]*$"});
    if let Some(choices) = choices {
        value["enum"] = json!(choices);
    }
    value
}

fn token_schema(
    slots: &[Positional],
    repeatable: bool,
    choices: Option<&[String]>,
) -> Result<Value> {
    ensure!(
        slots.iter().filter(|slot| slot.multiple).count() <= 1,
        "ambiguous multiple variadic values"
    );
    ensure!(
        !slots
            .iter()
            .take(slots.len().saturating_sub(1))
            .any(|slot| slot.multiple),
        "variadic values must be last"
    );
    ensure!(
        !repeatable || slots.len() == 1 && !slots[0].multiple && slots[0].required,
        "unsupported repeatable multi-value option"
    );
    let mut optional = false;
    for slot in slots {
        ensure!(
            !slot.required || !optional,
            "required value after an optional value is ambiguous"
        );
        optional |= !slot.required;
    }
    let minimum = slots.iter().filter(|slot| slot.required).count();
    let multiple = repeatable || slots.last().is_some_and(|slot| slot.multiple);
    let mut schema = json!({
        "type":"array","minItems":minimum,
        "maxItems":if multiple { TOKEN_LIMIT } else { slots.len() }
    });
    if slots.is_empty() {
        schema["items"] = Value::Bool(false);
    } else if multiple && slots.len() == 1 {
        schema["items"] = scalar_schema(choices.or(slots[0].choices.as_deref()));
    } else {
        schema["prefixItems"] = json!(
            slots
                .iter()
                .map(|slot| scalar_schema(choices.or(slot.choices.as_deref())))
                .collect::<Vec<_>>()
        );
        schema["items"] = if multiple {
            scalar_schema(choices.or(slots.last().unwrap().choices.as_deref()))
        } else {
            Value::Bool(false)
        };
    }
    Ok(schema)
}

/// Exact unindented headings delimit sections; examples and wrapped prose
/// elsewhere cannot become commands or options.
fn section<'a>(help: &'a str, title: &str) -> Vec<&'a str> {
    let mut found = false;
    let mut lines = Vec::new();
    for line in help.lines() {
        if line == title {
            found = true;
            continue;
        }
        if found && !line.is_empty() && !line.starts_with(char::is_whitespace) {
            break;
        }
        if found {
            lines.push(line);
        }
    }
    lines
}

fn command_words(help: &str) -> Result<BTreeMap<String, String>> {
    let mut commands = BTreeMap::new();
    for line in section(help, "Commands:") {
        if line.trim().is_empty() {
            continue;
        }
        ensure!(
            line.starts_with("  "),
            "ambiguous threetched command listing: {line}"
        );
        if line.starts_with("   ") {
            continue;
        }
        let trimmed = line.trim();
        let (name, description) = trimmed
            .split_once(char::is_whitespace)
            .unwrap_or((trimmed, ""));
        if name == "help" {
            continue;
        }
        ensure!(
            command_word_valid(name),
            "unsupported or ambiguous threetched command name {name}"
        );
        ensure!(
            commands
                .insert(name.into(), description.trim().into())
                .is_none(),
            "duplicate threetched command {name}"
        );
    }
    Ok(commands)
}

fn command_word_valid(name: &str) -> bool {
    !name.is_empty() && name != "help" && name.len() <= TOKEN_LIMIT
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn usage(help: &str) -> Result<String> {
    let values: Vec<_> = help
        .lines()
        .filter_map(|line| line.strip_prefix("Usage: "))
        .collect();
    ensure!(
        values.len() == 1,
        "threetched help requires one unambiguous Usage line"
    );
    Ok(values[0].to_owned())
}

fn optional_command(usage: &str) -> bool {
    usage.contains("[COMMAND]") || usage.contains("[SUBCOMMAND]")
}

fn definition(line: &str) -> &str {
    let trimmed = line.trim();
    trimmed
        .split_once("  ")
        .map_or(trimmed, |(definition, _)| definition)
}

fn slots(synopsis: &str) -> Result<Vec<Positional>> {
    let mut slots = Vec::new();
    let mut optional = false;
    for word in synopsis.split_whitespace() {
        let (base, multiple) = word
            .strip_suffix("...")
            .map_or((word, false), |base| (base, true));
        let (name, required) = if base.starts_with('<') && base.ends_with('>') {
            (&base[1..base.len() - 1], true)
        } else if base.starts_with('[') && base.ends_with(']') {
            (&base[1..base.len() - 1], false)
        } else {
            bail!("unsupported threetched value synopsis {synopsis}");
        };
        ensure!(
            !name.is_empty() && !name.contains(['<', '>', '[', ']']),
            "ambiguous threetched placeholder {word}"
        );
        ensure!(
            !required || !optional,
            "required value after an optional value is ambiguous"
        );
        optional |= !required;
        slots.push(Positional {
            name: name.into(),
            required,
            multiple,
            choices: None,
        });
    }
    Ok(slots)
}

fn choices(text: &str) -> Result<Option<Vec<String>>> {
    let Some((_, tail)) = text.split_once("[possible values:") else {
        ensure!(
            !text.contains("Possible values:"),
            "unsupported multiline threetched choices"
        );
        return Ok(None);
    };
    let (values, _) = tail
        .split_once(']')
        .context("unterminated threetched possible values")?;
    let values: Vec<_> = values
        .split(',')
        .map(|value| value.trim().to_owned())
        .collect();
    ensure!(
        !values.is_empty() && values.iter().all(|value| !value.is_empty()),
        "ambiguous threetched choices"
    );
    ensure!(
        values.iter().collect::<BTreeSet<_>>().len() == values.len(),
        "duplicate threetched choices"
    );
    Ok(Some(values))
}

fn declared_choices(lines: &[&str], index: usize) -> Result<Option<Vec<String>>> {
    if lines[index].trim() != "Possible values:" {
        return choices(lines[index].trim());
    }
    let mut values = Vec::new();
    for line in &lines[index + 1..] {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if values.is_empty() {
                continue;
            }
            break;
        }
        let Some(bullet) = trimmed.strip_prefix("- ") else {
            break;
        };
        let (value, _) = bullet.split_once(": ").unwrap_or((bullet, ""));
        let value = value.trim();
        ensure!(!value.is_empty(), "empty threetched bullet choice");
        values.push(value.to_owned());
    }
    ensure!(
        !values.is_empty(),
        "threetched Possible values header has no explicit bullet values"
    );
    ensure!(
        values.iter().collect::<BTreeSet<_>>().len() == values.len(),
        "duplicate threetched bullet choices"
    );
    Ok(Some(values))
}

fn required_option(usage: &str, spelling: &str) -> bool {
    let mut depth = 0usize;
    for (index, character) in usage.char_indices() {
        if character == '[' {
            depth += 1;
        }
        if character == ']' {
            depth = depth.saturating_sub(1);
        }
        if depth == 0
            && usage[index..].starts_with(spelling)
            && (index == 0 || usage.as_bytes()[index - 1] == b' ')
            && (usage[index + spelling.len()..].starts_with(' ')
                || index + spelling.len() == usage.len())
        {
            return true;
        }
    }
    false
}

fn parse_surface(
    argv: Vec<String>,
    summary: String,
    usage: String,
    help: String,
) -> Result<CommandSurface> {
    let mut options: Vec<OptionSpec> = Vec::new();
    let mut spellings = BTreeSet::new();
    let option_lines = section(&help, "Options:");
    for (index, line) in option_lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with('-') && !line.starts_with("        ") {
            let synopsis = definition(line);
            let mut words = synopsis.split_whitespace().peekable();
            let mut aliases = Vec::new();
            while let Some(word) = words.peek().copied() {
                if !word.starts_with('-') {
                    break;
                }
                words.next();
                let spelling = word.trim_end_matches(',');
                ensure!(
                    (spelling.starts_with("--")
                        && spelling.len() > 2
                        && spelling[2..].bytes().all(
                            |byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
                        ))
                        || (spelling.starts_with('-')
                            && spelling.len() == 2
                            && spelling.as_bytes()[1].is_ascii_alphanumeric()),
                    "unsupported threetched option spelling {spelling}"
                );
                ensure!(
                    spellings.insert(spelling.to_owned()),
                    "duplicate threetched option spelling {spelling}"
                );
                aliases.push(spelling.to_owned());
            }
            ensure!(!aliases.is_empty(), "threetched option has no spelling");
            let values = slots(&words.collect::<Vec<_>>().join(" "))?;
            let required = aliases.iter().any(|name| required_option(&usage, name));
            options.push(OptionSpec {
                spellings: aliases,
                values,
                required,
                repeatable: false,
                choices: None,
            });
        }
        if let Some(last) = options.last_mut() {
            if let Some(values) = declared_choices(&option_lines, index)? {
                ensure!(
                    last.choices.is_none(),
                    "duplicate threetched choices declaration"
                );
                last.choices = Some(values);
            }
            if trimmed.to_ascii_lowercase().contains("repeatable") {
                last.repeatable = true;
            }
        } else {
            bail!("threetched option description has no declaration: {line}");
        }
    }
    let mut positionals: Vec<Positional> = Vec::new();
    let argument_lines = section(&help, "Arguments:");
    for (index, line) in argument_lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let trimmed = line.trim();
        if (trimmed.starts_with('<') || trimmed.starts_with('[')) && !line.starts_with("        ") {
            let parsed = slots(definition(line))?;
            ensure!(parsed.len() == 1, "ambiguous positional declaration {line}");
            positionals.extend(parsed);
        }
        if let Some(values) = declared_choices(&argument_lines, index)? {
            let last = positionals
                .last_mut()
                .context("threetched positional choices have no declaration")?;
            ensure!(last.choices.is_none(), "duplicate positional choices");
            last.choices = Some(values);
        }
    }
    if !optional_command(&usage) && (usage.contains("<COMMAND>") || usage.contains("<SUBCOMMAND>"))
    {
        ensure!(
            positionals
                .iter()
                .any(|slot| matches!(slot.name.as_str(), "COMMAND" | "SUBCOMMAND")),
            "required command selector has no discovered child commands or positional declaration"
        );
    }
    token_schema(&positionals, false, None)?;
    for option in &options {
        token_schema(&option.values, false, option.choices.as_deref())?;
    }
    Ok(CommandSurface {
        argv,
        summary,
        usage,
        help,
        options,
        positionals,
    })
}

#[cfg(test)]
pub fn test_fixture_binary(directory: &Path) -> PathBuf {
    fixture_binary(directory, false)
}

/// Nested binary fixture shared with the FUSE integration tests.
#[cfg(test)]
pub fn nested_fixture_binary(directory: &Path) -> PathBuf {
    fixture_binary(directory, true)
}

#[cfg(test)]
fn fixture_binary(directory: &Path, nested: bool) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = directory.join("threetched-fixture.sh");
    let script = r#"#!/bin/sh
case "$*" in
"--version") echo 'threetched test-fixture 0.0.1';;
"config schema") printf '%s' '{"type":"object","title":"fixture-config","properties":{"model_routes":{"type":"array"}}}';;
"--help")
cat <<'HELP'
Usage: threetched [OPTIONS] <COMMAND>

Commands:
  self-test  Run diagnostic self-tests
  config     Manage configuration
HELP
if [ "$NESTED" = 1 ]; then printf '  widgets    Manage widgets\n'; fi
;;
"self-test --help")
cat <<'HELP'
Usage: threetched self-test [OPTIONS]

Options:
  -v, --verbose  More output
  -c, --config-dir <CONFIG_DIR>  Selected runtime configuration
HELP
;;
"config --help")
cat <<'HELP'
Usage: threetched config <COMMAND>

Commands:
  schema  Export configuration schema
  set     Set a property
HELP
;;
"config schema --help")
printf 'Usage: threetched config schema\n';;
"config set --help")
cat <<'HELP'
Usage: threetched config set <PATH> [VALUE]

Arguments:
  <PATH>   Property path
  [VALUE]  Literal property value
HELP
;;
"widgets --help")
cat <<'HELP'
Usage: threetched widgets <COMMAND>

Commands:
  publish  Publish a widget
  inspect
  route.map  A literal dotted threetched command word
HELP
if [ -f "$0.upgrade" ]; then printf '  archive    Newly added command\n'; fi
if [ -f "$0.collision" ]; then printf '  route      A distinct command group\n'; fi
;;
"widgets publish --help")
cat <<'HELP'
Usage: threetched widgets publish [OPTIONS] <NAME>

Arguments:
  <NAME>  Widget name

Options:
  -f, --format <FORMAT>  Output format
          [possible values: json, text]
  -v, --verbose  More output
      --count <COUNT>  Numeric value is still a CLI token
HELP
if [ -f "$0.upgrade" ]; then printf '      --new-flag <NEW>  Added after upgrade\n'; fi
;;
"widgets inspect --help")
cat <<'HELP'
Usage: threetched widgets inspect [OPTIONS] --target <TARGET>

Options:
      --target <TARGET>  Inspection target
  -q  Quiet
HELP
;;
"widgets archive --help") printf 'Usage: threetched widgets archive\n';;
"widgets route.map --help") printf 'Usage: threetched widgets route.map\n';;
"widgets route --help") printf 'Usage: threetched widgets route <COMMAND>\n\nCommands:\n  map  Nested command with colliding method ID\n';;
"widgets route map --help") printf 'Usage: threetched widgets route map\n';;
"self-test") echo ok;;
widgets\ publish*) printf '%s\n' "$*";;
*) exit 91;;
esac
"#;
    let script = if nested {
        script.replacen("#!/bin/sh\n", "#!/bin/sh\nNESTED=1\n", 1)
    } else {
        script.to_owned()
    };
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_context_guard_discovers_short_alias_from_captured_help() {
        let directory = tempfile::tempdir().unwrap();
        let surface = ThreetchedSurface::capture(test_fixture_binary(directory.path())).unwrap();
        assert_eq!(surface.option_spellings("self-test", "--config-dir").unwrap(),
            ["-c", "--config-dir"]);
        assert!(surface.option_spellings("self-test", "--absent").unwrap().is_empty());
        assert!(surface.option_spellings("unknown", "--config-dir").is_err());
        let restored = ThreetchedSurface::from_snapshot(surface.binary_path(), &surface.snapshot()).unwrap();
        assert_eq!(restored.option_spellings("self-test", "--config-dir").unwrap(),
            ["-c", "--config-dir"]);
    }

    #[test]
    fn serialized_snapshot_limit_counts_escaping_before_seal() {
        let directory = tempfile::tempdir().unwrap();
        let mut snapshot = ThreetchedSurface::capture(test_fixture_binary(directory.path())).unwrap().snapshot();
        // Raw bytes fit the discovery ceiling but escape expansion does not.
        snapshot.commands.get_mut("self-test").unwrap().summary = "\0".repeat(CAPTURE_BYTES / 4);
        assert!(snapshot.validate_size().is_err());
    }

    #[test]
    fn sealed_snapshot_retargets_binary_and_rejects_contract_or_identity_drift() {
        let directory = tempfile::tempdir().unwrap();
        let binary = test_fixture_binary(directory.path());
        let captured = ThreetchedSurface::capture(&binary).unwrap();
        let snapshot: SurfaceSnapshot = serde_json::from_value(
            serde_json::to_value(captured.snapshot()).unwrap()).unwrap();
        let runtime = directory.path().join("runtime-threetched");
        std::fs::copy(&binary, &runtime).unwrap();
        std::fs::remove_file(&binary).unwrap();
        let restored = ThreetchedSurface::from_snapshot(&runtime, &snapshot).unwrap();
        assert_eq!(restored.binary_sha256(), captured.binary_sha256());
        assert_eq!(restored.commands().collect::<Vec<_>>(), captured.commands().collect::<Vec<_>>());
        assert_eq!(restored.arguments("self-test", &json!({})).unwrap(), ["self-test"]);

        let mut changed = snapshot.clone();
        changed.commands.get_mut("self-test").unwrap().input_schema = json!({"type":"null"});
        assert!(ThreetchedSurface::from_snapshot(&runtime, &changed).is_err());
        let mut changed = snapshot.clone();
        let mut command = changed.commands.remove("self-test").unwrap();
        command.argv = vec!["other".into()];
        changed.commands.insert("other".into(), command);
        assert!(ThreetchedSurface::from_snapshot(&runtime, &changed).is_err());
        std::fs::write(&runtime, "different executable").unwrap();
        assert!(ThreetchedSurface::from_snapshot(&runtime, &snapshot).is_err());
    }

    #[test]
    fn snapshot_restore_reads_only_binary_content_and_never_spawns_discovery() {
        let directory = tempfile::tempdir().unwrap();
        let binary = test_fixture_binary(directory.path());
        let marker = directory.path().join("capture-marker");
        let script = std::fs::read_to_string(&binary).unwrap();
        let script = script.replacen("#!/bin/sh", &format!("#!/bin/sh\nprintf x >> '{}'", marker.display()), 1);
        std::fs::write(&binary, script).unwrap();
        let snapshot = ThreetchedSurface::capture_with_config(&binary, directory.path()).unwrap().snapshot();
        assert!(marker.exists());
        std::fs::remove_file(&marker).unwrap();
        ThreetchedSurface::from_snapshot(&binary, &snapshot).unwrap();
        assert!(!marker.exists());
        let mut value = serde_json::to_value(snapshot).unwrap();
        value["binary_path"] = json!("/untrusted/selector");
        assert!(serde_json::from_value::<SurfaceSnapshot>(value).is_err());
    }

    #[test]
    fn recursively_discovers_threetched_leaf_commands_without_config_type_guessing() {
        let directory = tempfile::tempdir().unwrap();
        let surface = ThreetchedSurface::capture(nested_fixture_binary(directory.path())).unwrap();
        assert_eq!(
            surface.commands().collect::<Vec<_>>(),
            [
                "config.schema",
                "config.set",
                "self-test",
                "widgets.inspect",
                "widgets.publish",
                "widgets.route.map"
            ]
        );
        assert_eq!(surface.schema_json()["title"], "fixture-config");
        assert_eq!(surface.version(), Some("threetched test-fixture 0.0.1"));
        assert!(!surface.schema_sha256().is_empty());
        assert!(surface.input_schema("config").is_err());
        assert!(surface.lookup_section_methods("model_routes").is_empty());
        assert_eq!(
            surface.lookup_section_methods("widgets"),
            ["inspect", "publish", "route.map"]
        );
        assert_eq!(
            surface.explode_section_methods("widgets")[1].argv,
            ["widgets", "publish"]
        );
        assert_eq!(
            surface.metadata("widgets.publish").unwrap()["argv"],
            json!(["widgets", "publish"])
        );
    }

    #[test]
    fn flags_keep_exact_spelling_token_types_choices_and_aliases() {
        let directory = tempfile::tempdir().unwrap();
        let surface = ThreetchedSurface::capture(nested_fixture_binary(directory.path())).unwrap();
        let schema = surface.input_schema("widgets.publish").unwrap();
        assert_eq!(
            schema["properties"]["--count"]["oneOf"][0]["prefixItems"][0]["type"],
            "string"
        );
        assert_eq!(
            surface
                .arguments(
                    "widgets.publish",
                    &json!({
                        "--format":["json"],"--verbose":true,"arguments":["-sample"]
                    })
                )
                .unwrap(),
            [
                "widgets",
                "publish",
                "--format",
                "json",
                "--verbose",
                "--",
                "-sample"
            ]
        );
        assert_eq!(
            surface
                .arguments(
                    "widgets.publish",
                    &json!({
                        "-f":["text"],"-v":true,"arguments":["sample"]
                    })
                )
                .unwrap(),
            ["widgets", "publish", "-f", "text", "-v", "--", "sample"]
        );
        for input in [
            json!({"arguments":["x"],"format":["json"]}),
            json!({"arguments":["x"],"--format":["xml"]}),
            json!({"arguments":["x"],"--count":[7]}),
            json!({"arguments":["x"],"--count":[]}),
            json!({"arguments":["x"],"--format":["json"],"-f":["text"]}),
            json!({"arguments":["x"],"--unknown":true}),
            json!({"arguments":["x"],"--count":["a","b"]}),
            json!({"arguments":["\0"]}),
        ] {
            assert!(
                surface.arguments("widgets.publish", &input).is_err(),
                "{input}"
            );
        }
    }

    #[test]
    fn usage_required_flags_and_optional_positionals_are_enforced() {
        let directory = tempfile::tempdir().unwrap();
        let surface = ThreetchedSurface::capture(nested_fixture_binary(directory.path())).unwrap();
        assert!(surface.arguments("widgets.inspect", &json!({})).is_err());
        assert_eq!(
            surface
                .arguments("widgets.inspect", &json!({"--target":["x"],"-q":true}))
                .unwrap(),
            ["widgets", "inspect", "--target", "x", "-q"]
        );
        assert!(surface.arguments("widgets.publish", &json!({})).is_err());
        assert!(
            surface
                .arguments("config.set", &json!({"arguments":[]}))
                .is_err()
        );
        assert_eq!(
            surface
                .arguments("config.set", &json!({"arguments":["path"]}))
                .unwrap(),
            ["config", "set", "--", "path"]
        );
        assert_eq!(
            surface
                .arguments("config.set", &json!({"arguments":["path","true"]}))
                .unwrap(),
            ["config", "set", "--", "path", "true"]
        );
        assert!(
            surface
                .arguments("config.set", &json!({"arguments":["x","y","z"]}))
                .is_err()
        );
    }

    #[test]
    fn optional_command_groups_remain_runnable_and_repeatability_is_explicit() {
        let surface = parse_surface(vec!["stop".into()],"".into(),
            "threetched stop [OPTIONS] [COMMAND]".into(),
            "Usage: threetched stop [OPTIONS] [COMMAND]\n\nOptions:\n      --domain <DOMAIN>  Domain (repeatable)\n".into()).unwrap();
        assert!(optional_command(&surface.usage));
        let directory = tempfile::tempdir().unwrap();
        let mut threetched = ThreetchedSurface::capture(test_fixture_binary(directory.path())).unwrap();
        threetched.commands = BTreeMap::from([("stop".into(), surface)]);
        assert_eq!(
            threetched
                .arguments("stop", &json!({"--domain":[["a"],["b"]]}))
                .unwrap(),
            ["stop", "--domain", "a", "--domain", "b"]
        );
        assert!(
            threetched
                .arguments("stop", &json!({"--domain":["a","b"]}))
                .is_err()
        );
    }

    #[test]
    fn schema_refresh_discovers_new_flags_without_feature_tables() {
        let directory = tempfile::tempdir().unwrap();
        let binary = nested_fixture_binary(directory.path());
        let before = ThreetchedSurface::capture(&binary).unwrap();
        assert!(
            before
                .arguments(
                    "widgets.publish",
                    &json!({"--new-flag":["x"],"arguments":["y"]})
                )
                .is_err()
        );
        std::fs::write(format!("{}.upgrade", binary.display()), "").unwrap();
        let after = ThreetchedSurface::capture(&binary).unwrap();
        assert!(
            after
                .arguments(
                    "widgets.publish",
                    &json!({"--new-flag":["x"],"arguments":["y"]})
                )
                .is_ok()
        );
        assert!(before.input_schema("widgets.archive").is_err());
        assert_eq!(
            after.arguments("widgets.archive", &json!({})).unwrap(),
            ["widgets", "archive"]
        );
    }

    #[test]
    fn ambiguous_help_is_rejected() {
        for help in [
            "Usage: z x\n\nOptions:\n      --arg[=<VALUE>]  Optional value\n",
            "Usage: z x\n\nOptions:\n      --mode <MODE>  Mode\n          Possible values:\n",
            "Usage: z x\n\nArguments:\n  [ONE]  optional\n  <TWO>  required\n",
        ] {
            assert!(
                parse_surface(
                    vec!["x".into()],
                    "".into(),
                    usage(help).unwrap(),
                    help.into()
                )
                .is_err(),
                "{help}"
            );
        }
        assert_eq!(
            command_words("Commands:\n  a.b  Literal dotted command\n").unwrap()["a.b"],
            "Literal dotted command"
        );
        assert!(command_words("Commands:\n  x  One\n  x  Two\n").is_err());
    }

    #[test]
    fn dotted_threetched_word_keeps_exact_argv_and_method_id_collisions_fail_closed() {
        let directory = tempfile::tempdir().unwrap();
        let binary = nested_fixture_binary(directory.path());
        let surface = ThreetchedSurface::capture(&binary).unwrap();
        assert_eq!(
            surface.arguments("widgets.route.map", &json!({})).unwrap(),
            ["widgets", "route.map"]
        );
        assert_eq!(
            surface.metadata("widgets.route.map").unwrap()["argv"],
            json!(["widgets", "route.map"])
        );
        std::fs::write(format!("{}.collision", binary.display()), "").unwrap();
        let error = ThreetchedSurface::capture(&binary).unwrap_err();
        assert!(format!("{error:#}").contains("duplicate threetched method ID widgets.route.map"));
    }

    #[test]
    fn threetched_bullet_choices_are_explicit_tokens_and_leading_flags_are_values() {
        let help = "Usage: threetched inspect [OPTIONS] --confirm\n\nOptions:\n      --format <FORMAT>\n          Output format\n\n          Possible values:\n          - table: Human-readable table\n          - json:  Machine-readable JSON\n\n          [default: table]\n      --value <VALUE>  Literal token\n  -m <MESSAGE>  Literal token\n      --confirm  Required presence\n";
        let command = parse_surface(
            vec!["inspect".into()],
            String::new(),
            usage(help).unwrap(),
            help.into(),
        )
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let mut threetched = ThreetchedSurface::capture(test_fixture_binary(directory.path())).unwrap();
        threetched.commands = BTreeMap::from([("inspect".into(), command)]);
        assert_eq!(
            threetched
                .arguments(
                    "inspect",
                    &json!({"--confirm":true,"--format":["json"],"--value":["--help"]})
                )
                .unwrap(),
            ["inspect", "--confirm", "--format", "json", "--value=--help"]
        );
        assert_eq!(
            threetched
                .arguments("inspect", &json!({"--confirm":true,"-m":["--help"]}))
                .unwrap(),
            ["inspect", "--confirm", "-m=--help"]
        );
        assert!(
            threetched
                .arguments("inspect", &json!({"--format":["yaml"],"--confirm":true}))
                .is_err()
        );
        assert!(
            threetched
                .arguments("inspect", &json!({"--confirm":false}))
                .is_err()
        );
        assert!(threetched.arguments("inspect", &json!({})).is_err());
        let mut argv = Vec::new();
        assert!(append_option(&mut argv, "--values", &[json!("-a"), json!("b")]).is_err());
    }

    #[test]
    fn selected_binary_symlink_and_digest_pin_open_surface_until_recapture() {
        use std::io::Write;
        use std::os::unix::fs::symlink;
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_binary = test_fixture_binary(first.path());
        let second_binary = test_fixture_binary(second.path());
        let selector = first.path().join("selected");
        symlink(&first_binary, &selector).unwrap();
        let surface = ThreetchedSurface::capture(&selector).unwrap();
        std::fs::remove_file(&selector).unwrap();
        symlink(&second_binary, &selector).unwrap();
        assert_eq!(surface.binary_path(), first_binary.canonicalize().unwrap());
        assert!(surface.arguments("self-test", &json!({})).is_ok());
        std::fs::OpenOptions::new()
            .append(true)
            .open(&first_binary)
            .unwrap()
            .write_all(b"\n# upgraded\n")
            .unwrap();
        assert!(surface.arguments("self-test", &json!({})).is_err());
        assert!(
            ThreetchedSurface::capture(&first_binary)
                .unwrap()
                .arguments("self-test", &json!({}))
                .is_ok()
        );
    }

    #[test]
    #[ignore = "explicit binary metadata discovery only; requires THREETCHED_FS_BINARY_CAPTURE"]
    fn actual_threetched_capture() {
        let path = std::env::var_os("THREETCHED_FS_BINARY_CAPTURE").expect("select threetched binary");
        let surface = ThreetchedSurface::capture(PathBuf::from(path)).unwrap();
        println!(
            "threetched discovery: {} methods, version {:?}",
            surface.commands.len(),
            surface.version()
        );
        assert!(!surface.commands.is_empty());
        for method in surface.commands() {
            jsonschema::Validator::new(&surface.input_schema(method).unwrap()).unwrap();
        }
    }
}
