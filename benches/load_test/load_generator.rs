use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    
    // 简易命令行参数解析，不依赖复杂 cli，保持极致自备
    let mut url = "http://127.0.0.1:9000/api/users/88".to_string();
    let mut connections = 50usize;
    let mut duration_secs = 5u64; // 默认缩短为 5s，减少长耗时
    let mut header_opt: Option<(String, String)> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--url" => { url = args[i+1].clone(); i += 2; }
            "-c" | "--connections" => { connections = args[i+1].parse()?; i += 2; }
            "-d" | "--duration" => { duration_secs = args[i+1].parse()?; i += 2; }
            "-H" | "--header" => {
                let parts: Vec<&str> = args[i+1].splitn(2, ':').collect();
                if parts.len() == 2 {
                    header_opt = Some((parts[0].trim().to_string(), parts[1].trim().to_string()));
                }
                i += 2;
            }
            _ => { i += 1; }
        }
    }

    println!("[*] 启动自研并发发包压测器...");
    println!("    目标 URL: {}", url);
    println!("    并发连接: {}", connections);
    println!("    持续时间: {} 秒", duration_secs);
    if let Some((ref k, ref v)) = header_opt {
        println!("    附加 Header: {}: {}", k, v);
    }

    let client_builder = reqwest::Client::builder()
        .pool_max_idle_per_host(connections)
        .tcp_nodelay(true);
    
    let client = Arc::new(client_builder.build()?);
    let url = Arc::new(url);
    
    let total_ok = Arc::new(AtomicUsize::new(0));
    let total_err = Arc::new(AtomicUsize::new(0));
    let total_latency_ms = Arc::new(AtomicUsize::new(0));

    let start_time = Instant::now();
    let end_time = start_time + Duration::from_secs(duration_secs);

    let (tx, mut rx) = mpsc::channel(connections);

    for _ in 0..connections {
        let client = client.clone();
        let url = url.clone();
        let total_ok = total_ok.clone();
        let total_err = total_err.clone();
        let total_latency_ms = total_latency_ms.clone();
        let header_opt = header_opt.clone();
        let tx = tx.clone();

        tokio::spawn(async move {
            while Instant::now() < end_time {
                let mut req = client.get(url.as_ref());
                if let Some((ref k, ref v)) = header_opt {
                    req = req.header(k, v);
                }

                let req_start = Instant::now();
                match req.send().await {
                    Ok(resp) => {
                        let latency = req_start.elapsed().as_millis() as usize;
                        // 2xx, 3xx 以及我们的 403 兜底响应在 Mock 中都视作成功命中
                        if resp.status().is_success() || resp.status().as_u16() == 403 {
                            total_ok.fetch_add(1, Ordering::Relaxed);
                        } else {
                            total_err.fetch_add(1, Ordering::Relaxed);
                        }
                        total_latency_ms.fetch_add(latency, Ordering::Relaxed);
                    }
                    Err(_) => {
                        total_err.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            let _ = tx.send(()).await;
        });
    }

    // 释放多余的 sender
    drop(tx);

    // 等待所有线程跑完
    while rx.recv().await.is_some() {}

    let actual_duration = start_time.elapsed();
    let ok_count = total_ok.load(Ordering::Relaxed);
    let err_count = total_err.load(Ordering::Relaxed);
    let total_requests = ok_count + err_count;
    let duration_secs_f64 = actual_duration.as_secs_f64();
    let qps = total_requests as f64 / duration_secs_f64;
    let avg_latency = if total_requests > 0 {
        total_latency_ms.load(Ordering::Relaxed) as f64 / total_requests as f64
    } else {
        0.0
    };

    println!("\n=========================================");
    println!("      RuPost Load Generator Results      ");
    println!("=========================================");
    println!("总请求数:       {}", total_requests);
    println!("成功请求数:     {}", ok_count);
    println!("失败请求数:     {}", err_count);
    println!("实际运行时间:   {:.3} 秒", duration_secs_f64);
    println!("平均吞吐率 QPS: {:.2}", qps);
    println!("平均响应延迟:   {:.2} 毫秒", avg_latency);
    println!("=========================================\n");

    Ok(())
}
