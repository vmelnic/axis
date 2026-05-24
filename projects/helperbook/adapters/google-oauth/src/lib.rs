use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Config {
    client_id: String,
    client_secret: String,
}

#[derive(Deserialize)]
struct VerifyTokenInput {
    token: String,
}

#[derive(Deserialize)]
struct GetProfileInput {
    access_token: String,
}

#[derive(Serialize)]
struct HttpRequest {
    url: String,
    method: String,
    headers: HashMap<String, String>,
}

#[derive(Serialize)]
struct TransformSpec {
    sub: String,
    email: String,
    name: String,
    picture: String,
    email_verified: String,
}

#[derive(Serialize)]
struct ProfileTransformSpec {
    sub: String,
    email: String,
    name: String,
    picture: String,
}

#[derive(Serialize)]
struct RequestEnvelope {
    _request: HttpRequest,
    _transform: serde_json::Value,
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
        "verify_token" => handle_verify_token(&args),
        "get_profile" => handle_get_profile(&args),
        _ => error_json(&format!("unknown method: {}", method)),
    }
}

fn handle_verify_token(args: &[serde_json::Value]) -> String {
    let input: VerifyTokenInput = match args.get(1) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(i) => i,
            Err(e) => return error_json(&format!("invalid input: {}", e)),
        },
        None => return error_json("missing input"),
    };

    // Config is accepted but not needed in the request itself;
    // the token is verified against Google's public keys server-side.
    let _config: Config = match args.get(2) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(c) => c,
            Err(e) => return error_json(&format!("invalid config: {}", e)),
        },
        None => return error_json("missing config"),
    };

    if input.token.is_empty() {
        return error_json("token is required");
    }

    let url = format!(
        "https://oauth2.googleapis.com/tokeninfo?id_token={}",
        url_encode(&input.token)
    );

    let headers = HashMap::new();

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url,
            method: "GET".to_string(),
            headers,
        },
        _transform: serde_json::to_value(TransformSpec {
            sub: "$.sub".to_string(),
            email: "$.email".to_string(),
            name: "$.name".to_string(),
            picture: "$.picture".to_string(),
            email_verified: "$.email_verified".to_string(),
        }).unwrap(),
    };

    serde_json::to_string(&envelope).unwrap()
}

fn handle_get_profile(args: &[serde_json::Value]) -> String {
    let input: GetProfileInput = match args.get(1) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(i) => i,
            Err(e) => return error_json(&format!("invalid input: {}", e)),
        },
        None => return error_json("missing input"),
    };

    let _config: Config = match args.get(2) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(c) => c,
            Err(e) => return error_json(&format!("invalid config: {}", e)),
        },
        None => return error_json("missing config"),
    };

    if input.access_token.is_empty() {
        return error_json("access_token is required");
    }

    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".to_string(),
        format!("Bearer {}", input.access_token),
    );

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url: "https://www.googleapis.com/oauth2/v3/userinfo".to_string(),
            method: "GET".to_string(),
            headers,
        },
        _transform: serde_json::to_value(ProfileTransformSpec {
            sub: "$.sub".to_string(),
            email: "$.email".to_string(),
            name: "$.name".to_string(),
            picture: "$.picture".to_string(),
        }).unwrap(),
    };

    serde_json::to_string(&envelope).unwrap()
}

fn error_json(msg: &str) -> String {
    serde_json::to_string(&ErrorResponse {
        error: msg.to_string(),
    }).unwrap()
}

/// RFC 3986 percent-encoding for URL components.
/// Encodes everything except unreserved characters (A-Z a-z 0-9 - _ . ~).
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
