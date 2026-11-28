//! Shell completion from the mounted 3tchedFS namespace: `zcall <TAB>` lists commands and
//! plugins, `zcall <plugin> <TAB>` its methods, then that method's flags, then
//! each flag's closed values (enum members, true/false). The shell shims call
//! `zcall __complete <shell> <line-up-to-cursor>`.

use crate::args::CALL_OPTIONS;
use crate::catalog::{Catalog, closed_values, is_switch, loose, type_label};

pub const COMMANDS: [(&str, &str); 8] = [
    ("list", "sealed plugins"),
    ("methods", "methods of a plugin"),
    ("help", "a method's flags and declaration"),
    ("state", "a plugin's live state through 3tchedFS"),
    ("expand", "show the operation path and arguments only"),
    ("ask", "describe a call in words; JEV fills it in"),
    ("completion", "print the shell completion script"),
    ("paths", "the 3tchedFS operation path of a method"),
];

const GLOBAL: [(&str, &str); 4] = [
    ("--mount", "3tchedFS mount (default /run/mounts/3tchedFS)"),
    ("--identity-mode", "expected mount identity mode"),
    ("--help", "show help"),
    ("--version", "show version"),
];

const GLOBAL_WITH_VALUE: [&str; 2] = ["--mount", "--identity-mode"];
const IDENTITY_MODES: [&str; 4] = ["host_session", "guest_kernel", "credential", "container_bound"];

pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "bash" => Some(Self::Bash),
            "zsh" => Some(Self::Zsh),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }
}

type Candidates = Vec<(String, String)>;

/// Split the line before the cursor like a shell does (quotes, backslashes),
/// returning the finished words and the word being typed.
fn tokenize(line: &str) -> (Vec<String>, String) {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') | (None, '\\') => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
                in_word = true;
            }
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            (None, c) => {
                current.push(c);
                in_word = true;
            }
        }
    }
    (words, current)
}

pub fn run(shell: Shell, line: &str) {
    let (mut words, current) = tokenize(line);
    if !words.is_empty() {
        words.remove(0); // the program name
    }
    let candidates = candidates(&words, &current);
    let prefix_len = match shell {
        // bash replaces only the text after its last word-break character.
        Shell::Bash => current.rfind(['=', ':']).map(|at| at + 1).unwrap_or(0),
        _ => 0,
    };
    for (value, description) in candidates {
        if !value.starts_with(&current) {
            continue;
        }
        match shell {
            Shell::Bash => println!("{}", &value[prefix_len..]),
            Shell::Zsh => println!("{}:{}", value.replace(':', "\\:"), one_line(&description, 120)),
            Shell::Fish => println!("{value}\t{}", one_line(&description, 120)),
        }
    }
}

fn candidates(words: &[String], current: &str) -> Candidates {
    // Global options may appear before the command; remember the mount choice.
    let mut mount = std::env::var("ZCALL_MOUNT").ok();
    let mut identity_mode = None;
    let mut rest = Vec::new();
    let mut index = 0;
    while index < words.len() {
        let word = &words[index];
        if rest.is_empty() && GLOBAL_WITH_VALUE.contains(&word.as_str()) {
            match words.get(index + 1) {
                Some(value) => {
                    match word.as_str() {
                        "--mount" => mount = Some(value.clone()),
                        _ => identity_mode = Some(value.clone()),
                    }
                    index += 2;
                    continue;
                }
                None => return global_value(word),
            }
        }
        if rest.is_empty() && word.starts_with('-') {
            index += 1;
            continue;
        }
        rest.push(word.clone());
        index += 1;
    }
    if rest.is_empty() && current.starts_with('-') {
        return own(&GLOBAL);
    }
    // The mounted namespace is the catalog; without a readable mount only
    // zcall's own commands complete.
    let catalog = Catalog::open(mount.as_deref(), identity_mode.as_deref()).ok();
    let Some(first) = rest.first() else {
        let mut list = own(&COMMANDS);
        list.extend(catalog.as_ref().map(plugins).unwrap_or_default());
        return list;
    };
    let Some(catalog) = catalog.as_ref() else {
        return match first.as_str() {
            "completion" if rest.len() == 1 => own(&[("bash", ""), ("zsh", ""), ("fish", "")]),
            _ => vec![],
        };
    };
    let args = &rest[1..];
    match first.as_str() {
        "list" | "ask" => vec![],
        "completion" => match args.len() {
            0 => own(&[("bash", ""), ("zsh", ""), ("fish", "")]),
            _ => vec![],
        },
        "methods" | "state" => match args.len() {
            0 => plugins(catalog),
            _ => vec![],
        },
        "help" | "paths" => match args.len() {
            0 => plugins(catalog),
            1 => methods(catalog, &args[0]),
            _ => vec![],
        },
        "expand" => call(catalog, args, current),
        _ => call(catalog, &rest, current),
    }
}

fn global_value(option: &str) -> Candidates {
    match option {
        "--identity-mode" => IDENTITY_MODES.iter().map(|m| (m.to_string(), String::new())).collect(),
        // Paths: the shims fall back to filename completion on no candidates.
        _ => vec![],
    }
}

fn own(list: &[(&str, &str)]) -> Candidates {
    list.iter().map(|(v, d)| (v.to_string(), d.to_string())).collect()
}

fn plugins(catalog: &Catalog) -> Candidates {
    catalog.plugins().into_iter().map(|p| (p, "plugin".to_owned())).collect()
}

fn methods(catalog: &Catalog, typed: &str) -> Candidates {
    let Some(plugin) = catalog.resolve_plugin(typed) else {
        return vec![];
    };
    catalog
        .methods(&plugin)
        .into_values()
        .map(|method| {
            let summary = method.summary();
            (method.name, summary)
        })
        .collect()
}

/// `<plugin> <method> [flags...]`
fn call(catalog: &Catalog, words: &[String], current: &str) -> Candidates {
    match words.len() {
        0 => return plugins(catalog),
        1 => return methods(catalog, &words[0]),
        _ => {}
    }
    let Some(plugin) = catalog.resolve_plugin(&words[0]) else {
        return vec![];
    };
    let Some(method) = catalog.resolve_method(&plugin, &words[1]) else {
        return vec![];
    };
    let tokens = &words[2..];

    // `--flag=<TAB>`: values for that flag, keeping the `--flag=` prefix.
    if let Some((name, _)) = current.split_once('=').filter(|(n, _)| n.starts_with('-')) {
        return method
            .find_flag(name)
            .map(|flag| {
                closed_values(&flag.schema)
                    .into_iter()
                    .map(|value| (format!("{name}={value}"), String::new()))
                    .collect()
            })
            .unwrap_or_default();
    }

    // The previous word is a flag still waiting for its value.
    if let Some(last) = tokens.last() {
        if !last.contains('=') {
            if let Some(flag) = method.find_flag(last) {
                if !is_switch(&flag.schema) {
                    return closed_values(&flag.schema)
                        .into_iter()
                        .map(|value| (value, type_label(&flag.schema)))
                        .collect();
                }
            } else if last == "--json" {
                return vec![];
            }
        }
    }

    // Offer the flags not used yet (array flags may repeat), then zcall's own.
    let used: Vec<String> = tokens
        .iter()
        .filter(|t| t.starts_with('-'))
        .map(|t| loose(t.split('=').next().unwrap_or(t)))
        .collect();
    let mut list: Candidates = method
        .flags()
        .into_iter()
        .filter(|flag| {
            type_label(&flag.schema).ends_with("[]") || !used.contains(&loose(&flag.flag))
        })
        .map(|flag| {
            let mut description = type_label(&flag.schema);
            if flag.required {
                description.push_str(", required");
            }
            if let Some(text) = flag.schema["description"].as_str() {
                description.push_str(" — ");
                description.push_str(&one_line(text, 80));
            }
            (flag.flag, description)
        })
        .collect();
    let method_spellings: Vec<String> = list.iter().map(|(f, _)| f.clone()).collect();
    list.extend(
        CALL_OPTIONS
            .iter()
            .filter(|(option, _)| !method_spellings.iter().any(|f| f == option))
            .map(|(o, d)| (o.to_string(), d.to_string())),
    );
    list
}

/// Completion lines are line-oriented: fold whitespace and cap the length.
fn one_line(text: &str, max: usize) -> String {
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.chars().count() <= max {
        folded
    } else {
        format!("{}…", folded.chars().take(max).collect::<String>())
    }
}

pub fn script(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => BASH,
        Shell::Zsh => ZSH,
        Shell::Fish => FISH,
    }
}

const BASH: &str = r#"# zcall bash completion: eval "$(zcall completion bash)"
_zcall() {
    local IFS=$'\n'
    COMPREPLY=($(command zcall __complete bash "${COMP_LINE:0:COMP_POINT}" 2>/dev/null))
}
complete -o default -o nosort -F _zcall zcall
"#;

const ZSH: &str = r#"#compdef zcall
# zcall zsh completion: source <(zcall completion zsh)
_zcall() {
    local -a candidates
    candidates=("${(@f)$(command zcall __complete zsh "${BUFFER[1,CURSOR]}" 2>/dev/null)}")
    candidates=(${candidates:#})
    if (( ${#candidates} )); then
        _describe -V zcall candidates
    else
        _files
    fi
}
compdef _zcall zcall
"#;

const FISH: &str = r#"# zcall fish completion: zcall completion fish | source
complete -c zcall -f -a '(command zcall __complete fish (commandline -cp))'
"#;
