mod constants;
mod data;
mod download;
mod httping;
mod ip;
mod tcping;

use crate::constants::{
    DEFAULT_DELAY_MAX, DEFAULT_DOWNLOAD_TEST_COUNT, DEFAULT_DOWNLOAD_TIMEOUT_SECS,
    DEFAULT_DOWNLOAD_URL, DEFAULT_PING_ROUTINES, DEFAULT_PING_TIMES, DEFAULT_PORT,
    MAX_PING_ROUTINES,
};
use clap::Parser;
use colored::Colorize;
use std::time::Duration;
use std::error::Error;

#[derive(Parser, Debug)]
#[command(name = "cfst", version, about = "CloudflareSpeedTest - 测试 Cloudflare CDN IP 延迟和速度")]
struct Args {
    /// 延迟测速线程数 (默认 200, 最多 1000)
    #[arg(short = 'n', default_value_t = DEFAULT_PING_ROUTINES)]
    routines: usize,

    /// 延迟测速次数 (默认 4)
    #[arg(short = 't', default_value_t = DEFAULT_PING_TIMES)]
    ping_times: u32,

    /// 下载测速数量 (默认 10)
    #[arg(long = "dn", default_value_t = DEFAULT_DOWNLOAD_TEST_COUNT)]
    download_num: usize,

    /// 下载测速时间 (秒, 默认 10)
    #[arg(long = "dt", default_value_t = DEFAULT_DOWNLOAD_TIMEOUT_SECS)]
    download_time: u64,

    /// 指定测速端口 (默认 443)
    #[arg(long = "tp", default_value_t = DEFAULT_PORT)]
    port: u16,

    /// 指定测速地址
    #[arg(long = "url", default_value_t = String::from(DEFAULT_DOWNLOAD_URL))]
    url: String,

    /// 切换为 HTTPing 测速模式
    #[arg(long = "httping", default_value_t = false)]
    httping: bool,

    /// 有效 HTTP 状态码 (仅 HTTPing 模式)
    #[arg(long = "httping-code", default_value = "0")]
    httping_code: i32,

    /// 匹配指定地区码 (仅 HTTPing 模式)
    #[arg(long = "cfcolo", default_value = "")]
    cfcolo: String,

    /// 平均延迟上限 (ms)
    #[arg(long = "tl", default_value_t = DEFAULT_DELAY_MAX)]
    max_delay: u64,

    /// 平均延迟下限 (ms)
    #[arg(long = "tll", default_value = "0")]
    min_delay: u64,

    /// 丢包几率上限 (0.00~1.00)
    #[arg(long = "tlr", default_value = "1.0")]
    max_loss_rate: f32,

    /// 下载速度下限 (MB/s)
    #[arg(long = "sl", default_value = "0.0")]
    min_speed: f64,

    /// 显示结果数量 (0 不显示)
    #[arg(short = 'p', default_value = "10")]
    print_num: usize,

    /// IP 段数据文件
    #[arg(short = 'f', default_value = "ip.txt")]
    ip_file: String,

    /// 直接指定 IP 段数据
    #[arg(long = "ip", default_value = "")]
    ip_text: String,

    /// 输出结果文件
    #[arg(short = 'o', default_value = "result.csv")]
    output: String,

    /// 禁用下载测速
    #[arg(long = "dd", default_value_t = false)]
    disable_download: bool,

    /// 测速全部 IP
    #[arg(long = "allip", default_value_t = false)]
    all_ip: bool,

    /// 调试输出模式
    #[arg(long = "debug", default_value_t = false)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Display banner
    println!("# CloudflareSpeedTest v{} \n", env!("CARGO_PKG_VERSION"));

    // Load IP ranges
    let ips = ip::load_ip_ranges(&args.ip_file, &args.ip_text, args.all_ip);
    let total_ips = ips.len();
    if total_ips == 0 {
        eprintln!("未加载到任何 IP 地址，请检查 IP 数据文件或参数。");
        std::process::exit(1);
    }

    // ----- Latency test phase -----
    let ping_data = if args.httping {
        let mut cfg = httping::HttpingConfig {
            routines: args.routines.min(MAX_PING_ROUTINES),
            port: args.port,
            ping_times: args.ping_times,
            url: args.url.clone(),
            httping_status_code: args.httping_code,
            httping_cf_colo: args.cfcolo.clone(),
            debug: args.debug,
        };
        cfg.normalize();
        println!(
            "{}",
            format!(
                "开始延迟测速（模式: HTTP, 端口: {}, 范围: {}~{} ms, 丢包: {:.2})",
                cfg.port, args.min_delay, args.max_delay, args.max_loss_rate
            )
            .cyan()
            .bold()
        );
        httping::run_httping(ips, cfg).await
    } else {
        let mut cfg = tcping::TcpingConfig {
            routines: args.routines.min(MAX_PING_ROUTINES),
            port: args.port,
            ping_times: args.ping_times,
        };
        cfg.normalize();
        println!(
            "{}",
            format!(
                "开始延迟测速（模式: TCP, 端口: {}, 范围: {}~{} ms, 丢包: {:.2})",
                cfg.port, args.min_delay, args.max_delay, args.max_loss_rate
            )
            .cyan()
            .bold()
        );
        tcping::run_tcping(ips, cfg).await
    };

    // Apply delay/loss filters (IpDataSet::new() sorts once; filters only retain)
    let ping_set = data::IpDataSet::new(ping_data);
    let ping_set = ping_set.filter_max_loss_rate(args.max_loss_rate);
    let ping_set = if args.max_delay < DEFAULT_DELAY_MAX || args.min_delay > 0 {
        let max_dur = Duration::from_millis(args.max_delay);
        let min_dur = Duration::from_millis(args.min_delay);
        ping_set.filter_max_delay(max_dur).filter_min_delay(min_dur)
    } else {
        ping_set
    };

    let filtered_data = ping_set.into_inner();
    if filtered_data.is_empty() {
        println!("{}", "[信息] 延迟测速结果 IP 数量为 0，跳过下载测速。".yellow());
        let speed_set = data::DownloadSpeedSet::new(Vec::new());
        speed_set.print(args.print_num, &args.output);
        data::export_csv(&speed_set.into_inner(), &args.output);
        return Ok(());
    }

    // ----- Download speed test phase -----
    let mut dl_cfg = download::DownloadConfig {
        url: args.url.clone(),
        timeout: Duration::from_secs(args.download_time),
        test_count: args.download_num,
        min_speed: args.min_speed,
        port: args.port,
        disable: args.disable_download,
        debug: args.debug,
    };
    dl_cfg.normalize();

    if !dl_cfg.disable {
        println!(
            "{}",
            format!(
                "开始下载测速（下限: {:.2} MB/s, 数量: {}, 队列: {}）",
                dl_cfg.min_speed,
                dl_cfg.test_count,
                filtered_data.len()
            )
            .cyan()
            .bold()
        );
    }

    let speed_set = download::run_download(filtered_data, dl_cfg).await;

    // ----- Output phase -----
    speed_set.print(args.print_num, &args.output);
    data::export_csv(&speed_set.into_inner(), &args.output);
    Ok(())
}
