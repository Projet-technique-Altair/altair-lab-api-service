/**
 * @file health_check — basic health endpoint tests.
 *
 * @remarks
 * Contains simple tests to verify that the health endpoint
 * responds correctly and remains stable.
 *
 * Test coverage:
 *
 *  - Ensures the health endpoint returns a successful response
 *  - Verifies the response type and non-empty content
 *
 * Key characteristics:
 *
 *  - Lightweight and fast execution
 *  - No external dependencies required
 *  - Validates basic service availability contract
 *
 * These tests act as a minimal safety check to ensure
 * the service is up and responding as expected.
 *
 * @packageDocumentation
 */

use crate::routes::health;

#[tokio::test]
async fn test_health_returns_ok() {
    let result = health::health().await;
    assert_eq!(result, "OK");
}

#[tokio::test]
async fn test_health_response_is_static_str() {
    let result: &'static str = health::health().await;
    assert!(!result.is_empty());
}
