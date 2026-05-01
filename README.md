# Altaïr Lab API Service

> **kubernetes orchestrator for ephemeral lab environments with interactive WebShell access**
> 

[![Cloud Run](https://img.shields.io/badge/deploy-Cloud%20Run-blue)](https://cloud.google.com/run)

[![GKE](https://img.shields.io/badge/kubernetes-GKE-326CE5)](https://cloud.google.com/kubernetes-engine)

[![Rust](https://img.shields.io/badge/rust-nightly-orange)](https://www.rust-lang.org)

---

## Description

The **Altaïr Lab API Service** is a stateless runtime orchestrator that provisions ephemeral Kubernetes pods for hands-on cybersecurity lab environments. It creates isolated containers on-demand, manages their lifecycle, and exposes interactive browser-based terminals via WebSocket.

This service acts as the **bridge between the Altaïr platform and GKE**, handling pod creation, container registry authentication, readiness verification, and WebSocket-based shell access.

**Key capabilities:**

- Create isolated Kubernetes pods on-demand for lab sessions
- Provision private container registry credentials (GCR/Artifact Registry)
- Wait for pod readiness with timeout handling
- Expose interactive WebShell via `kubectl exec` over WebSocket
- Manage pod lifecycle (status checks, deletion)
- Auto-cleanup after 2 hours via `activeDeadlineSeconds`

---

## ⚠️ Security Notice

**This service is currently in PoC stage and has critical security limitations:**

- ❌ **No authentication** on WebShell endpoint – anyone with the URL can access the terminal
- ❌ **No authorization** – any caller can spawn/stop any pod
- ❌ **Secret accumulation** – ImagePullSecrets are never cleaned up
- ❌ **Panic on missing resources** – service crashes instead of returning HTTP errors

**Deployment requirement:** Must be deployed behind an authenticated API Gateway. Do NOT expose directly to the internet.

---

## Architecture

```
┌─────────────┐      ┌──────────────────┐      ┌─────────────┐
│  Frontend   │      │   API Gateway    │      │ Lab API     │
│             │─────▶│  (validates JWT) │─────▶│ Service     │
│ (Browser)   │      │                  │      │ (Cloud Run) │
└─────────────┘      └──────────────────┘      └──────┬──────┘
                                                       │
                                                       ▼
                                              ┌────────────────┐
                                              │  GKE Cluster   │
                                              ├────────────────┤
                                              │  ┌──────────┐  │
                                              │  │ Lab Pod  │  │
                                              │  │ :student │  │
                                              │  └──────────┘  │
                                              │                │
                                              │  ┌──────────┐  │
                                              │  │ Lab Pod  │  │
                                              │  │ :student │  │
                                              │  └──────────┘  │
                                              └────────────────┘
                                                       ▲
                                                       │
┌─────────────┐      WebSocket /spawn/webshell        │
│  Browser    │──────────────────────────────────────┘
│  Terminal   │           (kubectl exec bridge)
└─────────────┘
```

### Service Flow

1. **Gateway** receives authenticated request from Frontend
2. **Lab API** creates ImagePullSecret for private registry
3. **Lab API** creates Pod in GKE with resource limits
4. **Lab API** waits for Pod readiness (30s timeout)
5. **Frontend** opens WebSocket to `/spawn/webshell/{pod_name}`
6. **Lab API** bridges WebSocket ↔ `kubectl exec` for interactive shell

---

## Tech Stack

| Component | Technology | Purpose |
| --- | --- | --- |
| **Language** | Rust (nightly) | High-performance async runtime |
| **HTTP Framework** | Axum | HTTP + WebSocket support |
| **Async Runtime** | Tokio | Async I/O and concurrency |
| **Kubernetes Client** | kube-rs | GKE API interaction |
| **Cloud Auth** | gcp_auth | Application Default Credentials |
| **CI/CD** | GitHub Actions | fmt, clippy, tests, release build |
| **Deployment** | Google Cloud Run | Serverless auto-scaling |
| **Orchestration** | Google Kubernetes Engine | Pod runtime environment |

---

## Requirements

### Development

- **Rust** nightly toolchain
- **kubectl** configured with access to a Kubernetes cluster
- **Local Kubernetes cluster** (Minikube, Kind, or GKE access)
- **Docker** (for building lab images)

### Production (Cloud Run)

- **GKE Cluster** with API access from Cloud Run
- **Service Account** with Kubernetes Engine Developer role
- **Environment Variables** (see Configuration section)

### Environment Variables

#### Local Development

```bash
# No configuration needed - uses ~/.kube/config automatically
RUST_LOG=info  # Optional: logging level
PORT=8085      # Optional: server port (default: 8085)
```

#### Cloud Run Deployment

```bash
# GKE Connection (required)
GKE_CLUSTER_ENDPOINT=https://34.xxx.xxx.xxx  # GKE API endpoint
GKE_CLUSTER_CA=LS0tLS1CRUdJTi...              # Base64-encoded cluster CA cert

# WebShell Configuration
WEBSHELL_BASE_URL=wss://labs-api.altair.io    # WebSocket base URL

# Server Configuration
PORT=8085                                      # Server port (default: 8085)
RUST_LOG=info                                  # Log level filter
```

#### How to Get GKE Credentials

```bash
# Get the cluster endpoint
gcloud container clusters describe <CLUSTER_NAME> \
  --zone <ZONE> \
  --format="value(endpoint)"

# Get the CA certificate (base64-encoded)
gcloud container clusters describe <CLUSTER_NAME> \
  --zone <ZONE> \
  --format="value(masterAuth.clusterCaCertificate)"
```

---

## Installation

### Local Development

```bash
# 1. Ensure kubectl is configured
kubectl cluster-info

# 2. Run the service
cargo run

# 3. Test the health endpoint
curl http://localhost:8085/health
```

### Building Docker Image

```bash
# Build the container
docker build -t altair-lab-api:latest .

# Run locally (requires kubeconfig volume mount)
docker run --rm -it \
  -p 8085:8085 \
  -v ~/.kube/config:/root/.kube/config:ro \
  altair-lab-api:latest
```

### Deploying to Cloud Run

```bash
# 1. Build and push to Artifact Registry
gcloud builds submit --tag europe-west9-docker.pkg.dev/PROJECT/altair/lab-api

# 2. Deploy to Cloud Run with GKE connection
gcloud run deploy altair-lab-api \
  --image europe-west9-docker.pkg.dev/PROJECT/altair/lab-api \
  --region europe-west9 \
  --platform managed \
  --allow-unauthenticated \
  --set-env-vars GKE_CLUSTER_ENDPOINT=https://34.xxx.xxx.xxx \
  --set-env-vars GKE_CLUSTER_CA=LS0tLS1... \
  --set-env-vars WEBSHELL_BASE_URL=wss://labs-api.altair.io \
  --service-account lab-api@PROJECT.iam.gserviceaccount.com
```

---

## Usage

### API Endpoints

#### **GET /health**

Health check for liveness/readiness probes.

**Response:**

```json
"OK"
```

---

#### **POST /spawn**

Create a new lab pod and return WebSocket shell access.

**Request:**

```json
{
  "session_id": "550e8400-e29b-41d4-a716-446655440000",
  "lab_type": "ctf_terminal_guided",
  "template_path": "europe-west9-docker.pkg.dev/project/altair/labs/intro-linux:v1"
}
```

**Validation:**

- `lab_type` must be exactly `"ctf_terminal_guided"` (returns `400 Bad Request` otherwise)
- `session_id` must be a valid UUID
- `template_path` must be a valid container image reference

**Processing Flow:**

1. Generate pod name: `ctf-session-{session_id}`
2. Generate secret name: `gcr-secret-{session_id}`
3. **Create ImagePullSecret:**
    - Fetch GCP access token with `cloud-platform` scope
    - Extract registry from first segment of `template_path`
    - Build `dockerconfigjson` credential
    - Delete old secret if exists
    - Create new secret in `default` namespace
4. **Create Pod** with resource limits and `activeDeadlineSeconds: 7200`
5. **Wait for readiness** (30s timeout):
    - Watch events with field selector [`metadata.name](http://metadata.name)=...`
    - Success: `phase == Running` AND all containers `ready == true`
    - Failure: `phase == Failed` OR container terminated with exit_code ≠ 0
6. Return response with WebShell URL

**Response (Success):**

```json
{
  "pod_name": "ctf-session-550e8400-e29b-41d4-a716-446655440000",
  "webshell_url": "wss://labs-api.altair.io/spawn/webshell/ctf-session-550e8400-e29b-41d4-a716-446655440000",
  "status": "RUNNING"
}
```

**Response (Failure - Invalid Lab Type):**

```json
{
  "error": "Invalid lab_type. Only 'ctf_terminal_guided' is supported."
}
```

**Response (Failure - Pod Failed):**

```json
{
  "error": "Pod failed to start",
  "details": "Container exited with code 1"
}
```

**Timeout:** 30 seconds for pod readiness. If exceeded, returns timeout error.

---

#### **POST /spawn/stop**

Delete a lab pod.

**Request:**

```json
{
  "container_id": "ctf-session-550e8400-e29b-41d4-a716-446655440000"
}
```

**Note:** Despite the name `container_id`, this field expects a pod name.

**Response:**

```json
{
  "status": "Stopped"
}
```

**⚠️ Known Issue:** Always returns success even if deletion fails. Does NOT clean up associated `gcr-secret-*` secret.

---

#### **GET /spawn/status/:container_id**

Check the status of a pod.

**Path parameter:**

- `container_id` – Pod name (e.g., `ctf-session-550e8400-...`)

**Response:**

```json
{
  "status": "Running"
}
```

**Possible statuses:** `Pending`, `Running`, `Succeeded`, `Failed`, `Unknown`

**⚠️ Critical Issue:** Uses `.expect()` which **crashes the entire service** if the pod doesn't exist, instead of returning a proper `404 Not Found`.

---

#### **GET /spawn/webshell/:pod_name** (WebSocket)

Open an interactive shell in a running pod.

**Protocol:** WebSocket upgrade

**Path parameter:**

- `pod_name` – Name of the running pod

**Shell Command Executed:**

```bash
/bin/bash -lc "exec su - student"
```

**Container Requirements:**

- `/bin/bash` must exist
- User `student` must exist and be configured
- `su` command must be available

**WebSocket Message Format:**

- **Client → Server:** Binary frames containing terminal input (keystrokes, commands)
- **Server → Client:** Binary frames containing terminal output (stdout)

**Connection Flow:**

1. Client opens WebSocket to `/spawn/webshell/{pod_name}`
2. Service performs `kubectl exec` equivalent with `AttachParams{stdin: true, stdout: true, stderr: false, tty: true}`
3. Bidirectional relay between WebSocket and pod stdin/stdout
4. Connection closes when shell exits or client disconnects

**Example (JavaScript):**

```jsx
const ws = new WebSocket('wss://labs-api.altair.io/spawn/webshell/ctf-session-123');
ws.binaryType = 'arraybuffer';

ws.onmessage = (event) => {
  const output = new TextDecoder().decode(event.data);
  terminal.write(output);
};

ws.send(new TextEncoder().encode('ls -la\n'));
```

**⚠️ Security Warning:** No authentication or authorization checks. Anyone with the WebSocket URL can access the shell.

---

## Pod Specification

### Generated Pod Configuration

**Namespace:** `default`

**Metadata:**

- **Name:** `ctf-session-{session_id}`
- **Labels:**
    - `app=altair-lab`
    - `session_id={uuid}`
    - `lab_type=ctf_terminal_guided`

**Spec:**

- **`imagePullSecrets`**: `[{name: gcr-secret-{session_id}}]`
- **`restartPolicy`**: `Never`
- **`activeDeadlineSeconds`**: `7200` (2 hours – pod auto-deletes after this)

**Container:** `lab-container`

| Setting | Value | Purpose |
| --- | --- | --- |
| **Image** | User-provided `template_path` | Lab environment image |
| **Resources (Requests)** | `256Mi` memory, `250m` CPU | Minimum guaranteed resources |
| **Resources (Limits)** | `512Mi` memory, `500m` CPU | Maximum allowed resources |
| **Volume Mount** | `/var/log/altair` (emptyDir) | Ephemeral log storage |

**Volumes:**

- `emptyDir` mounted at `/var/log/altair` (deleted with pod)

---

## ImagePullSecret Generation

The service automatically creates Kubernetes secrets to pull images from private Google Container Registry / Artifact Registry.

### Secret Structure

**Name:** `gcr-secret-{session_id}`  

**Type:** [`kubernetes.io/dockerconfigjson`](http://kubernetes.io/dockerconfigjson)

**Contents:**

```json
{
  "auths": {
    "europe-west9-docker.pkg.dev": {
      "username": "oauth2accesstoken",
      "password": "<GCP_ACCESS_TOKEN>",
      "auth": "<base64(oauth2accesstoken:<token>)>"
    }
  }
}
```

### Registry Extraction Logic

The registry is extracted from the first segment of `template_path`:

- [`europe-west9-docker.pkg.dev/project/repo/image:tag`](http://europe-west9-docker.pkg.dev/project/repo/image:tag) → [`europe-west9-docker.pkg.dev`](http://europe-west9-docker.pkg.dev)
- [`gcr.io/project/image:tag`](http://gcr.io/project/image:tag) → [`gcr.io`](http://gcr.io)

### Secret Lifecycle

1. **Creation:** New secret created before each pod spawn
2. **Reuse:** If secret already exists, it is deleted and recreated with fresh token
3. **Deletion:** ❌ **NOT IMPLEMENTED** – secrets accumulate indefinitely

**⚠️ Known Issue:** Secrets are never cleaned up, leading to resource accumulation.

---

## Project Structure

```
altair-lab-api-service/
├── Cargo.toml                    # Rust dependencies
├── Cargo.lock                    # Locked dependency versions
├── Dockerfile                    # Multi-stage build
├── README.md                     # This file
├── lab-api-tests.http            # HTTP test requests
├── .github/
│   └── workflows/
│       └── ci.yml               # CI pipeline
└── src/
    ├── main.rs                  # Server bootstrap, CORS, routes
    ├── models/
    │   ├── state.rs            # AppState (kube_client, token_provider)
    │   └── spawn.rs            # Request/response DTOs
    ├── routes/
    │   ├── mod.rs              # Route declarations
    │   ├── health.rs           # Health check endpoint
    │   ├── spawn.rs            # Spawn/stop/status handlers
    │   └── web_shell.rs        # WebSocket handler
    ├── services/
    │   ├── spawn.rs            # Pod creation, readiness, deletion
    │   └── web_shell.rs        # WebSocket ↔ kubectl exec bridge
    └── tests/
        └── *.rs                # Unit tests
```

---

## Deployment (Cloud Run)

The service is containerized and deployed to **Google Cloud Run** with GKE connectivity.

### Container Configuration

- Listens on port defined by `PORT` environment variable (default: `8085`)
- Multi-stage Docker build (Rust builder → Debian slim runtime)
- Stateless design enables auto-scaling

### Service Account Permissions

The Cloud Run service account requires:

- **`roles/container.developer`** – Full GKE API access
- **`roles/artifactregistry.reader`** – Pull private images
- Permissions to:
    - Create/delete Secrets in `default` namespace
    - Create/delete Pods in `default` namespace
    - Execute commands in Pods (exec/attach)

### Networking Requirements

- **VPC Connector** or **Private Google Access** for GKE API connectivity
- **WebSocket support** – Load balancer must allow WebSocket upgrades
- **Private deployment** – Must be behind authenticated API Gateway

### Scaling Behavior

- **Min instances:** 0 (scales to zero when idle)
- **Max instances:** Configurable (default: 100)
- **Cold start time:** ~2-5 seconds (Rust fast startup)
- **Concurrency:** 80 requests per instance (default)

---

## Known Issues & Limitations

### 🔴 Critical Issues

- **No authentication/authorization** – WebShell is publicly accessible
- **Service crashes on missing pods** – `GET /spawn/status/:id` panics instead of returning 404
- **Secret accumulation** – ImagePullSecrets never cleaned up
- **Stop endpoint always succeeds** – Returns success even if deletion fails

### 🟡 Security Concerns

- **No JWT validation** – Service trusts upstream Gateway
- **Hardcoded namespace** – All pods in `default` (no multi-tenancy)
- **WebShell command hardcoded** – Assumes `/bin/bash` and `student` user exist
- **No rate limiting** – Vulnerable to resource exhaustion attacks

### 🟡 Operational Limitations

- **30-second readiness timeout** – May fail for large images (>500MB)
- **No retry logic** – Network transients cause immediate failures
- **Hardcoded resource limits** – Cannot customize per lab type
- **2-hour hard limit** – Pods auto-terminate via `activeDeadlineSeconds: 7200`
- **Single lab type** – Only `ctf_terminal_guided` supported
- **No pod lifecycle events** – Cannot track pod failures after spawn

### 🟡 Missing Features

- **No metrics collection** – Cannot monitor spawn success rate
- **No structured logging** – Difficult to debug production issues
- **No health checks for pods** – Readiness only checks Kubernetes status
- **No graceful shutdown** – May interrupt active WebSocket connections

---

## CI/CD Pipeline

### GitHub Actions Workflow (`.github/workflows/ci.yml`)

1. **Format Check** – `cargo fmt --check`
2. **Linting** – `cargo clippy -- -D warnings` (warnings as errors)
3. **Tests** – `cargo test`
4. **Release Build** – `cargo build --release`

### Test Coverage

- **`spawn_[lab.rs](http://lab.rs)`** – Pod construction logic, readiness/failure detection
- [**`websocket.rs`**](http://websocket.rs) – AttachParams validation, buffer sizing
- **`health_[check.rs](http://check.rs)`** – Health endpoint verification

---

## Troubleshooting

### Service Won't Start

**Symptom:** Service fails to initialize.

**Solution:**

```bash
# Check GKE credentials are set
echo $GKE_CLUSTER_ENDPOINT
echo $GKE_CLUSTER_CA

# Verify service account has GKE access
gcloud projects get-iam-policy PROJECT_ID \
  --flatten="bindings[].members" \
  --filter="bindings.members:serviceAccount:lab-api@*"
```

### Pods Stuck in Pending

**Symptom:** `POST /spawn` times out after 30 seconds.

**Possible causes:**

- Image pull failures (check ImagePullSecrets)
- Insufficient cluster resources (check `kubectl describe pod`)
- Network issues pulling from registry

**Debug:**

```bash
# Check pod events
kubectl describe pod ctf-session-<uuid>

# Check secret exists
kubectl get secret gcr-secret-<uuid> -o yaml

# Manually pull image
docker pull <template_path>
```

### WebShell Connection Fails

**Symptom:** WebSocket closes immediately after connecting.

**Possible causes:**

- Pod not fully ready (container still starting)
- `/bin/bash` or `student` user missing from container
- Network timeout (check Cloud Run → GKE connectivity)

**Debug:**

```bash
# Check pod is running
kubectl get pod ctf-session-<uuid>

# Test exec manually
kubectl exec -it ctf-session-<uuid> -- /bin/bash -lc "exec su - student"
```

### Secret Accumulation

**Symptom:** Namespace cluttered with `gcr-secret-*` secrets.

**Workaround:**

```bash
# Clean up old secrets (manual)
kubectl delete secret -l app=altair-lab --field-selector='metadata.creationTimestamp<2024-01-01'
```

**Permanent fix:** Implement secret cleanup in `stop` endpoint (see TODO list).

---

## TODO / Roadmap

### High Priority (PoC → MVP)

- [ ]  **Add authentication to WebShell endpoint** (validate JWT tokens)
- [ ]  **Fix panic on missing resources** (return proper 404 errors)
- [ ]  **Implement secret cleanup** (delete `gcr-secret-*` on pod deletion)
- [ ]  **Add structured error responses** (consistent JSON error format)

### Medium Priority (MVP → Production)

- [ ]  **Support multiple lab types** (beyond `ctf_terminal_guided`)
- [ ]  **Dynamic resource allocation** (configure CPU/memory per lab)
- [ ]  **Increase readiness timeout** (configurable, with progress feedback)
- [ ]  **Add retry logic** (handle transient network failures)
- [ ]  **Implement metrics collection** (Prometheus/Grafana integration)

### Low Priority (Future Enhancements)

- [ ]  **Multi-namespace support** (tenant isolation)
- [ ]  **Custom shell commands** (configurable per lab type)
- [ ]  **Pod lifecycle webhooks** (notify Sessions MS on failures)
- [ ]  **Graceful shutdown** (drain active WebSocket connections)
- [ ]  **Rate limiting** (per-user spawn quotas)

---

## Project Status

**✅ Current Status: MVP (Minimum Viable Product)**

This service is **functional for MVP deployment** with core pod orchestration and WebShell capabilities operational. Security hardening and operational improvements remain before production-ready status.

**Known limitations to address for production:**

1. Authentication/authorization on WebShell endpoint
2. Panic-inducing error handling (status endpoint)
3. ImagePullSecret cleanup implementation
4. Comprehensive error response structures
5. Must remain behind authenticated API Gateway
6. Rate limiting and multi-tenancy support

**Maintainers:** Altaïr Platform Team

---

## Notes

- **Stateless design** – No database dependencies
- **Ephemeral pods** – All pods auto-delete after 2 hours
- **Single namespace** – All pods created in `default` (no multi-tenancy)
- **WebSocket protocol** – Binary frames only (not text)
- **GCP authentication** – Uses Application Default Credentials (ADC)
- **Must deploy behind Gateway** – Do NOT expose directly to internet

---

## License

Internal Altaïr Platform Service – Not licensed for external use.
## May 2026 Security And Platform Updates

- Runtime Docker image now installs only required packages with `--no-install-recommends` and runs as non-root UID `10001`.
- `.env` is for local development only and `.env.example` documents non-secret placeholders for local mode and lab web URLs.
- Kubernetes/GKE connection values such as cluster endpoint and CA data must come from deployment secrets or local-only environment files.
- Latest Trivy scan status for this repo: no HIGH or CRITICAL findings.
