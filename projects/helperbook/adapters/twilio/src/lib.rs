use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Config {
    account_sid: String,
    auth_token: String,
    verify_service_sid: String,
}

#[derive(Deserialize)]
struct SendOtpInput {
    phone: String,
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
        "send_otp" => handle_send_otp(&args),
        _ => serde_json::to_string(&ErrorResponse {
            error: format!("unknown method: {}", method),
        }).unwrap(),
    }
}

fn handle_send_otp(args: &[serde_json::Value]) -> String {
    let input: SendOtpInput = match args.get(1) {
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

    if input.phone.is_empty() {
        return error_json("phone is required");
    }

    let url = format!(
        "https://verify.twilio.com/v2/Services/{}/Verifications",
        url_encode(&config.verify_service_sid)
    );

    let auth = base64_encode(&format!("{}:{}", config.account_sid, config.auth_token));

    let body = format!(
        "To={}&Channel=sms",
        url_encode(&input.phone)
    );

    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".to_string(),
        format!("Basic {}", auth),
    );
    headers.insert(
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url,
            method: "POST".to_string(),
            headers,
            body,
        },
        _transform: TransformSpec {
            code: "$.sid".to_string(),
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

/// Standard base64 encoding (RFC 4648) with padding.
/// No external crate needed for this small adapter.
fn base64_encode(input: &str) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let bytes = input.as_bytes();
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    let chunks = bytes.chunks(3);

    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };

        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }

        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }

    out
}
