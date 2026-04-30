/**
 * @file state — application state and external clients.
 *
 * @remarks
 * Defines the shared runtime state for the Lab API service,
 * including authentication providers and infrastructure clients.
 *
 * Includes:
 *
 *  - GCP token provider (`TokenProvider`) for authenticated API calls
 *  - Kubernetes client (`kube::Client`) for runtime orchestration
 *  - Execution mode flag (`local_mode`)
 *
 * Key characteristics:
 *
 *  - Centralizes access to external dependencies (GCP, Kubernetes)
 *  - Supports both local and cloud execution modes
 *  - Uses shared, thread-safe references (`Arc`) where required
 *
 * This module provides the core dependencies needed by route handlers
 * and services to interact with the runtime infrastructure.
 *
 * @packageDocumentation
 */

use std::sync::Arc;

use gcp_auth::TokenProvider;
use kube::Client;

#[derive(Clone)]
pub struct State {
    pub token_provider: Option<Arc<dyn TokenProvider>>,
    pub kube_client: Client,
    pub local_mode: bool,
}
