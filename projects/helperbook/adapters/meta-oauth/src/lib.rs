use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Deserialize)]
struct ExchangeCodeInput {
    code: String,
}

#[derive(Deserialize)]
struct GetProfileInput {
    access_token: String,
}

#[derive(Deserialize)]
struct Config {
    app_id: String,
    app_secret: String,
    redirect_uri: String,
}

#[derive(Serialize)]
struct HttpRequest {
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: String,
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
    let args: Vec<Value> = serde_json::from_str(input).unwrap_or_default();
    let method = args.first()
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match method {
        "exchange_code" => handle_exchange_code(&args),
        "get_profile" => handle_get_profile(&args),
        _ => error_json(&format!("unknown method: {}", method)),
    }
}

fn handle_exchange_code(args: &[Value]) -> String {
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

    let url = format!(
        "https://graph.facebook.com/v19.0/oauth/access_token?client_id={}&client_secret={}&redirect_uri={}&code={}",
        url_encode(&config.app_id),
        url_encode(&config.app_secret),
        url_encode(&config.redirect_uri),
        url_encode(&input.code),
    );

    let headers = HashMap::new();

    let result = json!({
        "_request": {
            "method": "GET",
            "url": url,
            "headers": headers,
            "body": ""
        },
        "_transform": {
            "access_token": "$.access_token",
            "token_type": "$.token_type",
            "expires_in": "$.expires_in"
        }
    });

    serde_json::to_string(&result).unwrap()
}

fn handle_get_profile(args: &[Value]) -> String {
    let input: GetProfileInput = match args.get(1) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(i) => i,
            Err(e) => return error_json(&format!("invalid input: {}", e)),
        },
        None => return error_json("missing input"),
    };

    // Config not required for profile fetch, but accept it if present
    let _config: Option<Config> = args.get(2)
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    if input.access_token.is_empty() {
        return error_json("access_token is required");
    }

    let url = format!(
        "https://graph.facebook.com/v19.0/me?fields={}&access_token={}",
        url_encode("id,email,name,picture.type(large)"),
        url_encode(&input.access_token),
    );

    let headers = HashMap::new();

    let result = json!({
        "_request": {
            "method": "GET",
            "url": url,
            "headers": headers,
            "body": ""
        },
        "_transform": {
            "id": "$.id",
            "email": "$.email",
            "name": "$.name",
            "picture": "$.picture.data.url"
        }
    });

    serde_json::to_string(&result).unwrap()
}

/// Percent-encode a string for use in URL query parameters (RFC 3986).
/// Unreserved characters (A-Z, a-z, 0-9, '-', '_', '.', '~') pass through;
/// everything else becomes %XX.
fn url_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 2);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push(to_hex_digit(byte >> 4));
                encoded.push(to_hex_digit(byte & 0x0F));
            }
        }
    }
    encoded
}

fn to_hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + nibble - 10) as char,
        _ => unreachable!(),
    }
}

fn error_json(msg: &str) -> String {
    serde_json::to_string(&ErrorResponse {
        error: msg.to_string(),
    }).unwrap()
}
