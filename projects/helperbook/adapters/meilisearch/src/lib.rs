use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Config {
    host: String,
    api_key: String,
}

#[derive(Deserialize)]
struct IndexInput {
    index_name: String,
    document_id: String,
    body: String,
}

#[derive(Deserialize)]
struct SearchInput {
    index_name: String,
    query: String,
    filters: Option<String>,
    limit: Option<i32>,
}

#[derive(Deserialize)]
struct DeleteInput {
    index_name: String,
    document_id: String,
}

#[derive(Serialize)]
struct HttpRequest {
    url: String,
    method: String,
    headers: HashMap<String, String>,
    body: String,
}

#[derive(Serialize)]
struct TransformSpec {
    output: HashMap<String, String>,
}

#[derive(Serialize)]
struct RequestEnvelope {
    _request: HttpRequest,
    _transform: TransformSpec,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[no_mangle]
pub extern "C" fn alloc(len: i32) -> i32 {
    let buf = vec![0u8; len as usize];
    let ptr = buf.as_ptr() as i32;
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub extern "C" fn process(ptr: i32, len: i32) -> i32 {
    let input = unsafe {
        let slice = std::slice::from_raw_parts(ptr as *const u8, len as usize);
        std::str::from_utf8(slice).unwrap_or("{}")
    };

    let result = handle_request(input);
    let bytes = result.as_bytes();
    let out_len = bytes.len() as i32;

    let out_ptr = alloc(4 + out_len);
    unsafe {
        let mem = out_ptr as *mut u8;
        std::ptr::copy_nonoverlapping(out_len.to_le_bytes().as_ptr(), mem, 4);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), mem.add(4), bytes.len());
    }
    out_ptr
}

fn handle_request(input: &str) -> String {
    let args: Vec<serde_json::Value> = serde_json::from_str(input).unwrap_or_default();
    let method = args.first()
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match method {
        "index" => handle_index(&args),
        "search" => handle_search(&args),
        "delete" => handle_delete(&args),
        _ => error_json(&format!("unknown method: {}", method)),
    }
}

fn handle_index(args: &[serde_json::Value]) -> String {
    let input: IndexInput = match parse_input(args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let config: Config = match parse_config(args) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if input.index_name.is_empty() {
        return error_json("index_name is required");
    }
    if input.document_id.is_empty() {
        return error_json("document_id is required");
    }

    let url = format!(
        "{}/indexes/{}/documents",
        config.host.trim_end_matches('/'),
        url_encode(&input.index_name),
    );

    let body = serde_json::to_string(&serde_json::json!([
        { "id": input.document_id, "content": input.body }
    ])).unwrap();

    let mut output = HashMap::new();
    output.insert("task_id".to_string(), "$.taskUid".to_string());
    output.insert("success".to_string(), "true".to_string());

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url,
            method: "POST".to_string(),
            headers: auth_headers(&config.api_key),
            body,
        },
        _transform: TransformSpec { output },
    };

    serde_json::to_string(&envelope).unwrap()
}

fn handle_search(args: &[serde_json::Value]) -> String {
    let input: SearchInput = match parse_input(args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let config: Config = match parse_config(args) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if input.index_name.is_empty() {
        return error_json("index_name is required");
    }

    let url = format!(
        "{}/indexes/{}/search",
        config.host.trim_end_matches('/'),
        url_encode(&input.index_name),
    );

    let mut search_body = serde_json::json!({
        "q": input.query,
    });

    if let Some(filters) = &input.filters {
        if !filters.is_empty() {
            search_body["filter"] = serde_json::Value::String(filters.clone());
        }
    }

    if let Some(limit) = input.limit {
        search_body["limit"] = serde_json::Value::Number(limit.into());
    }

    let body = serde_json::to_string(&search_body).unwrap();

    let mut output = HashMap::new();
    output.insert("hits".to_string(), "$.hits".to_string());
    output.insert("total".to_string(), "$.estimatedTotalHits".to_string());

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url,
            method: "POST".to_string(),
            headers: auth_headers(&config.api_key),
            body,
        },
        _transform: TransformSpec { output },
    };

    serde_json::to_string(&envelope).unwrap()
}

fn handle_delete(args: &[serde_json::Value]) -> String {
    let input: DeleteInput = match parse_input(args) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let config: Config = match parse_config(args) {
        Ok(v) => v,
        Err(e) => return e,
    };

    if input.index_name.is_empty() {
        return error_json("index_name is required");
    }
    if input.document_id.is_empty() {
        return error_json("document_id is required");
    }

    let url = format!(
        "{}/indexes/{}/documents/{}",
        config.host.trim_end_matches('/'),
        url_encode(&input.index_name),
        url_encode(&input.document_id),
    );

    let mut output = HashMap::new();
    output.insert("task_id".to_string(), "$.taskUid".to_string());

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url,
            method: "DELETE".to_string(),
            headers: auth_headers(&config.api_key),
            body: String::new(),
        },
        _transform: TransformSpec { output },
    };

    serde_json::to_string(&envelope).unwrap()
}

fn auth_headers(api_key: &str) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".to_string(),
        format!("Bearer {}", api_key),
    );
    headers.insert(
        "Content-Type".to_string(),
        "application/json".to_string(),
    );
    headers
}

fn parse_input<T: serde::de::DeserializeOwned>(args: &[serde_json::Value]) -> Result<T, String> {
    args.get(1)
        .ok_or_else(|| error_json("missing input"))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| error_json(&format!("invalid input: {}", e)))
        })
}

fn parse_config(args: &[serde_json::Value]) -> Result<Config, String> {
    args.get(2)
        .ok_or_else(|| error_json("missing config"))
        .and_then(|v| {
            serde_json::from_value(v.clone())
                .map_err(|e| error_json(&format!("invalid config: {}", e)))
        })
}

fn error_json(msg: &str) -> String {
    serde_json::to_string(&ErrorResponse {
        error: msg.to_string(),
    }).unwrap()
}

/// RFC 3986 percent-encoding for URL path segments.
fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push(HEX_UPPER[(byte >> 4) as usize] as char);
                encoded.push(HEX_UPPER[(byte & 0x0F) as usize] as char);
            }
        }
    }
    encoded
}

const HEX_UPPER: &[u8; 16] = b"0123456789ABCDEF";
