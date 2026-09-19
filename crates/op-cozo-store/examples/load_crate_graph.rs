//! Load the generated crate graph (`docs/crate-graph/crate-graph.cozo`) into a
//! `CozoGraphShuttle` and print a few structural queries as proof it landed.
//!
//! ```sh
//! scripts/crate-graph.sh
//! cargo run -p op-cozo-store --example load_crate_graph -- docs/crate-graph/crate-graph.cozo
//! cargo run -p op-cozo-store --example load_crate_graph -- docs/crate-graph/crate-graph.cozo /path/to/rocksdb-dir
//! ```
//!
//! With a second argument the graph is written to a persistent RocksDB store
//! (`CozoGraphShuttle::new_persistent`); otherwise it lives in memory for the
//! duration of the run. The script targets the seed relations `graph_node` and
//! `graph_edge`, so it also loads into any store the shuttle already manages.

use std::path::PathBuf;

use op_cozo_store::CozoGraphShuttle;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let script_path = PathBuf::from(
        args.next()
            .ok_or("usage: load_crate_graph SCRIPT.cozo [ROCKSDB_DIR]")?,
    );
    let store = match args.next() {
        Some(dir) => CozoGraphShuttle::new_persistent(PathBuf::from(dir))?,
        None => CozoGraphShuttle::new_in_memory()?,
    };

    let script = std::fs::read_to_string(&script_path)?;
    store.run_query(&script, None)?;

    let queries: &[(&str, &str)] = &[
        (
            "nodes by label",
            "?[label, count(label)] := *graph_node[_, label, _] :order -count(label)",
        ),
        (
            "edges by rel",
            "?[rel, count(rel)] := *graph_edge[_, rel, _, _] :order -count(rel)",
        ),
        (
            "crate fan-in (depends_on)",
            r#"?[dst, count(dst)] := *graph_edge[_, "depends_on", dst, _] :order -count(dst) :limit 12"#,
        ),
        (
            "crate fan-out (depends_on)",
            r#"?[src, count(src)] := *graph_edge[src, "depends_on", _, _] :order -count(src) :limit 12"#,
        ),
        (
            "module-level imports not backed by a Cargo dependency",
            r#"
            dep[a, b] := *graph_edge[a, "depends_on", b, _]
            dep[a, b] := *graph_edge[a, "dev_depends_on", b, _]
            dep[a, b] := *graph_edge[a, "build_depends_on", b, _]
            ?[module, target] := *graph_edge[module, "imports", target, _],
                                 *graph_edge[owner, "contains", module, _],
                                 not dep[owner, target]
            :limit 20
            "#,
        ),
        (
            "plugin registrations whose module is never declared (phantoms)",
            r#"?[id, file] := *graph_node[id, "plugin", p], j = parse_json(p), get(j, "declared") == false, file = get(j, "file")"#,
        ),
        (
            "runit service -> binary -> crate",
            r#"?[svc, bin, crate] := *graph_edge[svc, "runs_bin", bin, _], *graph_edge[crate, "provides_bin", bin, _]"#,
        ),
        (
            "transitive closure size of op-core (reverse depends_on)",
            r#"
            up[c] := c = "crate:op-core"
            up[c] := up[d], *graph_edge[c, "depends_on", d, _]
            ?[count(c)] := up[c]
            "#,
        ),
    ];

    for (title, q) in queries {
        let rows = store.run_query(q, None)?;
        println!("== {title}");
        println!("{}", serde_json::to_string_pretty(&rows)?);
    }
    Ok(())
}
