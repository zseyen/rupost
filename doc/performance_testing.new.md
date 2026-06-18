# RuPost 性能测试与调优指南 (Performance Testing Guide)

本文档面向测试工程师与核心开发者，详细介绍如何运行微观基准测试 (Micro-benchmarks) 与端到端高并发压力测试 (E2E Load Test)，并给出核心性能指标解析及优化调优指引。

---

## 1. 性能测试架构设计

我们遵循“微观算法极速，宏观并发吞吐”的原则，将性能测试划分为两个层级，全部收拢在 `benches/` 目录下：

```text
benches/
├── parser_bench.rs            # [微观] 用例解析器性能基准
├── trie_matcher_bench.rs      # [微观] Trie模糊路由匹配与条件评估基准
├── variable_resolver_bench.rs # [微观] 变量解析与模板渲染基准
└── load_test/                 # [宏观] 端到端并发压力负载压测
    ├── load_generator.rs      # 自研高性能压测发包器 (Tokio & Reqwest)
    ├── mock_perf_config.json  # 压测专用 Mock 规则
    └── run_load_test.sh       # 自动化一键压测控制脚本 (生成用例 -> 压测 -> 清理)
```

---

## 2. 运行微观基准测试 (Micro-benchmarks)

微观测试采用 Rust 生态中权威的 `criterion` 库。它可以在纳秒 (ns) 到微秒 (μs) 级别精确测量纯 CPU 计算函数的耗时，自动规避 CPU 频率抖动，并生成漂亮的 HTML 统计图表。

### 2.1 执行命令
在项目根目录下，直接运行对应的基准测试：
```bash
# 运行所有的微观基准测试
cargo bench

# 仅运行用例解析器基准
cargo bench --bench parser_bench

# 仅运行 Trie 模糊匹配基准
cargo bench --bench trie_matcher_bench

# 仅运行变量上下文替换基准
cargo bench --bench variable_resolver_bench
```

### 2.2 结果分析与图表生成
Criterion 运行完成后，会在终端打印出均值 (Mean)、中位数 (Median) 以及标准偏差。同时它会在本地生成可视化的 HTML 报表：
*   **报告路径**：`target/criterion/report/index.html`
*   **分析重点**：
    *   **Slope (斜率)**：展示单次函数调用的真实执行耗时。
    *   **Probability Density (概率密度分布)**：图表越陡峭、越集中，说明代码执行性能越稳定，偏差越小。

---

## 3. 运行宏观并发负载测试 (E2E Load Testing)

宏观压测模拟了海量真实请求与文件，用来探测 Mock 服务器的吞吐上限（QPS）和批测试 DAG 拓扑调度的系统开销。

### 3.1 运行方式
在项目根目录下直接执行一键控制脚本：
```bash
chmod +x benches/load_test/run_load_test.sh
./benches/load_test/run_load_test.sh
```

### 3.2 压测控制流原理解析
1.  **自动编译**：脚本会自动编译 `rupost` 主程序和 `load_generator` 高并发发包器（采用 `--release` 模式）。
2.  **Mock 极限压测**：
    *   后台拉起监听在 `9000` 端口的 Mock 服务器。
    *   启动 `load_generator`，以 50~100 的并发连接压测 Mock 服务器的变体条件匹配路由。
    *   输出吞吐率（QPS）、总请求完成数和平均时延（Latency）。
3.  **DAG 拓扑调度器压测**：
    *   脚本在 `./tests_temp_dir/` 自动生成 150 个具有多叉 Fork-Join 依赖网的 `.http` 文件。
    *   调用 `rupost test --mode parallel --concurrency 50` 并发调度这 150 个文件。
    *   脚本使用跨平台的 Python 命令 `python3 -c "import time; print(int(time.time() * 1000))"` 获取毫秒时间戳，以此精确测量拓扑调度和并发发包的总耗时。
4.  **自动清理**：杀掉 Mock 进程，删除临时文件，恢复开发环境。

---

## 4. 核心性能指标与调优基线

当您运行压测或分析基准数据时，以下指标可以作为系统调优的参考基线：

### 4.1 微观性能指标基线 (Target Baselines)
*   **用例解析器 (Parser)**：解析单条普通 HTTP 用例耗时应在 **<= 10 μs**，Markdown 嵌套用例解析耗时在 **<= 50 μs**，防止解析吞吐成为瓶颈。
*   **Trie 路由搜索 (Trie Routing)**：在包含 500 条模糊规则的 Trie 匹配树中，单次检索平均耗时应在 **<= 5 μs**。
*   **变体匹配评估 (Variant Condition Evaluate)**：包括多重 JSONPath 提取和 Headers 比对，单次匹配决策耗时应在 **<= 10 μs**。
*   **变量替换渲染 (Variable Resolving)**：渲染带有 10 个 KV 占位符的请求行耗时应在 **<= 2 μs**。

### 4.2 宏观性能指标基线
*   **本地 Mock 服务器最大吞吐量 (QPS)**：在 macOS (M1/M2/M3) 或主流 x86_64 Linux 容器上，本地回环压测 QPS 应达到 **15,000+**。
*   **DAG 并行批处理调度纯耗时**：运行 150 个带有 State Cloning 的本地回环（0ms 响应）用例，拓扑调度器的额外系统开销（剔除网络实际回环交互时间）应限制在 **<= 20ms**。

---

## 5. 性能瓶颈排查与调优方案

若测试出的指标未达预期，推荐按照以下维度进行定位和调优：

### 5.1 内存分配调优 (Memory Allocation)
*   **现象**：微观测试中 Criterion 指出有频繁的 heap allocation（堆内存分配）。
*   **优化方案**：
    *   在 `VariableResolver` 进行正则匹配和替换时，尽量避免 `.to_string()` 和 `String::clone()`，使用 Rust 的生命周期引用（如 `&str` 或 `Cow<'a, str>` 写入缓冲区）。
    *   对 Trie 树的路径切片分词，避免使用 `split('/')` 产生临时的 `Vec<String>`，改用迭代器遍历匹配以降低堆分配。

### 5.2 锁竞争优化 (Lock Contention)
*   **现象**：在并行模式下增加并发度（如 `--concurrency` 从 10 提升到 100），但 QPS 或调度总耗时没有明显提升，甚至 CPU 占用率虚高。
*   **优化方案**：
    *   检查 `VariableContext` 中的全局共享变量锁 `global: Arc<RwLock<HashMap>>`。如果大量并发用例频繁修改全局变量，会导致写锁饥饿和读锁竞争。
    *   在批测试中，尽量限制用例文件对全局变量的写入频率，将不经常变动的变量放入 `rupost.toml` 环境段（只读，无锁竞争）。

### 5.3 Tokio 运行时调度优化
*   **优化方案**：
    *   批处理有大量网络 I/O，属于典型的 I/O 密集型任务。如果调度发生卡顿，可以在执行时配置 `RUSTFLAGS="-C target-cpu=native"` 编译，发挥机器架构的最佳指令集。
