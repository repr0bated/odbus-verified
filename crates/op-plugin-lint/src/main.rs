//! CLI for op-plugin-lint.
//!
//! Primary goal — input a plugin `.rs`, emit a full contract-shaped plugin
//! document (fields + typed methods), including introspect gaps when given:
//! ```text
//! cd /path/to/upstream && repomix
//! op-plugin-lint \
//!   --input crates/op-plugins/src/state_plugins/zeroclaw.rs \
//!   --output /tmp/zeroclaw.complete.json \
//!   --format complete \
//!   --introspect /path/to/upstream/repomix-output.xml \
//!   --surface-out /tmp/zeroclaw.surface.json
//! ```

use anyhow::{bail, Context, Result};
use clap::{Parser, ValueEnum};
use op_plugin_lint::{
    audit_source, audit_source_with_coverage, complete_to_json, complete_to_markdown,
    emit_complete_plugin, emit_inspector_rust, resolve_introspect_target_with, CoverageInputs,
    IntrospectOpts,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    /// Lint findings only (markdown).
    Md,
    /// Lint findings only (JSON).
    Json,
    /// Full contract plugin document (fields + typed methods + audit + introspect gaps).
    Complete,
    /// New Rust source with Inspector Gadget/Repomix contract candidates appended.
    Rust,
}

#[derive(Parser)]
#[command(
    name = "op-plugin-lint",
    about = "From a plugin .rs (+ optional Repomix introspect), emit a complete contract-shaped plugin document"
)]
struct Cli {
    #[arg(long, conflicts_with = "input_dir")]
    input: Option<PathBuf>,

    #[arg(long, conflicts_with = "output_dir")]
    output: Option<PathBuf>,

    #[arg(long, conflicts_with = "input")]
    input_dir: Option<PathBuf>,

    #[arg(long, conflicts_with = "output")]
    output_dir: Option<PathBuf>,

    /// `complete` = full plugin JSON/MD (default). `md`/`json` = lint findings only.
    #[arg(long, value_enum, default_value = "complete")]
    format: Format,

    /// External discovery (prefer Repomix XML). Gaps are folded into `--format complete`.
    /// Alias: `--intospec`.
    #[arg(long = "introspect", visible_alias = "intospec")]
    introspect: Option<String>,

    #[arg(long)]
    ssh: Option<String>,

    #[arg(long, default_value_t = 2)]
    introspect_depth: usize,

    /// Write raw introspect surface JSON here.
    #[arg(long)]
    surface_out: Option<PathBuf>,

    #[arg(long)]
    registry: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool> {
    let cli = Cli::parse();
    let _ = cli.registry;

    let iopts = IntrospectOpts {
        max_depth: cli.introspect_depth,
        ssh: cli.ssh.clone(),
    };

    if cli.input.is_none() && cli.input_dir.is_none() {
        let Some(target) = &cli.introspect else {
            bail!(
                "need --input FILE --output FILE [--introspect REPOMIX]\n\
                 example:\n\
                   --input crates/op-plugins/src/state_plugins/zeroclaw.rs \\\n\
                   --output /tmp/zeroclaw.complete.json --format complete \\\n\
                   --introspect /home/jeremy/zeroclaw/repomix-output.xml"
            );
        };
        let cov = resolve_introspect_target_with(target, None, &iopts)?;
        let Some(surface) = &cov.binary_surface_json else {
            bail!("--introspect did not produce a surface");
        };
        let out = cli
            .surface_out
            .as_ref()
            .or(cli.output.as_ref())
            .context("pass --surface-out or --output")?;
        fs::write(out, surface).with_context(|| format!("write {}", out.display()))?;
        eprintln!("wrote surface {}", out.display());
        return Ok(true);
    }

    let coverage = match &cli.introspect {
        Some(target) => {
            let plugin_hint = cli.input.as_deref();
            let cov = resolve_introspect_target_with(target, plugin_hint, &iopts)?;
            if let (Some(path), Some(surface)) = (&cli.surface_out, &cov.binary_surface_json) {
                fs::write(path, surface).with_context(|| format!("write {}", path.display()))?;
                eprintln!("wrote surface {}", path.display());
            }
            cov
        }
        None => CoverageInputs::default(),
    };

    match (&cli.input, &cli.output, &cli.input_dir, &cli.output_dir) {
        (Some(input), Some(output), None, None) => emit_one(input, output, cli.format, &coverage),
        (None, None, Some(input_dir), Some(output_dir)) => {
            emit_dir(input_dir, output_dir, cli.format, &coverage)
        }
        _ => bail!("use --input/--output or --input-dir/--output-dir"),
    }
}

fn emit_one(
    input: &Path,
    output: &Path,
    format: Format,
    coverage: &CoverageInputs,
) -> Result<bool> {
    let source = fs::read_to_string(input).with_context(|| format!("read {}", input.display()))?;
    let name = input
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("plugin.rs");

    let extras: Vec<String> = coverage.extra_rust_sources.clone();
    let extra_refs: Vec<&str> = extras.iter().map(|s| s.as_str()).collect();

    match format {
        Format::Complete => {
            let doc = emit_complete_plugin(name, &source, coverage, &extra_refs)?;
            let body = if output
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            {
                complete_to_markdown(&doc)
            } else {
                complete_to_json(&doc)?
            };
            write_body(output, &body)?;
            let gaps = doc
                .introspect
                .as_ref()
                .map(|i| i.gaps.missing_from_plugin)
                .unwrap_or(0);
            eprintln!(
                "{}: complete plugin name={} fields={} methods={} introspect_gaps={}",
                input.display(),
                doc.plugin.name,
                doc.plugin.fields.len(),
                doc.plugin.methods.len(),
                gaps
            );
            Ok(doc.audit.ok)
        }
        Format::Rust => {
            let doc = emit_complete_plugin(name, &source, coverage, &extra_refs)?;
            let body = emit_inspector_rust(&source, &doc)?;
            write_body(output, &body)?;
            eprintln!(
                "{}: generated Rust candidate fields={} methods={} -> {}",
                input.display(),
                doc.introspect
                    .as_ref()
                    .map(|i| i.gaps.missing_config_fields.len())
                    .unwrap_or(0),
                doc.introspect
                    .as_ref()
                    .map(|i| i.gaps.missing_cli_commands.len())
                    .unwrap_or(0),
                output.display()
            );
            Ok(doc.audit.ok)
        }
        Format::Md | Format::Json => {
            let report = if coverage.instance_json.is_some()
                || coverage.sealed_schema_json.is_some()
                || coverage.binary_surface_json.is_some()
            {
                audit_source_with_coverage(name, &source, coverage)?
            } else {
                audit_source(name, &source)?
            };
            let body = match format {
                Format::Md => report.to_markdown(),
                Format::Json => report.to_json()?,
                Format::Complete | Format::Rust => unreachable!(),
            };
            write_body(output, &body)?;
            eprintln!(
                "{}: {}",
                input.display(),
                if report.ok() { "PASS" } else { "FAIL" }
            );
            Ok(report.ok())
        }
    }
}

fn emit_dir(
    input_dir: &Path,
    output_dir: &Path,
    format: Format,
    coverage: &CoverageInputs,
) -> Result<bool> {
    fs::create_dir_all(output_dir).with_context(|| format!("create {}", output_dir.display()))?;

    let mut entries: Vec<PathBuf> = fs::read_dir(input_dir)
        .with_context(|| format!("read {}", input_dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("rs"))
        .filter(|p| {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            !matches!(
                name,
                "mod.rs" | "lib.rs" | "plugin_scaffold_helpers.rs" | "schemars_adapter.rs"
            )
        })
        .collect();
    entries.sort();
    if entries.is_empty() {
        bail!("no .rs plugin files in {}", input_dir.display());
    }

    let mut all_ok = true;
    let mut summary = String::from(
        "# op-plugin-lint batch summary\n\n| File | Status | Fields | Methods | Gaps |\n|---|---|---|---|---|\n",
    );

    for path in &entries {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("plugin.rs");
        let ext = match format {
            Format::Md => "md",
            Format::Json => "json",
            Format::Complete => "complete.json",
            Format::Rust => "generated.rs",
        };
        let out_path = output_dir.join(format!("{name}.{ext}"));
        let ok = emit_one(path, &out_path, format, coverage)?;
        if !ok {
            all_ok = false;
        }
        summary.push_str(&format!(
            "| `{name}` | {} | — | — | — |\n",
            if ok { "PASS" } else { "FAIL" }
        ));
    }
    fs::write(output_dir.join("SUMMARY.md"), summary)?;
    Ok(all_ok)
}

fn write_body(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(path, body).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
