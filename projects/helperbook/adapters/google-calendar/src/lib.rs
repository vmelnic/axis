use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct CreateEventInput {
    access_token: String,
    title: String,
    start_at: String,
    end_at: String,
    location: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct DeleteEventInput {
    access_token: String,
    event_id: String,
}

#[derive(Deserialize)]
struct ListEventsInput {
    access_token: String,
    time_min: String,
    time_max: String,
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
        "create_event" => handle_create_event(&args),
        "delete_event" => handle_delete_event(&args),
        "list_events" => handle_list_events(&args),
        _ => r#"{"error":"unknown method"}"#.to_string(),
    }
}

fn handle_create_event(args: &[Value]) -> String {
    let input: CreateEventInput = match args.get(1)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => return r#"{"error":"invalid input: requires access_token, title, start_at, end_at"}"#.to_string(),
    };

    let mut event_body = json!({
        "summary": input.title,
        "start": { "dateTime": input.start_at },
        "end": { "dateTime": input.end_at }
    });

    if let Some(loc) = &input.location {
        event_body["location"] = json!(loc);
    }
    if let Some(desc) = &input.description {
        event_body["description"] = json!(desc);
    }

    let result = json!({
        "_request": {
            "method": "POST",
            "url": "https://www.googleapis.com/calendar/v3/calendars/primary/events",
            "headers": {
                "Authorization": format!("Bearer {}", input.access_token),
                "Content-Type": "application/json"
            },
            "body": event_body
        },
        "_transform": {
            "event_id": "$.id",
            "html_link": "$.htmlLink"
        }
    });

    serde_json::to_string(&result).unwrap()
}

fn handle_delete_event(args: &[Value]) -> String {
    let input: DeleteEventInput = match args.get(1)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => return r#"{"error":"invalid input: requires access_token, event_id"}"#.to_string(),
    };

    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/primary/events/{}",
        input.event_id
    );

    let result = json!({
        "_request": {
            "method": "DELETE",
            "url": url,
            "headers": {
                "Authorization": format!("Bearer {}", input.access_token)
            }
        },
        "_transform": {
            "success": true
        }
    });

    serde_json::to_string(&result).unwrap()
}

fn handle_list_events(args: &[Value]) -> String {
    let input: ListEventsInput = match args.get(1)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => return r#"{"error":"invalid input: requires access_token, time_min, time_max"}"#.to_string(),
    };

    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/primary/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime",
        input.time_min, input.time_max
    );

    let result = json!({
        "_request": {
            "method": "GET",
            "url": url,
            "headers": {
                "Authorization": format!("Bearer {}", input.access_token)
            }
        },
        "_transform": {
            "events": "$.items",
            "total": "$.items.length"
        }
    });

    serde_json::to_string(&result).unwrap()
}
