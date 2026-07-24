# 构建环境与踩坑记录

## 环境信息

| 项目 | 值 |
|------|-----|
| OS | Ubuntu 22.04.5 LTS |
| Kernel | 5.4.0-163-generic x86_64 |
| CPU | Intel Xeon Platinum 8255C @ 2.50GHz (2 核) |
| 内存 | 1.9GB (可用约 700-800MB) |
| 磁盘 | /dev/vda2 50GB (89-99% 使用率，可用 500M-5G) |
| Rust | rustc 1.97.0 / cargo 1.97.0 |
| 链接器 | GNU ld 2.38 (已安装 lld 14.0 替代) |

### 目录布局

```
/_ext/                    # 工作根分区 (50G)
  home/headless/          # 用户 home
    .cargo/               # cargo 缓存 (917M)
      registry/
    .rustup/              # rust 工具链
    .local/state/maki/    # maki 运行时数据
    xm-zs01/fk-tontinton-maki/  # 项目目录
  down/02/bin/            # maki 旧版本二进制
  working/                # 其他工作区
```

### cargo 镜像配置

~/.cargo/config.toml 使用清华 tuna 镜像：
```toml
[source.crates-io]
replace-with = 'tuna'

[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"
```

### git 配置

```ini
[url "https://ghfast.top/https://github.com/.insteadof"]
  insteadof = https://github.com/
```

## 构建命令

### 首次完整构建
```bash
# 安装 lld 加速链接 (必须)
sudo apt install -y lld

# 完整构建 maki 二进制
CARGO_BUILD_JOBS=2 RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo build --package maki
```

### 增量构建（仅改了一个文件）
```bash
CARGO_BUILD_JOBS=2 RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo build -p maki
```

### 仅检查编译
```bash
CARGO_BUILD_JOBS=2 cargo check -p maki-providers
```

### 分阶段构建（避免链接器 OOM）
```bash
# Phase 1: 编库（不需要链接器）
CARGO_BUILD_JOBS=2 RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo build -p maki-providers --lib

# Phase 2: 编二进制（需要链接器）
CARGO_BUILD_JOBS=2 RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo build -p maki
```

## 编译耗时参考

| 操作 | 耗时 | 说明 |
|------|------|------|
| `cargo check -p maki-providers` (首次) | ~28min | 全依赖类型检查 |
| `cargo build -p maki` (首次) | ~15min | 全依赖编译 |
| `cargo build -p maki` (二次全量) | ~33min | 缓存失效后 |
| `cargo build -p maki` (增量 1 文件) | ~3-4min | 只变 maki-providers |
| `cargo build -p maki-providers --lib` | ~27min | 独立库编译 |

## 踩坑记录

### 1. 链接器崩溃 (SIGBUS)
**症状**: `collect2: fatal error: ld terminated with signal 7 [Bus error]`
**原因**: 磁盘空间不足 (< 540MB)。链接器尝试写入输出文件时失败。
**解决**: 
```bash
# 清理构建中间文件释放空间
rm -rf target/debug/{build,incremental}
find target/debug -name "*.o" -delete
find target/debug -name "*.rlib" -delete
df -h /_ext  # 确认空间 > 3G
```

### 2. 忘记加 `-p` 参数
**症状**: `cargo build` 编译整个 workspace（2 倍耗时）
**解决**: 始终指定 `--package maki`

### 3. `#[cfg(test)]` 模块的括号不匹配
**症状**: Rust 编译器报 "unclosed delimiter" 指向 `mod tests {` 
**原因**: 使用 `multiedit` 替换测试代码时，替换的 old_string 匹配了错误的位置，导致测试函数的闭合括号被吞掉
**排查**: 
```bash
# 用 python 检查括号平衡
python3 -c "
with open('file.rs') as f:
    c = f.read()
i = c.find('mod tests {')
t = c[i:]
d = 0
for j,ch in enumerate(t):
    if ch == '{': d += 1
    elif ch == '}': d -= 1
    if d < 0: print(f'Extra }} at line ...')
print(f'Depth: {d}')
"
# 更精确：用 rustc 直接检查
rustc --edition 2021 --crate-type lib file.rs
```
**教训**: `multiedit` 的 old_string 必须精确匹配文件的当前内容。建议先 `git diff` 确认替换范围。

### 4. `reasoning_text` 在 empty 检查前已被消费
**症状**: `error[E0382]: borrow of moved value: reasoning_text`
**原因**: `reasoning_text` 被 `ContentBlock::Thinking { thinking: reasoning_text }` 消费，之后再用 `reasoning_text.is_empty()` 报错
**解决**: 改为检查 `content_blocks.is_empty()` (已包含所有组装后的 block)

### 5. `has_tools` 变量未使用警告
**症状**: warning: unused variable: `has_tools`
**原因**: 修复 #4 后，`has_tools = !tool_accumulators.is_empty()` 不再需要
**解决**: 删除该变量

### 6. lld 默认未安装
**症状**: 使用 GNU ld 链接大 Rust 项目极慢（30min+）
**解决**: `sudo apt install -y lld`，然后在 `RUSTFLAGS` 中指定 `-C link-arg=-fuse-ld=lld`

### 7. 实测魔搭 API 间歇性空流
**症状**: HTTP 200 + SSE 仅含 `data: [DONE]`，无任何 choices/content
**原因**: 魔搭服务端问题（限流、配额耗尽、模型加载失败）
**验证**: 连续 10 次请求，约 20% 返回空流
**解决**: 在 `parse_sse()` 末尾检测空 content_blocks，返回 `AgentError::Api { status: 502 }` 触发自动重试

## 常用调试命令

```bash
# 查看魔搭原始 SSE 响应（检测空流）
curl -s -N -X POST "https://api-inference.modelscope.cn/v1/chat/completions" \
  -H "Authorization: Bearer $MSCOPE_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{"model":"deepseek-ai/DeepSeek-V4-Flash","messages":[{"role":"user","content":"hi"}],"stream":true,"max_tokens":10}' \
  | xxd | head -5

# 查看 HTTP 响应头
curl -s -D - -X POST ... | head -15

# 查看 rate limit 信息
# Modelscope-Ratelimit-Model-Requests-Remaining 响应头

# 运行 maki print 模式测试
MSCOPE_API_KEY="$MSCOPE_API_KEY" ./target/debug/maki -m cust07-mscope/<model> -p "hi" --output-format json

# 快速 python mock 验证 SSE 解析
python3 -c "
import json
for line in open('/dev/stdin'):
    line=line.strip()
    if line.startswith('data:'):
        d=line[5:].strip()
        if d=='[DONE]': break
        chunk=json.loads(d)
        for c in chunk.get('choices',[]):
            delta=c.get('delta',{})
            if delta.get('content'): print('CONTENT:', delta['content'])
"

# 检查括号平衡
python3 -c "
with open('file.rs') as f:
    c = f.read()
# 非测试模块部分
d=0
for ch in c:
    if ch=='{': d+=1
    elif ch=='}': d-=1
print(f'Global depth: {d}')
"

# 查看 maki 运行日志
tail -50 ~/.local/state/maki/maki.log | python3 -m json.tool --no-ensure-ascii 2>/dev/null | head -50
```
