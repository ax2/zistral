mod after;
mod before;
use serde_json::{Value, json};
use std::{hint::black_box, time::Instant};

fn fixture(api: &str, chunks: usize, width: usize) -> (Vec<u8>, String) {
    let part = "x".repeat(width - 7) + "中🙂";
    let expected = part.repeat(chunks);
    let mut wire = String::new();
    let mut event = |v: Value| {
        wire.push_str("data: ");
        wire.push_str(&v.to_string());
        wire.push_str("\n\n");
    };
    if api == "anthropic" {
        event(
            json!({"type":"message_start","message":{"id":"m","role":"assistant","stop_reason":null,"usage":{"input_tokens":7},"content":[]}}),
        );
        event(
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        );
    }
    for _ in 0..chunks {
        event(match api {
            "chat" => json!({"choices":[{"delta":{"content":part},"finish_reason":null}]}),
            "responses" => json!({"type":"response.output_text.delta","delta":part}),
            _ => {
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":part}})
            }
        });
    }
    match api {
        "chat" => {
            event(
                json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":{"completion_tokens":17}}),
            );
            wire.push_str("data: [DONE]\n\n");
        }
        "responses" => event(
            json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":expected}]}],"usage":{"output_tokens":17}}}),
        ),
        _ => {
            event(json!({"type":"content_block_stop","index":0}));
            event(
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":17}}),
            );
            event(json!({"type":"message_stop"}));
        }
    }
    (wire.into_bytes(), expected)
}

fn main() {
    let mut results = Vec::new();
    for api in ["chat", "anthropic", "responses"] {
        for (chunks, width) in [(2048, 128), (8192, 128), (8192, 512)] {
            let (wire, expected) = fixture(api, chunks, width);
            assert!(wire.len() < 16 * 1024 * 1024);
            let mut old_text = String::new();
            let old = before::decode(wire.as_slice(), api, 16 * 1024 * 1024, |t| {
                old_text.push_str(t);
                Ok(())
            })
            .unwrap();
            let mut new_text = String::new();
            let new = after::decode(wire.as_slice(), api, 16 * 1024 * 1024, |t| {
                new_text.push_str(t);
                Ok(())
            })
            .unwrap();
            assert_eq!(old, new);
            assert_eq!(old_text, expected);
            assert_eq!(new_text, expected);
            // Cut before the terminator: both decoders must retain exactly the same failure.
            let truncated = &wire[..wire.len() / 2];
            let old_error = before::decode(truncated, api, 16 * 1024 * 1024, |_| Ok(()))
                .unwrap_err()
                .to_string();
            let new_error = after::decode(truncated, api, 16 * 1024 * 1024, |_| Ok(()))
                .unwrap_err()
                .to_string();
            assert_eq!(old_error, new_error);
            let mut times = [Vec::new(), Vec::new()];
            for trial in 0..5 {
                for offset in 0..2 {
                    let index = (trial + offset) % 2;
                    let mut emitted = 0;
                    let start = Instant::now();
                    let value = if index == 0 {
                        before::decode(black_box(wire.as_slice()), api, 16 * 1024 * 1024, |t| {
                            emitted += black_box(t).len();
                            Ok(())
                        })
                    } else {
                        after::decode(black_box(wire.as_slice()), api, 16 * 1024 * 1024, |t| {
                            emitted += black_box(t).len();
                            Ok(())
                        })
                    }
                    .unwrap();
                    let elapsed = start.elapsed().as_secs_f64() * 1000.;
                    assert_eq!(emitted, expected.len());
                    black_box(value);
                    times[index].push(elapsed);
                }
            }
            for series in &mut times {
                series.sort_by(f64::total_cmp);
            }
            let row = json!({"api":api,"chunks":chunks,"fragment_utf8_bytes":width,"text_bytes":expected.len(),"wire_bytes":wire.len(),"before_ms_median":times[0][2],"after_ms_median":times[1][2],"before_ms_samples":times[0],"after_ms_samples":times[1],"value_and_callback_equal":true,"truncated_error_equal":true});
            eprintln!(
                "{api}: {} bytes / {chunks}fragments, {:.2} -> {:.2} ms",
                expected.len(),
                times[0][2],
                times[1][2]
            );
            results.push(row);
        }
    }
    println!(
        "{}",
        json!({"benchmark":"same-build full SSE decoder comparison","repetitions":5,"order":"alternating old/new","input":"in-memory controlled SSE transcripts","scope":"local decoder CPU time only, excludes provider/network/UI; not an end-to-end model latency benchmark","cases":results})
    );
}
