# CloudflareSpeedTest

Rust 实现的 Cloudflare CDN IP 延迟和速度测试工具。基于 [XIU2/CloudflareSpeedTest](https://github.com/XIU2/CloudflareSpeedTest) (Go 版) 的功能设计和优化，使用异步并发架构，性能显著提升。

## 功能特性

- **双模式延迟测速** — TCPing（默认）和 HTTPing 两种测速模式
- **并发下载测速** — 异步并发下载，EWMA 平滑测速，自动筛选达标 IP
- **CIDR 自动扩展** — 支持 IPv4/IPv6 CIDR 网段，自动采样或全量扩展
- **多 CDN 地区码识别** — 从响应头识别 Cloudflare / CDN77 / BunnyCDN / AWS CloudFront / Fastly / Gcore 等 CDN 的地区码
- **智能过滤排序** — 按丢包率、延迟、下载速度多级排序和过滤
- **CSV 结果导出** — 自动保存测速结果到 CSV 文件
- **Hosts 自动替换** — 附带批处理脚本，自动将系统 Hosts 中 CDN IP 替换为最快 IP

## 性能对比 (vs Go 版)

| 场景 | Go v2.3.5 | Rust v2.4.0_rust | 提升 |
|------|-----------|-------------|------|
| TCPing 25 CIDR (2601 IPs) | >600s | ~89s | **~7×** |
| TCPing 500 IPs | ~61s | ~19s | **~3.2×** |
| HTTPing 500 IPs | ~122s | ~18s | **~6.8×** |

核心优化：`buffer_unordered` 异步并发替代 `tokio::spawn + Arc<Mutex>`、零共享内存争用、per-IP `reqwest::Client` 配合 `resolve()` 避免重复 DNS 解析。

## 快速开始

```bash
# 编译
cargo build --release

# 使用默认配置测速（从 ip.txt 读取 IP 段）
./target/release/cfst.exe

# 使用自定义 IP 段文件
./target/release/cfst.exe -f my_ip.txt

# 使用 HTTPing 模式
./target/release/cfst.exe --httping
```

## 命令行参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `-n` | 延迟测速并发数 | 200 |
| `-t` | 每 IP 测速次数 | 4 |
| `--dn` | 下载测速数量 | 10 |
| `--dt` | 下载测速超时(秒) | 10 |
| `--tp` | 测速端口 | 443 |
| `--url` | 下载测速地址 | `https://cf.xiu2.xyz/url` |
| `--httping` | HTTPing 模式 | false |
| `--httping-code` | 有效 HTTP 状态码 | 0 (接受 100~599) |
| `--cfcolo` | 地区码过滤 (逗号分隔) | 全部 |
| `--tl` | 延迟上限 (ms) | 9999 |
| `--tll` | 延迟下限 (ms) | 0 |
| `--tlr` | 丢包率上限 | 1.0 |
| `--sl` | 下载速度下限 (MB/s) | 0.0 |
| `-p` | 显示结果数量 | 10 |
| `-f` | IP 段数据文件 | ip.txt |
| `--ip` | 直接指定 IP 段 | "" |
| `-o` | 输出文件 | result.csv |
| `--dd` | 禁用下载测速 | false |
| `--allip` | 测速全部 IP | false |
| `--debug` | 调试模式 | false |

## 使用示例

```bash
# 指定并发数和延迟上限
cfst.exe -n 500 -t 6 --tl 500

# HTTPing + 地区码筛选 + 禁用下载
cfst.exe --httping --cfcolo "SJC,LAX" --dd

# 全量 IP 测试 + 下载速度下限
cfst.exe --allip --sl 1.0

# 指定端口和 URL
cfst.exe --tp 8080 --url "https://example.com/test.bin"
```

## 自动更新 Hosts

将 `target/release/cfst_hosts.bat` 复制到工作目录并运行：

1. 首次运行需输入当前 Hosts 中使用的 Cloudflare CDN IP
2. 脚本自动测速获取最快 IP
3. 自动备份并替换系统 Hosts 文件中的 CDN IP

*需要管理员权限运行。*

## 项目结构

```
src/
├── main.rs       # 主程序 + CLI 参数解析
├── constants.rs  # 共享常量配置
├── ip.rs         # IP 加载与 CIDR 扩展
├── tcping.rs     # TCPing 延迟测速
├── httping.rs    # HTTPing 延迟测速 + 地区码识别
├── download.rs   # 下载测速 (EWMA)
└── data.rs       # 数据结构 + 排序过滤 + CSV 导出
```

## 数据文件格式

`ip.txt` — 每行一个 CIDR 网段或单个 IP：

```
173.245.48.0/20
103.21.244.0/22
104.16.0.0/12
162.158.0.0/15
```

## 构建要求

- Rust 1.70+ (edition 2021)
- 支持 Windows / Linux / macOS

## 协议

GPL-3.0 License

---

本项目基于 [XIU2/CloudflareSpeedTest](https://github.com/XIU2/CloudflareSpeedTest) (Go) 的设计理念，使用 Rust 重写并优化了并发性能和代码质量。
