use std::time::{Duration, Instant};

use flume::Sender;
use futures_lite::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use isahc::{AsyncReadResponseExt, HttpClient, Request};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, warn};

use super::ResolvedAuth;
use crate::{
    AgentError, ContentBlock, Message, ProviderEvent, Role, StopReason, StreamResponse, TokenUsage,
};

const STREAM_DONE: &str = "[DONE]";

pub(crate) struct OpenAiCompatConfig {
    pub slug: &'static str,
    pub api_key_env: &'static str,
    pub base_url: &'static str,
    pub max_tokens_field: &'static str,
    pub include_stream_usage: bool,
    pub provider_name: &'static str,
}

pub(crate) struct OpenAiCompatProvider {
    client: HttpClient,
    config: &'static OpenAiCompatConfig,
    stream_timeout: Duration,
}

impl OpenAiCompatProvider {
    pub fn new(config: &'static OpenAiCompatConfig, timeouts: super::Timeouts) -> Self {
        Self {
            client: super::http_client(timeouts),
            config,
            stream_timeout: timeouts.stream,
        }
    }

    pub(crate) fn client(&self) -> &HttpClient {
        &self.client
    }

    pub(crate) fn config(&self) -> &'static OpenAiCompatConfig {
        self.config
    }

    pub(crate) fn stream_timeout(&self) -> Duration {
        self.stream_timeout
    }

    pub(crate) async fn get_text(
        &self,
        auth: &ResolvedAuth,
        url: &str,
    ) -> Result<String, AgentError> {
        let request = auth
            .configure_request(
                Request::builder()
                    .method("GET")
                    .uri(url)
                    .header("user-agent", super::user_agent()),
            )
            .body(())?;
        let mut response = self.client.send_async(request).await?;
        if response.status().as_u16() != 200 {
            return Err(AgentError::from_response(response).await);
        }
        Ok(response.text().await?)
    }

    pub(crate) async fn post_text(
        &self,
        auth: &ResolvedAuth,
        url: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<String, AgentError> {
        let mut builder = Request::builder()
            .method("POST")
            .uri(url)
            .header("user-agent", super::user_agent());
        for (key, value) in &auth.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }
        let request = builder
            .header("content-type", content_type)
            .body(body.to_vec())?;
        let mut response = self.client.send_async(request).await?;
        if response.status().as_u16() != 200 {
            return Err(AgentError::from_response(response).await);
        }
        Ok(response.text().await?)
    }

    pub fn build_body(
        &self,
        model: &crate::model::Model,
        messages: &[Message],
        system: &str,
        tools: &Value,
    ) -> Value {
        let wire_messages = convert_messages(messages, system);
        let wire_tools = convert_tools(tools);

        let mut body = json!({
            "model": model.id,
            "messages": wire_messages,
            "stream": true,
        });
        if let Some(max_output) = model.max_output_tokens {
            body[self.config.max_tokens_field] = json!(max_output);
        }
        if self.config.include_stream_usage {
            body["stream_options"] = json!({"include_usage": true});
        }
        if wire_tools.as_array().is_some_and(|a| !a.is_empty()) {
            body["tools"] = wire_tools;
        }
        body
    }

    /// Effective base URL: an auth-supplied value (dynamic/custom providers)
    /// wins, then the `<SLUG>_BASE_URL` env override, then the static default.
    fn base_url(&self, auth: &ResolvedAuth) -> String {
        if let Some(explicit) = auth.base_url.as_deref() {
            return explicit.to_string();
        }
        maki_config::providers::base_url_override(self.config.slug)
            .unwrap_or_else(|| self.config.base_url.to_string())
    }

    fn build_request(
        &self,
        method: &str,
        path: &str,
        auth: &ResolvedAuth,
    ) -> isahc::http::request::Builder {
        let base = self.base_url(auth);
        auth.configure_request(
            Request::builder()
                .method(method)
                .uri(format!("{base}{path}"))
                .header("user-agent", super::user_agent()),
        )
    }

    pub async fn do_stream(
        &self,
        model: &crate::model::Model,
        extra_headers: &[(&str, &str)],
        body: &Value,
        event_tx: &Sender<ProviderEvent>,
        auth: &ResolvedAuth,
    ) -> Result<StreamResponse, AgentError> {
        let json_body = serde_json::to_vec(body)?;
        let mut request = self
            .build_request("POST", "/chat/completions", auth)
            .header("content-type", "application/json");
        for &(key, value) in extra_headers {
            request = request.header(key, value);
        }

        let request = request.body(json_body)?;

        debug!(
            model = %model.id,
            provider = self.config.provider_name,
            "sending API request"
        );

        let response = self.client.send_async(request).await?;
        let status = response.status().as_u16();
        debug!("[SSE] response status: {}", status);

        if status == 200 {
            match parse_sse(
                BufReader::new(response.into_body()),
                event_tx,
                self.stream_timeout,
            )
            .await
            {
                Ok(resp) => {
                    debug!(
                        text_len = resp.message.first_text_content().map_or(0, |t| t.len()),
                        content_blocks = resp.message.content.len(),
                        "[SSE] parse_sse Ok"
                    );
                    Ok(resp)
                }
                Err(e) => {
                    debug!(error = %e, "[SSE] parse_sse error");
                    Err(e)
                }
            }
        } else {
            Err(AgentError::from_response(response).await)
        }
    }

    pub async fn fetch_and_parse_models(
        &self,
        auth: &ResolvedAuth,
        parse_fn: impl Fn(&Value) -> Option<crate::model::ModelInfo>,
    ) -> Result<Vec<crate::model::ModelInfo>, AgentError> {
        let base = self.base_url(auth);
        let url = format!("{base}/models");
        let body_text = self.get_text(auth, &url).await?;
        let body: Value = serde_json::from_str(&body_text)?;

        let mut models: Vec<crate::model::ModelInfo> = body["data"]
            .as_array()
            .map(|arr| arr.iter().filter_map(parse_fn).collect())
            .unwrap_or_default();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }

    fn default_model_parser(m: &Value) -> Option<crate::model::ModelInfo> {
        let id = m["id"].as_str()?;
        let context_window = m["context_length"]
            .as_u64()
            .or_else(|| m["max_model_len"].as_u64())
            .or_else(|| m["max_context_length"].as_u64())
            .and_then(|v| u32::try_from(v).ok());
        let max_output_tokens = m["max_tokens"].as_u64().and_then(|v| u32::try_from(v).ok());
        let pricing = m["pricing"]
            .as_object()
            .and_then(|p| {
                Some(crate::model::ModelPricing {
                    input: p.get("prompt")?.as_str()?.parse().ok()?,
                    output: p.get("completion")?.as_str()?.parse().ok()?,
                    cache_write: p
                        .get("cache_creation")?
                        .as_str()?
                        .parse::<f64>()
                        .ok()
                        .unwrap_or(0.0),
                    cache_read: p
                        .get("cache_read")?
                        .as_str()?
                        .parse::<f64>()
                        .ok()
                        .unwrap_or(0.0),
                    fast: None,
                })
            })
            .unwrap_or_default();
        Some(crate::model::ModelInfo {
            id: id.to_string(),
            context_window,
            max_output_tokens,
            pricing: Some(pricing),
            supports_thinking: None,
            supports_vision: None,
            provider_info: None,
        })
    }

    pub async fn do_list_models(
        &self,
        auth: &ResolvedAuth,
    ) -> Result<Vec<crate::model::ModelInfo>, AgentError> {
        self.fetch_and_parse_models(auth, Self::default_model_parser)
            .await
    }
}

pub fn convert_messages(messages: &[Message], system: &str) -> Vec<Value> {
    let mut out = vec![json!({"role": "system", "content": system})];

    for msg in messages {
        match msg.role {
            Role::User => {
                let mut tool_results = Vec::new();
                let mut text_parts: Vec<&str> = Vec::new();
                let mut image_parts = Vec::new();

                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } => text_parts.push(text.as_str()),
                        ContentBlock::Image { source } => {
                            image_parts.push(json!({
                                "type": "image_url",
                                "image_url": { "url": source.to_data_url() }
                            }));
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } => {
                            tool_results.push(json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": content,
                            }));
                        }
                        ContentBlock::ToolUse { .. }
                        | ContentBlock::Thinking { .. }
                        | ContentBlock::RedactedThinking { .. } => {}
                    }
                }

                // Tool messages must directly follow the assistant's
                // tool_calls, before any user content.
                out.extend(tool_results);
                if !image_parts.is_empty() {
                    let mut parts = image_parts;
                    if !text_parts.is_empty() {
                        parts.push(json!({"type": "text", "text": text_parts.join("\n")}));
                    }
                    out.push(json!({"role": "user", "content": parts}));
                } else if !text_parts.is_empty() {
                    out.push(json!({"role": "user", "content": text_parts.join("\n")}));
                }
            }
            Role::Assistant => {
                let mut text = String::new();
                let mut reasoning_text = String::new();
                let mut tool_calls = Vec::new();

                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text: t } => text.push_str(t),
                        ContentBlock::Thinking { thinking, .. } => {
                            reasoning_text.push_str(thinking);
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            tool_calls.push(json!({
                                "id": id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": input.to_string(),
                                }
                            }));
                        }
                        ContentBlock::ToolResult { .. }
                        | ContentBlock::Image { .. }
                        | ContentBlock::RedactedThinking { .. } => {}
                    }
                }

                if !text.is_empty() || !tool_calls.is_empty() || !reasoning_text.is_empty() {
                    // Always emit string `content` (""): some OpenAI-compatible
                    // backends (e.g. Cloudflare Workers AI gpt-oss) reject
                    // omitted/null content on assistant tool-call messages.
                    let mut msg_obj = json!({"role": "assistant", "content": text});
                    if !reasoning_text.is_empty() {
                        msg_obj["reasoning_content"] = Value::String(reasoning_text);
                    }
                    if !tool_calls.is_empty() {
                        msg_obj["tool_calls"] = Value::Array(tool_calls);
                    }
                    out.push(msg_obj);
                }
            }
        }
    }

    out
}

pub fn convert_tools(anthropic_tools: &Value) -> Value {
    let Some(tools) = anthropic_tools.as_array() else {
        return json!([]);
    };

    Value::Array(
        tools
            .iter()
            .filter_map(|t| {
                Some(json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name")?,
                        "description": t.get("description")?,
                        "parameters": t.get("input_schema")?,
                    }
                }))
            })
            .collect(),
    )
}

#[derive(Deserialize)]
struct ToolCallDelta {
    index: usize,
    id: Option<String>,
    name: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct ChunkDelta {
    content: Option<ContentDelta>,
    #[serde(alias = "reasoning")]
    reasoning_content: Option<String>,
    tool_calls: Option<Vec<ToolCallDelta>>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum ContentDelta {
    Array(Vec<ContentDeltaPart>),
    String(String),
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ContentDeltaPart {
    Text { text: String },
    Thinking { thinking: Vec<ThinkingDelta> },
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum ThinkingDelta {
    Block(ThinkingDeltaBlock),
    String(String),
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ThinkingDeltaBlock {
    Text { text: String },
}

#[derive(Deserialize)]
struct ChunkChoice {
    #[serde(alias = "message")]
    delta: Option<ChunkDelta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: u32,
}

#[derive(Deserialize)]
struct ChunkUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    prompt_tokens_details: Option<PromptTokensDetails>,
    /// DeepSeek reports cache hits here instead of `prompt_tokens_details`.
    #[serde(default)]
    prompt_cache_hit_tokens: u32,
}

#[derive(Deserialize)]
struct SseChunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
    usage: Option<ChunkUsage>,
}

struct ToolAccumulator {
    id: String,
    name: String,
    arguments: String,
}

pub async fn parse_sse(
    reader: impl AsyncBufRead + Unpin,
    event_tx: &Sender<ProviderEvent>,
    stream_timeout: Duration,
) -> Result<StreamResponse, AgentError> {
    let mut lines = reader.lines();

    let mut text = String::new();
    let mut reasoning_text = String::new();
    let mut tool_accumulators: Vec<ToolAccumulator> = Vec::new();
    let mut usage = TokenUsage::default();
    let mut stop_reason: Option<StopReason> = None;
    let mut is_first_content = true;
    let mut deadline = Instant::now() + stream_timeout;

    while let Some(line) = super::next_sse_line(&mut lines, &mut deadline, stream_timeout).await? {
        // Strip UTF-8 BOM from the first line; some Chinese cloud APIs
        // (e.g. ModelScope) prepend it, which breaks strip_prefix("data:").
        let line = if line.starts_with('\u{feff}') {
            &line['\u{feff}'.len_utf8()..]
        } else {
            &line
        };
        let data = match line.strip_prefix("data:") {
            Some(d) => d.trim(),
            None => continue,
        };

        if data == STREAM_DONE {
            break;
        }

        if data.contains("\"error\"")
            && let Ok(ev) = serde_json::from_str::<super::SseErrorPayload>(data)
        {
            warn!(error_type = %ev.error.r#type, message = %ev.error.message, "SSE error in stream");
            return Err(ev.into_agent_error());
        }

        let chunk: SseChunk = match serde_json::from_str(data) {
            Ok(c) => {
                if data.contains("tool_call") {
                    debug!(raw_sse = %data, "SSE tool_call chunk");
                }
                c
            }
            Err(e) => {
                warn!(error = %e, "failed to parse SSE chunk");
                continue;
            }
        };

        if let Some(u) = chunk.usage {
            let cached = u
                .prompt_tokens_details
                .map_or(0, |d| d.cached_tokens)
                .max(u.prompt_cache_hit_tokens);
            usage = TokenUsage {
                input: u.prompt_tokens.saturating_sub(cached),
                output: u.completion_tokens,
                cache_read: cached,
                cache_creation: 0,
            };
        }

        let Some(choice) = chunk.choices.into_iter().next() else {
            continue;
        };

        if let Some(reason) = choice.finish_reason {
            stop_reason = Some(StopReason::from_openai(&reason));
        }

        let Some(delta) = choice.delta else {
            continue;
        };

        if let Some(reasoning) = delta.reasoning_content
            && !reasoning.is_empty()
        {
            reasoning_text.push_str(&reasoning);
            event_tx
                .send_async(ProviderEvent::ThinkingDelta { text: reasoning })
                .await?;
        }

        match delta.content {
            Some(ContentDelta::String(content_str)) if !content_str.is_empty() => {
                let content = if is_first_content {
                    is_first_content = false;
                    content_str.trim_start().to_string()
                } else {
                    content_str
                };

                if !content.is_empty() {
                    text.push_str(&content);
                    event_tx
                        .send_async(ProviderEvent::TextDelta { text: content })
                        .await?;
                }
            }
            Some(ContentDelta::Array(content_array)) => {
                for part in content_array {
                    match part {
                        ContentDeltaPart::Thinking { thinking } => {
                            for thinking_block in thinking {
                                let content = match thinking_block {
                                    ThinkingDelta::Block(ThinkingDeltaBlock::Text {
                                        text: content_str,
                                    }) => content_str,
                                    ThinkingDelta::String(content_str) => content_str,
                                };

                                if content.is_empty() {
                                    continue;
                                }

                                reasoning_text.push_str(&content);
                                event_tx
                                    .send_async(ProviderEvent::ThinkingDelta { text: content })
                                    .await?;
                            }
                        }
                        ContentDeltaPart::Text { text: content_str } => {
                            let content = if is_first_content {
                                is_first_content = false;
                                content_str.trim_start().to_string()
                            } else {
                                content_str
                            };

                            if !content.is_empty() {
                                text.push_str(&content);
                                event_tx
                                    .send_async(ProviderEvent::TextDelta { text: content })
                                    .await?;
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        if let Some(tc_deltas) = delta.tool_calls {
            for tc in tc_deltas {
                while tool_accumulators.len() <= tc.index {
                    tool_accumulators.push(ToolAccumulator {
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                }
                let acc = &mut tool_accumulators[tc.index];
                let was_unnamed = acc.name.is_empty();
                let was_idless = acc.id.is_empty();
                if let Some(id) = tc.id {
                    // Same guard as name: don't overwrite a non-empty id
                    // with an empty one. Shangtang API sends id="" on every
                    // delta after the first, clobbering the correct id.
                    if !id.is_empty() || acc.id.is_empty() {
                        acc.id = id;
                    }
                }
                if let Some(func) = tc.function {
                    if let Some(name) = func.name.as_ref() {
                        // Don't overwrite a non-empty name with an empty one;
                        // some APIs send the name at the top level first, then
                        // send function.name as empty string in later deltas.
                        if !name.is_empty() || acc.name.is_empty() {
                            acc.name = name.clone();
                        }
                    }
                    if let Some(args) = func.arguments {
                        acc.arguments.push_str(&args);
                    }
                }
                // Fallback: some OpenAI-compatible APIs place the tool name
                // at the top level (tool_calls[i].name) instead of inside
                // function.name. Without this, acc.name stays empty and the
                // downstream dispatch produces "maki_unknown_tool".
                if acc.name.is_empty() && let Some(name) = tc.name.as_ref() {
                    acc.name = name.clone();
                }
                // Notify the UI only when both id and name are known.
                // Sending ToolUseStart with an empty id creates a pending
                // entry that never gets matched by ToolDone (which carries
                // the real id), leaving the spinner spinning forever.
                if !acc.id.is_empty() && !acc.name.is_empty() && (was_idless || was_unnamed) {
                    event_tx
                        .send_async(ProviderEvent::ToolUseStart {
                            id: acc.id.clone(),
                            name: acc.name.clone(),
                        })
                        .await?;
                }
            }
        }
    }

    // Record lengths before content_blocks assembly (which moves the values)
    let final_text_len = text.len();
    let final_reasoning_len = reasoning_text.len();
    let final_tool_count = tool_accumulators.len();

    let mut content_blocks: Vec<ContentBlock> = Vec::new();

    if !reasoning_text.is_empty() {
        content_blocks.push(ContentBlock::Thinking {
            thinking: reasoning_text,
            signature: None,
        });
    }

    if !text.is_empty() {
        content_blocks.push(ContentBlock::Text { text });
    }

    for (idx, acc) in tool_accumulators.into_iter().enumerate() {
        let input: Value = match serde_json::from_str(&acc.arguments) {
            Ok(v) => {
                debug!(tool = %acc.name, json = %acc.arguments, "tool input JSON");
                v
            }
            Err(e) => {
                warn!(error = %e, tool = %acc.name, json = %acc.arguments, "malformed tool JSON, falling back to {{}}");
                Value::Object(Default::default())
            }
        };
        let id = if acc.id.is_empty() {
            warn!(raw_name = %acc.name, raw_args = %acc.arguments, "provider sent empty tool_use id; substituting placeholder");
            format!("maki_unnamed_{idx}")
        } else {
            acc.id
        };
        let name = if acc.name.is_empty() {
            warn!(%id, raw_args = %acc.arguments, "provider sent empty tool_use name; substituting placeholder");
            "maki_unknown_tool".to_owned()
        } else {
            acc.name
        };
        content_blocks.push(ContentBlock::ToolUse { id, name, input });
    }

    // Some providers (e.g. ModelScope) return HTTP 200 with an SSE stream
    // that immediately contains only data: [DONE] and no content blocks.
    // Treat this as an error so the retry layer can re-request.
    if content_blocks.is_empty() {
        let reason = stop_reason.map(|s| s.to_string()).unwrap_or_default();
        warn!(
            text_len = final_text_len,
            reasoning_len = final_reasoning_len,
            tool_count = final_tool_count,
            stop_reason = %reason,
            "[SSE] EMPTY STREAM - returning 502 for retry"
        );
        return Err(AgentError::Api {
            status: 502,
            message: format!("model returned empty stream (stop_reason: {reason})"),
        });
    }

    debug!(
        text_len = final_text_len,
        reasoning_len = final_reasoning_len,
        content_block_count = content_blocks.len(),
        "[SSE] parse_sse returning Ok"
    );

    Ok(StreamResponse {
        message: Message {
            role: Role::Assistant,
            content: content_blocks,
            ..Default::default()
        },
        usage,
        stop_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_lite::io::Cursor;

    const TEST_STREAM_TIMEOUT: Duration = Duration::from_secs(300);

    #[test]
    fn parse_sse_text_and_usage() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"prompt_tokens_details\":{\"cached_tokens\":40}}}\n\
\n\
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            assert_eq!(resp.usage.input, 60);
            assert_eq!(resp.usage.output, 10);
            assert_eq!(resp.usage.cache_read, 40);
            assert_eq!(resp.stop_reason, Some(StopReason::EndTurn));
            assert!(
                matches!(&resp.message.content[0], ContentBlock::Text { text } if text == "Hello world")
            );
            assert!(!resp.message.has_tool_calls());

            let mut deltas = Vec::new();
            while let Ok(e) = rx.try_recv() {
                if let ProviderEvent::TextDelta { text } = e {
                    deltas.push(text);
                }
            }
            assert_eq!(deltas, vec!["Hello", " world"]);
        })
    }

    #[test]
    fn parse_sse_deepseek_cache_hit_tokens() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"prompt_cache_hit_tokens\":80,\"prompt_cache_miss_tokens\":20}}\n\
\n\
data: [DONE]\n";

            let (tx, _rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            assert_eq!(resp.usage.input, 20);
            assert_eq!(resp.usage.cache_read, 80);
            assert_eq!(resp.usage.output, 10);
        })
    }

    #[test]
    fn parse_sse_reasoning_and_content() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"Let me think\"}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"...\"}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\
\n\
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            assert!(
                matches!(&resp.message.content[0], ContentBlock::Thinking { thinking, .. } if thinking == "Let me think...")
            );
            assert!(
                matches!(&resp.message.content[1], ContentBlock::Text { text } if text == "Hello")
            );

            let mut thinking = Vec::new();
            let mut text_deltas = Vec::new();
            while let Ok(e) = rx.try_recv() {
                match e {
                    ProviderEvent::ThinkingDelta { text } => thinking.push(text),
                    ProviderEvent::TextDelta { text } => text_deltas.push(text),
                    ProviderEvent::ToolUseStart { .. } => {}
                    ProviderEvent::PromptProgress { .. } => {}
                }
            }
            assert_eq!(thinking, vec!["Let me think", "..."]);
            assert_eq!(text_deltas, vec!["Hello"]);
        })
    }

    #[test]
    fn convert_messages_structure() {
        let messages = vec![
            Message::user("hello".to_string()),
            Message {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::Text {
                        text: "thinking...".to_string(),
                    },
                    ContentBlock::ToolUse {
                        id: "tc_1".to_string(),
                        name: "bash".to_string(),
                        input: json!({"command": "ls"}),
                    },
                ],
                ..Default::default()
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "tc_1".to_string(),
                    content: "file.txt".to_string(),
                    is_error: false,
                }],
                ..Default::default()
            },
        ];

        let wire = convert_messages(&messages, "be helpful");

        assert_eq!(wire[0]["role"], "system");
        assert_eq!(wire[0]["content"], "be helpful");
        assert_eq!(wire[1]["role"], "user");
        assert_eq!(wire[1]["content"], "hello");
        assert_eq!(wire[2]["role"], "assistant");
        assert_eq!(wire[2]["content"], "thinking...");
        assert_eq!(wire[2]["tool_calls"][0]["id"], "tc_1");
        assert_eq!(wire[2]["tool_calls"][0]["type"], "function");
        assert_eq!(wire[2]["tool_calls"][0]["function"]["name"], "bash");
        assert_eq!(wire[3]["role"], "tool");
        assert_eq!(wire[3]["tool_call_id"], "tc_1");
        assert_eq!(wire[3]["content"], "file.txt");
    }

    #[test]
    fn convert_messages_assistant_tool_calls_only_has_content() {
        let messages = vec![
            Message::user("list files".to_string()),
            Message {
                role: Role::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "tc_1".to_string(),
                    name: "bash".to_string(),
                    input: json!({"command": "ls"}),
                }],
                ..Default::default()
            },
        ];

        let wire = convert_messages(&messages, "be helpful");

        assert_eq!(wire[2]["role"], "assistant");
        // `content` must be a present string ("") even with only tool_calls;
        // strict OpenAI-compatible backends reject null/omitted content.
        assert_eq!(wire[2]["content"], "");
        assert_eq!(wire[2]["tool_calls"][0]["function"]["name"], "bash");
    }

    #[test]
    fn convert_tools_structure() {
        let anthropic = json!([{
            "name": "bash",
            "description": "Run a command",
            "input_schema": {
                "type": "object",
                "properties": {"command": {"type": "string"}},
                "required": ["command"]
            }
        }]);

        let openai = convert_tools(&anthropic);
        let tool = &openai[0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "bash");
        assert_eq!(tool["function"]["description"], "Run a command");
        assert_eq!(tool["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn parse_sse_multiple_parallel_tool_calls() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"bash\",\"arguments\":\"\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"c2\",\"function\":{\"name\":\"read\",\"arguments\":\"\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"command\\\": \\\"ls\\\"}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"path\\\": \\\"/tmp\\\"}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3}}\n\
\n\
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            let tools: Vec<_> = resp.message.tool_uses().collect();
            assert_eq!(tools.len(), 2);
            assert_eq!(tools[0].0, "c1");
            assert_eq!(tools[0].1, "bash");
            assert_eq!(tools[0].2["command"], "ls");
            assert_eq!(tools[1].0, "c2");
            assert_eq!(tools[1].1, "read");
            assert_eq!(tools[1].2["path"], "/tmp");
            assert_eq!(resp.stop_reason, Some(StopReason::ToolUse));

            let starts: Vec<_> = rx
                .drain()
                .filter_map(|e| match e {
                    ProviderEvent::ToolUseStart { id, name } => Some((id, name)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                starts,
                vec![("c1".into(), "bash".into()), ("c2".into(), "read".into()),]
            );
        })
    }

    #[test]
    fn parse_sse_error_payload_returns_err() {
        smol::block_on(async {
            let sse = "\
data: {\"error\":{\"message\":\"Server overloaded\",\"type\":\"overloaded_error\"}}\n";

            let (tx, _rx) = flume::unbounded();
            let err = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap_err();

            match err {
                AgentError::Api { status, message } => {
                    assert_eq!(status, 529);
                    assert_eq!(message, "Server overloaded");
                }
                other => panic!("expected Api error, got: {other:?}"),
            }
        })
    }

    #[test]
    fn parse_sse_empty_tool_id_and_name_get_placeholders() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"tool_calls\\\":[{\\\"tool\\\":\\\"read\\\"}]}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\
\n\
data: [DONE]\n";

            let (tx, _rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            let tools: Vec<_> = resp.message.tool_uses().collect();
            assert_eq!(tools.len(), 1);
            assert!(!tools[0].0.is_empty(), "id must be non-empty for Bedrock");
            assert!(!tools[0].1.is_empty(), "name must be non-empty for Bedrock");
        })
    }

    #[test]
    fn parse_sse_malformed_tool_json_yields_empty_object() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"bash\",\"arguments\":\"\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{broken\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\
\n\
data: [DONE]\n";

            let (tx, _rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            let tools: Vec<_> = resp.message.tool_uses().collect();
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].1, "bash");
            assert_eq!(*tools[0].2, Value::Object(Default::default()));
        })
    }

    #[test]
    fn convert_messages_user_with_image() {
        use crate::types::{ImageMediaType, ImageSource};
        use std::sync::Arc;
        let source = ImageSource::new(ImageMediaType::Png, Arc::from("abc123"));
        let msgs = vec![Message::user_with_images("describe".into(), vec![source])];
        let result = convert_messages(&msgs, "system");
        let user = &result[1];
        let content = user["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "image_url");
        assert!(
            content[0]["image_url"]["url"]
                .as_str()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "describe");
    }

    #[test]
    fn convert_messages_tool_results_precede_tool_returned_image() {
        use crate::types::{ImageMediaType, ImageSource};
        use std::sync::Arc;
        let msgs = vec![Message {
            role: Role::User,
            content: vec![
                ContentBlock::ToolResult {
                    tool_use_id: "t1".into(),
                    content: "[image: pic.png 1KB]".into(),
                    is_error: false,
                },
                ContentBlock::Image {
                    source: ImageSource::new(ImageMediaType::Png, Arc::from("abc123")),
                },
            ],
            ..Default::default()
        }];
        let result = convert_messages(&msgs, "system");
        assert_eq!(result[1]["role"], "tool");
        assert_eq!(result[1]["tool_call_id"], "t1");
        assert_eq!(result[2]["role"], "user");
        assert_eq!(result[2]["content"][0]["type"], "image_url");
    }

    #[test]
    fn convert_messages_user_text_only_stays_string() {
        let msgs = vec![Message::user("hello".into())];
        let result = convert_messages(&msgs, "system");
        assert!(result[1]["content"].is_string());
    }

    #[test]
    fn convert_messages_assistant_with_reasoning() {
        let messages = vec![Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "Let me think...".into(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "Hello".into(),
                },
            ],
            ..Default::default()
        }];
        let wire = convert_messages(&messages, "");
        let asst = &wire[1];
        assert_eq!(asst["role"], "assistant");
        assert_eq!(asst["content"], "Hello");
        assert_eq!(asst["reasoning_content"], "Let me think...");
    }

    #[test]
    fn convert_messages_assistant_reasoning_only() {
        let messages = vec![Message {
            role: Role::Assistant,
            content: vec![ContentBlock::Thinking {
                thinking: "Just thinking...".into(),
                signature: None,
            }],
            ..Default::default()
        }];
        let wire = convert_messages(&messages, "");
        let asst = &wire[1];
        assert_eq!(asst["role"], "assistant");
        assert_eq!(asst["reasoning_content"], "Just thinking...");
        assert_eq!(asst["content"], "");
    }

    #[test]
    fn parse_sse_empty_stream() {
        // Empty stream (only data: [DONE]) returns 502 to trigger retry.
        smol::block_on(async {
            let sse = "data: [DONE]\n";
            let (tx, _rx) = flume::unbounded();
            let err = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap_err();
            match err {
                AgentError::Api { status: 502, .. } => {}
                other => panic!("expected 502 error, got: {other:?}"),
            }
        })
    }

    #[test]
    fn parse_sse_content_as_array_with_thinking() {
        smol::block_on(async {
            // Test parsing content as an array with thinking blocks
            let sse = "\
data: {\"choices\":[{\"delta\":{\"content\":[{\"type\":\"thinking\",\"thinking\":[{\"type\":\"text\",\"text\":\"Let me think\"}]}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"content\":[{\"type\":\"thinking\",\"thinking\":[{\"type\":\"text\",\"text\":\"...\"}]}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\
\n\
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            assert!(
                matches!(&resp.message.content[0], ContentBlock::Thinking { thinking, .. } if thinking == "Let me think..."),
                "{:?}",
                resp.message.content[0],
            );
            assert!(
                matches!(&resp.message.content[1], ContentBlock::Text { text } if text == "Hello")
            );

            let mut thinking_deltas = Vec::new();
            let mut text_deltas = Vec::new();
            while let Ok(e) = rx.try_recv() {
                match e {
                    ProviderEvent::ThinkingDelta { text } => thinking_deltas.push(text),
                    ProviderEvent::TextDelta { text } => text_deltas.push(text),
                    _ => {}
                }
            }

            assert_eq!(text_deltas, vec!["Hello"]);
            assert_eq!(thinking_deltas, vec!["Let me think", "..."]);
        })
    }

    #[test]
    /// Simulates Shangtang API: tool name at top level (tool_calls[i].name),
    /// id and arguments in function. Verifies name is picked up from the
    /// fallback path and ToolUseStart is sent only once with a non-empty id.
    fn sse_tool_name_at_top_level_fallback() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"name\":\"bash\",\"function\":{\"arguments\":\"\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_abc\",\"function\":{\"arguments\":\"{\\\"command\\\":\\\"date\\\"}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3}}\n\
\n\
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            let tools: Vec<_> = resp.message.tool_uses().collect();
            assert_eq!(tools.len(), 1, "one tool call expected");
            assert!(
                !tools[0].0.is_empty(),
                "id must be non-empty, got: {:?}",
                tools[0].0
            );
            assert_eq!(tools[0].0, "call_abc", "id should be the provider's id");
            assert_eq!(tools[0].1, "bash", "tool name should be 'bash'");
            assert_eq!(tools[0].2["command"], "date", "arguments should be complete");

            // ToolUseStart should be sent exactly ONCE, with non-empty id
            let starts: Vec<_> = rx
                .drain()
                .filter_map(|e| match e {
                    ProviderEvent::ToolUseStart { id, name } => Some((id, name)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                starts.len(),
                1,
                "ToolUseStart should be sent exactly once, got: {:?}",
                starts
            );
            assert!(
                !starts[0].0.is_empty(),
                "ToolUseStart id must be non-empty, got: {:?}",
                starts[0].0
            );
        })
    }

    #[test]
    fn parse_sse_bom_prefix_skips_first_event() {
        // Chinese cloud APIs (including ModelScope) may prepend a UTF-8 BOM.
        // The BOM stripping logic removes it, so all events are captured.
        smol::block_on(async {
            let sse = "\
\u{feff}data: {\"choices\":[{\"delta\":{\"content\":\"first event\"}}]}
\n
data: {\"choices\":[{\"delta\":{\"content\":\" second event\"}}]}
\n
data: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}]}
\n
data: [DONE]\n";

            let (tx, _rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            // BOM is stripped; both events are captured
            assert_eq!(resp.message.content.len(), 1); // single text block
            assert!(
                matches!(&resp.message.content[0], ContentBlock::Text { text } if text == "first event second event")
            );
        })
    }

    #[test]
    fn parse_sse_bom_prefix_only_one_event() {
        // Single SSE event with BOM prefix: BOM is stripped, content is captured.
        smol::block_on(async {
            let sse = "\u{feff}data: {\"choices\":[{\"delta\":{\"content\":\"only content\"}}]}\n
data: [DONE]\n";

            let (tx, _rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            assert!(
                matches!(&resp.message.content[0], ContentBlock::Text { text } if text == "only content")
            );
            assert_eq!(resp.message.content.len(), 1);
        })
    }

    #[test]
    /// Shangtang API scenario: first delta has name at top level, later
    /// delta overwrites function.name with empty string. The empty
    /// string must NOT overwrite the already-parsed top-level name.
    fn sse_tool_name_top_then_empty_function_name() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"name\":\"read\",\"function\":{\"arguments\":\"\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"\",\"arguments\":\"\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_r1\",\"function\":{\"arguments\":\"{\\\"path\\\":\\\"/tmp\\\"}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3}}\n\
\n\
data: [DONE]\n";

            let (tx, _rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            let tools: Vec<_> = resp.message.tool_uses().collect();
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].0, "call_r1");
            assert_eq!(tools[0].1, "read", "name must survive empty overwrite");
            assert_eq!(tools[0].2["path"], "/tmp");
        })
    }

    #[test]
    /// Name and id in same delta at top level (non-standard but possible).
    fn sse_tool_name_and_id_at_top_level() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_top\",\"name\":\"grep\",\"function\":{\"arguments\":\"{\\\"pattern\\\":\\\"test\\\"}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3}}\n\
\n\
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            let tools: Vec<_> = resp.message.tool_uses().collect();
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].0, "call_top");
            assert_eq!(tools[0].1, "grep");
            assert_eq!(tools[0].2["pattern"], "test");

            let starts: Vec<_> = rx
                .drain()
                .filter_map(|e| match e {
                    ProviderEvent::ToolUseStart { id, name } => Some((id, name)),
                    _ => None,
                })
                .collect();
            assert_eq!(starts.len(), 1, "must send ToolUseStart exactly once");
            assert_eq!(starts[0], ("call_top".into(), "grep".into()));
        })
    }

    #[test]
    /// Standard OpenAI format: name and id in function, both in first delta.
    fn sse_tool_name_in_function_standard() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c_std\",\"function\":{\"name\":\"bash\",\"arguments\":\"{\\\"command\\\":\\\"ls\\\"}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\
\n\
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            let tools: Vec<_> = resp.message.tool_uses().collect();
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].0, "c_std");
            assert_eq!(tools[0].1, "bash");
            assert_eq!(tools[0].2["command"], "ls");

            let starts: Vec<_> = rx
                .drain()
                .filter_map(|e| match e {
                    ProviderEvent::ToolUseStart { id, name } => Some((id, name)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                starts,
                vec![("c_std".into(), "bash".into())],
                "standard format: one ToolUseStart with correct id+name"
            );
        })
    }

    #[test]
    /// Multiple parallel tool calls where some deltas split name to
    /// top level and id to function. Verifies no duplicate pending
    /// entries and no empty-id ToolUseStart.
    fn sse_parallel_tool_calls_mixed_format() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"name\":\"bash\",\"function\":{\"arguments\":\"\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"c2\",\"function\":{\"name\":\"grep\",\"arguments\":\"\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"arguments\":\"{\\\"command\\\":\\\"date\\\"}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"pattern\\\":\\\"foo\\\"}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3}}\n\
\n\
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            let tools: Vec<_> = resp.message.tool_uses().collect();
            assert_eq!(tools.len(), 2);
            assert_eq!(tools[0].0, "c1");
            assert_eq!(tools[0].1, "bash");
            assert_eq!(tools[0].2["command"], "date");
            assert_eq!(tools[1].0, "c2");
            assert_eq!(tools[1].1, "grep");
            assert_eq!(tools[1].2["pattern"], "foo");

            let starts: Vec<_> = rx
                .drain()
                .filter_map(|e| match e {
                    ProviderEvent::ToolUseStart { id, name } => Some((id, name)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                starts.len(),
                2,
                "exactly two ToolUseStart events, got: {:?}",
                starts
            );
            // Both must have non-empty ids
            for (i, (id, _)) in starts.iter().enumerate() {
                assert!(!id.is_empty(), "ToolUseStart {} has empty id", i);
            }
            assert_eq!(starts[0], ("c2".into(), "grep".into()));
            assert_eq!(starts[1], ("c1".into(), "bash".into()));
        })
    }

    #[test]
    /// Shangtang API pattern: first delta has valid id+name, subsequent
    /// deltas repeatedly send id="" and name="" (empty). The parser must
    /// not overwrite the already-known id and name with empty strings,
    /// otherwise ToolUseStart's id and ContentBlock.ToolUse.id diverge,
    /// causing the UI spinner to spin forever.
    fn sse_shangtang_subsequent_empty_id_and_name() {
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_abc\",\"type\":\"function\",\"function\":{\"name\":\"bash\",\"arguments\":\"\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"\",\"type\":\"\",\"function\":{\"name\":\"\",\"arguments\":\"{\\\"command\\\":\\\"date\\\"}\"}}]}}]}\n\
\n\
data: {\"choices\":[{\"finish_reason\":\"tool_calls\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":3}}\n\
\n\
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            let tools: Vec<_> = resp.message.tool_uses().collect();
            assert_eq!(tools.len(), 1);
            // id must NOT be "maki_unnamed_0" (placeholder) - it must be
            // preserved from the first delta despite subsequent empty ids.
            assert_eq!(tools[0].0, "call_abc", "id must survive empty overwrites");
            assert_eq!(tools[0].1, "bash", "name must survive empty overwrites");
            assert_eq!(tools[0].2["command"], "date");

            // ToolUseStart must be sent only ONCE with the correct id
            let starts: Vec<_> = rx
                .drain()
                .filter_map(|e| match e {
                    ProviderEvent::ToolUseStart { id, name } => Some((id, name)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                starts.len(),
                1,
                "exactly one ToolUseStart, got: {:?}",
                starts
            );
        })
    }

    #[test]
    fn parse_sse_reasoning_only_no_content() {
        // Simulates a thinking model that never produces visible text content
        // (e.g. Qwen on ModelScope when stream ends during thinking phase).
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"Let me think about this\"}}]}
\n
data: {\"choices\":[{\"delta\":{\"reasoning_content\":\" step by step\"}}]}
\n
data: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}
\n
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            // Reasoning text should be captured
            assert!(
                matches!(&resp.message.content[0], ContentBlock::Thinking { thinking, .. } if thinking == "Let me think about this step by step")
            );
            // No text block (model never produced visible content)
            assert_eq!(resp.message.content.len(), 1, "only thinking block, no text block");
            assert_eq!(resp.stop_reason, Some(StopReason::EndTurn));
            assert_eq!(resp.usage.output, 5);

            let mut thinking = Vec::new();
            while let Ok(e) = rx.try_recv() {
                match e {
                    ProviderEvent::ThinkingDelta { text } => thinking.push(text),
                    _ => {}
                }
            }
            assert_eq!(thinking, vec!["Let me think about this", " step by step"]);
        })
    }

    #[test]
    fn parse_sse_content_null_in_delta() {
        // ModelScope may send content: null explicitly in thinking chunks.
        // This must not cause a deserialization error.
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"thinking\",\"content\":null}}]}
\n
data: {\"choices\":[{\"delta\":{\"reasoning_content\":null,\"content\":\"answer text\"}}]}
\n
data: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}]}
\n
data: [DONE]\n";

            let (tx, _rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            assert!(
                matches!(&resp.message.content[0], ContentBlock::Thinking { thinking, .. } if thinking == "thinking")
            );
            assert!(
                matches!(&resp.message.content[1], ContentBlock::Text { text } if text == "answer text")
            );
        })
    }

    #[test]
    fn parse_sse_usage_only_chunk_with_empty_choices() {
        // ModelScope sends a final chunk with empty choices and usage
        // when stream_options.include_usage is true.
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}
\n
data: {\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":20}}
\n
data: [DONE]\n";

            let (tx, _rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            assert!(
                matches!(&resp.message.content[0], ContentBlock::Text { text } if text == "Hello")
            );
            assert_eq!(resp.usage.input, 100);
            assert_eq!(resp.usage.output, 20);
            // stop_reason should be None since finish_reason was never sent
            assert_eq!(resp.stop_reason, None);
        })
    }

    #[test]
    fn parse_sse_reasoning_with_empty_content() {
        // Some providers send reasoning_content alongside empty content string.
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"deep thoughts\",\"content\":\"\"}}]}
\n
data: {\"choices\":[{\"delta\":{\"reasoning_content\":null,\"content\":\"actual answer\"}}]}
\n
data: {\"choices\":[{\"finish_reason\":\"stop\",\"delta\":{}}]}
\n
data: [DONE]\n";

            let (tx, _rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            // Empty content string should not produce a TextDelta, but thinking is captured
            assert!(
                matches!(&resp.message.content[0], ContentBlock::Thinking { thinking, .. } if thinking == "deep thoughts")
            );
            assert!(
                matches!(&resp.message.content[1], ContentBlock::Text { text } if text == "actual answer")
            );
        })
    }

    #[test]
    fn parse_sse_non_streaming_json_without_data_prefix() {
        // If the API ignores stream:true and returns a plain JSON response
        // without SSE framing, the parser returns 502 (empty stream).
        smol::block_on(async {
            let sse = "{\"id\":\"chatcmpl-abc\",\"object\":\"chat.completion\",\"created\":123,\"model\":\"qwen\",\"choices\":[{\"index\":0,\"message\":{\"role\":\"assistant\",\"content\":\"Hello\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n";

            let (tx, _rx) = flume::unbounded();
            let err = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap_err();
            match err {
                AgentError::Api { status: 502, .. } => {}
                other => panic!("expected 502 error, got: {other:?}"),
            }
        })
    }

    #[test]
    fn parse_sse_message_alias_with_stream_content() {
        // Some providers use "message" instead of "delta" in streaming chunks.
        // The #[serde(alias = "message")] on delta should handle this.
        smol::block_on(async {
            let sse = "\
data: {\"choices\":[{\"message\":{\"content\":\"Hello\"}}]}
\n
data: {\"choices\":[{\"message\":{\"content\":\" world\"}}]}
\n
data: {\"choices\":[{\"finish_reason\":\"stop\",\"message\":{}}]}
\n
data: [DONE]\n";

            let (tx, rx) = flume::unbounded();
            let resp = parse_sse(Cursor::new(sse.as_bytes()), &tx, TEST_STREAM_TIMEOUT)
                .await
                .unwrap();

            assert!(
                matches!(&resp.message.content[0], ContentBlock::Text { text } if text == "Hello world")
            );
            assert_eq!(resp.stop_reason, Some(StopReason::EndTurn));

            let mut deltas = Vec::new();
            while let Ok(e) = rx.try_recv() {
                if let ProviderEvent::TextDelta { text } = e {
                    deltas.push(text);
                }
            }
            assert_eq!(deltas, vec!["Hello", " world"]);
        })
    }
}
