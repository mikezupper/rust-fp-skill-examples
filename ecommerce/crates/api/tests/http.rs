//! HTTP-level tests: the adapter, the error envelope, and the status-code mapping.
//!
//! The workflow suite proves the domain behaves. This suite proves the translation
//! layer does — that the typed error track surfaces as the right status and the right
//! JSON, and that a request whose body fails to parse never reaches a workflow at all.

// Integration tests are their own crates, so `cfg(test)` is not set for them and the
// workspace deny tier applies in full. Relaxed at the crate root with a reason: in a
// test, `expect` and `panic!` are assertions — a fixture that fails to build means the
// test is wrong, and failing loudly is the correct report.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test assertions: a failed precondition should abort the test loudly"
)]

mod common;

use api::{AppState, router};
use common::test_ctx;
use serde_json::{Value, json};
use tokio::net::TcpListener;

/// Binds port 0 so the OS picks a free port — tests can run in parallel and in CI
/// without a hardcoded port collision.
async fn spawn() -> String {
    let ctx = test_ctx().await;
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("port 0 always binds");
    let addr = listener
        .local_addr()
        .expect("bound listener has an address");
    let app = router(AppState::new(ctx));

    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    format!("http://{addr}")
}

async fn register(client: &reqwest::Client, base: &str, mail: &str) -> String {
    let response = client
        .post(format!("{base}/auth/register"))
        .json(&json!({ "email": mail, "password": "correct horse battery" }))
        .send()
        .await
        .expect("request completes");

    assert_eq!(response.status(), 201);
    let body: Value = response.json().await.expect("response is JSON");
    body["token"]
        .as_str()
        .expect("token is a string")
        .to_owned()
}

#[tokio::test]
async fn health_is_ok() {
    let base = spawn().await;
    let response = reqwest::get(format!("{base}/health"))
        .await
        .expect("request completes");
    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn the_catalog_is_public() {
    let base = spawn().await;

    let categories: Value = reqwest::get(format!("{base}/catalog/categories"))
        .await
        .expect("request completes")
        .json()
        .await
        .expect("response is JSON");
    assert!(categories.as_array().is_some_and(|a| !a.is_empty()));

    let products: Value = reqwest::get(format!("{base}/catalog/products?search=Laptop"))
        .await
        .expect("request completes")
        .json()
        .await
        .expect("response is JSON");
    assert_eq!(products.as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn an_unknown_product_is_404_with_a_structured_body() {
    let base = spawn().await;
    let response = reqwest::get(format!("{base}/catalog/products/p-nope"))
        .await
        .expect("request completes");

    assert_eq!(response.status(), 404);
    let body: Value = response.json().await.expect("response is JSON");
    // A tagged union: `_tag` names the domain failure, its own fields sit alongside.
    // A client switches on the tag and reads typed fields, never a message string.
    assert_eq!(body["_tag"], "ProductNotFound");
    assert_eq!(body["productId"], "p-nope");
}

#[tokio::test]
async fn protected_routes_reject_missing_and_malformed_tokens() {
    let base = spawn().await;
    let client = reqwest::Client::new();

    for request in [
        client.get(format!("{base}/cart")),
        client
            .get(format!("{base}/cart"))
            .bearer_auth("not-a-real-token"),
        client.post(format!("{base}/orders")),
        client.get(format!("{base}/orders")),
    ] {
        let response = request.send().await.expect("request completes");
        assert_eq!(response.status(), 401);
        let body: Value = response.json().await.expect("response is JSON");
        // The 401 body is deliberately empty of detail: an unauthenticated caller
        // must not learn whether the token was unknown, expired, or malformed.
        assert_eq!(body["_tag"], "Unauthorized");
        assert!(body.get("message").is_none());
    }
}

#[tokio::test]
async fn a_body_that_fails_domain_validation_is_rejected_at_the_boundary() {
    let base = spawn().await;
    let client = reqwest::Client::new();

    // Each of these fails inside a domain type's `Deserialize`, before any workflow
    // runs. There is no validation code in the handler to test, because there is no
    // validation code in the handler.
    for bad in [
        json!({ "email": "not-an-email", "password": "correct horse battery" }),
        json!({ "email": "a@example.com", "password": "short" }),
        json!({ "email": "a@example.com" }),
        json!({ "email": "a@example.com", "password": "correct horse battery", "admin": true }),
    ] {
        let response = client
            .post(format!("{base}/auth/register"))
            .json(&bad)
            .send()
            .await
            .expect("request completes");

        assert_eq!(response.status(), 400, "should reject {bad}");
        let body: Value = response.json().await.expect("response is JSON");
        assert_eq!(body["_tag"], "HttpApiDecodeError");
    }
}

#[tokio::test]
async fn an_out_of_range_quantity_never_reaches_a_workflow() {
    let base = spawn().await;
    let client = reqwest::Client::new();
    let token = register(&client, &base, "a@example.com").await;

    for quantity in [0, 100, 1000] {
        let response = client
            .put(format!("{base}/cart/items"))
            .bearer_auth(&token)
            .json(&json!({ "productId": "p-laptop-pro", "quantity": quantity }))
            .send()
            .await
            .expect("request completes");

        // `Quantity` is bounded 1..=99 by its type, so this is a parse failure at the
        // boundary rather than a business-rule check somewhere downstream.
        assert_eq!(
            response.status(),
            400,
            "quantity {quantity} should be rejected"
        );
    }
}

#[tokio::test]
async fn a_duplicate_registration_is_409() {
    let base = spawn().await;
    let client = reqwest::Client::new();
    register(&client, &base, "a@example.com").await;

    let response = client
        .post(format!("{base}/auth/register"))
        .json(&json!({ "email": "a@example.com", "password": "correct horse battery" }))
        .send()
        .await
        .expect("request completes");

    assert_eq!(response.status(), 409);
    let body: Value = response.json().await.expect("response is JSON");
    assert_eq!(body["_tag"], "EmailTaken");
}

#[tokio::test]
async fn the_full_purchase_journey_works_over_http() {
    let base = spawn().await;
    let client = reqwest::Client::new();
    let token = register(&client, &base, "shopper@example.com").await;

    let cart: Value = client
        .put(format!("{base}/cart/items"))
        .bearer_auth(&token)
        .json(&json!({ "productId": "p-laptop-pro", "quantity": 2 }))
        .send()
        .await
        .expect("request completes")
        .json()
        .await
        .expect("response is JSON");
    assert_eq!(cart["totalCents"], 2 * 199_900);

    let response = client
        .post(format!("{base}/orders"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request completes");
    assert_eq!(response.status(), 201);

    let order: Value = response.json().await.expect("response is JSON");
    assert_eq!(order["totalCents"], 2 * 199_900);

    let history: Value = client
        .get(format!("{base}/orders"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request completes")
        .json()
        .await
        .expect("response is JSON");
    assert_eq!(history.as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn insufficient_stock_is_409_and_reports_what_is_available() {
    let base = spawn().await;
    let client = reqwest::Client::new();
    let token = register(&client, &base, "shopper@example.com").await;

    // Headphones are seeded with stock 3.
    client
        .put(format!("{base}/cart/items"))
        .bearer_auth(&token)
        .json(&json!({ "productId": "p-headphones", "quantity": 50 }))
        .send()
        .await
        .expect("request completes");

    let response = client
        .post(format!("{base}/orders"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request completes");

    assert_eq!(response.status(), 409);
    let body: Value = response.json().await.expect("response is JSON");
    assert_eq!(body["_tag"], "InsufficientStock");
    // The structured fields survive all the way from the SQL statement that failed to
    // reserve, through two error enums, to the JSON a browser can render.
    assert_eq!(body["requested"], 50);
    assert_eq!(body["available"], 3);
}

#[tokio::test]
async fn checking_out_an_empty_cart_is_409() {
    let base = spawn().await;
    let client = reqwest::Client::new();
    let token = register(&client, &base, "shopper@example.com").await;

    let response = client
        .post(format!("{base}/orders"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("request completes");

    assert_eq!(response.status(), 409);
    let body: Value = response.json().await.expect("response is JSON");
    assert_eq!(body["_tag"], "CartEmpty");
}
