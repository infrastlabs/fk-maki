#!/usr/bin/env python3
"""
精确对比 maki vs zerostack 的请求差异。

排查方向：找出 maki 与 zerostack 在 HTTP 请求层面的真正差异。
不是 isahc vs reqwest（无法在 Python 中测试），而是请求构造的差异。
"""
import os
import sys
import json
import time
import urllib.request
import urllib.error
import ssl

API_KEY = os.environ.get("MSCOPE_API_KEY")
if not API_KEY:
    print("请设置环境变量 MSCOPE_API_KEY")
    sys.exit(1)

MODEL = os.environ.get("MSCOPE_MODEL", "Qwen/Qwen3.5-35B-A3B")
URL = "https://api-inference.modelscope.cn/v1/chat/completions"


def test_request(description: str, payload: dict, headers: dict, verbose: bool = True):
    """发送请求并分析响应"""
    if verbose:
        print(f"\n--- {description} ---")

    data = json.dumps(payload).encode("utf-8")
    if verbose:
        print(f"请求体 ({len(data)} bytes): {data[:200]}...")

    req = urllib.request.Request(URL, data=data, method="POST", headers=headers)

    # 创建 SSL context（模拟 isahc 默认行为）
    ctx = ssl.create_default_context()

    start = time.time()
    try:
        resp = urllib.request.urlopen(req, timeout=60, context=ctx)

        # 响应元信息
        resp_headers = dict(resp.headers)
        transport = resp._fp.fp.raw if hasattr(resp, '_fp') else None

        if verbose:
            print(f"HTTP {resp.status}")
            print(f"协议: {resp.version}")  # 11 = HTTP/1.1, 20 = HTTP/2
            for k in ["content-type", "transfer-encoding", "connection", "server", "alt-svc"]:
                kl = k.lower()
                for hk, hv in resp_headers.items():
                    if hk.lower() == kl:
                        print(f"  {k}: {hv}")

        # 读取响应
        first_byte = None
        line_count = 0
        content_chunks = []

        for raw_line in resp:
            line = raw_line.decode("utf-8", errors="replace").strip()
            if not line:
                continue
            line_count += 1
            if first_byte is None:
                first_byte = time.time() - start
                if verbose:
                    print(f"首字节: {first_byte:.1f}s")

            if line.startswith("data:"):
                d = line[5:].strip()
                if d == "[DONE]":
                    break
                if verbose and len(content_chunks) < 3:
                    content_chunks.append(d[:80])

        elapsed = time.time() - start
        if verbose:
            print(f"耗时: {elapsed:.1f}s, 行数: {line_count}")
            if content_chunks:
                print(f"✅ 收到内容, 前3块:")
                for i, c in enumerate(content_chunks):
                    print(f"  [{i}] {c}")
            else:
                print("⚠️  空流")

        return {"status": "ok", "lines": line_count, "time": elapsed}

    except urllib.error.HTTPError as e:
        body = e.read().decode()[:200]
        if verbose:
            print(f"❌ HTTP {e.code}: {body}")
        return {"status": "error", "code": e.code}
    except Exception as e:
        if verbose:
            print(f"❌ {type(e).__name__}: {e}")
        return {"status": "error", "message": str(e)}


def main():
    print("=" * 70)
    print("maki vs zerostack 请求层精确对比")
    print("=" * 70)

    # maki 的请求构造（精确还原）
    # 来自 openai_compat.rs: build_body()
    maki_payload = {
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": True,
        "max_tokens": 16384,  # maki 默认用 max_output_tokens
        "stream_options": {"include_usage": True}  # maki OpenAI config 设置
    }

    # maki 的 headers
    # user-agent: "maki/v{version}-g{git_hash}"
    # 注意：maki 不设置 accept, connection 等
    maki_headers = {
        "user-agent": "maki/v0.4.2-gabcdef",
        "content-type": "application/json",
        "authorization": f"Bearer {API_KEY}",
    }

    # 对比 1: 精确 maki 请求
    test_request("maki 精确还原 (max_tokens + stream_options)", maki_payload, maki_headers)

    # 对比 2: maki 但不用 stream_options（部分 API 不兼容）
    maki_no_options = {
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": True,
        "max_tokens": 100,
    }
    test_request("maki 无 stream_options", maki_no_options, maki_headers)

    # 对比 3: 用 max_completion_tokens 替代 max_tokens
    # (maki OpenAI 配置使用 max_completion_tokens)
    maki_max_completion = {
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": True,
        "max_completion_tokens": 100,
        "stream_options": {"include_usage": True}
    }
    test_request("maki 用 max_completion_tokens", maki_max_completion, maki_headers)

    # 对比 4: 最小请求（裸模型请求）
    minimal_payload = {
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": True,
    }
    test_request("最小请求 (无 max_tokens)", minimal_payload, maki_headers)

    # 对比 5: 多次连续请求（测试连接复用）
    print("\n" + "=" * 70)
    print("连续 3 次请求（测试连接复用/状态累积效应）")
    print("=" * 70)
    for i in range(3):
        result = test_request(f"第 {i+1} 次请求", maki_no_options, maki_headers, verbose=False)
        status = result.get("status")
        if status == "ok":
            print(f"  第 {i+1} 次: ✅ {result['lines']} lines, {result['time']:.1f}s")
        else:
            print(f"  第 {i+1} 次: ❌ {result}")


if __name__ == "__main__":
    main()
