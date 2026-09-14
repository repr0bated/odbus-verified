use op_web::users::UserStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let email = std::env::args().nth(1).expect("usage: check_user <email>");
    let store = UserStore::new("/var/lib/op-dbus/users-cozo").await?;
    match store.get_user_by_email(&email).await {
        Some(u) => println!("{:#?}", u),
        None => println!("No user found for {email}"),
    }
    Ok(())
}
