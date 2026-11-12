//! 跨平台（Linux/macOS/Windows）集成测试用原生 Mock 插件可执行体生成器。
//! 通过 `rustc` 在测试二进制生命周期内按需编译一次零依赖原生二进制，
//! 并以同目录 `<exe>.mock` 指令文件驱动，彻底消除 Windows 上 `#!/bin/sh` 报 os error 193 的问题。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const MOCK_PLUGIN_SRC: &str = r#"
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("$ENV{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 5..];
        if let Some(end) = after.find('}') {
            let key = &after[..end];
            if let Ok(val) = std::env::var(key) {
                out.push_str(&val);
            }
            rest = &after[end + 1..];
        } else {
            out.push_str("$ENV{");
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn read_all_stdin() -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    buf
}

fn main() {
    let exe = std::env::current_exe().expect("current_exe");
    let cfg_path = PathBuf::from(format!("{}.mock", exe.display()));
    let cfg = fs::read_to_string(&cfg_path).unwrap_or_default();
    let args: Vec<String> = std::env::args().skip(1).collect();

    for line in cfg.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        match key {
            "READ_STDIN" => {
                let _ = read_all_stdin();
            }
            "CAPTURE_STDIN" => {
                let buf = read_all_stdin();
                fs::write(val, &buf).expect("capture stdin");
            }
            "APPEND_STDIN" => {
                let buf = read_all_stdin();
                let s = String::from_utf8_lossy(&buf);
                let trimmed = s.trim_end_matches(&['\r', '\n'][..]);
                let mut f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(val)
                    .expect("open append stdin");
                writeln!(f, "{trimmed}").expect("append stdin");
            }
            "CAPTURE_ARGS_ENV" => {
                let (path, env_key) = val.split_once('|').expect("CAPTURE_ARGS_ENV format");
                let env_val = std::env::var(env_key).unwrap_or_default();
                let content = format!("args={}\nenv={}\n", args.join(" "), env_val);
                fs::write(path, content).expect("capture args/env");
            }
            "CHECK_ENV_ABSENT" => {
                if std::env::var_os(val).is_some() {
                    eprintln!("leaked");
                    process::exit(1);
                }
            }
            "CHECK_ENV_PRESENT" => {
                if std::env::var(val).unwrap_or_default().is_empty() {
                    process::exit(2);
                }
            }
            "APPEND_LINE" => {
                let (path, text) = val.split_once('|').expect("APPEND_LINE format");
                let mut f = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .expect("open append line");
                writeln!(f, "{text}").expect("append line");
            }
            "TOUCH" => {
                fs::write(val, b"").expect("touch");
            }
            "WAIT_FILE" => {
                let mut parts = val.splitn(3, '|');
                let path = parts.next().unwrap();
                let timeout_ms: u64 = parts.next().unwrap().parse().unwrap();
                let fail_code: i32 = parts.next().unwrap().parse().unwrap();
                let deadline = Instant::now() + Duration::from_millis(timeout_ms);
                while !PathBuf::from(path).exists() {
                    if Instant::now() >= deadline {
                        process::exit(fail_code);
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
            "SLEEP_MS" => {
                let ms: u64 = val.parse().expect("SLEEP_MS");
                thread::sleep(Duration::from_millis(ms));
            }
            "STDERR_FLOOD" => {
                let total: usize = val.parse().expect("STDERR_FLOOD");
                let chunk = vec![b'x'; 65536];
                let mut err = std::io::stderr();
                let mut rem = total;
                while rem > 0 {
                    let n = rem.min(chunk.len());
                    let _ = err.write_all(&chunk[..n]);
                    rem -= n;
                }
                let _ = err.flush();
            }
            "STDERR_LINE" => {
                eprintln!("{val}");
            }
            "STDOUT_LINE" => {
                println!("{}", expand_env(val));
            }
            "EXIT_CODE" => {
                let code: i32 = val.parse().expect("EXIT_CODE");
                process::exit(code);
            }
            _ => {}
        }
    }
}
"#;

static MOCK_BIN: OnceLock<(tempfile::TempDir, PathBuf)> = OnceLock::new();

fn template_mock_bin() -> &'static Path {
    let (_, bin_path) = MOCK_BIN.get_or_init(|| {
        let dir = tempfile::tempdir().expect("create mock compiler tempdir");
        let src = dir.path().join("mock_plugin.rs");
        let bin = dir
            .path()
            .join(format!("mock_plugin{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&src, MOCK_PLUGIN_SRC).expect("write mock_plugin.rs");
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let out = Command::new(rustc)
            .arg(&src)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("invoke rustc for mock plugin");
        assert!(
            out.status.success(),
            "compile mock_plugin failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        (dir, bin)
    });
    bin_path.as_path()
}

/// 在 `dir` 下安装名为 `base_name` 的跨平台原生 Mock 可执行体（Windows 自动附加 `.exe`）及 `.mock` 指令文件。
pub fn install_mock_executable(dir: &Path, base_name: &str, directives: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("create plugin dir");
    let exe_name = format!("{base_name}{}", std::env::consts::EXE_SUFFIX);
    let exe_path = dir.join(exe_name);
    std::fs::copy(template_mock_bin(), &exe_path).expect("copy mock executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod 755 mock executable");
    }
    let cfg_path = PathBuf::from(format!("{}.mock", exe_path.display()));
    std::fs::write(&cfg_path, directives).expect("write mock directives");
    exe_path
}
