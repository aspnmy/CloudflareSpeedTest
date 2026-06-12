use crate::constants::{
    DEFAULT_DOWNLOAD_URL, DEFAULT_PING_ROUTINES, DEFAULT_PING_TIMES, DEFAULT_PORT,
    HTTP_TIMEOUT_SECS, MAX_PING_ROUTINES, USER_AGENT,
};
use crate::data::{CloudflareIpData, PingData};
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::Client;
use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

const HTTP_TIMEOUT: Duration = Duration::from_secs(HTTP_TIMEOUT_SECS);

static RE_COLO_IATA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Z]{3}").unwrap());
static RE_COLO_COUNTRY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Z]{2}").unwrap());
static RE_COLO_GCORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z]{2}").unwrap());

/// HTTP ping configuration
#[derive(Debug, Clone)]
pub struct HttpingConfig {
    pub routines: usize,
    pub port: u16,
    pub ping_times: u32,
    pub url: String,
    pub httping_status_code: i32,
    pub httping_cf_colo: String,
    pub debug: bool,
}

impl Default for HttpingConfig {
    fn default() -> Self {
        Self {
            routines: DEFAULT_PING_ROUTINES,
            port: DEFAULT_PORT,
            ping_times: DEFAULT_PING_TIMES,
            url: DEFAULT_DOWNLOAD_URL.to_string(),
            httping_status_code: 0,
            httping_cf_colo: String::new(),
            debug: false,
        }
    }
}

impl HttpingConfig {
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
        if self.url.is_empty() {
            self.url = DEFAULT_DOWNLOAD_URL.to_string();
        }
    }
}

/// Extract colo (region code) from response headers
pub fn get_header_colo(headers: &reqwest::header::HeaderMap) -> String {
    // Cloudflare: cf-ray header -> e.g. "7bd32409eda7b020-SJC"
    if let Some(cf_ray) = headers.get("cf-ray").and_then(|v| v.to_str().ok()) {
        if let Some(m) = RE_COLO_IATA.find(cf_ray) {
            return m.as_str().to_string();
        }
    }
    // CDN77: x-77-pop -> e.g. "frankfurtDE"
    if let Some(x77) = headers.get("x-77-pop").and_then(|v| v.to_str().ok()) {
        if let Some(m) = RE_COLO_COUNTRY.find(x77) {
            return m.as_str().to_string();
        }
    }
    // Bunny CDN: server header -> e.g. "BunnyCDN-TW1-1121"
    if let Some(server) = headers.get("server").and_then(|v| v.to_str().ok()) {
        if let Some(stripped) = server.strip_prefix("BunnyCDN-") {
            if let Some(m) = RE_COLO_COUNTRY.find(stripped) {
                return m.as_str().to_string();
            }
        }
    }
    // AWS CloudFront: x-amz-cf-pop -> e.g. "SIN52-P1"
    if let Some(cf_pop) = headers.get("x-amz-cf-pop").and_then(|v| v.to_str().ok()) {
        if let Some(m) = RE_COLO_IATA.find(cf_pop) {
            return m.as_str().to_string();
        }
    }
    // Fastly: x-served-by -> e.g. "cache-fra-etou8220141-FRA"
    if let Some(x_served) = headers.get("x-served-by").and_then(|v| v.to_str().ok()) {
        let matches: Vec<&str> = RE_COLO_IATA.find_iter(x_served).map(|m| m.as_str()).collect();
        if let Some(last) = matches.last() {
            return last.to_string();
        }
    }
    // Gcore: x-id-fe -> e.g. "fr5-hw-edge-gc17"
    if let Some(x_id_fe) = headers.get("x-id-fe").and_then(|v| v.to_str().ok()) {
        if let Some(m) = RE_COLO_GCORE.find(x_id_fe) {
            return m.as_str().to_uppercase();
        }
    }
    String::new()
}

/// Build colo filter set
fn build_colo_filter(colos: &str) -> Option<Arc<HashSet<String>>> {
    if colos.is_empty() {
        return None;
    }
    let set: HashSet<String> = colos
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .collect();
    Some(Arc::new(set))
}

/// Run HTTP ping test on all IPs concurrently.
/// Uses `buffer_unordered` for bounded concurrency (no Mutex/spawn overhead).
pub async fn run_httping(ips: Vec<IpAddr>, config: HttpingConfig) -> Vec<CloudflareIpData> {
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

    // Parse URL once and extract immutable parts (shared via Arc to avoid per-task clone)
    let parsed_url = match reqwest::Url::parse(&config.url) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("解析测速地址失败 [{}]: {}", config.url, e);
            bar.finish_and_clear();
            return Vec::new();
        }
    };
    let scheme: Arc<str> = Arc::from(parsed_url.scheme());
    let path: Arc<str> = Arc::from(parsed_url.path());
    let original_host: Arc<str> = Arc::from(parsed_url.host_str().unwrap_or_default());

    let colo_filter = build_colo_filter(&config.httping_cf_colo);
    let routines = config.routines;

    // Build concurrent stream
    let stream = stream::iter(ips).map(move |ip| {
        let scheme = Arc::clone(&scheme);
        let path = Arc::clone(&path);
        let original_host = Arc::clone(&original_host);
        let colo_filter = colo_filter.clone();
        let cfg = config.clone();

        async move {
            httping_ip(ip, &scheme, &path, &original_host, &cfg, &colo_filter).await
        }
    });

    let mut results = Vec::with_capacity(total);
    let mut stream = stream.buffer_unordered(routines);

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

/// HTTPing a single IP — creates a per-IP reqwest::Client with resolve() for correct TLS SNI
async fn httping_ip(
    ip: IpAddr,
    scheme: &str,
    path: &str,
    original_host: &str,
    config: &HttpingConfig,
    colo_filter: &Option<Arc<HashSet<String>>>,
) -> Option<CloudflareIpData> {
    // Build URL with the original domain name (not IP) for correct TLS SNI
    let url = format!("{}://{}:{}{}", scheme, original_host, config.port, path);

    // Per-IP Client with resolve() to bypass DNS and route directly to the target IP
    let addr = SocketAddr::new(ip, config.port);
    let client = Client::builder()
        .timeout(HTTP_TIMEOUT)
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .resolve(original_host, addr)
        .build()
        .ok()?;

    // First HEAD request to check status code and get colo
    let req = client
        .head(&url)
        .header("User-Agent", USER_AGENT)
        .build()
        .ok()?;

    let resp = client.execute(req).await;
    let colo = match resp {
        Ok(response) => {
            let sc = response.status().as_u16() as i32;
            // Cloudflare 测速：任何服务器主动返回的状态码（包括 403、404 等）
            // 都表示该 IP 可以访问 Cloudflare 服务器，应视为有效连接
            let is_valid = if config.httping_status_code == 0
                || !(100..=599).contains(&config.httping_status_code)
            {
                (100..600).contains(&sc)
            } else {
                sc == config.httping_status_code
            };
            if !is_valid {
                if config.debug {
                    eprintln!(
                        "[调试] IP: {}, 延迟测速终止, HTTP 状态码: {}, 测速地址: {}",
                        ip, sc, url
                    );
                }
                return None;
            }
            let colo = get_header_colo(response.headers());
            drop(response);
            colo
        }
        Err(e) => {
            if config.debug {
                eprintln!("[调试] IP: {}, 延迟测速失败, 错误: {}, 测速地址: {}", ip, e, url);
            }
            return None;
        }
    };

    // Filter by colo
    if let Some(filter) = colo_filter {
        if !colo.is_empty() && !filter.contains(&colo.to_uppercase()) {
            if config.debug {
                eprintln!("[调试] IP: {}, 地区码不匹配: {}", ip, colo);
            }
            return None;
        }
    }

    // Multiple HEAD requests for latency measurement
    let mut received = 0u32;
    let mut total_delay = Duration::ZERO;
    for _ in 0..config.ping_times {
        let start = Instant::now();
        let req = client
            .head(&url)
            .header("User-Agent", USER_AGENT)
            .build()
            .ok();
        if let Some(r) = req {
            if let Ok(resp) = client.execute(r).await {
                received += 1;
                total_delay += start.elapsed();
                drop(resp);
            }
        }
    }

    if received == 0 {
        return None;
    }

    Some(CloudflareIpData {
        ping_data: PingData {
            ip,
            sent: config.ping_times,
            received,
            delay: total_delay / received,
            colo,
        },
        download_speed: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn test_colo_from_cf_ray() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-ray", HeaderValue::from_static("7bd32409eda7b020-SJC"));
        assert_eq!(get_header_colo(&headers), "SJC");
    }

    #[test]
    fn test_colo_from_x77_pop() {
        let mut headers = HeaderMap::new();
        headers.insert("x-77-pop", HeaderValue::from_static("frankfurtDE"));
        assert_eq!(get_header_colo(&headers), "DE");
    }

    #[test]
    fn test_colo_from_bunny_server() {
        let mut headers = HeaderMap::new();
        headers.insert("server", HeaderValue::from_static("BunnyCDN-TW1-1121"));
        assert_eq!(get_header_colo(&headers), "TW");
    }

    #[test]
    fn test_colo_from_amz_cf_pop() {
        let mut headers = HeaderMap::new();
        headers.insert("x-amz-cf-pop", HeaderValue::from_static("SIN52-P1"));
        assert_eq!(get_header_colo(&headers), "SIN");
    }

    #[test]
    fn test_colo_from_fastly() {
        let mut headers = HeaderMap::new();
        headers.insert("x-served-by", HeaderValue::from_static("cache-fra-etou8220141-FRA"));
        assert_eq!(get_header_colo(&headers), "FRA");
    }

    #[test]
    fn test_colo_from_gcore() {
        let mut headers = HeaderMap::new();
        headers.insert("x-id-fe", HeaderValue::from_static("fr5-hw-edge-gc17"));
        assert_eq!(get_header_colo(&headers), "FR");
    }

    #[test]
    fn test_colo_no_match() {
        let headers = HeaderMap::new();
        assert_eq!(get_header_colo(&headers), "");
    }

    #[test]
    fn test_build_colo_filter_empty() {
        assert!(build_colo_filter("").is_none());
    }

    #[test]
    fn test_build_colo_filter_multiple() {
        let filter = build_colo_filter("SJC,LAX,FRA").unwrap();
        assert!(filter.contains("SJC"));
        assert!(filter.contains("LAX"));
        assert!(filter.contains("FRA"));
        assert!(!filter.contains("NRT"));
    }
}
