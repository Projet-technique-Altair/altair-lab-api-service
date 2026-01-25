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
