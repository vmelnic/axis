use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Config {
    bucket: String,
    region: String,
    access_key: String,
    #[allow(dead_code)]
    secret_key: String,
    endpoint: Option<String>,
    cdn_url: Option<String>,
}

#[derive(Deserialize)]
struct PresignUploadInput {
    key: String,
    content_type: String,
}

#[derive(Deserialize)]
struct DeleteObjectInput {
    key: String,
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
    /// Fields to extract/set on the response passed back to the flow.
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
        "presign_upload" => handle_presign_upload(&args),
        "delete_object" => handle_delete_object(&args),
        _ => error_json(&format!("unknown method: {}", method)),
    }
}

fn handle_presign_upload(args: &[serde_json::Value]) -> String {
    let input: PresignUploadInput = match args.get(1) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(i) => i,
            Err(e) => return error_json(&format!("invalid input: {}", e)),
        },
        None => return error_json("missing input"),
    };

    let config: Config = match args.get(2) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(c) => c,
            Err(e) => return error_json(&format!("invalid config: {}", e)),
        },
        None => return error_json("missing config"),
    };

    if input.key.is_empty() {
        return error_json("key is required");
    }

    let host = s3_host(&config);
    let url = format!("https://{}/{}", host, url_encode(&input.key));

    let cdn_base = config.cdn_url.as_deref().unwrap_or("");
    let public_url = if cdn_base.is_empty() {
        url.clone()
    } else {
        let base = cdn_base.trim_end_matches('/');
        format!("{}/{}", base, input.key)
    };

    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), input.content_type.clone());
    headers.insert("Host".to_string(), host.clone());

    // WASM can't do AWS Sig v4, so pass signing material to the host.
    // The host sees _request and signs it with the secret_key from config.
    let mut output = HashMap::new();
    output.insert("upload_url".to_string(), "$.signed_url".to_string());
    output.insert("public_url".to_string(), public_url);
    output.insert("bucket".to_string(), config.bucket.clone());
    output.insert("key".to_string(), input.key.clone());
    output.insert("content_type".to_string(), input.content_type.clone());
    output.insert("region".to_string(), config.region.clone());
    output.insert("access_key".to_string(), config.access_key.clone());

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url,
            method: "PUT".to_string(),
            headers,
            body: String::new(),
        },
        _transform: TransformSpec { output },
    };

    serde_json::to_string(&envelope).unwrap()
}

fn handle_delete_object(args: &[serde_json::Value]) -> String {
    let input: DeleteObjectInput = match args.get(1) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(i) => i,
            Err(e) => return error_json(&format!("invalid input: {}", e)),
        },
        None => return error_json("missing input"),
    };

    let config: Config = match args.get(2) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(c) => c,
            Err(e) => return error_json(&format!("invalid config: {}", e)),
        },
        None => return error_json("missing config"),
    };

    if input.key.is_empty() {
        return error_json("key is required");
    }

    let host = s3_host(&config);
    let url = format!("https://{}/{}", host, url_encode(&input.key));

    let mut headers = HashMap::new();
    headers.insert("Host".to_string(), host.clone());

    let mut output = HashMap::new();
    output.insert("deleted".to_string(), "true".to_string());

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url,
            method: "DELETE".to_string(),
            headers,
            body: String::new(),
        },
        _transform: TransformSpec { output },
    };

    serde_json::to_string(&envelope).unwrap()
}

/// Build the S3 virtual-hosted-style hostname.
/// If a custom endpoint is configured (for MinIO, R2, etc.), use that instead.
fn s3_host(config: &Config) -> String {
    if let Some(endpoint) = &config.endpoint {
        if !endpoint.is_empty() {
            let ep = endpoint
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_end_matches('/');
            return ep.to_string();
        }
    }
    format!("{}.s3.{}.amazonaws.com", config.bucket, config.region)
}

fn error_json(msg: &str) -> String {
    serde_json::to_string(&ErrorResponse {
        error: msg.to_string(),
    }).unwrap()
}

/// RFC 3986 percent-encoding for URL path components.
/// Encodes everything except unreserved characters (A-Z a-z 0-9 - _ . ~)
/// and forward slash (preserved for S3 key paths).
fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' | b'/' => {
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
