use colored::Colorize;
use std::cmp::Ordering;
use std::net::IpAddr;
use std::time::Duration;

/// Result of a single ping (latency test) for an IP
#[derive(Debug, Clone)]
pub struct PingData {
    pub ip: IpAddr,
    pub sent: u32,
    pub received: u32,
    pub delay: Duration, // average delay
    pub colo: String,    // region code, empty if unknown
}

/// Full data for a Cloudflare IP including download speed
#[derive(Debug, Clone)]
pub struct CloudflareIpData {
    pub ping_data: PingData,
    pub download_speed: f64, // bytes per second
}

impl CloudflareIpData {
    pub fn loss_rate(&self) -> f32 {
        let lost = self.ping_data.sent - self.ping_data.received;
        if self.ping_data.sent == 0 {
            return 1.0;
        }
        lost as f32 / self.ping_data.sent as f32
    }
}

/// IP data set — sorted by loss rate then delay ascending.
/// Created via `new()` which sorts once; all `filter_*` methods only retain,
/// never re-sort, preserving the original ordering.
#[derive(Debug)]
pub struct IpDataSet(pub Vec<CloudflareIpData>);

impl IpDataSet {
    /// Create a new set, sorting by loss rate then delay ascending.
    pub fn new(mut data: Vec<CloudflareIpData>) -> Self {
        data.sort_by(|a, b| {
            a.loss_rate()
                .partial_cmp(&b.loss_rate())
                .unwrap_or(Ordering::Equal)
                .then(a.ping_data.delay.cmp(&b.ping_data.delay))
        });
        Self(data)
    }

    /// Filter by max delay — retains items with delay <= max, keeps sort order.
    pub fn filter_max_delay(mut self, max: Duration) -> Self {
        self.0.retain(|x| x.ping_data.delay <= max);
        self
    }

    /// Filter by min delay — retains items with delay >= min.
    pub fn filter_min_delay(mut self, min: Duration) -> Self {
        self.0.retain(|x| x.ping_data.delay >= min);
        self
    }

    /// Filter by max loss rate (0.0 ~ 1.0).
    pub fn filter_max_loss_rate(mut self, max_rate: f32) -> Self {
        self.0.retain(|x| x.loss_rate() <= max_rate);
        self
    }

    pub fn into_inner(self) -> Vec<CloudflareIpData> {
        self.0
    }
}

/// Download speed set — sorted by speed descending
#[derive(Debug)]
pub struct DownloadSpeedSet(pub Vec<CloudflareIpData>);

impl DownloadSpeedSet {
    pub fn new(mut data: Vec<CloudflareIpData>) -> Self {
        data.sort_by(|a, b| {
            b.download_speed
                .partial_cmp(&a.download_speed)
                .unwrap_or(Ordering::Equal)
        });
        Self(data)
    }

    pub fn into_inner(self) -> Vec<CloudflareIpData> {
        self.0
    }

    pub fn print(&self, print_num: usize, output: &str) {
        if print_num == 0 {
            return;
        }
        if self.0.is_empty() {
            println!("\n[信息] 完整测速结果 IP 数量为 0，跳过输出结果。");
            return;
        }

        // Detect if any IPv6 addresses are present to adjust column width
        let has_ipv6 = self.0.iter().any(|d| d.ping_data.ip.is_ipv6());
        let ip_width: usize = if has_ipv6 { 42 } else { 18 };

        let head = format!(
            "{:<ip_width$}{:<8}{:<8}{:<8}{:<10}{:<16}{:<8}",
            "IP 地址",
            "已发送",
            "已接收",
            "丢包率",
            "平均延迟",
            "下载速度(MB/s)",
            "地区码",
            ip_width = ip_width,
        );
        println!("{}", head.bold().cyan());

        for d in self.0.iter().take(print_num) {
            println!(
                "{:<ip_width$}{:<8}{:<8}{:<8}{:<10}{:<16}{:<8}",
                d.ping_data.ip,
                d.ping_data.sent.to_string(),
                d.ping_data.received.to_string(),
                format!("{:.2}", d.loss_rate()),
                format!("{:.2}", d.ping_data.delay.as_secs_f64() * 1000.0),
                format!("{:.2}", d.download_speed / 1024.0 / 1024.0),
                if d.ping_data.colo.is_empty() {
                    "N/A"
                } else {
                    &d.ping_data.colo
                },
                ip_width = ip_width,
            );
        }

        if !output.is_empty() && output != " " {
            println!(
                "\n完整测速结果已写入 {} 文件，可使用记事本/表格软件查看。",
                output
            );
        }
    }
}

/// Export data to CSV file
pub fn export_csv(data: &[CloudflareIpData], output: &str) {
    if output.is_empty() || output == " " || data.is_empty() {
        return;
    }
    let mut wtr = match csv::Writer::from_path(output) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("创建文件 [{}] 失败: {}", output, e);
            return;
        }
    };
    if let Err(e) = wtr.write_record([
        "IP 地址",
        "已发送",
        "已接收",
        "丢包率",
        "平均延迟",
        "下载速度(MB/s)",
        "地区码",
    ]) {
        eprintln!("写入 CSV 表头失败: {}", e);
        return;
    }
    for d in data {
        let colo = if d.ping_data.colo.is_empty() {
            "N/A".to_string()
        } else {
            d.ping_data.colo.clone()
        };
        if let Err(e) = wtr.write_record(&[
            d.ping_data.ip.to_string(),
            d.ping_data.sent.to_string(),
            d.ping_data.received.to_string(),
            format!("{:.2}", d.loss_rate()),
            format!("{:.2}", d.ping_data.delay.as_secs_f64() * 1000.0),
            format!("{:.2}", d.download_speed / 1024.0 / 1024.0),
            colo,
        ]) {
            eprintln!("写入 CSV 记录失败: {}", e);
            return;
        }
    }
    if let Err(e) = wtr.flush() {
        eprintln!("刷新 CSV 缓冲区失败: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    fn make_data(ip: &str, loss_rate: f32, delay_ms: u64, speed: f64) -> CloudflareIpData {
        // received = sent * (1 - loss_rate), but at least 0
        let sent = 4u32;
        let received = (sent as f32 * (1.0 - loss_rate)).round() as u32;
        CloudflareIpData {
            ping_data: PingData {
                ip: IpAddr::V4(Ipv4Addr::from_str(ip).unwrap()),
                sent,
                received,
                delay: Duration::from_millis(delay_ms),
                colo: String::new(),
            },
            download_speed: speed,
        }
    }

    #[test]
    fn test_ip_data_set_sort_by_loss_then_delay() {
        let data = vec![
            make_data("1.1.1.1", 0.5, 100, 0.0),
            make_data("1.1.1.2", 0.0, 50, 0.0),
            make_data("1.1.1.3", 0.0, 30, 0.0),
            make_data("1.1.1.4", 0.25, 80, 0.0),
        ];
        let inner = IpDataSet::new(data).into_inner();
        assert_eq!(inner[0].ping_data.ip.to_string(), "1.1.1.3");
        assert_eq!(inner[1].ping_data.ip.to_string(), "1.1.1.2");
        assert_eq!(inner[2].ping_data.ip.to_string(), "1.1.1.4");
        assert_eq!(inner[3].ping_data.ip.to_string(), "1.1.1.1");
    }

    #[test]
    fn test_filter_max_delay_retains_order() {
        let data = vec![
            make_data("1.1.1.1", 0.0, 100, 0.0),
            make_data("1.1.1.2", 0.0, 200, 0.0),
            make_data("1.1.1.3", 0.0, 50, 0.0),
        ];
        let set = IpDataSet::new(data).filter_max_delay(Duration::from_millis(150));
        assert_eq!(set.into_inner().len(), 2);
    }

    #[test]
    fn test_filter_min_delay() {
        let data = vec![
            make_data("1.1.1.1", 0.0, 100, 0.0),
            make_data("1.1.1.2", 0.0, 50, 0.0),
        ];
        let inner = IpDataSet::new(data)
            .filter_min_delay(Duration::from_millis(75))
            .into_inner();
        assert_eq!(inner.len(), 1);
        assert_eq!(inner[0].ping_data.ip.to_string(), "1.1.1.1");
    }

    #[test]
    fn test_filter_max_loss_rate() {
        let data = vec![
            make_data("1.1.1.1", 0.5, 100, 0.0),
            make_data("1.1.1.2", 0.0, 50, 0.0),
            make_data("1.1.1.3", 0.25, 80, 0.0),
        ];
        let inner = IpDataSet::new(data).filter_max_loss_rate(0.3).into_inner();
        assert_eq!(inner.len(), 2);
    }

    #[test]
    fn test_download_speed_set_sorted_descending() {
        let data = vec![
            make_data("1.1.1.1", 0.0, 10, 5_000_000.0),
            make_data("1.1.1.2", 0.0, 10, 10_000_000.0),
            make_data("1.1.1.3", 0.0, 10, 1_000_000.0),
        ];
        let inner = DownloadSpeedSet::new(data).into_inner();
        assert_eq!(inner[0].ping_data.ip.to_string(), "1.1.1.2");
        assert_eq!(inner[1].ping_data.ip.to_string(), "1.1.1.1");
        assert_eq!(inner[2].ping_data.ip.to_string(), "1.1.1.3");
    }

    #[test]
    fn test_loss_rate_calculation() {
        let d = CloudflareIpData {
            ping_data: PingData {
                ip: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
                sent: 4,
                received: 3,
                delay: Duration::from_millis(100),
                colo: String::new(),
            },
            download_speed: 0.0,
        };
        assert!((d.loss_rate() - 0.25).abs() < 0.001);
    }

    #[test]
    fn test_loss_rate_zero_sent() {
        let d = CloudflareIpData {
            ping_data: PingData {
                ip: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
                sent: 0,
                received: 0,
                delay: Duration::ZERO,
                colo: String::new(),
            },
            download_speed: 0.0,
        };
        assert!((d.loss_rate() - 1.0).abs() < 0.001);
    }
}
