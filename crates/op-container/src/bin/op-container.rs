//! `op-container from-seed <name>`: turn the empty, stopped Incus container
//! `<name>` on 3tched-pool into a writable CoW snapshot of `images/@seed`.
//! Create the instance first: `incus create --empty <name> -s 3tched-pool`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.as_slice() {
        [cmd, name] if cmd == "from-seed" => {
            op_container::create_from_seed(&op_container::SeedLocation::default(), name)
        }
        _ => {
            eprintln!("usage: op-container from-seed <name>");
            std::process::exit(2);
        }
    };
    match result {
        Ok(done) => println!(
            "{} volume {} (tree {}) is a snapshot of seed {}",
            done.name, done.volume_uuid, done.volume_treeid, done.seed_uuid
        ),
        Err(err) => {
            eprintln!("op-container: {err:#}");
            std::process::exit(1);
        }
    }
}
