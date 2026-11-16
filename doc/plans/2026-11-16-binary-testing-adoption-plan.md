# 二进制级自动化测试采纳计划（跨平台 · 测试编译产物）

**日期：** 2026-11-16
**状态：** Ready for implementation
**定位：** 把"手动测试指南"（doc/guides/manual_test_guide.md，18 模块）逐层自动化为**跨平台二进制级测试**——被测对象是**编译产物** `target/release(rust 目标)/rupost`，不是进程内 API。
**前置共识：** 现有四层（cargo test 76 套件 / verify_features.sh / run_all.sh / CI 矩阵）与其边界（bash 不在 CI、Windows 无冒烟层）见对话记录；本计划按 ROI 混合方案 A(1)+A(2)+A(4) 优先、A(3)/A(5) 渐进落地。

---

## 1. 方案选型（四批，可独立合并）

| 批次 | 方案 | 覆盖 | 增量依赖 | 工作量 |
|---|---|---|---|---|
| **T1** | 方案 4：bash 冒烟进 CI | verify_features.sh 全特性语义断言（ubuntu+macos；Windows 走方案 1 补位） | 零 | S |
| **T2** | 方案 1：assert_cmd 契约测试（首批 10 例：退出码三值表 + incident 闭环 + validate 形态） | M18/M7/M13/M1 的可断言表面 | assert_cmd + predicates（dev） | M |
| **T3** | 方案 2：trycmd 黄金文件（`ast`×3 / `plugin schema` / `--help` 面） | 输出形态快照（确定性表面） | trycmd（dev） | S |
| **T4** | 方案 5：pytest 异构黑盒（手动指南参数化）**——按裁决直接进 CI**（2026-11-16） | hashlib 交叉验证/第三方视角 | actions/setup-python（CI 标准件） | S-M |
| **T5（可选）** | cargo-nextest + 分桶 | flake 治理（plugin 系限并发） | nextest（可选 CI 层） | S |
| 暂缓 | 方案 3 cucumber（三闭环叙事） | 验收叙事 | cucumber-rs | L（有人力再立项） |

---

## 2. WP-T0：脚手架（T2/T3 前置）

**Cargo.toml [dev-dependencies] 追加：**

```toml
assert_cmd = "2"
predicates = "3"
trycmd = "1.2"   # 审查核对:cargo search 实测最新 1.2.1(原稿 0.14 有误)
```

**原则（跨平台五条军规的测试侧映射）**：
1. 二进制定位统一 `Command::cargo_bin("rupost")`（禁手拼 target 路径）；
2. 路径断言一律 `PathBuf::join` + 指南 M18 的 posix 归一形态；
3. 交互式 TOFU 一律**指纹预置**（复用 `tests/cli_plugin_route_test.rs::preseed_tofu` 手法），禁交互等待；
4. 动态内容（端口/时间戳/绝对路径）**不进黄金文件**——要么固定端口、要么在 runner 里做 env/替换；
5. 颜色/TTY 确定性：子进程非 TTY 时 `colored` 默认去色，但仍统一注入 `NO_COLOR=1`（防 CI 差异）。

---

## 3. WP-T1：verify_features.sh 进 CI（方案 4）

**改动（.github/workflows/ci.yml）：** **独立 ubuntu job**（审查修正 C1：原稿串行追加进 3-OS 矩阵内——每个矩阵项各做一次 debug+release 双构建，CI 时长不可控；独立 job 单次 release 构建、与 test job 并行）：

```yaml
  # Job: WP-T1 全特性 bash 冒烟（ubuntu 先行；macos 视首跑时长再定）
  smoke-bash:
    name: Feature Verification Smoke (bash)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          cache-on-failure: true
      - name: Feature verification smoke
        run: bash tests/verify_features.sh
        env:
          RUPOST_TEST_PORT_BASE: 19000   # 固定基址：与随机 :0 端口物理隔离
```

**CI 可跑性核查（2026-11-16 实测代码）**：①TUI pty 步骤依赖 python3——ubuntu-latest 预装 ✓；②插件冒烟全部为 hook 型（无 TOFU 交互，TOFU 仅限 command 插件）✓；③`RUPOST_TEST_PORT_BASE=19000` 起 7 个端口，cargo test 的随机 `:0` 端口与分步执行不重叠 ✓；④脚本自含 `cargo build --release`，Swatinem 缓存后增量。

**要点：**
- 脚本自身已自含 `cargo build --release`（CI 上增量缓存可用）；
- TUI pty 步骤在 CI 无 tty：脚本已用 `python3 pty` 模拟（实测可跑）；
- **验收**：ubuntu/macos 两个矩阵项绿；Windows 项跳过该步不算失败。

**提交：** `ci: run feature verification smoke on linux/macos`

---

## 4. WP-T2：assert_cmd 契约测试（首批 10 例）

**文件：** `tests/binary_contract_test.rs`（新；**不重复** incident_bundle/contract_drift 已锁的闭环——只新增 assert_cmd 独有价值的表面：stdin/env/超时/退出码表/损坏输入）。

### 4.1 退出码三值表（M18 的数据驱动锁定）

```rust
mod common;
use assert_cmd::Command;
use predicates::str::contains;
use std::path::Path;

/// 统一构造：NO_COLOR + 历史隔离 + cwd
fn cmd(dir: &Path, history: &Path) -> Command {
    let mut c = Command::cargo_bin("rupost").unwrap();
    c.env("NO_COLOR", "1")
        .env("RUPOST_HISTORY_DIR", history)
        .current_dir(dir);
    c
}

/// M18 退出码三值表：exit 2 的三来源鉴别（stderr 前缀各不相同）
#[test]
fn test_exit_code_2_three_sources_disambiguation() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();

    // 来源1 clap 参数错误:前缀 "error: "
    cmd(dir.path(), history.path())
        .args(["t", "--no-such-flag"])
        .assert()
        .code(2)
        .stderr(contains("error:"));

    // 来源2 解析错误:前缀 "[ERROR] 解析错误: "
    std::fs::write(dir.path().join("bad.http"), "FTP https://x\n").unwrap();
    cmd(dir.path(), history.path())
        .args(["t", "bad.http"])
        .assert()
        .code(2)
        .stderr(contains("[ERROR] 解析错误:"));

    // 来源3 契约基线 I/O:损坏 JSON 基线 → "[ERROR] 契约基线工件错误: "
    std::fs::write(dir.path().join("ok.http"), "### ok\n# @assert status == 200\nGET http://127.0.0.1:9/ok\n").unwrap();
    std::fs::write(dir.path().join("broken.json"), "{not-json").unwrap();
    cmd(dir.path(), history.path())
        .args(["t", "ok.http", "--contract-baseline", "broken.json"])
        .assert()
        .code(2)
        .stderr(contains("[ERROR] 契约基线工件错误:"));
}
```

### 4.2 stdin 管道（assert_cmd 独有能力：**进程内写 stdin**，此前 bash 形态做不到）

```rust
/// M1 内联客户端从 stdin 读 body?若产品不支持 stdin body 则本例改为
/// --raw 形态并登记"stdin body 未支持"——先读 cli/parser.rs 确认再定稿
#[test]
fn test_inline_post_with_raw_body() {
    let dir = tempfile::tempdir().unwrap();
    let history = tempfile::tempdir().unwrap();
    // 本地 echo 服务器用 std TcpListener 现起(bind :0,无端口竞争)
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        if let Ok((mut s, _)) = listener.accept() {
            let mut buf = vec![0u8; 4096];
            let _ = std::io::Read::read(&mut s, &mut buf);
            let _ = std::io::Write::write_all(
                &mut s,
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            );
        }
    });

    cmd(dir.path(), history.path())
        .args(["POST", &format!("http://127.0.0.1:{port}/echo"), "--raw", "hello"])
        .assert()
        .code(0)
        .stdout(contains("200"));
    server.join().unwrap();
}
```

### 4.3 首批 10 例清单（映射手动指南模块）

| # | 用例 | 断言面 | 对应指南 |
|---|---|---|---|
| 1 | exit 2 三来源鉴别 | stderr 前缀 + code 2 ×3 | M18 |
| 2 | 内联 POST --raw | stdout 200 | M1.2 |
| 3 | 全局旗标位置契约 | URL 后置 `--verbose` → 被吸入(行为锁定) | M1.3 |
| 4 | --only 越界 | 可读错误 + code 1 | M2.3 |
| 5 | json 报告 stdout 纯净 | stdout 为合法 JSON 数组 | M5 |
| 6 | junit 落盘 + 覆盖警告 | stderr contains 已存在 | M5 |
| 7 | ~~incident 目标已存在~~ → **替换**：`--no-history` 后 history.jsonl 不存在（安全红线，无重复） | code 0 + 文件不存在 | 安全红线 |
| 8 | 契约基线首跑→复跑漂移(异常路径:损坏基线) | code 2 | M8 |
| 9 | plugin validate 合法/违规两态 | ✓ ready / ✗ failed 前缀 | M13.1 |
| 10 | `--no-history` 旗标 | 进程后 history.jsonl 不存在 | 安全红线 |

### 4.4 验收与提交

- **验收**：10 例绿；`cargo test --test binary_contract_test` 在 ubuntu/macos/windows 三平台 CI 绿（assert_cmd 派生为原生进程，Windows 兼容）；
- **反假守护**：例 1 的三来源各自破坏一次（如 main.rs 移除 ContractBaselineError 映射）→ 对应断言红；
- **提交**：`test(binary): assert_cmd contract tests — exit-code table, inline client, artifact errors`

---

## 5. WP-T3：trycmd 黄金文件（确定性表面铺面）

**文件：** `tests/trycmd_runner.rs` + `tests/cmd/*.trycmd`

```rust
#[test]
fn cli_golden_files() {
    trycmd::Test::new()
        .case("tests/cmd/*.trycmd")
        .default_bin("rupost")
        .env("NO_COLOR", "1")                       // 颜色确定性(黄金文件禁 ANSI)
        .env("RUPOST_HISTORY_DIR", env!("CARGO_TARGET_TMPDIR"))  // 历史落 target/tmp,零仓库污染
        .run();
}
```

**场景文件示例 1**（`tests/cmd/ast_contract.trycmd`——ast 输出完全确定，黄金文件最佳表面）：

```markdown
```rust
```

```text
$ rupost ast fixture.http
{
  "schema_version": "1.0",
  "source_path": "fixture.http",
  "dependencies": [],
  "requests": [
    {
      "number": 1,
      "method": "GET",
      "url": "{{base_url}}/ok",
      "headers": [],
      "body": null,
      "name": "ok",
      "skip": false,
      "assertions": ["status == 200"],
      "captures": [],
      "source_line": 3
    }
  ]
}
```
```

**同目录放 `fixture.http`**（静态三行：`### ok` / `# @assert status == 200` / `GET {{base_url}}/ok`——无网络依赖，ast 不执行请求）。

**场景文件示例 2**（`tests/cmd/plugin_validate.trycmd`）：

```markdown
$ rupost plugin validate rupost-plugin-cmd-hello.json
✓ ready: cmd-hello v0.1.0（commands: 1, hooks: 0）
```

**要点：**
- **trycmd 1.x API 核实项（审查新增）**：runner 的 `.env(KEY, VALUE)` 与 `.default_bin("rupost")` 需在实现时对 trycmd 1.2 文档核实（若 `.env` 缺位，用场景内 `[!env]` 指令或 conftest 化的 `std::env::set_var` 兜底）；
- **确定性甄别**：进黄金文件的表面必须满足"同一输入 → 逐字节同一输出"。当前确定过的：`ast`（纯投影）、`plugin validate`（固定清单）、`--help`；**禁入**：含 mock 端口/时间戳/tempdir 绝对路径的输出；
- **行尾**：trycmd 自带行尾归一，Windows CRLF 不破黄金文件；
- **验收**：`cargo test --test trycmd_runner` 绿；手改一处黄金文件预期 → 红（反假守护）；
- **提交**：`test(binary): trycmd golden files for deterministic surfaces (ast/validate/help)`

---

## 6. WP-T4：pytest 异构黑盒（**CI 常驻 job**，2026-11-16 裁决修订）

**定位修正（裁决）**：自动化测试直接进 CI,不做"发版前本地手动"——原"发版前批/不入 CI"定位废除。

**pytest 的作用（三个不可替代点，与 Rust 侧测试的边界）：**
1. **异构交叉验证（防"实现自证"）**：Rust 侧测试与实现共享 sha2 crate/编码写法/路径习惯——pytest 用 Python 标准库（hashlib/json/pathlib）独立复算，同错互证盲区在此被第三方实现照亮（NIST 向量是"点"锚定，hashlib 是"面"锚定）；
2. **手动指南的 1:1 可执行化**：18 模块手册的每条命令翻译成参数化用例——指南从此不是"会过时的文档"，而是**可回归的测试资产**（文档腐烂由 CI 直接报警）；
3. **独立断言生态**：jsonschema 校验报告结构、pytest 参数化/夹具管理 18 模块矩阵——比手写 Rust 断言表达成本低。

**CI job 设计（.github/workflows/ci.yml 新增，ubuntu 常驻）：**

```yaml
  # Job 6: WP-T4 异构黑盒验收（Python 独立实现交叉验证 + 手动指南参数化）
  e2e-pytest:
    name: E2E Blackbox (pytest)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          cache-on-failure: true
      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'
      - name: Build release binary
        run: cargo build --release --bin rupost
      - name: Install pytest
        run: pip install pytest
      - name: Run heterogeneous blackbox suite
        env:
          RUPOST_BIN: ${{ github.workspace }}/target/release/rupost
        run: pytest tests_e2e -q
```

**骨架（tests_e2e/，requirements: pytest；断言全标准库）：**

```python
# tests_e2e/conftest.py
import subprocess, tempfile, os, pytest

BIN = os.environ.get("RUPOST_BIN", "target/release/rupost")

@pytest.fixture
def run():
    ws = tempfile.TemporaryDirectory()
    hist = tempfile.TemporaryDirectory()
    def _run(*args):
        return subprocess.run(
            [BIN, *args],
            cwd=ws.name,
            env={**os.environ, "RUPOST_HISTORY_DIR": hist.name},
            capture_output=True, text=True, timeout=60,
        )
    yield _run
    ws.cleanup(); hist.cleanup()
```

```python
# tests_e2e/test_manual_guide_M7.py —— 手动指南 M7 的 1:1 参数化（异构 hashlib 交叉验证）
import hashlib, json

def test_incident_digest_independent_of_rust_impl(run, tmp_path):
    # NIST 思路的再一层：用 Python hashlib（非 sha2 crate）交叉验证 rust 侧摘要
    (tmp_path / "pay.http").write_text(
        "### c\n# @assert status == 200\nPOST http://127.0.0.1:1/x\n")
    r = run("t", "pay.http", "--incident-out", str(tmp_path / "inc"))
    assert r.returncode == 1
    manifest = json.loads((tmp_path / "inc" / "manifest.json").read_text())
    for d in manifest["artifact_digests"]:
        blob = (tmp_path / "inc" / d["file"]).read_bytes()
        assert d["sha256"] == hashlib.sha256(blob).hexdigest()   # 独立实现
        assert d["bytes"] == len(blob)
```

**要点：**
- **CI 成本**：release 构建（缓存后增量）+ pip install pytest ≈ 增量 1-2 分钟——自动化换手工，值；
- **被测对象**：`RUPOST_BIN` 指向 CI 构建的 release 产物（与本计划"测试编译产物"定位一致）；
- **参数化铺面节奏**：M7 → M18 退出码表 → M3 级联，逐模块把手动指南翻译为 `@pytest.mark.parametrize` 用例；
- **不重复原则**：与 Rust 侧 e2e 重叠的语义闭环（如 incident 目标已存在保护）不重复铺，pytest 只锁"异构视角"（hashlib 交叉/指南 1:1/独立 JSON schema 校验）。

**验收**：job 在 CI 绿；**提交**：`ci(e2e): pytest heterogeneous blackbox job (guide parametrized)`

## 7. 查漏补缺（对本计划自身的对抗性审查）

1. **assert_cmd 例 2 依赖网络形态假设**：echo 服务器为裸 TCP 应答，rupost 客户端可能要求合法响应头集——已按最小合法 HTTP/1.1 应答构造；若产品对 HTTP/1.0/1.1 有要求，改用 wiremock（进程内 HTTP，跨平台）替代裸 TCP——**实现时先实跑再定稿**。
2. **trycmd 的 `default_bin("rupost")` 依赖包名**：Cargo.toml `name = "rupost"` 已核实 ✓；若未来改名，此处是单点。
3. **黄金文件的隐藏动态源**：`--version` 输出含版本号（升版即改黄金文件——可接受，版本变化本就应审）；`plugin validate` 输出含版本号 v0.1.0（cmd-hello 清单升版需同步）——两处都已登记。
4. **T1 的 CI 时长**：verify_features.sh 含 release 构建，CI 冷缓存首跑 +3~5 分钟——用 Swatinem/rust-cache 既有缓存缓解；不可接受时改 `cargo build --release --bin rupost` 精确编译。
5. **T4 的 timeout=60**：大套件可能超——参数化时逐用例收紧；parse 类 <1s。
6. **重复覆盖风险**：T2 例 7/8 与既有 incident_bundle/contract_drift e2e 有场景重叠——T2 只锁"assert_cmd 形态"（退出码/stderr 前缀），语义细节以既有 e2e 为准，不重复铺断言。
7. **nextest 不入本计划必跑**：nextest 改变测试执行拓扑（每用例独立进程），对现有进程派生型测试是行为变化——先在 CI 以 optional job 试点，稳定后再转正。

---

## 8. 执行顺序与门禁

**T0 → T1 → T2 → T3 → T4（均进 CI）→ T5（可选）**；每批独立提交：
- T1 `ci: run feature verification smoke on linux/macos`
- T2 `test(binary): assert_cmd contract tests — exit-code table, inline client, artifact errors`
- T3 `test(binary): trycmd golden files for deterministic surfaces (ast/validate/help)`
- T4 `test(e2e): pytest heterogeneous blackbox suite (guide parametrized)`

**最终门禁**（累计）：既有 7/7 + `cargo test --test binary_contract_test` + `cargo test --test trycmd_runner` + **CI e2e-pytest job 绿**（T4 按裁决进 CI，不再本地手动）。
