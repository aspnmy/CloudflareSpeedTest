use crate::constants::{
    DEFAULT_PING_ROUTINES, DEFAULT_PING_TIMES, DEFAULT_PORT, MAX_PING_ROUTINES,
    TCP_CONNECT_TIMEOUT_SECS,
};
use crate::data::{CloudflareIpData, PingData};
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use std::net::IpAddr;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout};

const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(TCP_CONNECT_TIMEOUT_SECS);

/// TCP ping configuration
#[derive(Debug, Clone)]
pub struct TcpingConfig {
    pub routines: usize,
    pub port: u16,
    pub ping_times: u32,
}

impl Default for TcpingConfig {
    fn default() -> Self {
        Self {
            routines: DEFAULT_PING_ROUTINES,
            port: DEFAULT_PORT,
            ping_times: DEFAULT_PING_TIMES,
        }
    }
}

impl TcpingConfig {
    pub fn normalize(&mut self) {
        if self.routines == 0 || self.routines > MAX_PING_ROUTINES {
            self.routines = DEFAULT_PING_ROUTINES;
        }
        if self.port == 0 {
            self.port = DEFAULT_PORT;
        }
        if self.ping_times == 0 {
            self.ping_times = DEFAULT_PING_TIMES;
        }
    }
}

/// Run TCP ping test on all IPs concurrently.
/// Uses `buffer_unordered` for bounded concurrency (no Mutex/spawn overhead).
pub async fn run_tcping(
    ips: Vec<IpAddr>,
    config: TcpingConfig,
) -> Vec<CloudflareIpData> {
    let total = ips.len();
    if total == 0 {
        return Vec::new();
    }

    let bar = ProgressBar::new(total as u64);
    bar.set_style(
        ProgressStyle::default_bar()
            .template("{pos}/{len} [{bar:40}] {msg}")
            .unwrap()
            .progress_chars("##-"),
    );
    bar.set_message("可用: 0");

    // Build a concurrent stream: bounded by buffer_unordered(config.routines)
    let stream = stream::iter(ips).map(|ip| {
        let cfg = config.clone();
        async move { tcping_ip(&ip, &cfg).await }
    });

    let mut results = Vec::with_capacity(total);
    let mut stream = stream.buffer_unordered(config.routines);

    while let Some(data) = stream.next().await {
        bar.inc(1);
        if let Some(d) = data {
            results.push(d);
            bar.set_message(format!("可用: {}", results.len()));
        }
    }

    bar.finish_and_clear();
    results
}

/// Ping a single IP via TCP connect
async fn tcping_ip(ip: &IpAddr, config: &TcpingConfig) -> Option<CloudflareIpData> {
    let mut received = 0u32;
    let mut total_delay = Duration::ZERO;

    let addr = match ip {
        IpAddr::V4(v4) => format!("{}:{}", v4, config.port),
        IpAddr::V6(v6) => format!("[{}]:{}", v6, config.port),
    };
    for _ in 0..config.ping_times {
        let start = Instant::now();
        match timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(&addr)).await {
            Ok(Ok(stream)) => {
                drop(stream);
                received += 1;
                total_delay += start.elapsed();
            }
            _ => continue,
        }
    }

    if received == 0 {
        return None;
    }

    Some(CloudflareIpData {
        ping_data: PingData {
            ip: *ip,
            sent: config.ping_times,
            received,
            delay: total_delay / received,
            colo: String::new(),
        },
        download_speed: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let cfg = TcpingConfig::default();
        assert_eq!(cfg.routines, DEFAULT_PING_ROUTINES);
        assert_eq!(cfg.port, DEFAULT_PORT);
        assert_eq!(cfg.ping_times, DEFAULT_PING_TIMES);
    }

    #[test]
    fn test_normalize_clamps_routines_to_max() {
        let mut cfg = TcpingConfig {
            routines: 2000,
            ..Default::default()
        };
        cfg.normalize();
        assert_eq!(cfg.routines, DEFAULT_PING_ROUTINES);
    }

    #[test]
    fn test_normalize_allows_valid_routines() {
        let mut cfg = TcpingConfig {
            routines: 500,
            ..Default::default()
        };
        cfg.normalize();
        assert_eq!(cfg.routines, 500);
    }

    #[test]
    fn test_normalize_fixes_zero_port() {
        let mut cfg = TcpingConfig {
            port: 0,
            ..Default::default()
        };
        cfg.normalize();
        assert_eq!(cfg.port, DEFAULT_PORT);
    }

    #[test]
    fn test_normalize_fixes_zero_ping_times() {
        let mut cfg = TcpingConfig {
            ping_times: 0,
            ..Default::default()
        };
        cfg.normalize();
        assert_eq!(cfg.ping_times, DEFAULT_PING_TIMES);
    }

    #[tokio::test]
    async fn test_run_tcping_empty_returns_empty() {
        let result = run_tcping(vec![], TcpingConfig::default()).await;
        assert!(result.is_empty());
    }
}
