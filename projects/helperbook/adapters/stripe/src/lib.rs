use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// --- Input types ---

#[derive(Deserialize)]
struct CreateCheckoutInput {
    user_id: String,
    plan: String,
    return_url: String,
}

#[derive(Deserialize)]
struct VerifyReceiptInput {
    provider: String,
    receipt_data: String,
}

#[derive(Deserialize)]
struct Config {
    secret_key: String,
    #[allow(dead_code)]
    webhook_secret: String,
    price_id_plus: String,
}

// --- Output envelope ---

#[derive(Serialize)]
struct AdapterResponse {
    _request: HttpRequest,
    _transform: Transform,
}

#[derive(Serialize)]
struct HttpRequest {
    url: String,
    method: String,
    headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
}

#[derive(Serialize)]
struct Transform {
    /// Maps response JSON paths to output field names
    fields: HashMap<String, String>,
}

// --- WASM ABI ---

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
        std::str::from_utf8(slice).unwrap_or("[]")
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

// --- Request handling ---

fn handle_request(input: &str) -> String {
    let args: Vec<serde_json::Value> = match serde_json::from_str(input) {
        Ok(v) => v,
        Err(_) => return error_json("invalid input: expected JSON array"),
    };

    let method = match args.first().and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return error_json("missing method name"),
    };

    let input_val = args.get(1).cloned().unwrap_or(serde_json::Value::Object(Default::default()));
    let config_val = args.get(2).cloned().unwrap_or(serde_json::Value::Object(Default::default()));

    let config: Config = match serde_json::from_value(config_val) {
        Ok(c) => c,
        Err(e) => return error_json(&format!("invalid config: {e}")),
    };

    match method {
        "create_checkout" => handle_create_checkout(input_val, &config),
        "verify_receipt" => handle_verify_receipt(input_val, &config),
        _ => error_json(&format!("unknown method: {method}")),
    }
}

fn handle_create_checkout(input_val: serde_json::Value, config: &Config) -> String {
    let input: CreateCheckoutInput = match serde_json::from_value(input_val) {
        Ok(v) => v,
        Err(e) => return error_json(&format!("invalid input for create_checkout: {e}")),
    };

    let price_id = resolve_price_id(&input.plan, config);

    let success_url = if input.return_url.contains('?') {
        format!("{}&session_id={{CHECKOUT_SESSION_ID}}", input.return_url)
    } else {
        format!("{}?session_id={{CHECKOUT_SESSION_ID}}", input.return_url)
    };
    let cancel_url = input.return_url.clone();

    let body = form_urlencoded(&[
        ("mode", "subscription"),
        ("line_items[0][price]", &price_id),
        ("line_items[0][quantity]", "1"),
        ("success_url", &success_url),
        ("cancel_url", &cancel_url),
        ("client_reference_id", &input.user_id),
    ]);

    let mut headers = HashMap::new();
    headers.insert("Authorization".into(), format!("Bearer {}", config.secret_key));
    headers.insert("Content-Type".into(), "application/x-www-form-urlencoded".into());

    let mut transform_fields = HashMap::new();
    transform_fields.insert("url".into(), "checkout_url".into());
    transform_fields.insert("id".into(), "session_id".into());

    let response = AdapterResponse {
        _request: HttpRequest {
            url: "https://api.stripe.com/v1/checkout/sessions".into(),
            method: "POST".into(),
            headers,
            body: Some(body),
        },
        _transform: Transform {
            fields: transform_fields,
        },
    };

    serde_json::to_string(&response).unwrap_or_else(|e| error_json(&format!("serialize error: {e}")))
}

fn handle_verify_receipt(input_val: serde_json::Value, config: &Config) -> String {
    let input: VerifyReceiptInput = match serde_json::from_value(input_val) {
        Ok(v) => v,
        Err(e) => return error_json(&format!("invalid input for verify_receipt: {e}")),
    };

    let session_id = &input.receipt_data;
    let url = format!("https://api.stripe.com/v1/checkout/sessions/{}", url_encode(session_id));

    let mut headers = HashMap::new();
    headers.insert("Authorization".into(), format!("Bearer {}", config.secret_key));

    // Stripe checkout session response fields:
    //   payment_status: "paid" | "unpaid" | "no_payment_required"
    //   subscription: "sub_..." (subscription ID, holds plan + current_period_end)
    //   metadata, mode, etc.
    //
    // The transform maps:
    //   payment_status == "paid" -> valid (host evaluates equality)
    //   subscription -> plan (the subscription ID; host can expand via a follow-up call)
    //   current_period_end -> expires_at (available when expanding subscription)
    let mut transform_fields = HashMap::new();
    transform_fields.insert("payment_status".into(), "valid".into());
    transform_fields.insert("subscription".into(), "plan".into());
    transform_fields.insert("current_period_end".into(), "expires_at".into());

    let response = AdapterResponse {
        _request: HttpRequest {
            url,
            method: "GET".into(),
            headers,
            body: None,
        },
        _transform: Transform {
            fields: transform_fields,
        },
    };

    serde_json::to_string(&response).unwrap_or_else(|e| error_json(&format!("serialize error: {e}")))
}

// --- Helpers ---

fn resolve_price_id(plan: &str, config: &Config) -> String {
    match plan {
        "plus" | "pro" => config.price_id_plus.clone(),
        _ => config.price_id_plus.clone(),
    }
}

fn form_urlencoded(pairs: &[(&str, &str)]) -> String {
    pairs.iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), url_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn url_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push(hex_char(byte >> 4));
                encoded.push(hex_char(byte & 0x0F));
            }
        }
    }
    encoded
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + nibble - 10) as char,
        _ => '0',
    }
}

fn error_json(msg: &str) -> String {
    format!(r#"{{"error":"{}"}}"#, msg.replace('"', "\\\""))
}
