#!/usr/bin/env python3
"""
深度对比测试：精确模拟 maki 的请求头，对比标准请求。

排查方向：
1. header 差异（maki 不设置 accept、connection 等）
2. HTTP 版本差异
3. 连接复用行为
4. chunked transfer decoding 差异
"""
import os
import sys
import json
import time
import socket
import urllib.request
import urllib.error

API_KEY = os.environ.get("MSCOPE_API_KEY")
if not API_KEY:
    print("请设置环境变量 MSCOPE_API_KEY")
    sys.exit(1)

MODEL = os.environ.get("MSCOPE_MODEL", "Qwen/Qwen3.5-35B-A3B")
URL = "https://api-inference.modelscope.cn/v1/chat/completions"


def test_request(description: str, headers: dict, data: bytes):
    print(f"\n--- {description} ---")

    req = urllib.request.Request(URL, data=data, method="POST", headers=headers)

    start = time.time()
    try:
        resp = urllib.request.urlopen(req, timeout=60)

        # 打印响应头
        print(f"HTTP {resp.status}")
        resp_headers = dict(resp.headers)
        for k in ["content-type", "transfer-encoding", "connection", "server"]:
            if k in resp_headers:
                print(f"  {k}: {resp_headers[k]}")

        # 读取前几行
        first_byte_time = None
        line_count = 0
        content_chunks = []

        for raw_line in resp:
            line = raw_line.decode("utf-8", errors="replace").strip()
            if not line:
                continue
            line_count += 1
            if first_byte_time is None:
                first_byte_time = time.time() - start
                print(f"首个字节: {first_byte_time:.1f}s")

            if line.startswith("data:"):
                d = line[5:].strip()
                if d == "[DONE]":
                    break
                content_chunks.append(d[:60])

        elapsed = time.time() - start
        print(f"总耗时: {elapsed:.1f}s, 行数: {line_count}")
        if content_chunks:
            print(f"✅ {len(content_chunks)} chunks")
            for i, c in enumerate(content_chunks[:3]):
                print(f"  [{i}] {c}")
        else:
            print("⚠️  空流")

    except Exception as e:
        print(f"❌ {type(e).__name__}: {e}")


def main():
    payload = json.dumps({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": True,
        "max_tokens": 50,
    }).encode()

    # 测试 1: 精确模拟 maki 的 headers
    # maki 只设置: user-agent, content-type, authorization
    maki_headers = {
        "user-agent": "maki/v0.4.2-gabcdef",
        "content-type": "application/json",
        "authorization": f"Bearer {API_KEY}",
    }
    test_request("精确模拟 maki (仅基本 headers)", maki_headers, payload)

    # 测试 2: 加上 accept: text/event-stream
    headers_with_accept = {
        **maki_headers,
        "accept": "text/event-stream",
    }
    test_request("maki + accept: text/event-stream", headers_with_accept, payload)

    # 测试 3: 加上 connection: close (禁用 keep-alive)
    headers_with_close = {
        **maki_headers,
        "connection": "close",
    }
    test_request("maki + connection: close", headers_with_close, payload)

    # 测试 4: 完整标准 headers
    standard_headers = {
        **maki_headers,
        "accept": "text/event-stream",
        "cache-control": "no-cache",
    }
    test_request("完整标准 SSE headers", standard_headers, payload)


if __name__ == "__main__":
    main()
