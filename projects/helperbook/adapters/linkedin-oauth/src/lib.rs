use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Config {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

#[derive(Deserialize)]
struct ExchangeCodeInput {
    code: String,
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
    body: String,
}

#[derive(Serialize)]
struct TransformSpec {
    code: String,
    success: bool,
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
        "exchange_code" => handle_exchange_code(&args),
        "get_profile" => handle_get_profile(&args),
        _ => error_json(&format!("unknown method: {}", method)),
    }
}

fn handle_exchange_code(args: &[serde_json::Value]) -> String {
    let input: ExchangeCodeInput = match args.get(1) {
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

    if input.code.is_empty() {
        return error_json("code is required");
    }

    let body = format!(
        "grant_type=authorization_code&code={}&client_id={}&client_secret={}&redirect_uri={}",
        url_encode(&input.code),
        url_encode(&config.client_id),
        url_encode(&config.client_secret),
        url_encode(&config.redirect_uri),
    );

    let mut headers = HashMap::new();
    headers.insert(
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url: "https://www.linkedin.com/oauth/v2/accessToken".to_string(),
            method: "POST".to_string(),
            headers,
            body,
        },
        _transform: TransformSpec {
            code: "{ \"access_token\": $.access_token, \"expires_in\": $.expires_in }".to_string(),
            success: true,
        },
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
            url: "https://api.linkedin.com/v2/userinfo".to_string(),
            method: "GET".to_string(),
            headers,
            body: String::new(),
        },
        _transform: TransformSpec {
            code: "{ \"sub\": $.sub, \"email\": $.email, \"name\": $.name, \"picture\": $.picture }".to_string(),
            success: true,
        },
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
