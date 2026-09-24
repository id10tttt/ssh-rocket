use std::{fs, time::Instant};

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemMetrics {
    pub cpu_percent: f32,
    pub ram_percent: f32,
    pub gpu_percent: f32,
    pub sys_upload_speed: u64,   // bytes per second
    pub sys_download_speed: u64, // bytes per second
}

pub struct SystemMonitor {
    last_sample_time: Instant,
    last_cpu_idle: u64,
    last_cpu_total: u64,
    last_net_tx: u64,
    last_net_rx: u64,
}

impl Default for SystemMonitor {
    fn default() -> Self {
        let (cpu_idle, cpu_total) = Self::read_cpu_raw().unwrap_or((0, 0));
        let (net_tx, net_rx) = Self::read_net_raw();
        Self {
            last_sample_time: Instant::now(),
            last_cpu_idle: cpu_idle,
            last_cpu_total: cpu_total,
            last_net_tx: net_tx,
            last_net_rx: net_rx,
        }
    }
}

impl SystemMonitor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 采样并返回当前系统硬件指标与整机物理网速
    pub fn sample(&mut self) -> SystemMetrics {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_sample_time).as_secs_f64().max(0.1);
        self.last_sample_time = now;

        // 1. CPU
        let mut cpu_percent = 0.0f32;
        if let Some((cur_idle, cur_total)) = Self::read_cpu_raw() {
            let delta_idle = cur_idle.saturating_sub(self.last_cpu_idle);
            let delta_total = cur_total.saturating_sub(self.last_cpu_total);
            if delta_total > 0 {
                let usage = 1.0 - (delta_idle as f64 / delta_total as f64);
                cpu_percent = (usage * 100.0).clamp(0.0, 100.0) as f32;
            }
            self.last_cpu_idle = cur_idle;
            self.last_cpu_total = cur_total;
        }

        // 2. RAM
        let ram_percent = Self::read_ram_percent().unwrap_or(0.0);

        // 3. GPU
        let gpu_percent = Self::read_gpu_percent().unwrap_or(0.0);

        // 4. Network (物理网卡)
        let (cur_tx, cur_rx) = Self::read_net_raw();
        let delta_tx = cur_tx.saturating_sub(self.last_net_tx);
        let delta_rx = cur_rx.saturating_sub(self.last_net_rx);
        self.last_net_tx = cur_tx;
        self.last_net_rx = cur_rx;

        let sys_upload_speed = ((delta_tx as f64) / elapsed).round() as u64;
        let sys_download_speed = ((delta_rx as f64) / elapsed).round() as u64;

        SystemMetrics {
            cpu_percent,
            ram_percent,
            gpu_percent,
            sys_upload_speed,
            sys_download_speed,
        }
    }

    /// 从 /proc/stat 读取 (idle_ticks, total_ticks)
    fn read_cpu_raw() -> Option<(u64, u64)> {
        let content = fs::read_to_string("/proc/stat").ok()?;
        let first_line = content.lines().next()?;
        if !first_line.starts_with("cpu ") {
            return None;
        }
        let parts: Vec<u64> = first_line
            .split_whitespace()
            .skip(1)
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        if parts.len() < 4 {
            return None;
        }
        // user, nice, system, idle, iowait, irq, softirq, steal
        let idle = parts[3] + parts.get(4).copied().unwrap_or(0);
        let total: u64 = parts.iter().sum();
        Some((idle, total))
    }

    /// 从 /proc/meminfo 读取内存占用百分比
    fn read_ram_percent() -> Option<f32> {
        let content = fs::read_to_string("/proc/meminfo").ok()?;
        let mut total_kb: Option<u64> = None;
        let mut avail_kb: Option<u64> = None;

        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
            } else if line.starts_with("MemAvailable:") {
                avail_kb = line.split_whitespace().nth(1).and_then(|v| v.parse().ok());
            }
            if total_kb.is_some() && avail_kb.is_some() {
                break;
            }
        }

        let total = total_kb?;
        let avail = avail_kb?;
        if total == 0 {
            return None;
        }
        let used = total.saturating_sub(avail);
        Some(((used as f64 / total as f64) * 100.0).clamp(0.0, 100.0) as f32)
    }

    /// 读取 GPU 繁忙百分比 (优先从 sysfs DRM 接口读取，如 AMD/Intel)
    fn read_gpu_percent() -> Option<f32> {
        // 1. AMD DRM: /sys/class/drm/card*/device/gpu_busy_percent
        if let Ok(entries) = fs::read_dir("/sys/class/drm") {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str.starts_with("card") && !name_str.contains('-') {
                    let busy_path = entry.path().join("device").join("gpu_busy_percent");
                    if let Ok(val_str) = fs::read_to_string(&busy_path) {
                        if let Ok(val) = val_str.trim().parse::<f32>() {
                            return Some(val.clamp(0.0, 100.0));
                        }
                    }
                }
            }
        }

        None
    }

    /// 从 /proc/net/dev 读取所有物理网卡的 (tx_bytes, rx_bytes)
    fn read_net_raw() -> (u64, u64) {
        let Ok(content) = fs::read_to_string("/proc/net/dev") else {
            return (0, 0);
        };

        let mut total_tx = 0u64;
        let mut total_rx = 0u64;

        for line in content.lines().skip(2) {
            let Some((iface, data)) = line.split_once(':') else {
                continue;
            };
            let iface = iface.trim();
            // 过滤虚拟、容器、回环接口
            if iface == "lo"
                || iface.starts_with("docker")
                || iface.starts_with("veth")
                || iface.starts_with("br-")
                || iface.starts_with("tun")
                || iface.starts_with("tap")
                || iface.starts_with("sshrocket")
                || iface.starts_with("tailscale")
                || iface.starts_with("wg")
            {
                continue;
            }

            let fields: Vec<&str> = data.split_whitespace().collect();
            // fields[0] = rx_bytes, fields[8] = tx_bytes
            if fields.len() >= 9 {
                if let (Ok(rx), Ok(tx)) = (fields[0].parse::<u64>(), fields[8].parse::<u64>()) {
                    total_rx += rx;
                    total_tx += tx;
                }
            }
        }

        (total_tx, total_rx)
    }
}
