use bollard::container::ListContainersOptions;
use bollard::models::{EndpointSettings, EventMessage, EventMessageTypeEnum};
use bollard::network::{ConnectNetworkOptions, CreateNetworkOptions, ListNetworksOptions};
use bollard::system::EventsOptions;
use bollard::Docker;
use futures_util::stream::StreamExt;
use std::collections::HashMap;
use tokio::signal;

const NETWORK_NAME: &str = "apps-internal";

async fn ensure_network(docker: &Docker) {
    let filters = HashMap::from([("name", vec![NETWORK_NAME])]);
    let networks = docker
        .list_networks(Some(ListNetworksOptions { filters }))
        .await
        .unwrap();

    if networks.is_empty() {
        println!("Network '{}' not found, creating...", NETWORK_NAME);

        let create_opts = CreateNetworkOptions {
            name: NETWORK_NAME,
            check_duplicate: true,
            driver: "bridge",
            ..Default::default()
        };

        match docker.create_network(create_opts).await {
            Ok(info) => println!("Created network: {:?}", info),
            Err(err) => eprintln!("Error creating network: {}", err),
        }
    } else {
        println!("Network '{}' already exists.", NETWORK_NAME);
    }
}

async fn connect_container_to_network(docker: &Docker, container_id: &str) {
    let mut endpoint_config = EndpointSettings::default();

    let info = docker.inspect_container(&container_id, None).await.unwrap();
    let labels = info.config.and_then(|cfg| cfg.labels).unwrap_or_default();

    let project_name = labels.get("com.docker.compose.project").cloned();
    let service_name = labels.get("com.docker.compose.service").cloned();

    if let (Some(service), Some(project)) = (service_name, project_name) {
        let alias = format!("{}.{}.svc.cluster.local", service, project);
        endpoint_config.aliases = Some(vec![alias]);
        println!(
            "[Watcher] Will connect {} with alias {}",
            container_id,
            endpoint_config.aliases.as_ref().unwrap()[0]
        );
    } else {
        println!(
            "[Watcher] Will connect {} without alias (missing labels)",
            container_id
        );
    }

    let connect_opts = ConnectNetworkOptions {
        container: container_id.to_string(),
        endpoint_config: endpoint_config,
    };

    match docker.connect_network(NETWORK_NAME, connect_opts).await {
        Ok(_) => println!(
            "[Watcher] Connected container {} to network {}",
            container_id, NETWORK_NAME
        ),
        Err(e) => eprintln!(
            "[Error] Failed to connect container {}: {:?}",
            container_id, e
        ),
    }
}

async fn connect_existing_containers(docker: &Docker) {
    let filters: HashMap<String, Vec<String>> = HashMap::new();
    let containers = docker
        .list_containers(Some(ListContainersOptions {
            all: true,
            filters,
            ..Default::default()
        }))
        .await
        .unwrap();

    println!("[Watcher] Checking existing containers...");

    for container in containers {
        if let Some(id) = container.id {
            let container_info = docker.inspect_container(&id, None).await.unwrap();
            let networks = container_info.network_settings.unwrap().networks.unwrap();

            if !networks.contains_key(NETWORK_NAME) {
                println!("[Watcher] Connecting pre-existing container: {}", id);
                connect_container_to_network(docker, &id).await;
            } else {
                println!(
                    "[Watcher] Container {} already on network {}",
                    id, NETWORK_NAME
                );
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let docker = Docker::connect_with_socket_defaults().expect("Failed to connect to Docker");

    ensure_network(&docker).await;

    let filters = HashMap::new();
    let mut events = docker.events(Some(EventsOptions::<String> {
        since: None,
        until: None,
        filters,
    }));

    tokio::spawn({
        let docker = docker.clone();
        async move {
            connect_existing_containers(&docker).await;
        }
    });

    println!("[Watcher] Listening for container events...");

    let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
        .expect("Failed to register SIGTERM handler");
    let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
        .expect("Failed to register SIGINT handler");

    loop {
        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(EventMessage { typ, action, actor, .. })) => {
                        if let (Some(t), Some(a)) = (typ, action) {
                            if t == EventMessageTypeEnum::CONTAINER && a == "start" {
                                if let Some(id) = actor.and_then(|act| act.id) {
                                    println!("[Watcher] New container started: {}", id);
                                    connect_container_to_network(&docker, &id).await;
                                }
                            }
                        }
                    }
                    Some(Err(e)) => eprintln!("[Error] Event stream error: {}", e),
                    None => {
                        println!("[Watcher] Docker event stream closed.");
                        break;
                    }
                }
            }

            _ = sigterm.recv() => {
                println!("[Watcher] Received SIGTERM, shutting down gracefully...");
                break;
            }

            _ = sigint.recv() => {
                println!("[Watcher] Received SIGINT (Ctrl+C), shutting down...");
                break;
            }
        }
    }
}
