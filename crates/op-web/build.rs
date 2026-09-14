fn main() {
    // The current UI is native Rust/egui. Keep source changes visible to
    // Cargo, but do not require the retired Vite dist tree for server builds.
    println!("cargo:rerun-if-changed=ui/src");
}
