//! Runit service-manager gRPC client.

use std::env;

use op_services::grpc::proto::service_manager_client::ServiceManagerClient;
use op_services::grpc::proto::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let mut client = ServiceManagerClient::connect("http://[::1]:50053").await?;

    match args[1].as_str() {
        "start" => {
            let name = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing service name"))?;
            let resp = client.start(StartRequest { name: name.clone() }).await?;
            println!("Started {}", name);
            if let Some(status) = resp.into_inner().status {
                println!(
                    "State: {:?}",
                    ServiceState::try_from(status.state).unwrap_or(ServiceState::StateStopped)
                );
            }
        }
        "stop" => {
            let name = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing service name"))?;
            let _resp = client.stop(StopRequest { name: name.clone() }).await?;
            println!("Stopped {}", name);
        }
        "restart" => {
            let name = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing service name"))?;
            client
                .restart(RestartRequest { name: name.clone() })
                .await?;
            println!("Restarted {}", name);
        }
        "status" => {
            let name = args
                .get(2)
                .ok_or_else(|| anyhow::anyhow!("missing service name"))?;
            let resp = client.get(GetRequest { name: name.clone() }).await?;
            if let Some(status) = resp.into_inner().status {
                let state =
                    ServiceState::try_from(status.state).unwrap_or(ServiceState::StateStopped);
                println!("● {} - {:?}", name, state);
                if let Some(pid) = status.pid {
                    println!("  PID: {}", pid);
                }
                if let Some(err) = status.error {
                    println!("  Error: {}", err);
                }
            }
        }
        "list-units" | "list" => {
            let resp = client.list(ListRequest { filter: None }).await?;
            for svc in resp.into_inner().services {
                println!("{}", svc.name);
            }
        }
        _ => print_usage(),
    }

    Ok(())
}

fn print_usage() {
    eprintln!("Usage: op-svctl <command> [service]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  start <service>    Start a service");
    eprintln!("  stop <service>     Stop a service");
    eprintln!("  restart <service>  Restart a service");
    eprintln!("  status <service>   Show service status");
    eprintln!("  list-units         List all services");
}
