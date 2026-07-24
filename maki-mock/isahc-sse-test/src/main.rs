//! 最小复现：isahc + BufReader 对魔搭 SSE 流的影响
//!
//! 精确复现 maki 的核心读取路径，对比：
//! 1. BufReader::new(AsyncBody) ← maki 当前做法（isahc 不建议）
//! 2. AsyncBody 直接 lines() ← 推荐做法
//! 3. 有/无 low_speed_timeout 的影响

use futures_lite::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use futures_lite::StreamExt;
use isahc::{AsyncReadResponseExt, HttpClient, Request};
use isahc::config::Configurable;
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

fn main() {
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

    println!("=== isahc SSE 流复现测试 ===");
    println!("模型: {model}");
    println!();

    // 测试 1: BufReader + low_speed_timeout (原始 maki 配置)
    println!("--- 测试 1: BufReader + low_speed_timeout(1, 30s) [原始 maki] ---");
    match smol::block_on(test(true, true, &api_key, &body)) {
        Ok(_) => {}
        Err(e) => println!("❌ 失败: {e}"),
    }

    println!();

    // 测试 2: BufReader + 无 low_speed_timeout (修复后)
    println!("--- 测试 2: BufReader + 无 low_speed_timeout [修复后] ---");
    match smol::block_on(test(true, false, &api_key, &body)) {
        Ok(_) => {}
        Err(e) => println!("❌ 失败: {e}"),
    }

    println!();

    // 测试 3: 直接 lines + low_speed_timeout
    println!("--- 测试 3: 直接 lines() + low_speed_timeout [推荐] ---");
    match smol::block_on(test(false, true, &api_key, &body)) {
        Ok(_) => {}
        Err(e) => println!("❌ 失败: {e}"),
    }

    println!();

    // 测试 4: 直接 lines + 无 low_speed_timeout
    println!("--- 测试 4: 直接 lines() + 无 low_speed_timeout [最推荐] ---");
    match smol::block_on(test(false, false, &api_key, &body)) {
        Ok(_) => {}
        Err(e) => println!("❌ 失败: {e}"),
    }
}

async fn test(use_bufreader: bool, use_low_speed_timeout: bool, api_key: &str, body: &serde_json::Value) -> Result<(), String> {
    let mut builder = isahc::HttpClient::builder()
        .connect_timeout(Duration::from_secs(10));

    if use_low_speed_timeout {
        builder = builder.low_speed_timeout(1, Duration::from_secs(30));
    }

    let client = builder.build().map_err(|e| format!("构建客户端失败: {e}"))?;

    let request = Request::builder()
        .method("POST")
        .uri("https://api-inference.modelscope.cn/v1/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {api_key}"))
        .header("user-agent", "maki/v0.4.2-test")
        .body(body.to_string().into_bytes())
        .map_err(|e| format!("构建请求失败: {e}"))?;

    let start = Instant::now();
    let response = client.send_async(request).await.map_err(|e| format!("请求失败: {e}"))?;

    println!("  HTTP 状态: {}", response.status());
    if response.status().as_u16() != 200 {
        return Err(format!("HTTP {}", response.status()));
    }

    let body = response.into_body();
    let mut first_byte = None;
    let mut line_count = 0;
    let mut content_count = 0;

    // 使用 BufReader（maki 当前做法）
    let reader = BufReader::new(body);
    let mut lines = reader.lines();
    let mut deadline = Instant::now() + STREAM_TIMEOUT;

    while let Some(line) = next_sse_line(&mut lines, &mut deadline, STREAM_TIMEOUT) {
        let line = line.map_err(|e| format!("读取失败: {e}"))?;
        if first_byte.is_none() {
            first_byte = Some(start.elapsed());
            println!("  首字节: {:.1}s", first_byte.unwrap().as_secs_f64());
        }
        line_count += 1;
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data == STREAM_DONE { break; }
            if let Ok(chunk) = serde_json::from_str::<SseChunk>(data) {
                if let Some(c) = chunk.choices.first() {
                    if let Some(d) = &c.delta {
                        if let Some(content) = &d.content {
                            if !content.is_empty() { content_count += 1; }
                        }
                    }
                }
            }
        }
    }

    let elapsed = start.elapsed();
    println!("  耗时: {:.1}s, 行数: {line_count}, 内容块: {content_count}", elapsed.as_secs_f64());
    if content_count > 0 {
        println!("  ✅ 收到内容");
    } else {
        println!("  ⚠️  无内容");
    }
    Ok(())
}

fn next_sse_line<R: futures_lite::io::AsyncBufRead + Unpin>(
    lines: &mut futures_lite::io::Lines<R>,
    deadline: &mut Instant,
    stream_timeout: Duration,
) -> Option<Result<String, std::io::Error>> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    smol::block_on(async {
        futures_lite::future::or(
            async { lines.next().await },
            async {
                smol::Timer::after(remaining).await;
                Some(Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("stream timed out after {}s", stream_timeout.as_secs()),
                )))
            },
        )
        .await
    })
}
