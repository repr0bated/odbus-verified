use op_web::users::UserStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let email = std::env::args()
        .nth(1)
        .expect("usage: check_and_delete_user <email>");
    let store = UserStore::new("/var/lib/op-dbus/users-cozo").await?;
    let user = store.get_user_by_email(&email).await;
    match user {
        Some(u) => {
            println!(
                "Found user: id={} email={} verified={} container={:?} route_id={:?} connected={}",
                u.id,
                u.email,
                u.email_verified,
                u.privacy_container_name,
                u.privacy_route_id,
                u.privacy_network_connected
            );
            if u.privacy_container_name.is_some() || u.privacy_network_connected {
                println!("REFUSING: user has a provisioned container/route — needs proper storage detach first, not a bare delete.");
                return Ok(());
            }
            store.delete_user(&u.id).await?;
            println!("Deleted user {} ({})", u.id, u.email);
        }
        None => println!("No user found for {email}"),
    }
    Ok(())
}
