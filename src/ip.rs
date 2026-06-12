use ipnetwork::IpNetwork;
use rand::Rng;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::net::IpAddr;
use std::path::Path;

const DEFAULT_INPUT_FILE: &str = "ip.txt";

/// Load IP addresses from file or direct argument.
/// If `ip_text` is non-empty, parses from that string (comma-separated CIDRs).
/// Otherwise reads from `ip_file` (defaults to "ip.txt" if empty).
pub fn load_ip_ranges(ip_file: &str, ip_text: &str, test_all: bool) -> Vec<IpAddr> {
    let mut ips = Vec::new();

    if !ip_text.is_empty() {
        for part in ip_text.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                expand_ip_range(part, test_all, &mut ips);
            }
        }
    } else {
        let file_path = if ip_file.is_empty() {
            DEFAULT_INPUT_FILE
        } else {
            ip_file
        };
        let path = Path::new(file_path);
        let file = File::open(path).unwrap_or_else(|e| {
            eprintln!("打开文件 [{}] 失败: {}", file_path, e);
            std::process::exit(1);
        });
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line.unwrap_or_default();
            let line = line.trim().to_string();
            if !line.is_empty() {
                expand_ip_range(&line, test_all, &mut ips);
            }
        }
    }

    ips
}

/// Parse a CIDR or single IP and expand into concrete IPs
fn expand_ip_range(cidr: &str, test_all: bool, ips: &mut Vec<IpAddr>) {
    let network: IpNetwork = cidr.parse().unwrap_or_else(|e| {
        eprintln!("解析CIDR [{}] 失败: {}", cidr, e);
        std::process::exit(1);
    });

    match network {
        IpNetwork::V4(net) => {
            if net.prefix() == 32 {
                ips.push(IpAddr::V4(net.network()));
            } else if test_all {
                for ip in net.iter() {
                    ips.push(IpAddr::V4(ip));
                }
            } else {
                let base_ip = net.network();
                let prefix = net.prefix();
                if prefix >= 24 {
                    // Single /24 or smaller — randomize last octet
                    let mut rng = rand::thread_rng();
                    let last_byte = rng.gen_range(1..=254u8);
                    let mut octets = base_ip.octets();
                    octets[3] = last_byte;
                    ips.push(IpAddr::V4(octets.into()));
                } else {
                    // Iterate through each /24 subrange and pick one random IP
                    let step = 1 << (24 - prefix);
                    for i in 0..step {
                        let octets = base_ip.octets();
                        let base = (octets[0] as u32) << 24
                            | (octets[1] as u32) << 16
                            | (octets[2] as u32) << 8
                            | (octets[3] as u32);
                        let new_base = base + i * 256;
                        let mut rng = rand::thread_rng();
                        let last_byte = rng.gen_range(1..=254u8);
                        let new_ip = new_base | last_byte as u32;
                        ips.push(IpAddr::V4(std::net::Ipv4Addr::from(new_ip)));
                    }
                }
            }
        }
        IpNetwork::V6(net) => {
            if net.prefix() == 128 {
                ips.push(IpAddr::V6(net.network()));
            } else {
                // For IPv6, generate random IPs within the prefix.
                // Randomize only the host portion bytes (after prefix / 8).
                let host_bytes = ((128 - net.prefix()) / 8) as usize;
                let start_byte = (net.prefix() / 8) as usize;
                // Cap at 256 probes; if the prefix has fewer than 4 host bytes,
                // the available address space is smaller — generate all combos.
                let num_ips = if host_bytes >= 4 {
                    256
                } else {
                    1 << (host_bytes * 8)
                };
                let mut rng = rand::thread_rng();

                for _ in 0..num_ips {
                    let mut octets = net.network().octets();
                    for j in 0..host_bytes.min(16 - start_byte) {
                        octets[start_byte + j] = rng.gen();
                    }
                    ips.push(IpAddr::V6(octets.into()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_ipv4() {
        let ips = load_ip_ranges("", "1.1.1.1/32", false);
        assert_eq!(ips.len(), 1);
        assert_eq!(ips[0].to_string(), "1.1.1.1");
    }

    #[test]
    fn test_single_ipv6_128() {
        let ips = load_ip_ranges("", "::1/128", false);
        assert_eq!(ips.len(), 1);
        assert_eq!(ips[0].to_string(), "::1");
    }

    #[test]
    fn test_ipv4_24_random() {
        let ips = load_ip_ranges("", "1.1.1.0/24", false);
        assert_eq!(ips.len(), 1);
        let ip = ips[0].to_string();
        assert!(ip.starts_with("1.1.1."), "Expected 1.1.1.x, got {}", ip);
    }

    #[test]
    fn test_ipv4_23_random() {
        let ips = load_ip_ranges("", "1.1.0.0/23", false);
        // /23 has 2 /24 sub-ranges → generates 2 IPs
        assert_eq!(ips.len(), 2);
        for ip in &ips {
            let s = ip.to_string();
            assert!(s.starts_with("1.1."), "Expected 1.1.x.x, got {}", s);
        }
    }

    #[test]
    fn test_ipv4_24_all() {
        let ips = load_ip_ranges("", "1.1.1.0/24", true);
        assert_eq!(ips.len(), 256);
    }

    #[test]
    fn test_comma_separated() {
        let ips = load_ip_ranges("", "1.1.1.0/32,2.2.2.0/32", false);
        assert_eq!(ips.len(), 2);
        assert_eq!(ips[0].to_string(), "1.1.1.0");
        assert_eq!(ips[1].to_string(), "2.2.2.0");
    }

    #[test]
    fn test_ipv6_random_generates_correct_prefix() {
        let ips = load_ip_ranges("", "2001:db8::/32", false);
        assert!(!ips.is_empty());
        for ip in &ips {
            let s = ip.to_string();
            assert!(s.starts_with("2001:db8:"), "Expected 2001:db8::/32 prefix, got {}", s);
        }
    }
}
