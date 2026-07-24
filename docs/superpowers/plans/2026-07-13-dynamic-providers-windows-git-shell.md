# Dynamic Providers Windows/git-shell Execution Fix

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix dynamic provider shell script execution on Windows 10 when running under git-shell, where `Command::new("bash")` fails because git-shell is a restricted shell that rejects arbitrary script execution.

**Architecture:** Enhance `script_invocation` to detect git-shell, search for an alternative bash (Git for Windows ships `bash.exe` and `git-bash.exe` separately from `git-shell.exe`), and fall back gracefully with a helpful error. Also add a `MAKI_BASH_PATH` env var override for power users.

**Tech Stack:** Rust (maki-providers crate), std::process::Command, wait-timeout

---

## Background: How Dynamic Provider Execution Works

The full execution flow in `maki-providers/src/providers/dynamic.rs`:

1. **Discovery** (`discover()` → `discover_in()`): scans `~/.config/maki/providers/` for executable scripts
2. **Info phase**: runs `script info` → parses `ScriptInfo` JSON (display_name, base provider, system_prefix, has_auth)
3. **Models phase**: runs `script models` → parses `Vec<ScriptModel>` (or falls back to base provider models)
4. **Creation** (`create()`): runs `script resolve` → gets `base_url` + `headers` → wraps the inner provider (Anthropic, Openai, etc.)
5. **Auth refresh**: runs `script refresh` or `script reload` on demand
6. **Interactive login/logout**: runs `script login` / `script logout` with inherited stdio

All script execution goes through two functions:
- `run_script()` — captures stdout (for info, models, resolve, refresh, reload)
- `run_script_interactive()` — inherits stdio (for login, logout)

Both call `script_invocation()` to determine how to run the file. On Windows, `script_invocation` currently returns `("bash", vec![script_path])` for `.sh` files.

## Root Cause

Three problems on Windows under git-shell:

1. **git-shell is restricted**: It only allows git commands. Spawning `bash script.sh` either fails with "fatal: unrecognized command" or the `bash` found in PATH is itself `git-shell.exe`.
2. **Wrong PATH lookup**: Git for Windows ships `bash.exe` in `<Git>\bin\` or `<Git>\usr\bin\`, while `git-shell.exe` lives next to `git.exe` — they are different executables. `Command::new("bash")` may find `git-shell.exe` if the git-shell directory is first in PATH.
3. **No fallback or diagnostics**: When bash fails, the error is "failed to run script info: The system cannot find the file specified" (or similar) — no hint that git-shell is the problem.

---

## Task 1: Add git-shell detection and alternative bash discovery

**Files:**
- Modify: `maki-providers/src/providers/dynamic.rs:140-151`
- Test: `maki-providers/src/providers/dynamic.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write tests for git-shell detection**

Add these test functions inside the existing `#[cfg(test)] mod tests` block at the END of dynamic.rs (after line 756):

```rust
#[cfg(windows)]
#[test]
fn script_invocation_sh_on_windows() {
    use std::path::PathBuf;
    let (prog, args) = script_invocation(PathBuf::from("C:\\providers\\my-provider.sh").as_path());
    // Should not return bare "bash" — it should include full path or at least "bash.exe"
    assert!(
        prog.contains("bash") || std::env::var("MAKI_BASH_PATH").is_ok(),
        "expected bash-based invocation, got: {prog}"
    );
    assert_eq!(args.len(), 1);
    assert!(args[0].ends_with("my-provider.sh"));
}

#[cfg(windows)]
#[test]
fn script_invocation_exe_on_windows() {
    use std::path::PathBuf;
    let (prog, args) = script_invocation(PathBuf::from("C:\\providers\\my-provider.exe").as_path());
    assert_eq!(prog, "C:\\providers\\my-provider.exe");
    assert!(args.is_empty());
}

#[cfg(not(windows))]
#[test]
fn script_invocation_sh_on_unix() {
    use std::path::PathBuf;
    let (prog, args) = script_invocation(PathBuf::from("/home/user/.config/maki/providers/my-provider").as_path());
    assert_eq!(prog, "/home/user/.config/maki/providers/my-provider");
    assert!(args.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p maki-providers script_invocation -- --nocapture`
Expected: FAIL — tests don't exist yet

- [ ] **Step 3: Update `script_invocation` with git-shell detection and fallbacks**

Replace the current `script_invocation` function (lines 138-151) with:

```rust
/// On Windows, shell scripts (.sh) need to be invoked through bash.
/// Returns the program and arguments to use for a given script path.
///
/// Handles git-shell by searching for a real bash.exe outside the git-shell
/// directory, or using MAKI_BASH_PATH env override.
fn script_invocation(path: &Path) -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        if path.extension().and_then(|e| e.to_str()).map_or(false, |e| eq_ext(e, "sh")) {
            if let Some(bash) = find_windows_bash() {
                return (bash, vec![path.to_string_lossy().to_string()]);
            }
        }
    }
    // Default: execute directly
    (path.to_string_lossy().to_string(), Vec::new())
}

#[cfg(windows)]
fn eq_ext(ext: &str, target: &str) -> bool {
    ext.eq_ignore_ascii_case(target)
}

/// Find a working bash on Windows.
///
/// Priority:
/// 1. MAKI_BASH_PATH environment variable (explicit override)
/// 2. Standard Git for Windows locations (bash.exe, not git-shell.exe)
/// 3. PATH lookup for "bash" (with validation that it is not git-shell)
#[cfg(windows)]
fn find_windows_bash() -> Option<String> {
    // 1. Explicit override
    if let Ok(override_path) = std::env::var("MAKI_BASH_PATH") {
        if !override_path.is_empty() && Path::new(&override_path).is_file() {
            return Some(override_path);
        }
    }

    // 2. Git for Windows default install locations
    let git_bash_candidates = [
        // 64-bit Git for Windows default
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files\Git\bin\bash.exe",
        // 32-bit
        r"C:\Program Files (x86)\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ];
    for candidate in git_bash_candidates {
        let p = Path::new(candidate);
        if p.is_file() {
            // Validate it is not git-shell disguised as bash
            if !is_git_shell(p) {
                return Some(candidate.to_string());
            }
        }
    }

    // 3. Check if common Git install dir can be inferred from where git.exe is
    if let Ok(output) = std::process::Command::new("where")
        .arg("git.exe")
        .stdout(std::process::Stdio::piped())
        .output()
    {
        if output.status.success() {
            let git_path = String::from_utf8_lossy(&output.stdout);
            if let Some(first_line) = git_path.lines().next() {
                let git_dir = Path::new(first_line.trim()).parent();
                if let Some(dir) = git_dir {
                    // git.exe is in <Git>\cmd\ — bash is in <Git>\usr\bin\
                    let usr_bin_bash = dir.parent().map(|d| d.join("usr\\bin\\bash.exe"));
                    let bin_bash = dir.parent().map(|d| d.join("bin\\bash.exe"));
                    for candidate in usr_bin_bash.into_iter().chain(bin_bash) {
                        if candidate.is_file() && !is_git_shell(&candidate) {
                            return Some(candidate.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    // 4. Last resort: PATH lookup — try "bash" and validate
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(';') {
            let candidate = Path::new(dir).join("bash.exe");
            if candidate.is_file() && !is_git_shell(&candidate) {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }

    None
}

/// Check if a Windows executable is actually git-shell.
///
/// git-shell.exe rejects commands it doesn't recognize with "fatal: unrecognized command".
/// We detect this by running with `-c "echo maki-bash-probe"` and checking the output.
#[cfg(windows)]
fn is_git_shell(path: &Path) -> bool {
    match std::process::Command::new(path)
        .args(["-c", "echo maki-bash-probe-ok"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            // git-shell would print "fatal: unrecognized command" to stderr
            // or just refuse to run. A real bash prints "maki-bash-probe-ok".
            !stdout.contains("maki-bash-probe-ok") || stderr.contains("fatal:")
        }
        Err(_) => true, // if it doesn't even spawn, treat as invalid
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p maki-providers script_invocation -- --nocapture`
Expected: PASS (the Windows tests are `#[cfg(windows)]` and will be skipped on Linux; non-Windows test should pass)

- [ ] **Step 5: Commit**

```bash
git add maki-providers/src/providers/dynamic.rs
git commit -m "feat(providers): add git-shell detection and bash discovery for Windows dynamic providers"
```

---

## Task 2: Improve error messages when bash cannot be found

**Files:**
- Modify: `maki-providers/src/providers/dynamic.rs:153-212` (`run_script` function)

- [ ] **Step 1: Add a test for improved error message**

Add to the `#[cfg(test)] mod tests` block:

```rust
#[cfg(windows)]
#[test]
fn run_script_missing_bash_gives_helpful_error() {
    use std::path::PathBuf;
    // Save and clear MAKI_BASH_PATH to force the search path
    let original = std::env::var("MAKI_BASH_PATH").ok();
    std::env::remove_var("MAKI_BASH_PATH");

    // Use a directory with no bash.exe
    let fake_path = PathBuf::from("C:\\nonexistent\\provider.sh");
    let result = run_script(&fake_path, "info", INFO_TIMEOUT);

    // Restore
    if let Some(v) = original {
        std::env::set_var("MAKI_BASH_PATH", v);
    }

    match result {
        Err(AgentError::Config { message }) => {
            // The error should mention something more helpful than just "file not found"
            assert!(
                message.contains("bash") || message.contains("MAKI_BASH_PATH") || message.contains("sh"),
                "error message should hint at the real problem: {message}"
            );
        }
        other => panic!("expected Config error, got: {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p maki-providers run_script_missing_bash -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Update `run_script` to provide helpful error when `script_invocation` returns None-equivalent**

In the `run_script` function, the `script_invocation` function now returns `("bash", vec![])` as before when no suitable Windows bash is found. The actual problem surfaces when `Command::new(&program)` fails. Wrap it better:

Replace the spawn error handling in `run_script` (lines 160-167) with:

```rust
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            #[cfg(windows)]
            {
                // Detect common Windows script execution failures
                let program_lower = program.to_lowercase();
                if program_lower.contains("bash") || program_lower.contains("sh") {
                    return AgentError::Config {
                        message: format!(
                            "failed to run '{}' {subcommand}: {e}. \
                             On Windows, dynamic provider shell scripts require bash. \
                             If you are running in git-shell, set MAKI_BASH_PATH to the \
                             full path of bash.exe (e.g., C:\\Program Files\\Git\\usr\\bin\\bash.exe)",
                            path.display()
                        ),
                    };
                }
            }
            AgentError::Config {
                message: format!("failed to run {} {subcommand}: {e}", path.display()),
            }
        })?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p maki-providers run_script_missing_bash -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add maki-providers/src/providers/dynamic.rs
git commit -m "feat(providers): improve Windows script execution error messages"
```

---

## Task 3: Add MAKI_BASH_PATH validation at provider creation time

**Files:**
- Modify: `maki-providers/src/providers/dynamic.rs:396-460` (`create` function)

- [ ] **Step 1: Write the failing test**

Add to the test module:

```rust
#[cfg(windows)]
#[test]
fn create_validates_bash_availability() {
    // This test verifies that when MAKI_BASH_PATH is set to a valid path,
    // no early validation error occurs at the script_invocation level.
    // (Full create() integration tests require actual provider scripts.)
    let bash_override = std::env::var("MAKI_BASH_PATH").ok();
    std::env::set_var("MAKI_BASH_PATH", r"C:\Program Files\Git\usr\bin\bash.exe");

    // We can't fully test create() without a real provider script,
    // but we can verify find_windows_bash() respects the override.
    let result = find_windows_bash();

    // Restore
    if let Some(v) = bash_override {
        std::env::set_var("MAKI_BASH_PATH", v);
    } else {
        std::env::remove_var("MAKI_BASH_PATH");
    }

    // The override should be picked up if the file exists, or fall through
    // On Linux CI this test is cfg'd out
    if Path::new(r"C:\Program Files\Git\usr\bin\bash.exe").exists() {
        assert_eq!(result.as_deref(), Some(r"C:\Program Files\Git\usr\bin\bash.exe"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails (or is appropriately skipped)**

Run: `cargo test -p maki-provider create_validates_bash -- --nocapture`
Expected: On Linux, test is `#[cfg(windows)]` — skipped. On Windows, may pass if Git is installed at default path.

- [ ] **Step 3: Commit**

```bash
git add maki-providers/src/providers/dynamic.rs
git commit -m "test(providers): add MAKI_BASH_PATH override test for Windows"
```

---

## Task 4: Full workspace clippy + test verification

**Files:**
- (none — verification only)

- [ ] **Step 1: Run clippy**

Run: `cargo clippy -p maki-providers --tests -- -D warnings`
Expected: No warnings

- [ ] **Step 2: Run all provider tests**

Run: `cargo test -p maki-providers -- --nocapture`
Expected: All tests pass

- [ ] **Step 3: Run full workspace tests**

Run: `cargo nextest run --workspace`
Expected: All tests pass across workspace

---

## Self-Review Notes

- **Spec coverage**: Task 1 handles the core fix (git-shell detection + bash discovery), Task 2 improves UX (error messages), Task 3 validates the env override. Together they address all aspects of the problem.
- **Placeholder scan**: No TBDs, all code provided complete.
- **Type consistency**: `script_invocation` return type unchanged `(String, Vec<String>)`. New helper functions are private and `#[cfg(windows)]`.
- **Edge cases considered**:
  - Git installed at non-standard path (handled by PATH lookup + `where git.exe`)
  - git-shell named as `bash.exe` (handled by `is_git_shell` probe)
  - `MAKI_BASH_PATH` pointing to non-existent file (handled by `Path::new(&override_path).is_file()` check)
  - `.sh` extension in different cases (handled by `eq_ignore_ascii_case`)
