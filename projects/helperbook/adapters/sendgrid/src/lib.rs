use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Config {
    api_key: String,
    from_email: String,
    from_name: Option<String>,
}

#[derive(Deserialize)]
struct SendInput {
    to: String,
    subject: String,
    body: String,
    html: Option<bool>,
}

#[derive(Deserialize)]
struct SendTemplateInput {
    to: String,
    template_id: String,
    variables: Option<String>,
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
    success: bool,
    message_id: String,
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
        "send" => handle_send(&args),
        "send_template" => handle_send_template(&args),
        _ => error_json(&format!("unknown method: {}", method)),
    }
}

fn handle_send(args: &[serde_json::Value]) -> String {
    let input: SendInput = match args.get(1) {
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

    if input.to.is_empty() {
        return error_json("to is required");
    }
    if input.subject.is_empty() {
        return error_json("subject is required");
    }

    let from_name = config.from_name.as_deref().unwrap_or("Helperbook");
    let content_type = if input.html.unwrap_or(false) {
        "text/html"
    } else {
        "text/plain"
    };

    let body_json = serde_json::json!({
        "personalizations": [{
            "to": [{ "email": input.to }]
        }],
        "from": {
            "email": config.from_email,
            "name": from_name
        },
        "subject": input.subject,
        "content": [{
            "type": content_type,
            "value": input.body
        }]
    });

    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".to_string(),
        format!("Bearer {}", config.api_key),
    );
    headers.insert(
        "Content-Type".to_string(),
        "application/json".to_string(),
    );

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url: "https://api.sendgrid.com/v3/mail/send".to_string(),
            method: "POST".to_string(),
            headers,
            body: body_json.to_string(),
        },
        _transform: TransformSpec {
            success: true,
            message_id: "$.headers.x-message-id".to_string(),
        },
    };

    serde_json::to_string(&envelope).unwrap()
}

fn handle_send_template(args: &[serde_json::Value]) -> String {
    let input: SendTemplateInput = match args.get(1) {
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

    if input.to.is_empty() {
        return error_json("to is required");
    }
    if input.template_id.is_empty() {
        return error_json("template_id is required");
    }

    let from_name = config.from_name.as_deref().unwrap_or("Helperbook");

    // Parse variables from JSON string, default to empty object
    let template_data: serde_json::Value = input.variables
        .as_deref()
        .and_then(|v| serde_json::from_str(v).ok())
        .unwrap_or(serde_json::json!({}));

    let body_json = serde_json::json!({
        "personalizations": [{
            "to": [{ "email": input.to }],
            "dynamic_template_data": template_data
        }],
        "from": {
            "email": config.from_email,
            "name": from_name
        },
        "template_id": input.template_id
    });

    let mut headers = HashMap::new();
    headers.insert(
        "Authorization".to_string(),
        format!("Bearer {}", config.api_key),
    );
    headers.insert(
        "Content-Type".to_string(),
        "application/json".to_string(),
    );

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url: "https://api.sendgrid.com/v3/mail/send".to_string(),
            method: "POST".to_string(),
            headers,
            body: body_json.to_string(),
        },
        _transform: TransformSpec {
            success: true,
            message_id: "$.headers.x-message-id".to_string(),
        },
    };

    serde_json::to_string(&envelope).unwrap()
}

fn error_json(msg: &str) -> String {
    serde_json::to_string(&ErrorResponse {
        error: msg.to_string(),
    }).unwrap()
}
