#!/usr/bin/env python3
"""
模拟 isahc low_speed_timeout 行为，验证魔搭 API 的流式响应。

核心问题：isahc 的 low_speed_timeout(1 byte/s, 30s) 在模型思考超过 30s 时
会静默关闭连接。Python 的 urllib 没有这个限制，可以用来对比验证。
"""
import os
import sys
import json
import time
import urllib.request
import urllib.error
import socket

API_KEY = os.environ.get("MSCOPE_API_KEY")
if not API_KEY:
    print("请设置环境变量 MSCOPE_API_KEY")
    sys.exit(1)

MODEL = os.environ.get("MSCOPE_MODEL", "Qwen/Qwen3.5-35B-A3B")
URL = "https://api-inference.modelscope.cn/v1/chat/completions"


def test_stream(description, read_timeout):
    """测试流式请求

    read_timeout: None = 无超时（模拟移除 low_speed_timeout）
                  30   = 30s 超时（模拟 isahc low_speed_timeout）
    """
    print(f"\n--- {description} ---")
    print(f"模型: {MODEL}, read_timeout: {read_timeout}")

    payload = json.dumps({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello, 一句话介绍你自己"}],
        "stream": True,
        "max_tokens": 100,
    }).encode()

    req = urllib.request.Request(
        URL,
        data=payload,
        headers={
            "Authorization": f"Bearer {API_KEY}",
            "Content-Type": "application/json",
        },
        method="POST",
    )

    start = time.time()
    try:
        # 设置超时
        if read_timeout:
            timeout = read_timeout
        else:
            timeout = 10  # 仅连接超时
        
        resp = urllib.request.urlopen(req, timeout=timeout)

        first_byte_time = None
        line_count = 0
        content_lines = []

        for raw_line in resp:
            line = raw_line.decode("utf-8", errors="replace").strip()
            if not line:
                continue
            line_count += 1
            if first_byte_time is None:
                first_byte_time = time.time() - start
                print(f"首个字节到达: {first_byte_time:.1f}s")

            if line.startswith("data:"):
                data = line[5:].strip()
                if data == "[DONE]":
                    break
                content_lines.append(data[:80])

        elapsed = time.time() - start
        print(f"总耗时: {elapsed:.1f}s, 行数: {line_count}")
        if content_lines:
            print(f"✅ 收到 {len(content_lines)} 个 SSE chunk")
            for i, c in enumerate(content_lines[:5]):
                print(f"  chunk {i}: {c}")
        else:
            print("⚠️  无内容（空流）")

    except socket.timeout:
        elapsed = time.time() - start
        print(f"❌ 读超时! 耗时 {elapsed:.1f}s (read_timeout={read_timeout})")
        print("   这就是 isahc low_speed_timeout 触发时的表现")
    except urllib.error.HTTPError as e:
        print(f"❌ HTTP 错误: {e.code} {e.reason}")
        print(f"   {e.read().decode()[:200]}")
    except Exception as e:
        print(f"❌ 错误: {type(e).__name__}: {e}")


def main():
    print("=" * 60)
    print("魔搭 API 流式响应测试")
    print("对比: 有/无 read_timeout 的行为差异")
    print("=" * 60)

    # 测试 1: 模拟 isahc 的 low_speed_timeout (30s 超时)
    test_stream("模拟 isahc low_speed_timeout(1, 30s)", read_timeout=30)

    # 测试 2: 无超时（模拟移除 low_speed_timeout）
    test_stream("无 read_timeout (模拟修复后)", read_timeout=None)


if __name__ == "__main__":
    main()
