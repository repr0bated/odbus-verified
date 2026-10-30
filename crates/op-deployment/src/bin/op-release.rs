//! Root-owner base-system release controller. Application deployment is native
//! Btrfs snapshot/send/receive followed by read-only direct-target OverlayFS.
//! This binary does not import or administer the storage daemon or its guard.
mod release;

fn main() -> std::process::ExitCode {
    match release::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("op-release: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
