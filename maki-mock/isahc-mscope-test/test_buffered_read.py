#!/usr/bin/env python3
"""
对比测试：直接读取 vs 缓冲读取 SSE 流。

验证 isahc 文档声明的问题：
"The response body uses its own buffering internally.
It is therefore undesirable to wrap the body in additional buffering readers."

maki 用 BufReader 包裹 isahc::AsyncBody 可能导致读取异常。
"""
import os
import sys
import json
import time
import socket
import urllib.request

API_KEY = os.environ.get("MSCOPE_API_KEY")
if not API_KEY:
    print("请设置环境变量 MSCOPE_API_KEY")
    sys.exit(1)

MODEL = os.environ.get("MSCOPE_MODEL", "Qwen/Qwen3.5-35B-A3B")
URL = "https://api-inference.modelscope.cn/v1/chat/completions"


def test_read_mode(description: str, use_buffer: bool):
    """测试不同读取模式"""
    print(f"\n--- {description} ---")

    payload = json.dumps({
        "model": MODEL,
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": True,
        "max_tokens": 50,
    }).encode()

    req = urllib.request.Request(
        URL,
        data=payload,
        headers={
            "user-agent": "maki/v0.4.2-test",
            "content-type": "application/json",
            "authorization": f"Bearer {API_KEY}",
        },
        method="POST",
    )

    start = time.time()
    try:
        resp = urllib.request.urlopen(req, timeout=60)
        first_byte = None
        line_count = 0
        content_chunks = []
        hang_detected = False

        if use_buffer:
            # 模拟 BufReader：用较大的 buffer size 读取，再按行分割
            buffer = b""
            while True:
                # 模拟 BufReader::fill_buf + consume
                chunk = resp.read(8192)  # 8KB buffer
                if not chunk:
                    break
                buffer += chunk
                while b"\n" in buffer:
                    line, buffer = buffer.split(b"\n", 1)
                    line_str = line.decode("utf-8", errors="replace").strip()
                    if not line_str:
                        continue
                    line_count += 1
                    if first_byte is None:
                        first_byte = time.time() - start
                    if line_str.startswith("data:"):
                        d = line_str[5:].strip()
                        if d == "[DONE]":
                            break
                        content_chunks.append(d[:60])
        else:
            # 直接逐行读取（推荐方式）
            for raw_line in resp:
                line = raw_line.decode("utf-8", errors="replace").strip()
                if not line:
                    continue
                line_count += 1
                if first_byte is None:
                    first_byte = time.time() - start
                if line.startswith("data:"):
                    d = line[5:].strip()
                    if d == "[DONE]":
                        break
                    content_chunks.append(d[:60])

        elapsed = time.time() - start
        print(f"首字节: {first_byte:.1f}s, 耗时: {elapsed:.1f}s, 行数: {line_count}")
        if content_chunks:
            print(f"✅ {len(content_chunks)} chunks")
        else:
            print("⚠️  空流")
    except Exception as e:
        print(f"❌ {type(e).__name__}: {e}")


def main():
    print("=" * 60)
    print("对比：直接读取 vs 缓冲读取（模拟 BufReader）")
    print("=" * 60)

    test_read_mode("直接逐行读取（推荐）", use_buffer=False)
    test_read_mode("BufReader 式读取（8KB buffer）", use_buffer=True)


if __name__ == "__main__":
    main()
