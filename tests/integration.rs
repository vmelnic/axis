use axum::body::Body;
use http_body_util::BodyExt;
use hyper::Request;
use serde_json::{json, Value};
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

async fn app() -> axum::Router {
    axis::serve::build_app_from_source(TODO_SOURCE, "sqlite::memory:")
        .await
        .expect("failed to build app")
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
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
