//! Load test: N fake installs send a ping, event batches and a performance report each, over
//! keep-alive HTTP/1.1, and the tool prints requests per second, the latency and the server's peak
//! RAM (VmHWM of its pid, when given).
//!
//! Run a server with `RAZDOR_TRUST_PROXY=1` (the tool spreads its installs over fake client IPs
//! through `X-Real-IP`, so the per-IP limits do not trip on one loopback address), then:
//!
//! `cargo run --release -p razdor-server --example load -- 127.0.0.1:8080 2000 8 64 <server-pid>`
//!
//! Arguments (all optional): address, installs, event batches per install, concurrency, server pid.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

struct Request {
    path: &'static str,
    body: Vec<u8>,
    ip: String,
}

fn install_id(n: usize) -> String {
    format!("{:032x}", 0x5eed_0000_0000_0000_0000_0000_0000_0000u128 | n as u128)
}

fn requests_for(n: usize, batches: usize) -> Vec<Request> {
    let id = install_id(n);
    let ip = format!("10.{}.{}.{}", (n >> 16) & 255, (n >> 8) & 255, n & 255);
    let mut out = Vec::new();
    let ping: Value = json!({
        "schema": 1, "install_id": id, "razdor": "1.5.4",
        "platform": "linux x86_64 64", "game": "dt", "lang": "ru"
    });
    out.push(Request { path: "/v1/ping", body: ping.to_string().into_bytes(), ip: ip.clone() });
    for b in 0..batches {
        let events: Vec<Value> = (0..20)
            .map(|i| {
                json!({ "at": 1_769_000_000 + b * 20 + i, "kind": "progress", "day": i, "hero_level": 3, "army_size": 12 })
            })
            .collect();
        let batch = json!({ "schema": 1, "install_id": id, "razdor": "1.5.4", "events": events });
        out.push(Request { path: "/v1/events", body: batch.to_string().into_bytes(), ip: ip.clone() });
    }
    let perf = json!({
        "schema": 1, "install_id": id, "razdor": "1.5.4",
        "device": { "os": "linux", "os_version": "6.x", "gpu": "RTX 4060 Ti", "gpu_driver": "550", "cpu_cores": 8,
                    "ram_mb": 32000, "screen_w": 1920, "screen_h": 1080, "scale_pct": 100 },
        "quality": { "preset": "high" },
        "screens": [ { "screen": "map", "median_ms": 16.0, "p99_ms": 25.0, "p999_ms": 40.0, "minutes": 2.0 } ],
        "loads": [ { "what": "map", "ms": 1200 } ],
        "memory": { "peak_ram_mb": 400, "low_memory_warnings": 0 },
        "profile": []
    });
    out.push(Request { path: "/v1/perf", body: perf.to_string().into_bytes(), ip, });
    out
}

/// One request on a kept-alive connection; returns the status and the latency.
async fn send(stream: &mut TcpStream, req: &Request, addr: &str) -> std::io::Result<u16> {
    let head = format!(
        "POST {} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         X-Real-IP: {}\r\nConnection: keep-alive\r\n\r\n",
        req.path,
        req.body.len(),
        req.ip
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&req.body).await?;
    let mut buf = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    let header_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head_text = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let status: u16 = head_text.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let length: usize = head_text
        .lines()
        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse().ok()))
        .unwrap_or(0);
    while buf.len() < header_end + length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(status)
}

fn peak_rss_mb(pid: &str) -> Option<f64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kb: f64 = text.lines().find_map(|l| l.strip_prefix("VmHWM:"))?.split_whitespace().next()?.parse().ok()?;
    Some(kb / 1024.0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let addr = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1:8080".to_string());
    let installs: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2000);
    let batches: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8);
    let concurrency: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(64);
    let pid = args.get(5).cloned();

    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("runtime");
    runtime.block_on(async move {
        let next = Arc::new(AtomicUsize::new(0));
        let ok = Arc::new(AtomicUsize::new(0));
        let failed = Arc::new(AtomicUsize::new(0));
        let latencies = Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));
        let started = Instant::now();
        let mut tasks = Vec::new();
        for _ in 0..concurrency {
            let (next, ok, failed, latencies, addr) = (next.clone(), ok.clone(), failed.clone(), latencies.clone(), addr.clone());
            tasks.push(tokio::spawn(async move {
                let mut stream: Option<TcpStream> = None;
                let mut local = Vec::new();
                loop {
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    if n >= installs {
                        break;
                    }
                    for req in requests_for(n, batches) {
                        if stream.is_none() {
                            stream = TcpStream::connect(&addr).await.ok();
                        }
                        let Some(s) = stream.as_mut() else {
                            failed.fetch_add(1, Ordering::Relaxed);
                            continue;
                        };
                        let t = Instant::now();
                        match tokio::time::timeout(Duration::from_secs(10), send(s, &req, &addr)).await {
                            Ok(Ok(200)) => {
                                ok.fetch_add(1, Ordering::Relaxed);
                            }
                            Ok(Ok(_)) => {
                                failed.fetch_add(1, Ordering::Relaxed);
                            }
                            _ => {
                                failed.fetch_add(1, Ordering::Relaxed);
                                stream = None;
                            }
                        }
                        local.push(t.elapsed().as_micros().min(u32::MAX as u128) as u32);
                    }
                }
                latencies.lock().unwrap_or_else(|e| e.into_inner()).extend(local);
            }));
        }
        for t in tasks {
            let _ = t.await;
        }
        let elapsed = started.elapsed().as_secs_f64();
        let mut lat = latencies.lock().unwrap_or_else(|e| e.into_inner()).clone();
        lat.sort_unstable();
        let pct = |p: f64| -> f64 {
            if lat.is_empty() {
                0.0
            } else {
                lat[((lat.len() as f64 - 1.0) * p) as usize] as f64 / 1000.0
            }
        };
        let total = ok.load(Ordering::Relaxed) + failed.load(Ordering::Relaxed);
        println!("installs {installs}, batches {batches}, concurrency {concurrency}, address {addr}");
        println!(
            "requests {total} (ok {}, failed {}) in {elapsed:.2} s: {:.0} req/s",
            ok.load(Ordering::Relaxed),
            failed.load(Ordering::Relaxed),
            total as f64 / elapsed.max(1e-9)
        );
        println!("latency ms: p50 {:.2}, p99 {:.2}, max {:.2}", pct(0.5), pct(0.99), pct(1.0));
        match pid.as_deref().and_then(peak_rss_mb) {
            Some(mb) => println!("server peak RAM (VmHWM): {mb:.1} MB"),
            None => println!("server peak RAM: pass the server pid as the fifth argument to read it"),
        }
    });
}
