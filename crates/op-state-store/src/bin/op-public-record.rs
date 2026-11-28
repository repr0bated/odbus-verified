//! Public record and hash-chain entry, from the command line. Same commands,
//! flags, output and exit codes as subsidiary's `ui/scripts/record.ts`, which
//! this replaces for everything on the hash-chain side.
//!
//! ```text
//! build  --record R.json --classes C.json --bundle B.json --entry E.json
//!        Split a record into the public bundle (to publish with subsidiary's
//!        web3/script/publish-record.sh) and the private entry (full record and
//!        salts; it belongs on the hash chain only, written with mode 600).
//!        C.json maps each top-level field to its protection floor:
//!        "none" | "private" | "secret". Unlisted fields are secret.
//! verify --bundle B.json [--entry E.json]
//!        Check the bundle's proofs, and with the entry, that the bundle shows
//!        exactly what may be shown.
//! reveal --entry E.json --path FIELD
//!        Print one field with its salt and proof, to hand to someone who
//!        should see it. Off-chain.
//! link   --entry E.json --public P.json --event V.json [--seconds N] [--skew N]
//!        Tie the public ids (P.json, from publish-record.sh), the hash-chain
//!        event (V.json, one event from snowball.query_events) and its vector
//!        id inside one time bubble. Prints the link.
//! check-link --link L.json [--public P.json] [--event V.json] [--entry E.json]
//! ```
//!
//! Every command exits 0 only if everything checks out; problems go to stderr.
//! Exit 64 is a usage error, 66 a file that cannot be read or parsed.

use op_state_store::public_link::{build_link, check_link, link_text, Link, LinkedEventInput, PublicRef};
use op_state_store::public_record::{
    build_record, bundle_text, canonical_json, entry_text, reveal_field, verify_bundle, verify_entry,
    ProtectionFloor, PublicBundle, RecordEntry,
};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::process::exit;

struct Args(HashMap<String, String>);

fn usage(why: &str) -> ! {
    eprintln!("{why}\nsee `op-public-record` in crates/op-state-store/src/bin/op-public-record.rs for the commands");
    exit(64);
}

fn fail(why: impl std::fmt::Display) -> ! {
    eprintln!("FAIL  {why}");
    exit(1);
}

impl Args {
    fn parse(rest: &[String]) -> Self {
        let mut map = HashMap::new();
        let mut it = rest.iter();
        while let Some(flag) = it.next() {
            let Some(name) = flag.strip_prefix("--") else { usage(&format!("bad argument {flag}")) };
            let Some(value) = it.next() else { usage(&format!("bad argument {flag}")) };
            map.insert(name.to_owned(), value.clone());
        }
        Args(map)
    }

    fn need(&self, k: &str) -> &str {
        self.0.get(k).map(String::as_str).unwrap_or_else(|| usage(&format!("--{k} is required")))
    }

    fn read<T: DeserializeOwned>(&self, k: &str) -> T {
        let path = self.need(k);
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string()));
        parsed.unwrap_or_else(|e| {
            eprintln!("--{k}: {e}");
            exit(66);
        })
    }

    fn opt<T: DeserializeOwned>(&self, k: &str) -> Option<T> {
        self.0.contains_key(k).then(|| self.read(k))
    }

    fn int(&self, k: &str) -> Option<i64> {
        self.0.get(k).map(|v| v.parse().unwrap_or_else(|_| usage(&format!("--{k} must be a whole number"))))
    }
}

fn finish(problems: Vec<String>, ok: &str) -> ! {
    for p in &problems {
        eprintln!("FAIL  {p}");
    }
    if problems.is_empty() {
        eprintln!("PASS  {ok}");
        exit(0);
    }
    exit(1);
}

fn write_file(path: &str, text: &str, mode: u32) {
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    if let Err(e) = written {
        fail(format!("{path}: {e}"));
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = argv.split_first() else { usage("unknown command ") };
    let args = Args::parse(rest);
    match command.as_str() {
        "build" => {
            let record: serde_json::Value = args.read("record");
            let floors: HashMap<String, ProtectionFloor> = args.read("classes");
            let built = build_record(&record, &floors, &HashMap::new()).unwrap_or_else(|e| fail(e));
            let bundle = bundle_text(&built.bundle).unwrap_or_else(|e| fail(e));
            let entry = entry_text(&built.entry).unwrap_or_else(|e| fail(e));
            write_file(args.need("bundle"), &bundle, 0o644);
            write_file(args.need("entry"), &entry, 0o600);
            println!("{}", built.root);
            finish(
                verify_entry(&built.entry, Some(&built.bundle)),
                &format!(
                    "root {}: {} shown or redacted, {} withheld",
                    built.root,
                    built.bundle.fields.len(),
                    built.bundle.withheld
                ),
            );
        }
        "verify" => {
            let bundle: PublicBundle = args.read("bundle");
            let entry: Option<RecordEntry> = args.opt("entry");
            let mut problems = verify_bundle(&bundle);
            if let Some(e) = &entry {
                problems.extend(verify_entry(e, Some(&bundle)));
            }
            let and_entry = if entry.is_some() { " and entry" } else { "" };
            finish(problems, &format!("bundle {}{and_entry}", bundle.root));
        }
        "reveal" => {
            let entry: RecordEntry = args.read("entry");
            let reveal = reveal_field(&entry, args.need("path")).unwrap_or_else(|e| fail(e));
            let value = serde_json::to_value(&reveal).expect("reveal serializes");
            println!("{}", canonical_json(&value).unwrap_or_else(|e| fail(e)));
        }
        "link" => {
            let public: PublicRef = args.read("public");
            let event: LinkedEventInput = args.read("event");
            let entry: RecordEntry = args.read("entry");
            let link = build_link(&public, &event, &entry, args.int("seconds"), args.int("skew"))
                .unwrap_or_else(|e| fail(e));
            println!("{}", link_text(&link).unwrap_or_else(|e| fail(e)));
        }
        "check-link" => {
            let link: Link = args.read("link");
            let public: Option<PublicRef> = args.opt("public");
            let event: Option<LinkedEventInput> = args.opt("event");
            let entry: Option<RecordEntry> = args.opt("entry");
            let problems = check_link(&link, public.as_ref(), event.as_ref(), entry.as_ref());
            finish(problems, &format!("link for record {}", link.public.record_id));
        }
        other => usage(&format!("unknown command {other}")),
    }
}
