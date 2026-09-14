use std::path::PathBuf;

use op_cozo_store::CozoGraphShuttle;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let db_path = PathBuf::from(
        args.next()
            .ok_or("usage: delete_identity_session DB SESSION")?,
    );
    let session_id = args
        .next()
        .ok_or("usage: delete_identity_session DB SESSION")?;
    if args.next().is_some() || session_id.is_empty() {
        return Err("usage: delete_identity_session DB SESSION".into());
    }

    let store = CozoGraphShuttle::new_persistent(db_path)?;
    let before = store.get_identity_sled(&session_id)?.is_some();
    store.delete_session(&session_id)?;
    let after = store.get_identity_sled(&session_id)?.is_some();
    println!("session_id={session_id} existed_before={before} exists_after={after}");
    Ok(())
}
