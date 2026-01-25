# Altair Lab API Service 

Microservice responsible for launching and stopping Kubernetes pods as well as allowing access to them via a websocket.
---

##  Features (PoC version)

- `POST /spawn`  
  Returns a mock container ID and a WebShell URL.

- `POST /spawn/stop`  
  Simulates stopping a lab session.

- `GET /spawn/status`
 
  Returns status of a pod by name. 

- `/health`  
  Simple liveness endpoint.

- `WEBSOCKET /spawn/webshell`

  Allows to connect to the webshell of a pod by name

## Environment Variables

### Local Development

When running locally, the service will use your default kubeconfig (`~/.kube/config`) to connect to Kubernetes.

### Cloud Run Deployment (GKE Connection)

When deploying to Cloud Run, set the following environment variables to connect to your GKE cluster:

| Variable               | Description                                                                                                                  |
|------------------------|------------------------------------------------------------------------------------------------------------------------------|
| `GKE_CLUSTER_ENDPOINT` | The GKE cluster API endpoint (e.g., `https://34.xxx.xxx.xxx`)                                                                |
| `GKE_CLUSTER_CA`       | Base64-encoded cluster CA certificate                                                                                        |
| `WEBSHELL_BASE_URL`    | Base URL for WebSocket connections (e.g., `ws://example.com:8080` or `wss://example.com`), defaults to `ws://localhost:8085` |
| `PORT`                 | (Optional) Server port, defaults to `8085`                                                                                   |

#### How to get GKE cluster credentials:

```bash
# Get the cluster endpoint
gcloud container clusters describe <CLUSTER_NAME> --zone <ZONE> --format="value(endpoint)"

# Get the CA certificate (base64-encoded)
gcloud container clusters describe <CLUSTER_NAME> --zone <ZONE> --format="value(masterAuth.clusterCaCertificate)"
```

## TODO:
  * Proper error handling for all the various problems that may arise
  * Authentication for the webshell
  * Pulling the proper image by a lab id (currently using basic debian)
