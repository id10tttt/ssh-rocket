use ssh_rocket_core::Language;
use std::sync::atomic::{AtomicU8, Ordering};

static CURRENT_LANG: AtomicU8 = AtomicU8::new(0); // 0 = Chinese, 1 = English

pub fn set_language(lang: Language) {
    let val = match lang {
        Language::Chinese => 0,
        Language::English => 1,
    };
    CURRENT_LANG.store(val, Ordering::SeqCst);
}

pub fn current_language() -> Language {
    match CURRENT_LANG.load(Ordering::SeqCst) {
        1 => Language::English,
        _ => Language::Chinese,
    }
}

pub fn tr(key: &'static str) -> &'static str {
    let is_en = CURRENT_LANG.load(Ordering::Relaxed) == 1;
    match key {
        // Navigation & Windows
        "nav.connect" => if is_en { "Connections" } else { "节点连接" },
        "nav.rules" => if is_en { "Rules" } else { "分流规则" },
        "nav.traffic" => if is_en { "Traffic" } else { "流量监控" },
        "nav.logs" => if is_en { "Logs" } else { "运行日志" },
        "nav.settings" => if is_en { "Settings" } else { "应用设置" },
        "btn.back" => if is_en { "Back" } else { "返回" },
        "btn.add_connection" => if is_en { "Add Connection" } else { "添加连接" },
        "status.disconnected" => if is_en { "Disconnected" } else { "未连接" },
        "status.connected" => if is_en { "Connected" } else { "已连接" },
        "status.connecting" => if is_en { "Connecting..." } else { "正在连接..." },

        // Traffic Tabs
        "traffic.tab.overview" => if is_en { "Overview" } else { "监控总览" },
        "traffic.tab.apps" => if is_en { "App Usage" } else { "应用统计" },
        "traffic.tab.connections" => if is_en { "Active Connections" } else { "实时连接" },

        // Traffic Overview
        "traffic.overview.title" => if is_en { "Session & Traffic Overview" } else { "会话与传输总览" },
        "traffic.overview.total" => if is_en { "Total Traffic" } else { "总传输量" },
        "traffic.overview.proxy" => if is_en { "Proxy Traffic" } else { "代理流量" },
        "traffic.overview.direct" => if is_en { "Direct Traffic" } else { "直连流量" },
        "traffic.speed.title" => if is_en { "Real-time Speed" } else { "实时速率" },
        "traffic.speed.download" => if is_en { "Download (↓)" } else { "下载速率 (↓)" },
        "traffic.speed.upload" => if is_en { "Upload (↑)" } else { "上传速率 (↑)" },
        "traffic.speed.now" => if is_en { "Now" } else { "现在" },
        "traffic.session.duration" => if is_en { "Duration" } else { "连接时长" },
        "traffic.session.started" => if is_en { "Started" } else { "始于" },

        // Traffic Apps
        "traffic.apps.title" => if is_en { "Process Traffic Statistics" } else { "进程流量统计" },
        "traffic.apps.search" => if is_en { "Search apps or processes" } else { "搜索应用或进程" },
        "traffic.apps.filter.all" => if is_en { "All Traffic" } else { "全部流量" },
        "traffic.apps.filter.proxy" => if is_en { "Proxy Only" } else { "仅代理" },
        "traffic.apps.filter.direct" => if is_en { "Direct & Local" } else { "本地与直连" },
        "traffic.apps.sort.traffic" => if is_en { "Sort by Traffic" } else { "按流量" },
        "traffic.apps.sort.name" => if is_en { "Sort by Name" } else { "按名称" },

        // Traffic Connections
        "traffic.conn.title" => if is_en { "Active Connections" } else { "活跃连接监控" },
        "traffic.conn.search" => if is_en { "Search target IP, port or process" } else { "搜索目标 IP、端口或进程" },
        "traffic.conn.rules_dist" => if is_en { "Rule Distribution" } else { "规则策略分布" },
        "traffic.conn.proxy" => if is_en { "Proxy" } else { "代理" },
        "traffic.conn.reject" => if is_en { "Block" } else { "拦截" },
        "traffic.conn.direct" => if is_en { "Direct" } else { "直连" },
        "traffic.conn.local" => if is_en { "Local" } else { "本地" },

        // Settings View
        "settings.title" => if is_en { "Settings" } else { "设置" },
        "settings.appearance.group" => if is_en { "Appearance & Theme" } else { "外观与主题" },
        "settings.appearance.desc" => if is_en { "Personalize application appearance" } else { "个性化客户端外观配色" },
        "settings.theme.title" => if is_en { "Theme Mode" } else { "主题模式" },
        "settings.theme.subtitle" => if is_en { "Follow system appearance or force light/dark" } else { "跟随系统或强制指定浅色/深色" },
        "settings.theme.auto" => if is_en { "Auto (Follow System)" } else { "跟随系统 (Auto)" },
        "settings.theme.light" => if is_en { "Light" } else { "浅色模式 (Light)" },
        "settings.theme.dark" => if is_en { "Dark" } else { "深色模式 (Dark)" },

        "settings.language.group" => if is_en { "Language & Region" } else { "语言与区域" },
        "settings.language.desc" => if is_en { "Configure interface display language" } else { "配置界面显示语言" },
        "settings.language.title" => if is_en { "Language" } else { "界面语言" },
        "settings.language.subtitle" => if is_en { "Switch between Chinese and English" } else { "在中英文之间无缝切换" },
        "settings.language.zh" => if is_en { "简体中文" } else { "简体中文 (Chinese)" },
        "settings.language.en" => if is_en { "English" } else { "English" },

        _ => key,
    }
}
