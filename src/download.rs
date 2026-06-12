use crate::constants::{
    DEFAULT_DOWNLOAD_TEST_COUNT, DEFAULT_DOWNLOAD_TIMEOUT_SECS, DEFAULT_DOWNLOAD_URL,
    USER_AGENT,
};
use crate::data::{CloudflareIpData, DownloadSpeedSet};
use crate::httping::get_header_colo;
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::ClientBuilder;
use std::net::SocketAddr;
use std::time::{Duration, Instant};


const DOWNLOAD_CONCURRENCY: usize = 10;
/// Number of speed samples to take during the download timeout period.
/// The sampling interval = timeout / SPEED_SAMPLES.
const SPEED_SAMPLES: u32 = 100;

/// Simple EWMA (Exponentially Weighted Moving Average) implementation.
/// Alpha defaults to 1/3, matching the Go library github.com/VividCortex/ewma.
/// The value is in bytes/second.
struct Ewma {
    alpha: f64,
    value: f64,
    initialized: bool,
}

impl Ewma {
    fn new() -> Self {
        Self {
            alpha: 1.0 / 3.0,
            value: 0.0,
            initialized: false,
        }
    }

    /// Add a sample value (expected unit: bytes/sec).
    fn add(&mut self, sample: f64) {
        if !self.initialized {
            self.value = sample;
            self.initialized = true;
        } else {
            self.value = self.alpha * sample + (1.0 - self.alpha) * self.value;
        }
    }

    fn get(&self) -> f64 {
        self.value
    }
}

/// Download speed test configuration
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    pub url: String,
    pub timeout: Duration,
    pub test_count: usize,
    pub min_speed: f64, // MB/s
    pub port: u16,
    pub disable: bool,
    pub debug: bool,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            url: DEFAULT_DOWNLOAD_URL.to_string(),
            timeout: Duration::from_secs(DEFAULT_DOWNLOAD_TIMEOUT_SECS),
            test_count: DEFAULT_DOWNLOAD_TEST_COUNT,
            min_speed: 0.0,
            port: 443,
            disable: false,
            debug: false,
        }
    }
}

impl DownloadConfig {
    pub fn normalize(&mut self) {
        if self.url.is_empty() {
            self.url = DEFAULT_DOWNLOAD_URL.to_string();
        }
        if self.timeout <= Duration::ZERO {
            self.timeout = Duration::from_secs(DEFAULT_DOWNLOAD_TIMEOUT_SECS);
        }
        if self.test_count == 0 {
            self.test_count = DEFAULT_DOWNLOAD_TEST_COUNT;
        }
    }
}

/// Run download speed test on IPs — concurrent with per-IP Client + resolve().
pub async fn run_download(
    ip_data: Vec<CloudflareIpData>,
    config: DownloadConfig,
) -> DownloadSpeedSet {
    if config.disable || ip_data.is_empty() {
        return if config.disable {
            DownloadSpeedSet::new(ip_data)
        } else {
            DownloadSpeedSet::new(Vec::new())
        };
    }

    let total = ip_data.len();
    let actual_target = config.test_count.min(total);

    // min_speed == 0.0 means "no speed filter" — skip download entirely
    if config.min_speed == 0.0 {
        return DownloadSpeedSet::new(ip_data);
    }

    // Parse URL components once
    let parsed_url = match reqwest::Url::parse(&config.url) {
        Ok(u) => u,
        Err(_) => return DownloadSpeedSet::new(ip_data),
    };
    let scheme = parsed_url.scheme().to_string();
    let original_host = parsed_url.host_str().unwrap_or("").to_string();
    let path = parsed_url.path().to_string();
    let query = parsed_url.query().map(|q| format!("?{}", q)).unwrap_or_default();
    let path_and_query = format!("{}{}", path, query);

    let bar = ProgressBar::new(total as u64);
    bar.set_style(
        ProgressStyle::default_bar()
            .template("{pos}/{len} [{bar:40}]")
            .unwrap()
            .progress_chars("##-"),
    );

    let min_bytes = config.min_speed * 1024.0 * 1024.0;
    let cfg_timeout = config.timeout;
    let cfg_debug = config.debug;

    // Build concurrent download stream: each IP gets its own Client with resolve()
    let download_stream = stream::iter(ip_data.iter().take(total)).map(|d| {
        let ip = d.ping_data.ip;
        let download_url = format!("{}://{}:{}{}", scheme, original_host, config.port, path_and_query);
        let d = d.clone();
        let host = original_host.clone();
        let port = config.port;
        let timeout = cfg_timeout;
        let debug = cfg_debug;

        async move {
            let client = match ClientBuilder::new()
                .timeout(timeout)
                .danger_accept_invalid_certs(true)
                .no_proxy()
                .resolve(&host, SocketAddr::new(ip, port))
                .redirect(reqwest::redirect::Policy::limited(10))
                .build()
            {
                Ok(c) => c,
                Err(_) => return None,
            };

            let mut response = match client
                .get(&download_url)
                .header("User-Agent", USER_AGENT)
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(_) => return None,
            };

            let status = response.status();
            if status != reqwest::StatusCode::OK {
                if debug {
                    eprintln!(
                        "[调试] IP: {}, 下载测速终止, HTTP 状态码: {}, 下载测速地址: {}",
                        ip,
                        status.as_u16(),
                        download_url
                    );
                }
                return None;
            }

            let colo = get_header_colo(response.headers());
            let speed = measure_download_speed(&mut response, timeout).await;
            let mut result = d;
            result.download_speed = speed;
            if result.ping_data.colo.is_empty() {
                result.ping_data.colo = colo;
            }
            Some(result)
        }
    });

    let mut speed_set = Vec::new();
    let concurrency = DOWNLOAD_CONCURRENCY.min(config.test_count);
    let mut stream = download_stream.buffer_unordered(concurrency);

    while let Some(result) = stream.next().await {
        bar.inc(1);
        if let Some(data) = result {
            let speed = data.download_speed;
            if speed >= min_bytes {
                speed_set.push(data);
                if speed_set.len() >= actual_target {
                    break;
                }
            }
        }
    }

    bar.finish_and_clear();

    if cfg_debug && speed_set.is_empty() {
        eprintln!(
            "[调试] 没有满足 下载速度下限 条件的 IP，忽略条件返回所有测速数据（方便下次测速时调整条件）。"
        );
        speed_set = ip_data.clone();
    }

    DownloadSpeedSet::new(speed_set)
}

/// Measure download speed by reading chunks with EWMA smoothing.
/// Samples the instantaneous rate every `timeout / SPEED_SAMPLES` interval.
/// Returns speed in bytes/second.
async fn measure_download_speed(
    response: &mut reqwest::Response,
    timeout: Duration,
) -> f64 {
    let sample_interval = timeout / SPEED_SAMPLES;
    let time_end = Instant::now() + timeout;
    let mut next_sample = Instant::now() + sample_interval;

    let mut total_bytes: i64 = 0;
    let mut last_bytes: i64 = 0;
    let mut ewma = Ewma::new();

    loop {
        if Instant::now() >= time_end {
            break;
        }

        // Check if it's time to sample
        if Instant::now() >= next_sample {
            let delta = total_bytes - last_bytes;
            // Instantaneous rate: bytes per sample_interval -> bytes/sec
            let rate = delta as f64 / sample_interval.as_secs_f64();
            ewma.add(rate);
            last_bytes = total_bytes;
            next_sample = Instant::now() + sample_interval;
        }

        match response.chunk().await {
            Ok(Some(chunk)) => {
                total_bytes += chunk.len() as i64;
            }
            Ok(None) | Err(_) => break,
        }
    }

    if ewma.initialized {
        ewma.get() // bytes/sec
    } else if total_bytes > 0 {
        // Fallback: average speed over configured timeout
        total_bytes as f64 / timeout.as_secs_f64()
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ewma_first_value() {
        let mut ewma = Ewma::new();
        ewma.add(100.0);
        assert!((ewma.get() - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_ewma_convergence() {
        let mut ewma = Ewma::new();
        // Feed stable value; EWMA should converge to it
        for _ in 0..100 {
            ewma.add(50.0);
        }
        assert!((ewma.get() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_ewma_smoothing() {
        let mut ewma = Ewma::new();
        ewma.add(100.0); // initialize
        ewma.add(0.0);   // drop to 0
        // After one step with alpha=1/3: value = 1/3 * 0 + 2/3 * 100 = 66.67
        assert!((ewma.get() - 66.67).abs() < 0.01);
    }

    #[test]
    fn test_ewma_not_initialized() {
        let ewma = Ewma::new();
        assert!((ewma.get() - 0.0).abs() < 0.001);
    }
}
