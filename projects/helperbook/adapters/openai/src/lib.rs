use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Config {
    api_key: String,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default = "default_moderation_model")]
    moderation_model: String,
    #[serde(default = "default_embedding_model")]
    embedding_model: String,
}

fn default_model() -> String { "gpt-4o-mini".into() }
fn default_moderation_model() -> String { "omni-moderation-latest".into() }
fn default_embedding_model() -> String { "text-embedding-3-small".into() }

#[derive(Deserialize)]
struct ModerateInput {
    text: String,
}

#[derive(Deserialize)]
struct TranslateInput {
    text: String,
    target_lang: String,
}

#[derive(Deserialize)]
struct SummarizeInput {
    text: String,
    #[serde(default = "default_max_sentences")]
    max_sentences: i32,
}

fn default_max_sentences() -> i32 { 3 }

#[derive(Deserialize)]
struct SuggestRepliesInput {
    context: String,
    role: String,
}

#[derive(Deserialize)]
struct EmbedInput {
    text: String,
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

fn handle_request(input: &str) -> String {
    let args: Vec<Value> = serde_json::from_str(input).unwrap_or_default();

    let method = args.first()
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let fields = args.get(1).cloned().unwrap_or(json!({}));
    let config_val = args.get(2).cloned().unwrap_or(json!({}));

    let config: Config = serde_json::from_value(config_val).unwrap_or(Config {
        api_key: String::new(),
        model: default_model(),
        moderation_model: default_moderation_model(),
        embedding_model: default_embedding_model(),
    });

    let result = match method {
        "moderate" => build_moderate(&fields, &config),
        "translate" => build_translate(&fields, &config),
        "summarize" => build_summarize(&fields, &config),
        "suggest_replies" => build_suggest_replies(&fields, &config),
        "embed" => build_embed(&fields, &config),
        _ => json!({"error": format!("unknown method: {}", method)}),
    };

    serde_json::to_string(&result).unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.into())
}

fn auth_header(api_key: &str) -> Value {
    json!({
        "Authorization": format!("Bearer {}", api_key),
        "Content-Type": "application/json"
    })
}

fn build_moderate(fields: &Value, config: &Config) -> Value {
    let input: ModerateInput = serde_json::from_value(fields.clone())
        .unwrap_or(ModerateInput { text: String::new() });

    json!({
        "_request": {
            "method": "POST",
            "url": "https://api.openai.com/v1/moderations",
            "headers": auth_header(&config.api_key),
            "body": {
                "input": input.text,
                "model": config.moderation_model
            }
        },
        "_transform": {
            "flagged": "results.0.flagged",
            "categories": "results.0.categories"
        }
    })
}

fn build_translate(fields: &Value, config: &Config) -> Value {
    let input: TranslateInput = serde_json::from_value(fields.clone())
        .unwrap_or(TranslateInput { text: String::new(), target_lang: "en".into() });

    json!({
        "_request": {
            "method": "POST",
            "url": "https://api.openai.com/v1/chat/completions",
            "headers": auth_header(&config.api_key),
            "body": {
                "model": config.model,
                "messages": [
                    {
                        "role": "system",
                        "content": format!(
                            "Translate to {}. Return only the translation.",
                            input.target_lang
                        )
                    },
                    {
                        "role": "user",
                        "content": input.text
                    }
                ]
            }
        },
        "_transform": {
            "translated": "choices.0.message.content"
        }
    })
}

fn build_summarize(fields: &Value, config: &Config) -> Value {
    let input: SummarizeInput = serde_json::from_value(fields.clone())
        .unwrap_or(SummarizeInput { text: String::new(), max_sentences: default_max_sentences() });

    json!({
        "_request": {
            "method": "POST",
            "url": "https://api.openai.com/v1/chat/completions",
            "headers": auth_header(&config.api_key),
            "body": {
                "model": config.model,
                "messages": [
                    {
                        "role": "system",
                        "content": format!(
                            "Summarize in at most {} sentences.",
                            input.max_sentences
                        )
                    },
                    {
                        "role": "user",
                        "content": input.text
                    }
                ]
            }
        },
        "_transform": {
            "summary": "choices.0.message.content"
        }
    })
}

fn build_suggest_replies(fields: &Value, config: &Config) -> Value {
    let input: SuggestRepliesInput = serde_json::from_value(fields.clone())
        .unwrap_or(SuggestRepliesInput { context: String::new(), role: "assistant".into() });

    json!({
        "_request": {
            "method": "POST",
            "url": "https://api.openai.com/v1/chat/completions",
            "headers": auth_header(&config.api_key),
            "body": {
                "model": config.model,
                "messages": [
                    {
                        "role": "system",
                        "content": format!(
                            "Given this conversation context, suggest 3 professional replies as a {}. Return as JSON array.",
                            input.role
                        )
                    },
                    {
                        "role": "user",
                        "content": input.context
                    }
                ]
            }
        },
        "_transform": {
            "suggestions": "choices.0.message.content"
        }
    })
}

fn build_embed(fields: &Value, config: &Config) -> Value {
    let input: EmbedInput = serde_json::from_value(fields.clone())
        .unwrap_or(EmbedInput { text: String::new() });

    json!({
        "_request": {
            "method": "POST",
            "url": "https://api.openai.com/v1/embeddings",
            "headers": auth_header(&config.api_key),
            "body": {
                "input": input.text,
                "model": config.embedding_model
            }
        },
        "_transform": {
            "embedding": "data.0.embedding"
        }
    })
}
