use axum::body::Body;
use http_body_util::BodyExt;
use hyper::Request;
use serde_json::{Value, json};
use tower::ServiceExt;

const TODO_SOURCE: &str = r#"SHAPE Todo
  id UUID PK AUTO
  title STRING 200 REQUIRED
  completed BOOL DEFAULT false
  created_at TIMESTAMP AUTO

SOURCE todos SQLITE
  SHAPE Todo
  INDEX id

REALM api
  CAPABILITY read todos
  CAPABILITY write todos

FLOW list_todos get /todos
  REALM api
  LET todos
    QUERY todos
  RETURN 200 todos

FLOW get_todo get /todos/:id
  REALM api
  LET todo
    FETCH todos
      FILTER id EQ path.id
    OR 404
  RETURN 200 todo

FLOW create_todo post /todos
  REALM api
  BODY NewTodo
    title STRING 200 REQUIRED
  INSERT todos
    title body.title
  AS todo
  RETURN 201 todo

FLOW update_todo put /todos/:id
  REALM api
  BODY UpdateTodo
    title STRING 200
    completed BOOL
  UPDATE todos
    WHERE id EQ path.id
    SET title body.title
    SET completed body.completed
  AS todo
  OR 404
  RETURN 200 todo

FLOW delete_todo delete /todos/:id
  REALM api
  DELETE todos
    WHERE id EQ path.id
  OR 404
  RETURN 204
"#;

const MESSENGER_PRIMITIVES_SOURCE: &str = r#"SHAPE Delivery
  id STRING 64 PK
  value STRING 200 REQUIRED

SOURCE deliveries SQLITE
  SHAPE Delivery
  INDEX id UNIQUE

FLOW get_delivery get /deliveries/:id
  LET delivery
    FETCH deliveries
      FILTER id EQ path.id
    OR 404
  RETURN 200 delivery

FLOW deliver post /deliveries/:id
  HEADER idempotency_key STRING 255 REQUIRED
  BODY DeliveryCommand
    actor_id STRING 64 REQUIRED
    value STRING 200 REQUIRED
    recipient_ids LIST STRING 64 REQUIRED
  IDEMPOTENCY header.idempotency_key SCOPE body.actor_id TTL 86400
  UPSERT deliveries
    KEY id path.id
    SET value body.value
  AS delivery
  FANOUT recipient_id IN body.recipient_ids
    INSERT deliveries
      id recipient_id
      value body.value
  RETURN 201 delivery
    HEADER X-Delivery-Id path.id
"#;

async fn app() -> axum::Router {
    axis::serve::build_app_from_source(TODO_SOURCE, "sqlite::memory:")
        .await
        .expect("failed to build app")
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

fn delivery_request(id: &str, key: &str, value: &str, recipients: &[&str]) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(format!("/deliveries/{id}"))
        .header("content-type", "application/json")
        .header("idempotency-key", key)
        .body(Body::from(
            json!({
                "actor_id": "actor-1",
                "value": value,
                "recipient_ids": recipients,
            })
            .to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn test_create_and_get_todo() {
    let app = app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/todos")
                .header("content-type", "application/json")
                .body(Body::from(json!({"title": "Buy milk"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let created = body_json(resp).await;
    assert_eq!(created["title"], "Buy milk");
    assert!(created["completed"] == json!(false) || created["completed"] == json!(0));
    let id = created["id"].as_str().unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/todos/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let fetched = body_json(resp).await;
    assert_eq!(fetched["id"], id);
    assert_eq!(fetched["title"], "Buy milk");
}

#[tokio::test]
async fn test_list_todos() {
    let app = app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/todos")
                .header("content-type", "application/json")
                .body(Body::from(json!({"title": "Item 1"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/todos")
                .header("content-type", "application/json")
                .body(Body::from(json!({"title": "Item 2"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/todos")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let list = body_json(resp).await;
    let arr = list["items"].as_array().unwrap();
    assert!(arr.len() >= 2);
}

#[tokio::test]
async fn test_update_todo() {
    let app = app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/todos")
                .header("content-type", "application/json")
                .body(Body::from(json!({"title": "Original"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let created = body_json(resp).await;
    let id = created["id"].as_str().unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/todos/{id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"title": "Updated", "completed": true}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let updated = body_json(resp).await;
    assert_eq!(updated["title"], "Updated");
    assert_eq!(updated["completed"], 1);
}

#[tokio::test]
async fn test_delete_todo() {
    let app = app().await;

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/todos")
                .header("content-type", "application/json")
                .body(Body::from(json!({"title": "To delete"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let created = body_json(resp).await;
    let id = created["id"].as_str().unwrap();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/todos/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/todos/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_get_nonexistent_returns_404() {
    let app = app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/todos/00000000-0000-0000-0000-000000000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let app = app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("axis_request_total"));
}

#[tokio::test]
async fn test_idempotent_upsert_and_transactional_fanout() {
    let app = axis::serve::build_app_from_source(MESSENGER_PRIMITIVES_SOURCE, "sqlite::memory:")
        .await
        .expect("failed to build primitives app");

    let first = app
        .clone()
        .oneshot(delivery_request(
            "message-1",
            "operation-1",
            "hello",
            &["recipient-1", "recipient-2"],
        ))
        .await
        .unwrap();
    assert_eq!(first.status(), 201);
    assert_eq!(first.headers()["idempotency-replayed"], "false");
    assert_eq!(first.headers()["x-delivery-id"], "message-1");
    let first_body = body_json(first).await;
    assert_eq!(first_body["id"], "message-1");

    let replay = app
        .clone()
        .oneshot(delivery_request(
            "message-1",
            "operation-1",
            "hello",
            &["recipient-1", "recipient-2"],
        ))
        .await
        .unwrap();
    assert_eq!(replay.status(), 201);
    assert_eq!(replay.headers()["idempotency-replayed"], "true");
    assert_eq!(replay.headers()["x-delivery-id"], "message-1");
    assert_eq!(body_json(replay).await, first_body);

    let conflict = app
        .clone()
        .oneshot(delivery_request(
            "message-1",
            "operation-1",
            "changed",
            &["recipient-1", "recipient-2"],
        ))
        .await
        .unwrap();
    assert_eq!(conflict.status(), 409);

    for id in ["message-1", "recipient-1", "recipient-2"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/deliveries/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "missing transactional row {id}");
    }
}

#[tokio::test]
async fn test_idempotent_failure_rolls_back_reservation_and_all_writes() {
    let app = axis::serve::build_app_from_source(MESSENGER_PRIMITIVES_SOURCE, "sqlite::memory:")
        .await
        .expect("failed to build primitives app");

    let failed = app
        .clone()
        .oneshot(delivery_request(
            "rolled-back",
            "retryable-operation",
            "first attempt",
            &["duplicate", "duplicate"],
        ))
        .await
        .unwrap();
    assert_eq!(failed.status(), 500);

    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/deliveries/rolled-back")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);

    let retry = app
        .oneshot(delivery_request(
            "rolled-back",
            "retryable-operation",
            "second attempt",
            &["recipient-ok"],
        ))
        .await
        .unwrap();
    assert_eq!(retry.status(), 201);
    assert_eq!(retry.headers()["idempotency-replayed"], "false");
}

#[tokio::test]
async fn test_concurrent_idempotent_requests_execute_once() {
    let app = axis::serve::build_app_from_source(MESSENGER_PRIMITIVES_SOURCE, "sqlite::memory:")
        .await
        .expect("failed to build primitives app");

    let first = app.clone().oneshot(delivery_request(
        "concurrent-message",
        "concurrent-operation",
        "same payload",
        &["concurrent-recipient"],
    ));
    let second = app.clone().oneshot(delivery_request(
        "concurrent-message",
        "concurrent-operation",
        "same payload",
        &["concurrent-recipient"],
    ));
    let (first, second) = tokio::join!(first, second);
    let first = first.unwrap();
    let second = second.unwrap();

    assert_eq!(first.status(), 201);
    assert_eq!(second.status(), 201);
    let replayed = [
        first.headers()["idempotency-replayed"].to_str().unwrap(),
        second.headers()["idempotency-replayed"].to_str().unwrap(),
    ];
    assert!(replayed.contains(&"false"));
    assert!(replayed.contains(&"true"));
}

#[tokio::test]
async fn test_multipart_upload_enforces_type_and_serves_public_local_file() {
    let upload_dir = std::env::temp_dir().join(format!("axis-upload-{}", uuid::Uuid::new_v4()));
    let source = format!(
        r#"STORAGE avatars
  BACKEND local
  BUCKET "{}"
  PREFIX originals
  ACCESS public
  MAX_SIZE 1024
  TYPES image/jpeg

FLOW upload_avatar post /avatars
  BODY MULTIPART AvatarUpload
    file BLOB REQUIRED
  UPLOAD body.file -> avatars AS avatar_url
  RETURN 201 avatar_url
"#,
        upload_dir.display()
    );
    let app = axis::serve::build_app_from_source(&source, "sqlite::memory:")
        .await
        .expect("failed to build upload app");

    let boundary = "axis-test-boundary";
    let mut multipart = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"avatar.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"
    )
    .into_bytes();
    let jpeg = [
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01,
    ];
    multipart.extend_from_slice(&jpeg);
    multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/avatars")
                .header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(multipart))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    let url = body_json(response).await.as_str().unwrap().to_owned();
    assert!(url.starts_with("/files/avatars/originals/"));
    assert!(url.ends_with(".jpg"));

    let response = app
        .oneshot(Request::builder().uri(url).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        jpeg.as_slice()
    );

    tokio::fs::remove_dir_all(upload_dir).await.unwrap();
}

#[test]
fn project_mode_composes_with_openapi_codegen() {
    let project_dir =
        std::env::temp_dir().join(format!("axis-project-openapi-{}", uuid::Uuid::new_v4()));
    let source_dir = project_dir.join("src");
    std::fs::create_dir_all(&source_dir).expect("create project source directory");
    std::fs::write(
        source_dir.join("records.axis"),
        "SHAPE Record\n  id UUID PK AUTO\n  title STRING 120 REQUIRED\n\nSOURCE records SQLITE\n  SHAPE Record\n  INDEX id\n",
    )
    .expect("write record schema");
    std::fs::write(
        source_dir.join("api.axis"),
        "FLOW list_records get /records\n  LET records\n    QUERY records\n  RETURN 200 records\n",
    )
    .expect("write API flow");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_axis"))
        .args(["--project", "--openapi"])
        .arg(&project_dir)
        .output()
        .expect("run Axis CLI");
    std::fs::remove_dir_all(&project_dir).expect("remove project fixture");

    assert!(
        output.status.success(),
        "Axis failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: Value = serde_json::from_slice(&output.stdout).expect("OpenAPI JSON");
    assert_eq!(document["openapi"], "3.1.0");
    assert!(document["paths"]["/records"]["get"].is_object());
}
