import docker
import time

NETWORK_NAME = "apps-internal"

def ensure_network(client):
    networks = client.networks.list(names=[NETWORK_NAME])
    if not networks:
        print(f"Creating network {NETWORK_NAME}")
        client.networks.create(NETWORK_NAME, driver="bridge")
    return client.networks.get(NETWORK_NAME)

def get_dns_name(container):
    labels = container.labels
    service = labels.get("com.docker.compose.service")
    project = labels.get("com.docker.compose.project")

    if service and project:
        return f"{service}.{project}.svc.cluster.local"
    else:
        return container.name


def connect_existing_containers(client, network):
    containers = client.containers.list()
    for container in containers:
        try:
            container_networks = container.attrs['NetworkSettings']['Networks']
            if NETWORK_NAME not in container_networks:
                network.connect(container, aliases=[get_dns_name(container)])
                print(f"✅ Connected existing container {container.name} to {NETWORK_NAME}")
        except Exception as e:
            print(f"⚠️ Could not connect {container.name}: {e}")

def watch_new_containers(client, network):
    print(f"Listening for new containers to attach to {NETWORK_NAME}...")
    for event in client.events(decode=True):
        if event.get("Type") == "container" and event.get("Action") == "start":
            container_id = event["id"]
            container = client.containers.get(container_id)
            try:
                container_networks = container.attrs['NetworkSettings']['Networks']
                if NETWORK_NAME not in container_networks:
                    network.connect(container, aliases=[get_dns_name(container)])
                    print(f"Connected new container {container.name} to {NETWORK_NAME}")
            except Exception as e:
                print(f"Could not connect {container.name}: {e}")

def main():
    client = docker.from_env()
    network = ensure_network(client)

    connect_existing_containers(client, network)

    while True:
        try:
            watch_new_containers(client, network)
        except Exception as e:
            print(f"Error: {e}")
            time.sleep(5)

if __name__ == "__main__":
    main()
