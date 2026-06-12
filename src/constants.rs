//! 共享常量 —— 集中定义，避免多文件重复

/// HTTP User-Agent 请求头（用于 httping / download）
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_12_6) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/98.0.4758.80 Safari/537.36";

/// 延迟测速最大并发数
pub const MAX_PING_ROUTINES: usize = 1000;
/// 延迟测速默认并发数
pub const DEFAULT_PING_ROUTINES: usize = 200;
/// 默认测速端口
pub const DEFAULT_PORT: u16 = 443;
/// 默认单 IP 测速次数
pub const DEFAULT_PING_TIMES: u32 = 4;

/// 默认下载测速 URL
pub const DEFAULT_DOWNLOAD_URL: &str = "https://cf.xiu2.xyz/url";
/// 默认下载测速超时（秒）
pub const DEFAULT_DOWNLOAD_TIMEOUT_SECS: u64 = 10;
/// 默认下载测速数量
pub const DEFAULT_DOWNLOAD_TEST_COUNT: usize = 10;

/// TCPing 连接超时（秒）
pub const TCP_CONNECT_TIMEOUT_SECS: u64 = 1;
/// HTTPing 请求超时（秒）
pub const HTTP_TIMEOUT_SECS: u64 = 2;

/// 延迟筛选默认上限（ms）—— 9999 表示无上限
pub const DEFAULT_DELAY_MAX: u64 = 9999;
