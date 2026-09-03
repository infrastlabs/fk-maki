# sensenova 目录提供商调查记录

**日期**: 2026-09-03
**主题**: 确认 sensenova 不是 Maki 内置提供商,而是 models.dev 目录动态发现的提供商

## 一、结论

**sensenova 从未通过 Maki 提交被"加入"**。它是 models.dev 目录中的提供商,通过 Maki 的目录系统自动发现并可用。

## 二、目录提供商关键时间线

| 日期 | 提交 | 作者 | 说明 |
|------|------|------|------|
| 2026-06-14 | `5b01751d` | Zsombor Gegesy | Opencode 提供商 + models.dev 目录发现,目录提供商归属 "opencode" |
| 2026-07-10 | `b370e16e` | oldhu | 添加 `enable_free_models` 选项(默认 false) |
| 2026-07-18 | `0b23fc69` | Zsombor | `maki auth login <slug>` 支持任意目录提供商 |
| 2026-07-25 | `73e6ffde` | brhutchins | **目录提供商从 Opencode 分离** — sensenova 等成为独立提供商 |

## 三、models.dev 中的 sensenova 数据

```json
{
  "id": "sensenova",
  "name": "SenseNova (China)",
  "api": "https://token.sensenova.cn/v1",
  "env": ["SENSENOVA_API_KEY"],
  "npm": "@ai-sdk/openai-compatible",
  "models": {
    "deepseek-v4-flash": {
      "cost": {"input": 0, "output": 0},
      "limit": {"context": 1000000, "output": 65536},
      "release_date": "2026-04-24"
    },
    "glm-5.2": {
      "cost": {"input": 0, "output": 0},
      "limit": {"context": 1000000, "output": 131072},
      "release_date": "2026-06-13"
    },
    "sensenova-6.8-flash-lite": {
      "cost": {"input": 0, "output": 0},
      "limit": {"context": 262144, "output": 65536},
      "release_date": "2026-08-11"
    }
  }
}
```

所有三个模型均为**免费** (`cost: 0`)

## 四、无专门的 Maki Issue/PR

由于是目录提供商,不存在针对 sensenova 的 Maki issue/PR/提交。"加入"发生在 models.dev 端。

## 五、在 Maki 中使用

```bash
# 1. 启用免费目录模型(全局)
echo 'providers.opencode.enable_free_models = true' >> ~/.config/maki/providers.toml

# 2. 或提供 API Key
export SENSENOVA_API_KEY=sk-xxx

# 3. 使用模型
maki -m sensenova/deepseek-v4-flash
```

## 六、相关代码位置

- 目录加载: `maki-providers/src/providers/catalog.rs`
- 免费模型门控: `catalog.rs:179` `is_free && !enable_free_models`
- 公共 token 回退: `catalog.rs:874` `free_fallback_allowed()`

---

## 七、追加:models.dev 目录全部免费模型清单(2026-09-03 检索)

> 追加于 2026-09-03,接续上文 sensenova 调查。全目录共 **71 个提供商、598 个免费模型**(input/output 均为 0)。

### 1. 开箱即用(无需 API key,`enable_free_models = true` 后走公共回退)

**opencode(31 个)**

| 模型 | 说明 |
|------|------|
| `big-pickle` | Big Pickle |
| `deepseek-v4-flash-free` | DeepSeek V4 Flash Free |
| `glm-4.7-free` | GLM-4.7 Free |
| `glm-5-free` | GLM-5 Free |
| `grok-code` | Grok Code Fast 1 |
| `hy3-free` | Hy3 Free |
| `hy3-preview-free` | Hy3 preview Free |
| `kimi-k2.5-free` | Kimi K2.5 Free |
| `laguna-s-2.1-free` | Laguna S 2.1 Free |
| `ling-2.6-flash-free` | Ling 2.6 Flash Free |
| `ling-3.0-flash-fin-free` | Ling 3.0 Flash Fin Free |
| `ling-3.0-flash-free` | Ling-3.0-flash Free |
| `ling-3.0-tiny-free` | Ling-3.0-tiny Free |
| `longcat-2.0-free` | LongCat-2.0 Free |
| `mimo-v2-flash-free` | MiMo V2 Flash Free |
| `mimo-v2-omni-free` | MiMo V2 Omni Free |
| `mimo-v2-pro-free` | MiMo V2 Pro Free |
| `mimo-v2.5-free` | MiMo V2.5 Free |
| `minimax-m2.1-free` | MiniMax-M2.1 Free |
| `minimax-m2.5-free` | MiniMax-M2.5 Free |
| `minimax-m3-free` | MiniMax-M3 Free |
| `muse-spark-1.2-contributor-free` | Muse Spark 1.2 Free |
| `muse-spark-1.3-contributor-free` | Muse Spark 1.3 Free |
| `nemotron-3-super-free` | Nemotron 3 Super Free |
| `nemotron-3-ultra-free` | Nemotron 3 Ultra Free |
| `nemotron-3.5-lightning-free` | Nemotron 3.5 Lightning Free |
| `north-mini-code-free` | North Mini Code Free |
| `qwen3.6-plus-free` | Qwen3.6 Plus Free |
| `ring-2.6-1t-free` | Ring 2.6 1T Free |
| `trinity-large-preview-free` | Trinity Large Preview |
| `x-preview-f-free` | Ox Alpha Free (Unlimited) |

**opencode-go(1 个)**: `ox-alpha-free`

### 2. 需 API key 的目录提供商(登录后免费)

| 提供商 | 免费模型数 | 代表模型 | 环境变量 |
|--------|-----------|---------|---------|
| nvidia | 99 | nemotron 系列、gemma、qwen、gpt-oss 等(含 embedding/语音/图像) | `NVIDIA_API_KEY` |
| alibaba-token-plan / -cn | 各 26 | deepseek-v4-*、glm-5.*、kimi-k2.*、qwen3.*、wan2.7 | 阿里云 key |
| gitlab | 24 | duo-chat-*(Claude/GPT 全系) | `GITLAB_TOKEN` |
| openrouter | 21 | `:free` 后缀(nemotron、minimax、inkling 等) | `OPENROUTER_API_KEY` |
| scnet-token-plan | 17 | DeepSeek-V4-*、GLM-5.*、Kimi-*、MiniMax-* | 天翼云 key |
| iflowcn | 14 | deepseek-r1/v3、glm-4.6、kimi-k2、qwen3-* | iFlow key |
| inferx | 12 | Qwen3-*、Devstral-2、deepseek-v4-flash | InferX key |
| requesty | 12 | nemotron-3-*、ling-3.0-tiny、laguna 等 | Requesty key |
| alibaba-coding-plan / -cn | 各 10 | qwen3-*、glm-4.7/5、kimi-k2.5、MiniMax-M2.5 | 阿里云 key |
| qvac | 9 | qwen3.5-*、qwen3.6-*、gemma4-31b、gpt-oss | QVAC key |
| volcengine-coding-plan | 8 | doubao-seed-2.*、deepseek-v4-*、glm-5.3 | 火山引擎 key |
| tencent-coding-plan | 8 | hunyuan-2.0-*、glm-5、kimi-k2.5 | 腾讯云 key |
| umans-ai-coding-plan | 8 | umans-*(deepseek/glm/kimi/qwen 混排) | Umans key |
| zai-coding-plan | 7 | glm-4.7/5.* | 智谱 key |
| zhipuai-coding-plan | 9 | glm-4.7/5.* | 智谱 key |
| minimax-coding-plan / -cn | 各 7 | MiniMax-M2/M2.1/M2.5/M2.7/M3 | MiniMax key |
| xiaomi-token-plan(-ams/-cn/-sgp) | 各 7 | mimo-v2.*(含 TTS) | 小米 key |
| modelscope | 7 | Qwen3-*、GLM-4.5/4.6 | ModelScope key |
| zenmux | 7 | claude-sonnet-5-free、kimi-k3-free、glm-5.2-free | ZenMux key |
| kimi-for-coding | 4 | k3、k3-256k、kimi-for-coding | Kimi key |
| atomic-chat | 5 | llama-3.1-8B、qwen3.5-9B、gemma-4 | Atomic key |
| orcarouter | 5 | deepseek-v4-flash-free、qwen3.8-27b-free、hy3-free | OrcaRouter key |
| sensenova | 3 | deepseek-v4-flash、glm-5.2、sensenova-6.8-flash-lite | `SENSENOVA_API_KEY` |
| zai | 2 | glm-4.5-flash、glm-4.7-flash | `ZAI_API_KEY` |
| zhipuai | 2 | glm-4.5-flash、glm-4.7-flash | `ZHIPUAI_API_KEY` |
| tencent-token-plan / tokenhub | 各 1~2 | hy3 | 腾讯 key |
| 其余 ~30 家 | 1~6 | — | 各自 key |

### 3. 其他注意

- 目录提供商默认**隐藏**免费模型,需在 `~/.config/maki/providers.toml` 设置:
  ```toml
  [opencode]
  enable_free_models = true
  ```
- 判断代码: `maki-providers/src/providers/catalog.rs:179` `is_free && !enable_free_models → 过滤`
- 开放权重小模型居多;适合 agent 使用的以 sensenova/zai 的 GLM-flash、opencode 的 deepseek-v4-flash-free 为代表
- 完整原始数据: `/tmp/modelsdev.json`(本次检索下载的 models.dev/api.json 缓存)