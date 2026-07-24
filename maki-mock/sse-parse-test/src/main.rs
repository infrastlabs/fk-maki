//! 最小复现：对比 BufReader 包裹 vs 直接流式读取
//!
//! 使用 reqwest + rustls (纯 Rust，无需 OpenSSL)
//! 测试两种读取模式对 SSE 流的影响

use futures::StreamExt;
use std::time::{Duration, Instant};

const STREAM_TIMEOUT: Duration = Duration::from_secs(300);
const STREAM_DONE: &str = "[DONE]";

#[derive(Debug, serde::Deserialize)]
struct SseChunk {
    choices: Vec<ChunkChoice>,
}

#[derive(Debug, serde::Deserialize)]
struct ChunkChoice {
    delta: Option<ChunkDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ChunkDelta {
    content: Option<String>,
    #[serde(alias = "reasoning")]
    reasoning_content: Option<String>,
}

#[tokio::main]
async fn main() {
    let api_key = match std::env::var("MSCOPE_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("请设置环境变量 MSCOPE_API_KEY");
            std::process::exit(1);
        }
    };

    let model = std::env::var("MSCOPE_MODEL").unwrap_or_else(|_| "Qwen/Qwen3.5-35B-A3B".to_string());

    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": "Hello, 一句话介绍你自己"}],
        "stream": true,
        "max_tokens": 100
    });

    println!("=== BufReader vs 直接流式读取 对比测试 ===");
    println!("模型: {model}");
    println!();

    // 测试 1: 直接流式读取 (推荐方式)
    println!("--- 测试 1: 直接流式读取 (推荐) ---");
    match test_direct_stream(&api_key, &body).await {
        Ok(_) => {}
        Err(e) => println!("❌ 失败: {e}"),
    }

    println!();

    // 测试 2: BufReader 包裹 (maki 当前做法，isahc 不建议)
    println!("--- 测试 2: BufReader::new(流) [maki 当前做法] ---");
    match test_buffered_stream(&api_key, &body).await {
        Ok(_) => {}
        Err(e) => println!("❌ 失败: {e}"),
    }
}

async fn test_direct_stream(api_key: &str, body: &serde_json::Value) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("构建客户端失败: {e}"))?;

    let response = client
        .post("https://api-inference.modelscope.cn/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {api_key}"))
        .header("user-agent", "sse-test/0.1")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;

    println!("HTTP 状态: {}", response.status());
    if response.status().as_u16() != 200 {
        return Err(format!("HTTP {}", response.status()));
    }

    let mut stream = response.bytes_stream();
    let start = Instant::now();
    let mut first_byte = None;
    let mut line = String::new();
    let mut line_count = 0;
    let mut content_count = 0;

    while let Some(chunk) = tokio::time::timeout(STREAM_TIMEOUT, stream.next()).await.map_err(|_| "读取超时")? {
        let chunk = chunk.map_err(|e| format!("读取 chunk 失败: {e}"))?;
        if first_byte.is_none() {
            first_byte = Some(start.elapsed());
            println!("首字节: {:.1}s", first_byte.unwrap().as_secs_f64());
        }

        for byte in chunk {
            if byte == b'\n' {
                line_count += 1;
                let trimmed = line.trim();
                if let Some(data) = trimmed.strip_prefix("data:") {
                    let data = data.trim();
                    if data == STREAM_DONE {
                        println!("耗时: {:.1}s, 行数: {line_count}, 内容块: {content_count}", start.elapsed().as_secs_f64());
                        println!("✅ 收到内容");
                        return Ok(());
                    }
                    if let Ok(chunk) = serde_json::from_str::<SseChunk>(data) {
                        if let Some(choice) = chunk.choices.first() {
                            if let Some(delta) = &choice.delta {
                                if let Some(content) = &delta.content {
                                    if !content.is_empty() {
                                        content_count += 1;
                                    }
                                }
                            }
                        }
                    }
                }
                line.clear();
            } else {
                line.push(byte as char);
            }
        }
    }

    println!("耗时: {:.1}s, 行数: {line_count}, 内容块: {content_count}", start.elapsed().as_secs_f64());
    if content_count > 0 {
        println!("✅ 收到内容");
    } else {
        println!("⚠️  无内容");
    }
    Ok(())
}

async fn test_buffered_stream(api_key: &str, body: &serde_json::Value) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("构建客户端失败: {e}"))?;

    let response = client
        .post("https://api-inference.modelscope.cn/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {api_key}"))
        .header("user-agent", "sse-test/0.1")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;

    println!("HTTP 状态: {}", response.status());
    if response.status().as_u16() != 200 {
        return Err(format!("HTTP {}", response.status()));
    }

    // 模拟 maki 的做法：把流收集到 Vec，再用 BufReader 读取
    // 这模拟了 AsyncBody 内部缓冲 + BufReader 的双重缓冲
    let mut stream = response.bytes_stream();
    let start = Instant::now();
    let mut first_byte = None;
    let mut all_bytes: Vec<u8> = Vec::new();

    // 收集所有数据（模拟 AsyncBody 的内部缓冲）
    while let Some(chunk) = tokio::time::timeout(STREAM_TIMEOUT, stream.next()).await.map_err(|_| "读取超时")? {
        let chunk = chunk.map_err(|e| format!("读取 chunk 失败: {e}"))?;
        if first_byte.is_none() {
            first_byte = Some(start.elapsed());
            println!("首字节: {:.1}s", first_byte.unwrap().as_secs_f64());
        }
        all_bytes.extend_from_slice(&chunk);
    }

    println!("数据完全接收: {:.1}s, 总字节: {}", start.elapsed().as_secs_f64(), all_bytes.len());

    // 现在用 BufReader 读取已缓冲的数据（模拟 maki 的双重缓冲）
    let cursor = std::io::Cursor::new(&all_bytes);
    let bufreader = std::io::BufReader::new(cursor);
    let lines = std::io::BufRead::lines(bufreader);

    let mut line_count = 0;
    let mut content_count = 0;

    for line_result in lines {
        let line = line_result.map_err(|e| format!("BufReader 读取失败: {e}"))?;
        line_count += 1;
        let trimmed = line.trim();
        if let Some(data) = trimmed.strip_prefix("data:") {
            let data = data.trim();
            if data == STREAM_DONE {
                break;
            }
            if let Ok(chunk) = serde_json::from_str::<SseChunk>(data) {
                if let Some(choice) = chunk.choices.first() {
                    if let Some(delta) = &choice.delta {
                        if let Some(content) = &delta.content {
                            if !content.is_empty() {
                                content_count += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    println!("BufReader 解析: 行数: {line_count}, 内容块: {content_count}");
    if content_count > 0 {
        println!("✅ BufReader 模式收到内容");
    } else {
        println!("⚠️  BufReader 模式无内容");
    }
    Ok(())
}
