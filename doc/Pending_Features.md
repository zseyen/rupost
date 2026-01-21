# Pending Features (Unimplemented)

Extracted from `next_phase_plan.md.resolved` as of 2026-01-24.

## 🔴 P0 - Must Implement (Next 2 Weeks)

1.  **[Partially Implemented] Detailed Debug Mode (详细调试模式)**
    *   `--debug` parameter (Network Diagnostics).
    *   Diagnostics metrics: DNS Loopup, TCP Connect, TLS Handshake, TTFB.
    *   Visualized timing output.
    *   `--debug-on-failure`.
    *   *(Current status: Only `--verbose` exists)*

2.  **Test Snapshots (测试快照保存与对比)**
    *   Snapshot JSON format definition.
    *   Save logic: `rupost test ... --save-snapshot`.
    *   Compare logic: `rupost test ... --compare-with`.
    *   Diff report generation.

## 🟠 P1 - Important (Next 1 Month)

3.  **Dynamic Variables (动态变量)**
    *   Built-in dynamic vars: `{{$timestamp}}`, `{{$uuid}}`, `{{$random_int}}`.
    *   Variable provider trait.

4.  **Request Dependency Control (请求依赖和顺序控制)**
    *   `@depends_on` directive.
    *   Skip dependent requests on failure.
    *   Parallel execution for independent requests (basic support).

5.  **Output Format Enhancement (输出格式增强)**
    *   JSON Report (`--format json`).
    *   JUnit XML Report (`--format junit`) for CI/CD.

## 🟡 P2 - Optimization (Next 2 Months)

6.  **Variable Encryption (变量加密)**
    *   SOPS-like encryption for sensitive config values (API keys).
    *   `ENC[...]` parsing support.

7.  **Parallel Execution Optimization (并行执行优化)**
    *   `--parallel` flag.
    *   Progress bar.
    *   Worker pool implementation.

8.  **Mock Server**
    *   Start mock server from snapshot: `rupost mock --snapshot baseline.json`.

## 🟢 P3 - Long Term

9.  **Log Integration & Tracing**
    *   Auto Trace ID extraction (`CaptureSource::TraceHeader`).
    *   Distributed tracing support.

10. **Performance Benchmarking**
    *   Response time statistics.
    *   Performance regression detection (based on Snapshots).

11. **Plugin System**
    *   Custom variables/assertions.
