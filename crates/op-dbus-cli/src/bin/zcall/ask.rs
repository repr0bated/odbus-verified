//! `zcall ask "<request>"`: JEV (TypeSafe System One) supplies typed
//! judgments, and this code makes every decision. JEV only picks among
//! candidates zcall supplies: sealed plugins, sealed methods, enum members and
//! words taken from the request itself. It never generates a value. The
//! result is printed as a zcall command line; nothing is called.
//!
//! All question wording lives in this file.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value, json};

use crate::catalog::{Catalog, Flag, Method, closed_values, is_switch, item_schema, value_type};

const SECRET_FILE: &str = "/etc/op-dbus/secrets/typesafe-api-key";
const YES: f64 = 0.7;
const NO: f64 = 0.3;
const TOP_PLUGINS: usize = 3;
const FINALISTS_PER_PLUGIN: usize = 5;
/// A top plugin below this probability contributes no finalists.
const MIN_PLUGIN_P: f64 = 0.05;
/// A plugin's own method Choice at or above this joins the finalists.
const CONFIDENT_METHOD: f64 = 0.8;
const MAX_WORDS: usize = 40;
/// TypeSafe's limit on options in one Choice.
const MAX_OPTIONS: usize = 255;
/// Jev's default limits are 64k tokens per request and 32k per question
/// (state + that question). Budgets in JSON characters assume about three
/// characters per token, below the usual ratio, so requests stay under limit.
const REQUEST_BUDGET_CHARS: usize = 150_000;
const QUESTION_BUDGET_CHARS: usize = 80_000;

struct Jev {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    key: String,
    show: bool,
}

impl Jev {
    fn new(show: bool) -> Result<Self> {
        let key = match std::env::var("TYPESAFE_API_KEY") {
            Ok(key) if !key.trim().is_empty() => key,
            _ => std::fs::read_to_string(SECRET_FILE)
                .with_context(|| {
                    format!("TYPESAFE_API_KEY is unset and {SECRET_FILE} is not readable by this user")
                })?,
        };
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()?,
            endpoint: std::env::var("TYPESAFE_ENDPOINT")
                .unwrap_or_else(|_| "https://api.typesafe.ai".into()),
            model: std::env::var("ZCALL_JEV_MODEL").unwrap_or_else(|_| "jev-latest".into()),
            key: key.trim().to_owned(),
            show,
        })
    }

    /// One System One request. A failed call is an error, never an answer.
    async fn ask(&self, state: Value, questions: Map<String, Value>) -> Result<Map<String, Value>> {
        let payload = json!({"model": self.model, "state": state, "questions": questions});
        if self.show {
            eprintln!("--- JEV request\n{}", serde_json::to_string_pretty(&payload)?);
        }
        let response = self
            .client
            .post(format!("{}/v1/systemone", self.endpoint.trim_end_matches('/')))
            .bearer_auth(&self.key)
            .json(&payload)
            .send()
            .await
            .context("JEV request failed")?;
        let status = response.status();
        let body: Value = response.json().await.context("JEV response is not JSON")?;
        if self.show {
            eprintln!("--- JEV response\n{}", serde_json::to_string_pretty(&body)?);
        }
        ensure!(status.is_success(), "JEV returned {status}: {body}");
        body["answers"]
            .as_object()
            .cloned()
            .context("JEV response has no answers")
    }

    /// Ask every question in `questions` over the same `state`, packed into as
    /// few requests as fit Jev's context limits, all sent at once. Any failed
    /// request fails the whole ask; partial answers are never used.
    async fn ask_all(
        self: &std::sync::Arc<Self>,
        state: Value,
        questions: Map<String, Value>,
    ) -> Result<Map<String, Value>> {
        let state_chars = state.to_string().len();
        ensure!(
            state_chars < QUESTION_BUDGET_CHARS,
            "JEV state alone is too large ({state_chars} chars)"
        );
        let mut batches: Vec<Map<String, Value>> = vec![Map::new()];
        let mut used = state_chars;
        for (id, question) in questions {
            let size = id.len() + question.to_string().len();
            ensure!(
                state_chars + size <= QUESTION_BUDGET_CHARS,
                "JEV question {id} is too large for one request ({size} chars)"
            );
            if used + size > REQUEST_BUDGET_CHARS && !batches.last().is_some_and(Map::is_empty) {
                batches.push(Map::new());
                used = state_chars;
            }
            used += size;
            batches.last_mut().expect("one batch").insert(id, question);
        }
        let mut tasks = tokio::task::JoinSet::new();
        for batch in batches.into_iter().filter(|b| !b.is_empty()) {
            let jev = std::sync::Arc::clone(self);
            let state = state.clone();
            tasks.spawn(async move { jev.ask(state, batch).await });
        }
        let mut answers = Map::new();
        while let Some(result) = tasks.join_next().await {
            answers.extend(result.context("JEV request task failed")??);
        }
        Ok(answers)
    }
}

fn noul(answers: &Map<String, Value>, id: &str) -> Option<f64> {
    answers.get(id)?["noul"].as_f64()
}

/// Choice options with their probabilities, best first.
fn ranked(answers: &Map<String, Value>, id: &str) -> Vec<(String, f64)> {
    let mut list: Vec<(String, f64)> = answers
        .get(id)
        .and_then(|a| a["probabilities"].as_object())
        .map(|p| p.iter().filter_map(|(k, v)| Some((k.clone(), v.as_f64()?))).collect())
        .unwrap_or_default();
    list.sort_by(|a, b| b.1.total_cmp(&a.1));
    list
}

fn choice(instructions: String, criteria: BTreeMap<String, String>) -> Value {
    json!({"type": "choice", "instructions": instructions, "criteria": criteria})
}

fn yes_no(instructions: String) -> Value {
    json!({"type": "noul", "instructions": instructions})
}

/// Words from the request that could be a value: split on whitespace, outer
/// quotes and trailing punctuation removed. Code finds candidates; JEV picks.
fn words(request: &str) -> Vec<String> {
    let mut seen = Vec::new();
    for raw in request.split_whitespace() {
        let word = raw
            .trim_matches(|c: char| matches!(c, '"' | '\'' | '`' | '(' | ')'))
            .trim_end_matches([',', ';', '?', '!']);
        let word = word.strip_suffix('.').filter(|w| !w.contains('/')).unwrap_or(word);
        if !word.is_empty() && !seen.iter().any(|s| s == word) {
            seen.push(word.to_owned());
        }
        if seen.len() == MAX_WORDS {
            break;
        }
    }
    seen
}

/// Single-word values a property's description names in backticks, skipping
/// names of other properties and expressions such as `event_id >= x`.
fn documented_values(schema: &Value) -> Vec<String> {
    let Some(text) = schema["description"].as_str() else { return vec![] };
    let mut values = Vec::new();
    for (index, part) in text.split('`').enumerate() {
        if index % 2 == 1
            && !part.is_empty()
            && !part.contains(char::is_whitespace)
            && !values.iter().any(|v| v == part)
        {
            values.push(part.to_owned());
        }
    }
    values
}

fn describe_method(method: &Method) -> String {
    let arguments: Vec<String> = method.flags().iter().map(|f| f.key.clone()).collect();
    format!(
        "{} ({}){}",
        method.name.replace(['_', '.', '-'], " "),
        method.side_effect,
        if arguments.is_empty() { String::new() } else { format!(", takes {}", arguments.join(", ")) }
    )
}

/// Split method names into at most MAX_OPTIONS groups of at most MAX_OPTIONS
/// members, grouping by the longest leading-word prefix that fits.
fn partition(names: Vec<String>) -> Vec<(String, Vec<String>)> {
    let prefix = |name: &str, words: usize| -> String {
        let parts: Vec<&str> = name.split(['_', '.', '-']).collect();
        parts[..words.min(parts.len().saturating_sub(1)).max(1).min(parts.len())].join("_")
    };
    for words in (1..=4).rev() {
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for name in &names {
            groups.entry(prefix(name, words)).or_default().push(name.clone());
        }
        if groups.len() <= MAX_OPTIONS && groups.values().all(|g| g.len() <= MAX_OPTIONS) {
            return groups.into_iter().collect();
        }
    }
    // Fallback: fixed-size chunks in name order.
    names
        .chunks(MAX_OPTIONS)
        .enumerate()
        .map(|(i, chunk)| (format!("{}_to_{}_{i}", chunk[0], chunk[chunk.len() - 1]), chunk.to_vec()))
        .collect()
}

fn describe_flag(flag: &Flag) -> String {
    let name = flag.key.trim_start_matches('-').replace(['_', '-'], " ");
    match flag.schema["description"].as_str() {
        Some(text) => format!("{name} ({text})"),
        None => name,
    }
}

pub async fn run(catalog: &Catalog, request: &str, show: bool) -> Result<()> {
    ensure!(!request.trim().is_empty(), "usage: zcall ask \"<what you want done>\"");
    let jev = std::sync::Arc::new(Jev::new(show)?);

    // Request 1 (fan-out): which plugin, whether an operation is asked for at
    // all, and — speculatively, in the same request — which method within
    // each plugin. Questions are answered independently; code combines them.
    let mut systems = BTreeMap::new();
    let mut catalog_methods: BTreeMap<String, BTreeMap<String, Method>> = BTreeMap::new();
    for plugin in catalog.plugins() {
        let Some(schema) = catalog.schema(&plugin) else { continue };
        let description = Some(schema.description)
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| plugin.replace('_', " "));
        let methods = catalog.methods(&plugin);
        if methods.is_empty() {
            continue;
        }
        systems.insert(plugin.clone(), description);
        catalog_methods.insert(plugin, methods);
    }
    ensure!(!systems.is_empty(), "the 3tchedFS mount {} declares no methods", catalog.path().display());
    let mut questions = Map::new();
    questions.insert(
        "system".into(),
        choice("Which system in `systems` does `request` want to act on?".into(), systems.clone()),
    );
    // Reads and actions are separate judgments; either one is a call.
    questions.insert(
        "asks_info".into(),
        yes_no("Does `request` ask for information held by one of the systems in `systems`?".into()),
    );
    questions.insert(
        "asks_action".into(),
        yes_no("Does `request` ask for one of the systems in `systems` to do something?".into()),
    );
    let ids: Vec<String> = catalog_methods.keys().cloned().collect();
    // Plugins with more methods than a Choice allows get a two-level choice:
    // group (by leading name words), then method within the group.
    let mut grouped: BTreeMap<usize, Vec<(String, Vec<String>)>> = BTreeMap::new();
    for (index, plugin) in ids.iter().enumerate() {
        let methods = &catalog_methods[plugin];
        if methods.len() < 2 {
            continue;
        }
        let question = format!("Which {plugin} operation does `request` want to run?");
        if methods.len() <= MAX_OPTIONS {
            let criteria = methods.values().map(|m| (m.name.clone(), describe_method(m))).collect();
            questions.insert(format!("m{index}"), choice(question, criteria));
            continue;
        }
        let groups = partition(methods.keys().cloned().collect());
        let criteria = groups
            .iter()
            .map(|(group, members)| {
                let sample: Vec<&str> = members.iter().take(6).map(String::as_str).collect();
                (group.clone(), format!("{} operations such as {}", group.replace(['_', '.', '-'], " "), sample.join(", ")))
            })
            .collect();
        questions.insert(format!("g{index}"), choice(question, criteria));
        for (group_index, (group, members)) in groups.iter().enumerate() {
            if members.len() < 2 {
                continue;
            }
            let criteria = members
                .iter()
                .map(|name| (name.clone(), describe_method(&methods[name])))
                .collect();
            questions.insert(
                format!("g{index}_{group_index}"),
                choice(format!("Which {plugin} {} operation does `request` want to run?", group.replace(['_', '.', '-'], " ")), criteria),
            );
        }
        grouped.insert(index, groups);
    }
    let answers = jev
        .ask_all(json!({"request": request, "systems": systems}), questions)
        .await?;
    let fits = noul(&answers, "asks_info")
        .unwrap_or(0.0)
        .max(noul(&answers, "asks_action").unwrap_or(0.0));
    if fits < NO {
        bail!("JEV finds no operation in that request (p={fits:.2}); nothing printed");
    }
    // Finalists: the best few methods of each top plugin. A per-plugin Choice
    // always picks something even when that plugin lacks the operation, so
    // the finalists are compared against each other in one more Choice.
    let mut finalists: Vec<(String, String, f64)> = Vec::new();
    // Any plugin whose own speculative method Choice is confident also
    // competes: the system Choice can miss a plugin that holds the operation.
    for (index, plugin) in ids.iter().enumerate() {
        if grouped.contains_key(&index) || catalog_methods[plugin].len() < 2 {
            continue;
        }
        if let Some((method, p)) = ranked(&answers, &format!("m{index}")).into_iter().next() {
            if p >= CONFIDENT_METHOD {
                finalists.push((plugin.clone(), method, 0.0));
            }
        }
    }
    finalists.sort_by(|a, b| a.0.cmp(&b.0));
    for (plugin, p_plugin) in ranked(&answers, "system")
        .into_iter()
        .take(TOP_PLUGINS)
        .filter(|(_, p)| *p >= MIN_PLUGIN_P)
    {
        let Some(index) = ids.iter().position(|id| *id == plugin) else { continue };
        let methods = &catalog_methods[&plugin];
        let within = if methods.len() == 1 {
            vec![(methods.keys().next().cloned().unwrap_or_default(), 1.0)]
        } else if let Some(groups) = grouped.get(&index) {
            // P(group) × P(method | group), best first.
            let mut combined = Vec::new();
            for (group, p_group) in ranked(&answers, &format!("g{index}")).into_iter().take(3) {
                let Some(group_index) = groups.iter().position(|(g, _)| *g == group) else { continue };
                let members = &groups[group_index].1;
                if members.len() == 1 {
                    combined.push((members[0].clone(), p_group));
                } else if let Some((name, p)) = ranked(&answers, &format!("g{index}_{group_index}")).into_iter().next() {
                    combined.push((name, p_group * p));
                }
            }
            combined.sort_by(|a, b| b.1.total_cmp(&a.1));
            combined
        } else {
            ranked(&answers, &format!("m{index}"))
        };
        for (method, p_method) in within.into_iter().take(FINALISTS_PER_PLUGIN) {
            if !finalists.iter().any(|(p, m, _)| *p == plugin && *m == method) {
                finalists.push((plugin.clone(), method, p_plugin * p_method));
            }
        }
    }
    finalists.truncate(MAX_OPTIONS);
    ensure!(!finalists.is_empty(), "JEV returned no plugin/method");
    let mut fits_any = 1.0;
    let (plugin, method_name, method_p) = if finalists.len() == 1 {
        finalists.remove(0)
    } else {
        let criteria: BTreeMap<String, String> = finalists
            .iter()
            .filter_map(|(plugin, method, _)| {
                let decl = catalog_methods.get(plugin)?.get(method)?;
                Some((
                    format!("{plugin}.{method}"),
                    format!("{plugin} ({}): {}", systems.get(plugin).cloned().unwrap_or_default(), describe_method(decl)),
                ))
            })
            .collect();
        let mut questions = Map::new();
        questions.insert(
            "finalist".into(),
            choice("Which operation does `request` want to run?".into(), criteria.clone()),
        );
        // The Choice always picks; this says whether anything fits.
        questions.insert(
            "any_fits".into(),
            yes_no("Does one of the operations in `operations` do what `request` asks?".into()),
        );
        let answers = jev
            .ask_all(json!({"request": request, "operations": criteria}), questions)
            .await?;
        fits_any = noul(&answers, "any_fits").unwrap_or(0.0);
        let (key, p) = ranked(&answers, "finalist")
            .into_iter()
            .next()
            .context("JEV returned no operation")?;
        let (plugin, method) = key.split_once('.').context("JEV picked an unknown operation")?;
        if fits_any < NO {
            bail!(
                "no sealed operation fits that request (p={fits_any:.2}); closest was {key} \
                 (p={p:.2}). Nothing printed."
            );
        }
        (plugin.to_owned(), method.to_owned(), p)
    };
    let method = catalog_methods
        .get_mut(&plugin)
        .and_then(|methods| methods.remove(&method_name))
        .context("JEV picked an unknown method")?;
    let key = format!("{plugin}.{method_name}");

    // Request 2 (fan-out): for every flag at once, is it specified, and which
    // candidate value?
    let candidates = words(request);
    let flags = method.flags();
    let mut questions = Map::new();
    let mut plans: Vec<(Flag, Vec<String>)> = Vec::new();
    for (index, flag) in flags.iter().enumerate() {
        let subject = describe_flag(flag);
        if is_switch(&flag.schema) {
            questions.insert(format!("on{index}"), yes_no(format!("Does `request` ask for {subject} to be turned on?")));
            plans.push((flag.clone(), vec![]));
            continue;
        }
        let closed = closed_values(&flag.schema);
        let options: Vec<String> = if closed.is_empty() {
            let numeric = matches!(
                value_type(item_schema(&flag.schema).unwrap_or(&flag.schema)).as_deref(),
                Some("integer" | "number")
            );
            // Values the sealed description documents in backticks (e.g.
            // `allow`, `deny`) come first, then words from the request.
            let mut options = documented_values(&flag.schema);
            for word in &candidates {
                if !options.contains(word) {
                    options.push(word.clone());
                }
            }
            options
                .into_iter()
                .filter(|w| !numeric || w.split(',').all(|p| p.parse::<f64>().is_ok()))
                .take(MAX_OPTIONS)
                .collect()
        } else {
            closed
        };
        if options.is_empty() {
            continue;
        }
        questions.insert(format!("set{index}"), yes_no(format!("Does `request` give a value for {subject}?")));
        if options.len() > 1 {
            let criteria = options
                .iter()
                .map(|o| (o.clone(), format!("{subject} is `{o}`")))
                .collect();
            questions.insert(format!("val{index}"), choice(format!("What value does `request` give for {subject}?"), criteria));
        }
        plans.push((flag.clone(), options));
    }
    let answers = if questions.is_empty() {
        Map::new()
    } else {
        jev.ask_all(json!({"request": request, "operation": key}), questions).await?
    };

    // Decisions, in code.
    let mut line = vec!["zcall".to_owned(), plugin.clone(), method.name.clone()];
    let mut notes = vec![format!("{key}  (method p={method_p:.2}, operation p={fits:.2})")];
    for (flag, options) in &plans {
        let index = flags.iter().position(|f| f.key == flag.key).unwrap_or(0);
        if is_switch(&flag.schema) {
            let p = noul(&answers, &format!("on{index}")).unwrap_or(0.0);
            if p >= YES {
                line.push(flag.flag.clone());
                notes.push(format!("{}  p={p:.2}", flag.flag));
            }
            continue;
        }
        let set = noul(&answers, &format!("set{index}")).unwrap_or(0.0);
        let (value, p) = if options.len() == 1 {
            (options[0].clone(), set)
        } else {
            ranked(&answers, &format!("val{index}")).into_iter().next().unwrap_or_default()
        };
        // A required flag is filled in the unsettled band (flagged UNSURE),
        // never when JEV says the request gives no value.
        if set >= YES || (flag.required && set > NO && !value.is_empty()) {
            if set < YES {
                notes.push(format!("{} {value}  UNSURE (required; given p={set:.2}) — check it", flag.flag));
            } else {
                notes.push(format!("{} {value}  p={p:.2}", flag.flag));
            }
            line.push(flag.flag.clone());
            line.push(value);
        } else if flag.required {
            notes.push(format!("{} MISSING (required) — add it yourself", flag.flag));
        } else if set > NO && !value.is_empty() {
            // Unsettled band: shown, not added.
            notes.push(format!("maybe {} {value}  (given p={set:.2}, not added)", flag.flag));
        }
    }
    if method.side_effect == "mutation" {
        notes.push("mutating method: add --yes to run it".into());
    }
    if fits < YES {
        notes.push(format!("JEV is unsure the request names an operation (p={fits:.2})"));
    }
    if fits_any < YES {
        notes.push(format!("JEV is unsure any sealed operation fits (p={fits_any:.2}) — check it"));
    }
    for note in notes {
        eprintln!("# {note}");
    }
    println!("{}", line.iter().map(|w| shell_quote(w)).collect::<Vec<_>>().join(" "));
    Ok(())
}

fn shell_quote(word: &str) -> String {
    if !word.is_empty()
        && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./:=,@%+".contains(&b))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}
