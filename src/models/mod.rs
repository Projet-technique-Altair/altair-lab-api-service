/**
 * @file mod — public model exports.
 *
 * @remarks
 * Aggregates and re-exports all data structures used by the Lab API service,
 * providing a unified access point for request/response types and shared state.
 *
 * Exposes:
 *
 *  - Runtime lifecycle models (`spawn`)
 *  - Application state (`state`)
 *
 * Key characteristics:
 *
 *  - Simplifies imports across the codebase
 *  - Encapsulates internal module structure
 *  - Ensures a clean and consistent public API
 *
 * This module acts as the central interface for all models
 * consumed by routes and services.
 *
 * @packageDocumentation
 */
mod spawn;
mod state;

pub use spawn::{
    SpawnRequest, SpawnResponse, SpawnResponseData, StatusResponse, StopRequest, StopResponse,
};
pub use state::State;
