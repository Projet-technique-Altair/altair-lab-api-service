# Altair Lab API Service (Mock)

This microservice is responsible for spawning and stopping lab instances for the Altaïr platform.
For the PoC (Proof of Concept), the service does **not** launch real containers.
Instead, it returns mock responses for integration tests with the frontend, gateway, and Grafana.

---

## ✨ Features (PoC version)

- `POST /spawn`  
  Returns a mock container ID and a mock WebShell URL.

- `POST /spawn/stop`  
  Simulates stopping a lab session.

- `/health`  
  Simple liveness endpoint.

---

## 🚀 Run locally (development)

```bash
cargo run
