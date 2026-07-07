//! Query-engine tests against a mocked Cosmos gateway (wiremock).

use cosq_client::cosmos::CosmosClient;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn pkranges_body(ids: &[&str]) -> serde_json::Value {
    json!({
        "PartitionKeyRanges": ids.iter().map(|id| json!({"id": id})).collect::<Vec<_>>()
    })
}

fn docs_response(ids: &[u32], charge: f64) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("x-ms-request-charge", charge.to_string().as_str())
        .set_body_json(json!({
            "Documents": ids.iter().map(|i| json!({"id": i.to_string()})).collect::<Vec<_>>(),
            "_count": ids.len()
        }))
}

#[tokio::test]
async fn fan_out_is_parallel_ordered_and_sums_ru() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/dbs/db/colls/c/pkranges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pkranges_body(&["0", "1", "2"])))
        .mount(&server)
        .await;
    // range 0 is SLOW; ranges 1/2 fast — parallelism means total < sum of delays
    Mock::given(method("POST"))
        .and(path("/dbs/db/colls/c/docs"))
        .and(header("x-ms-documentdb-partitionkeyrangeid", "0"))
        .respond_with(docs_response(&[1], 2.5).set_delay(std::time::Duration::from_millis(800)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/dbs/db/colls/c/docs"))
        .and(header("x-ms-documentdb-partitionkeyrangeid", "1"))
        .respond_with(docs_response(&[2, 3], 3.0).set_delay(std::time::Duration::from_millis(700)))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/dbs/db/colls/c/docs"))
        .and(header("x-ms-documentdb-partitionkeyrangeid", "2"))
        .respond_with(docs_response(&[4], 1.5).set_delay(std::time::Duration::from_millis(700)))
        .mount(&server)
        .await;

    let client = CosmosClient::with_token(&server.uri(), "test-token".into());
    let started = std::time::Instant::now();
    let result = client.query("db", "c", "SELECT * FROM c").await.unwrap();
    let elapsed = started.elapsed();

    // parallel: three delayed responses (800+700+700=2200ms serial) well under 1.6s
    assert!(
        elapsed < std::time::Duration::from_millis(1600),
        "fan-out looks serial: {elapsed:?}"
    );
    // order preserved by range order regardless of completion order
    let ids: Vec<&str> = result
        .documents
        .iter()
        .map(|d| d["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["1", "2", "3", "4"]);
    assert!((result.request_charge - 7.0).abs() < 1e-9);
    assert_eq!(result.per_range.len(), 3);
    assert_eq!(result.per_range[0], ("0".to_string(), 2.5));
}

#[tokio::test]
async fn per_range_continuation_tokens_are_followed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/dbs/db/colls/c/pkranges"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pkranges_body(&["0"])))
        .mount(&server)
        .await;
    // first page returns a continuation token, second page ends
    Mock::given(method("POST"))
        .and(path("/dbs/db/colls/c/docs"))
        .and(header("x-ms-continuation", "next-page"))
        .respond_with(docs_response(&[2], 1.0))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/dbs/db/colls/c/docs"))
        .respond_with(docs_response(&[1], 1.0).insert_header("x-ms-continuation", "next-page"))
        .mount(&server)
        .await;

    let client = CosmosClient::with_token(&server.uri(), "test-token".into());
    let result = client.query("db", "c", "SELECT * FROM c").await.unwrap();
    let ids: Vec<&str> = result
        .documents
        .iter()
        .map(|d| d["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["1", "2"]);
    assert!((result.request_charge - 2.0).abs() < 1e-9);
}
