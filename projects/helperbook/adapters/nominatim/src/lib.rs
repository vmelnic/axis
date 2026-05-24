use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
struct GeocodeInput {
    address: String,
}

#[derive(Deserialize)]
struct ReverseGeocodeInput {
    lat: f64,
    lng: f64,
}

#[derive(Deserialize)]
struct DistanceInput {
    lat1: f64,
    lng1: f64,
    lat2: f64,
    lng2: f64,
}

#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_nominatim_url")]
    nominatim_url: String,
    #[serde(default = "default_user_agent")]
    user_agent: String,
}

fn default_nominatim_url() -> String {
    "https://nominatim.openstreetmap.org".to_string()
}

fn default_user_agent() -> String {
    "helperbook/1.0".to_string()
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
        "geocode" => handle_geocode(&args),
        "reverse_geocode" => handle_reverse_geocode(&args),
        "distance" => handle_distance(&args),
        _ => r#"{"error":"unknown method"}"#.to_string(),
    }
}

fn parse_config(args: &[Value]) -> Config {
    args.get(2)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(Config {
            nominatim_url: default_nominatim_url(),
            user_agent: default_user_agent(),
        })
}

fn url_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push('%');
                encoded.push(hex_digit(byte >> 4));
                encoded.push(hex_digit(byte & 0x0F));
            }
        }
    }
    encoded
}

fn hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        _ => (b'A' + nibble - 10) as char,
    }
}

fn handle_geocode(args: &[Value]) -> String {
    let input: GeocodeInput = match args.get(1)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => return r#"{"error":"invalid input: requires address"}"#.to_string(),
    };

    let config = parse_config(args);
    let encoded_address = url_encode(&input.address);
    let url = format!(
        "{}/search?q={}&format=json&limit=1",
        config.nominatim_url, encoded_address
    );

    let result = json!({
        "_request": {
            "method": "GET",
            "url": url,
            "headers": {
                "User-Agent": config.user_agent
            }
        },
        "_transform": {
            "lat": "$[0].lat",
            "lng": "$[0].lon",
            "formatted_address": "$[0].display_name"
        }
    });

    serde_json::to_string(&result).unwrap()
}

fn handle_reverse_geocode(args: &[Value]) -> String {
    let input: ReverseGeocodeInput = match args.get(1)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => return r#"{"error":"invalid input: requires lat, lng"}"#.to_string(),
    };

    let config = parse_config(args);
    let url = format!(
        "{}/reverse?lat={}&lon={}&format=json",
        config.nominatim_url, input.lat, input.lng
    );

    let result = json!({
        "_request": {
            "method": "GET",
            "url": url,
            "headers": {
                "User-Agent": config.user_agent
            }
        },
        "_transform": {
            "address": "$.display_name",
            "city": "$.address.city",
            "country": "$.address.country"
        }
    });

    serde_json::to_string(&result).unwrap()
}

fn handle_distance(args: &[Value]) -> String {
    let input: DistanceInput = match args.get(1)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => return r#"{"error":"invalid input: requires lat1, lng1, lat2, lng2"}"#.to_string(),
    };

    let config = parse_config(args);
    let url = format!(
        "https://router.project-osrm.org/route/v1/driving/{},{};{},{}?overview=false",
        input.lng1, input.lat1, input.lng2, input.lat2
    );

    let result = json!({
        "_request": {
            "method": "GET",
            "url": url,
            "headers": {
                "User-Agent": config.user_agent
            }
        },
        "_transform": {
            "meters": "$.routes[0].distance",
            "duration_seconds": "$.routes[0].duration"
        }
    });

    serde_json::to_string(&result).unwrap()
}
