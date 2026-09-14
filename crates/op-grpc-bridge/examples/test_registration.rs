//! Throwaway tool to exercise the RegistrationService bootstrap path
//! (SendMagicLink) directly, to confirm the registration/pairing flow works
//! end-to-end independent of the host's shared identity session.
use op_grpc_bridge::proto::registration::registration_service_client::RegistrationServiceClient;
use op_grpc_bridge::proto::registration::SendMagicLinkRequest;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let email = std::env::args()
        .nth(1)
        .expect("usage: test_registration <email> [domain]");
    let domain = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "3tched.com".to_string());

    let mut client = RegistrationServiceClient::connect("http://127.0.0.1:8090").await?;
    let response = client
        .send_magic_link(SendMagicLinkRequest {
            email,
            domain,
            is_admin: false,
            custom_message: None,
        })
        .await?;

    println!("{:#?}", response.into_inner());
    Ok(())
}
