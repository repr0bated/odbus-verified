//! Emit the barebones taxonomy picker spec (draft rows from the OSCAL table) to stdout.
//! Run: cargo run --example taxonomy_picker > local/taxonomy-picker.spec.json

use catalog_routing::catalogs::specgen::taxonomy_spec;

fn main() {
    let spec = taxonomy_spec(
        "Taxonomy picker",
        "Tag → control → sealed operation · draft picker",
    );
    println!("{}", serde_json::to_string_pretty(&spec).unwrap());
}
