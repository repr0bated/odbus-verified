//! Offline graph preparation. Never opens a live service or database.
mod inventory;

fn main() -> anyhow::Result<()> {
    inventory::run(std::env::args().skip(1).collect())
}
