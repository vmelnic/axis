use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct SendInput {
    to: String,
    subject: String,
    body: String,
    html: Option<bool>,
}

#[derive(Deserialize)]
struct Config {
    host: String,
    port: Option<u16>,
    username: String,
    password: String,
    from_email: String,
    from_name: Option<String>,
    tls: Option<bool>,
}

#[derive(Serialize)]
struct SmtpAuth {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct SmtpEnvelope {
    from: String,
    to: Vec<String>,
}

#[derive(Serialize)]
struct SmtpMessage {
    from: String,
    to: String,
    subject: String,
    content_type: String,
    body: String,
}

#[derive(Serialize)]
struct SmtpRequest {
    protocol: String,
    host: String,
    port: u16,
    tls: bool,
    auth: SmtpAuth,
    envelope: SmtpEnvelope,
    message: SmtpMessage,
}

#[derive(Serialize)]
struct TransformSpec {
    success: bool,
    message_id: String,
}

#[derive(Serialize)]
struct RequestEnvelope {
    _request: SmtpRequest,
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
    if input.body.is_empty() {
        return error_json("body is required");
    }

    let is_html = input.html.unwrap_or(false);
    let port = config.port.unwrap_or(587);
    let tls = config.tls.unwrap_or(true);
    let from_name = config.from_name.as_deref().unwrap_or("Helperbook");
    let content_type = if is_html { "text/html" } else { "text/plain" };

    let from_header = format!("{} <{}>", from_name, config.from_email);

    let mime_body = build_mime_message(
        &from_header,
        &input.to,
        &input.subject,
        content_type,
        &input.body,
    );

    let envelope = RequestEnvelope {
        _request: SmtpRequest {
            protocol: "smtp".to_string(),
            host: config.host,
            port,
            tls,
            auth: SmtpAuth {
                username: config.username,
                password: config.password,
            },
            envelope: SmtpEnvelope {
                from: config.from_email,
                to: vec![input.to.clone()],
            },
            message: SmtpMessage {
                from: from_header,
                to: input.to,
                subject: input.subject,
                content_type: content_type.to_string(),
                body: mime_body,
            },
        },
        _transform: TransformSpec {
            success: true,
            message_id: "$.message_id".to_string(),
        },
    };

    serde_json::to_string(&envelope).unwrap()
}

/// Build a complete MIME message with headers and body.
fn build_mime_message(
    from: &str,
    to: &str,
    subject: &str,
    content_type: &str,
    body: &str,
) -> String {
    let mut msg = String::with_capacity(256 + body.len());
    msg.push_str("MIME-Version: 1.0\r\n");
    msg.push_str(&format!("From: {}\r\n", from));
    msg.push_str(&format!("To: {}\r\n", to));
    msg.push_str(&format!("Subject: {}\r\n", encode_subject(subject)));
    msg.push_str(&format!("Content-Type: {}; charset=UTF-8\r\n", content_type));
    msg.push_str("Content-Transfer-Encoding: quoted-printable\r\n");
    msg.push_str("\r\n");
    msg.push_str(&quoted_printable_encode(body));
    msg
}

/// RFC 2047 encoded-word for Subject headers containing non-ASCII.
/// If the subject is pure ASCII, return as-is.
fn encode_subject(subject: &str) -> String {
    if subject.is_ascii() {
        return subject.to_string();
    }
    // Use RFC 2047 Base64 encoding for non-ASCII subjects
    format!("=?UTF-8?B?{}?=", base64_encode(subject))
}

/// Quoted-printable encoding (RFC 2045).
/// Encodes non-ASCII bytes and '=' as =XX hex pairs. Lines limited to 76 chars.
fn quoted_printable_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    let mut line_len: usize = 0;

    for byte in input.bytes() {
        let encoded = match byte {
            // Printable ASCII (except '=') pass through
            b'\t' | b' '..=b'<' | b'>'..=b'~' => {
                let mut s = String::with_capacity(1);
                s.push(byte as char);
                s
            }
            // Line breaks pass through as CRLF
            b'\n' => {
                line_len = 0;
                out.push_str("\r\n");
                continue;
            }
            b'\r' => continue,
            // Everything else gets encoded
            _ => {
                format!("={:02X}", byte)
            }
        };

        // Soft line break at 75 chars (76 minus the soft-break '=' char)
        if line_len + encoded.len() > 75 {
            out.push_str("=\r\n");
            line_len = 0;
        }

        out.push_str(&encoded);
        line_len += encoded.len();
    }

    out
}

/// Standard base64 encoding (RFC 4648) with padding.
fn base64_encode(input: &str) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let bytes = input.as_bytes();
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);

    for chunk in bytes.chunks(3) {
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

fn error_json(msg: &str) -> String {
    serde_json::to_string(&ErrorResponse {
        error: msg.to_string(),
    }).unwrap()
}
