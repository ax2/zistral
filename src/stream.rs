//! Bounded SSE decoding for the three supported wire protocols; no extra runtime.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};

pub fn decode(
    reader: impl Read,
    api: &str,
    limit: u64,
    mut text: impl FnMut(&str) -> Result<()>,
) -> Result<Value> {
    let mut reader = BufReader::new(reader.take(limit + 1));
    let mut total = 0u64;
    let mut data = String::new();
    let mut state = State::new(api);
    loop {
        let mut bytes = Vec::new();
        let n = reader
            .read_until(b'\n', &mut bytes)
            .context("cannot read response stream")?;
        total += n as u64;
        if total > limit {
            bail!("API response stream exceeds 16 MiB limit");
        }
        let line = std::str::from_utf8(&bytes)
            .context("response stream is not UTF-8")?
            .trim_end_matches(['\r', '\n']);
        if line.is_empty() || n == 0 {
            if !data.is_empty() {
                if state.event(data.trim_end_matches('\n'), &mut text)? {
                    return state.finish();
                }
                data.clear();
            }
            if n == 0 {
                bail!(
                    "response stream ended before completion; partial streamed text may have been shown"
                );
            }
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
            data.push('\n');
        }
    }
}

fn append(value: &mut Value, key: &str, text: &str) {
    match &mut value[key] {
        Value::String(existing) => existing.push_str(text),
        slot => *slot = json!(text),
    }
}

struct State<'a> {
    api: &'a str,
    result: Value,
    arguments: Vec<String>,
}

impl<'a> State<'a> {
    fn new(api: &'a str) -> Self {
        Self {
            api,
            result: if api == "chat" {
                json!({"choices":[{"message":{"content":"","tool_calls":[]},"finish_reason":null}]})
            } else {
                json!({"content":[],"usage":{}})
            },
            arguments: Vec::new(),
        }
    }

    fn event(&mut self, data: &str, text: &mut impl FnMut(&str) -> Result<()>) -> Result<bool> {
        if data == "[DONE]" {
            if self.api != "chat" {
                bail!("unexpected stream terminator");
            }
            return Ok(true);
        }
        let event: Value =
            serde_json::from_str(data).context("invalid JSON event in response stream")?;
        if event.get("error").is_some() || event["type"] == "error" {
            bail!("provider reported a stream error; response details hidden");
        }
        match self.api {
            "chat" => {
                if event["usage"].is_object() {
                    self.result["usage"] = event["usage"].clone();
                }
                let Some(choice) = event.pointer("/choices/0") else {
                    return Ok(false);
                };
                if choice["index"].as_u64().is_some_and(|i| i != 0) {
                    return Ok(false);
                }
                let delta = &choice["delta"];
                if delta["refusal"].as_str().is_some_and(|s| !s.is_empty()) {
                    bail!("provider declined the request");
                }
                let message = &mut self.result["choices"][0]["message"];
                if let Some(value) = delta.get("content").filter(|v| !v.is_null()) {
                    let s = value.as_str().context("invalid streamed Chat text")?;
                    append(message, "content", s);
                    text(s)?;
                }
                if delta
                    .get("tool_calls")
                    .is_some_and(|calls| !calls.is_null() && !calls.is_array())
                {
                    bail!("streamed tool_calls must be an array");
                }
                if let Some(calls) = delta["tool_calls"].as_array() {
                    for call in calls {
                        let i = call["index"]
                            .as_u64()
                            .context("streamed tool call has no index")?;
                        if i >= 32 {
                            bail!("streamed tool index exceeds 31");
                        }
                        let calls = message["tool_calls"].as_array_mut().expect("tool array");
                        while calls.len() <= i as usize {
                            calls.push(json!({"id":"","type":"function","function":{"name":"","arguments":null}}));
                        }
                        let target = &mut calls[i as usize];
                        if let Some(value) = call.get("id").filter(|v| !v.is_null()) {
                            append(
                                target,
                                "id",
                                value.as_str().context("invalid streamed tool id")?,
                            );
                        }
                        for key in ["name", "arguments"] {
                            if let Some(value) = call["function"].get(key).filter(|v| !v.is_null())
                            {
                                append(
                                    &mut target["function"],
                                    key,
                                    value
                                        .as_str()
                                        .context("invalid streamed tool function field")?,
                                );
                            }
                        }
                    }
                }
                if !choice["finish_reason"].is_null() {
                    self.result["choices"][0]["finish_reason"] = choice["finish_reason"].clone();
                }
            }
            "responses" => match event["type"].as_str() {
                Some("response.output_text.delta") => {
                    text(event["delta"].as_str().context("missing text delta")?)?;
                }
                Some("response.completed") => {
                    self.result = event
                        .get("response")
                        .context("missing completed response")?
                        .clone();
                    return Ok(true);
                }
                Some(
                    "response.failed"
                    | "response.incomplete"
                    | "response.error"
                    | "response.refusal.delta",
                ) => bail!("provider returned an incomplete or declined response stream"),
                _ => (),
            },
            "anthropic" => match event["type"].as_str() {
                Some("message_start") => {
                    self.result = event
                        .get("message")
                        .context("missing streamed message")?
                        .clone();
                    self.result["content"] = json!([]);
                }
                Some("content_block_start") => {
                    let i = index(&event)?;
                    let blocks = self.result["content"]
                        .as_array_mut()
                        .context("invalid streamed message content")?;
                    while blocks.len() <= i {
                        blocks.push(Value::Null);
                    }
                    if !blocks[i].is_null() {
                        bail!("duplicate stream content block");
                    }
                    blocks[i] = event
                        .get("content_block")
                        .context("missing content block")?
                        .clone();
                    while self.arguments.len() <= i {
                        self.arguments.push(String::new());
                    }
                    if blocks[i]["type"] == "text" {
                        if let Some(s) = blocks[i]["text"].as_str() {
                            text(s)?;
                        }
                    }
                }
                Some("content_block_delta") => {
                    let i = index(&event)?;
                    let block = self.result["content"]
                        .get_mut(i)
                        .context("delta before content block")?;
                    let delta = &event["delta"];
                    match delta["type"].as_str() {
                        Some("text_delta") => {
                            let s = delta["text"].as_str().context("missing text delta")?;
                            append(block, "text", s);
                            text(s)?;
                        }
                        Some("input_json_delta") => {
                            self.arguments
                                .get_mut(i)
                                .context("tool delta before content block")?
                                .push_str(
                                    delta["partial_json"]
                                        .as_str()
                                        .context("missing tool JSON delta")?,
                                );
                        }
                        Some("thinking_delta") => append(
                            block,
                            "thinking",
                            delta["thinking"].as_str().unwrap_or_default(),
                        ),
                        Some("signature_delta") => append(
                            block,
                            "signature",
                            delta["signature"].as_str().unwrap_or_default(),
                        ),
                        _ => (),
                    }
                }
                Some("content_block_stop") => {
                    let i = index(&event)?;
                    if let Some(args) = self.arguments.get(i).filter(|s| !s.is_empty()) {
                        self.result["content"][i]["input"] = serde_json::from_str(args)
                            .context("invalid streamed tool arguments")?;
                    }
                }
                Some("message_delta") => {
                    if let Some(reason) = event["delta"].get("stop_reason") {
                        self.result["stop_reason"] = reason.clone();
                    }
                    if let Some(usage) = event["usage"].as_object() {
                        for (k, v) in usage {
                            self.result["usage"][k] = v.clone();
                        }
                    }
                }
                Some("message_stop") => return Ok(true),
                _ => (),
            },
            _ => bail!("unsupported streaming protocol"),
        }
        Ok(false)
    }

    fn finish(self) -> Result<Value> {
        if self.api == "chat" && self.result["choices"][0]["finish_reason"].is_null() {
            bail!("response stream has no finish reason");
        }
        Ok(self.result)
    }
}

fn index(event: &Value) -> Result<usize> {
    let i = event["index"]
        .as_u64()
        .context("streamed block has no index")?;
    if i >= 128 {
        bail!("stream content block limit exceeded");
    }
    Ok(i as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sse_comments_multiline_unicode_and_truncation() {
        let mut text = String::new();
        let s = concat!(
            ": ping\r\n\r\ndata: {\"choices\":\n",
            "data: [{\"delta\":{\"content\":\"你好\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
        );
        let result = decode(s.as_bytes(), "chat", 4096, |s| {
            text.push_str(s);
            Ok(())
        })
        .unwrap();
        assert_eq!(text, "你好");
        assert_eq!(result["choices"][0]["message"]["content"], "你好");
        assert!(decode(b"data: {}\n\n".as_slice(), "chat", 100, |_| Ok(())).is_err());
        assert!(decode(s.as_bytes(), "chat", 4, |_| Ok(())).is_err());
    }
}

#[cfg(test)]
mod accumulation_regressions {
    use super::*;
    fn event(value: Value) -> String {
        format!("data: {value}\n\n")
    }
    #[test]
    fn fragmented_chat_keeps_unicode_text_tool_ids_names_and_arguments() {
        let mut wire =
            event(json!({"choices":[{"delta":{"content":"你好"},"finish_reason":null}]}));
        wire += &event(
            json!({"choices":[{"delta":{"content":"🙂","tool_calls":[{"index":0,"id":"call_","function":{"name":"trans","arguments":"{\"operation\":"}}]},"finish_reason":null}]}),
        );
        wire += &event(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"7","function":{"name":"form_input","arguments":"\"trim\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"completion_tokens":13}}),
        );
        wire += "data: [DONE]\n\n";
        let mut text = String::new();
        let value = decode(wire.as_bytes(), "chat", 16384, |s| {
            text.push_str(s);
            Ok(())
        })
        .unwrap();
        assert_eq!(text, "你好🙂");
        let message = &value["choices"][0]["message"];
        assert_eq!(message["content"], text);
        assert_eq!(message["tool_calls"][0]["id"], "call_7");
        assert_eq!(
            message["tool_calls"][0]["function"]["name"],
            "transform_input"
        );
        assert_eq!(
            message["tool_calls"][0]["function"]["arguments"],
            "{\"operation\":\"trim\"}"
        );
        assert_eq!(value["usage"]["completion_tokens"], 13);
        assert_eq!(value["choices"][0]["finish_reason"], "tool_calls");
    }
    #[test]
    fn fragmented_anthropic_keeps_text_thinking_signature_and_usage() {
        let mut wire = event(
            json!({"type":"message_start","message":{"id":"m","role":"assistant","usage":{"input_tokens":7},"content":[]}}),
        );
        wire += &event(
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"prefix "}}),
        );
        for part in ["你好", "🙂", ""] {
            wire += &event(
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":part}}),
            );
        }
        wire += &event(json!({"type":"content_block_stop","index":0}));
        wire += &event(
            json!({"type":"content_block_start","index":1,"content_block":{"type":"thinking","thinking":null,"signature":5}}),
        );
        for (kind, key, parts) in [
            ("thinking_delta", "thinking", ["why", " next"]),
            ("signature_delta", "signature", ["sig", "nature"]),
        ] {
            for part in parts {
                let mut delta = json!({"type":kind});
                delta[key] = json!(part);
                wire += &event(json!({"type":"content_block_delta","index":1,"delta":delta}));
            }
        }
        wire += &event(json!({"type":"content_block_stop","index":1}));
        wire += &event(
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":11}}),
        );
        wire += &event(json!({"type":"message_stop"}));
        let mut text = String::new();
        let value = decode(wire.as_bytes(), "anthropic", 16384, |s| {
            text.push_str(s);
            Ok(())
        })
        .unwrap();
        assert_eq!(text, "prefix 你好🙂");
        assert_eq!(value["content"][0]["text"], text);
        assert_eq!(value["content"][1]["thinking"], "why next");
        assert_eq!(value["content"][1]["signature"], "signature");
        assert_eq!(value["usage"], json!({"input_tokens":7,"output_tokens":11}));
        assert_eq!(value["stop_reason"], "end_turn");
    }
}
