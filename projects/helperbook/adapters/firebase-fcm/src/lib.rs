use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
struct SendNotificationInput {
    user_id: String,
    title: String,
    body: String,
    data: Option<String>,
}

#[derive(Deserialize)]
struct Config {
    project_id: String,
    server_key: String,
}

#[derive(Serialize)]
struct FcmMessage {
    message: FcmMessageBody,
}

#[derive(Serialize)]
struct FcmMessageBody {
    topic: String,
    notification: FcmNotification,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<FcmData>,
}

#[derive(Serialize)]
struct FcmNotification {
    title: String,
    body: String,
}

#[derive(Serialize)]
struct FcmData {
    payload: String,
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
        "send_notification" => handle_send_notification(&args),
        _ => r#"{"error":"unknown method"}"#.to_string(),
    }
}

fn handle_send_notification(args: &[Value]) -> String {
    let input: SendNotificationInput = match args.get(1)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => return r#"{"error":"invalid input: requires user_id, title, body"}"#.to_string(),
    };

    let config: Config = match args.get(2)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => return r#"{"error":"invalid config: requires project_id, server_key"}"#.to_string(),
    };

    let url = format!(
        "https://fcm.googleapis.com/v1/projects/{}/messages:send",
        config.project_id
    );

    let fcm_body = FcmMessage {
        message: FcmMessageBody {
            topic: input.user_id,
            notification: FcmNotification {
                title: input.title,
                body: input.body,
            },
            data: input.data.map(|d| FcmData { payload: d }),
        },
    };

    let result = json!({
        "_request": {
            "method": "POST",
            "url": url,
            "headers": {
                "Authorization": format!("Bearer {}", config.server_key),
                "Content-Type": "application/json"
            },
            "body": fcm_body
        },
        "_transform": {
            "message_id": "$.name",
            "success": true
        }
    });

    serde_json::to_string(&result).unwrap()
}
