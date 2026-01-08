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

## TODO:
  * Proper error handling for all the various problems that may arise
  * Authentication for the webshell
  * Pulling the proper image by a lab id (currently using basic debian)
