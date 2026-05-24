use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct VerifyTokenInput {
    id_token: String,
    authorization_code: String,
}

#[derive(Deserialize)]
struct Config {
    client_id: String,
    team_id: String,
    key_id: String,
    private_key: String,
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
    sub: String,
    email: String,
    name: String,
    email_verified: String,
}

#[derive(Serialize)]
struct RequestEnvelope {
    _request: HttpRequest,
    _transform: TransformSpec,
    _jwt_claims: Option<JwtClaims>,
}

#[derive(Serialize)]
struct JwtClaims {
    sub: String,
    email: String,
    email_verified: bool,
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

    let config: Config = match args.get(2) {
        Some(v) => match serde_json::from_value(v.clone()) {
            Ok(c) => c,
            Err(e) => return error_json(&format!("invalid config: {}", e)),
        },
        None => return error_json("missing config"),
    };

    if input.authorization_code.is_empty() {
        return error_json("authorization_code is required");
    }
    if input.id_token.is_empty() {
        return error_json("id_token is required");
    }
    if config.client_id.is_empty() {
        return error_json("client_id is required");
    }

    // Decode JWT id_token payload to extract claims early.
    // Apple id_tokens are JWTs with three dot-separated base64url segments:
    // header.payload.signature — we decode the payload (middle) segment.
    let jwt_claims = decode_jwt_payload(&input.id_token);

    // Build the client_secret JWT placeholder. In production the runtime
    // signs this with the private key (ES256). The adapter provides the
    // raw material so the runtime or a signing escape-hatch can construct it.
    // For the token exchange request we pass the private_key fields through
    // so the runtime HTTP layer can build the signed JWT client_secret.
    let client_secret_parts = serde_json::json!({
        "algorithm": "ES256",
        "key_id": config.key_id,
        "team_id": config.team_id,
        "client_id": config.client_id,
        "private_key": config.private_key
    });

    // Form-urlencoded body for Apple's token endpoint.
    // client_secret is a JWT that must be signed with ES256 — the adapter
    // passes the signing material as a JSON blob in _client_secret_jwt so
    // the runtime can sign it before sending.
    let body = format!(
        "client_id={}&client_secret={}&code={}&grant_type=authorization_code",
        url_encode(&config.client_id),
        url_encode("__SIGN_JWT__"),
        url_encode(&input.authorization_code),
    );

    let mut headers = HashMap::new();
    headers.insert(
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    );

    let envelope = RequestEnvelope {
        _request: HttpRequest {
            url: "https://appleid.apple.com/auth/token".to_string(),
            method: "POST".to_string(),
            headers,
            body,
        },
        _transform: TransformSpec {
            sub: "$.id_token.sub".to_string(),
            email: "$.id_token.email".to_string(),
            name: "$.id_token.name".to_string(),
            email_verified: "$.id_token.email_verified".to_string(),
        },
        _jwt_claims: jwt_claims,
    };

    // Attach the client_secret signing material as a sibling field
    let mut out: serde_json::Value = serde_json::to_value(&envelope).unwrap();
    out["_client_secret_jwt"] = client_secret_parts;
    serde_json::to_string(&out).unwrap()
}

/// Decode the payload segment of a JWT (base64url-encoded JSON).
/// Returns extracted claims if the token is well-formed, None otherwise.
fn decode_jwt_payload(token: &str) -> Option<JwtClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let payload_b64 = parts[1];
    let payload_bytes = base64url_decode(payload_b64)?;
    let payload_str = std::str::from_utf8(&payload_bytes).ok()?;
    let claims: serde_json::Value = serde_json::from_str(payload_str).ok()?;

    Some(JwtClaims {
        sub: claims.get("sub")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        email: claims.get("email")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        email_verified: claims.get("email_verified")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

/// Decode base64url (RFC 4648 section 5) without padding.
/// Handles missing padding characters that JWTs typically omit.
fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    // Convert base64url to standard base64
    let mut b64 = input.replace('-', "+").replace('_', "/");

    // Add padding if needed
    let pad = (4 - b64.len() % 4) % 4;
    for _ in 0..pad {
        b64.push('=');
    }

    base64_decode_std(&b64)
}

/// Standard base64 decoding (RFC 4648).
fn base64_decode_std(input: &str) -> Option<Vec<u8>> {
    const DECODE: [i8; 128] = {
        let mut table = [-1i8; 128];
        let mut i = 0u8;
        while i < 26 {
            table[(b'A' + i) as usize] = i as i8;
            table[(b'a' + i) as usize] = (i + 26) as i8;
            i += 1;
        }
        let mut d = 0u8;
        while d < 10 {
            table[(b'0' + d) as usize] = (d + 52) as i8;
            d += 1;
        }
        table[b'+' as usize] = 62;
        table[b'/' as usize] = 63;
        table
    };

    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);

    let chunks = bytes.chunks(4);
    for chunk in chunks {
        let mut buf = [0u8; 4];
        let mut count = 0;
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'=' {
                buf[i] = 0;
            } else {
                if b >= 128 {
                    return None;
                }
                let val = DECODE[b as usize];
                if val < 0 {
                    return None;
                }
                buf[i] = val as u8;
                count = i + 1;
            }
        }

        if count == 0 {
            break;
        }

        let triple = ((buf[0] as u32) << 18)
            | ((buf[1] as u32) << 12)
            | ((buf[2] as u32) << 6)
            | (buf[3] as u32);

        out.push((triple >> 16) as u8);
        if count > 2 {
            out.push((triple >> 8) as u8);
        }
        if count > 3 {
            out.push(triple as u8);
        }
    }

    Some(out)
}

/// RFC 3986 percent-encoding for URL components.
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

fn error_json(msg: &str) -> String {
    serde_json::to_string(&ErrorResponse {
        error: msg.to_string(),
    }).unwrap()
}
