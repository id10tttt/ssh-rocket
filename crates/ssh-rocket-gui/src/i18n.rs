use gtk4::glib;
use ssh_rocket_core::Language;
use std::sync::atomic::{AtomicU8, Ordering};

static CONFIGURED_LANG: AtomicU8 = AtomicU8::new(0); // 0 = Auto, 1 = Chinese, 2 = English
static CURRENT_LANG: AtomicU8 = AtomicU8::new(0);    // 0 = Chinese, 1 = English

pub fn detect_system_is_english() -> bool {
    for loc in glib::language_names() {
        let loc_str = loc.to_lowercase();
        if loc_str.starts_with("zh") {
            return false;
        }
    }
    for var in &["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            let l = val.to_lowercase();
            if l.starts_with("zh") {
                return false;
            }
            if !l.is_empty() && l != "c" && l != "posix" {
                return true;
            }
        }
    }
    true
}

pub fn set_language(lang: Language) {
    let conf_val = match lang {
        Language::Auto => 0,
        Language::Chinese => 1,
        Language::English => 2,
    };
    CONFIGURED_LANG.store(conf_val, Ordering::SeqCst);

    let effective_en = match lang {
        Language::Auto => detect_system_is_english(),
        Language::Chinese => false,
        Language::English => true,
    };
    CURRENT_LANG.store(if effective_en { 1 } else { 0 }, Ordering::SeqCst);
}

pub fn configured_language() -> Language {
    match CONFIGURED_LANG.load(Ordering::SeqCst) {
        1 => Language::Chinese,
        2 => Language::English,
        _ => Language::Auto,
    }
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
        // Actions (Strictly isolated, no mixed brackets)
        "action.direct" => if is_en { "Direct" } else { "直连" },
        "action.proxy" => if is_en { "Proxy" } else { "代理" },
        "action.block" => if is_en { "Block" } else { "拦截" },
        "action.local" => if is_en { "Local" } else { "本地" },

        // Navigation & Global
        "nav.connect" => if is_en { "Connections" } else { "节点连接" },
        "nav.rules" => if is_en { "Rules" } else { "分流规则" },
        "nav.traffic" => if is_en { "Traffic" } else { "流量监控" },
        "nav.logs" => if is_en { "Logs" } else { "运行日志" },
        "nav.settings" => if is_en { "Settings" } else { "应用设置" },
        "btn.back" => if is_en { "Back" } else { "返回" },
        "btn.add_connection" => if is_en { "Add Connection" } else { "添加连接" },
        "status.disconnected" => if is_en { "Disconnected" } else { "未连接" },
        "status.connected" => if is_en { "Connected" } else { "已连接" },
        "status.connecting" => if is_en { "Connecting…" } else { "正在连接…" },
        "status.disconnecting" => if is_en { "Disconnecting…" } else { "正在断开…" },

        // Connection View
        "connect.empty.title" => if is_en { "No Nodes Configured" } else { "暂无节点配置" },
        "connect.empty.desc" => if is_en { "Add an SSH server to enable transparent proxy" } else { "添加 SSH 节点服务器以开启透明代理" },
        "connect.btn.add" => if is_en { "Add Connection" } else { "添加连接" },
        "connect.btn.connect" => if is_en { "Connect" } else { "连接" },
        "connect.btn.disconnect" => if is_en { "Disconnect" } else { "断开连接" },
        "connect.menu.edit" => if is_en { "Edit" } else { "编辑" },
        "connect.menu.delete" => if is_en { "Delete" } else { "删除" },
        "connect.menu.default" => if is_en { "Set as Default" } else { "设为默认" },
        "connect.card.server" => if is_en { "Server" } else { "服务器" },
        "connect.card.port" => if is_en { "Port" } else { "端口" },
        "connect.card.user" => if is_en { "User" } else { "用户名" },
        "connect.card.key" => if is_en { "Private Key" } else { "私钥" },
        "connect.card.password" => if is_en { "Password" } else { "密码" },
        "connect.card.show" => if is_en { "Show plain text" } else { "查看明文" },
        "connect.card.hide" => if is_en { "Hide plain text" } else { "隐藏明文" },

        // Dialogs
        "dialog.cancel" => if is_en { "Cancel" } else { "取消" },
        "dialog.save" => if is_en { "Save" } else { "保存" },
        "dialog.profile.title_new" => if is_en { "New Connection" } else { "新建连接" },
        "dialog.profile.title_edit" => if is_en { "Edit Connection" } else { "编辑连接" },
        "dialog.profile.name" => if is_en { "Name" } else { "名称" },
        "dialog.profile.host" => if is_en { "Server Address" } else { "服务器地址" },
        "dialog.profile.port" => if is_en { "Port" } else { "端口" },
        "dialog.profile.username" => if is_en { "Username" } else { "用户名" },
        "dialog.profile.auth_type" => if is_en { "Authentication" } else { "认证方式" },
        "dialog.profile.auth_key" => if is_en { "Private Key" } else { "私钥认证" },
        "dialog.profile.auth_password" => if is_en { "Password" } else { "密码认证" },
        "dialog.profile.identity" => if is_en { "Private Key File" } else { "私钥文件" },
        "dialog.profile.identity_btn" => if is_en { "Select Private Key File" } else { "选择私钥文件" },
        "dialog.profile.identity_dialog" => if is_en { "Select SSH Private Key File" } else { "选择 SSH 私钥文件" },
        "dialog.profile.password" => if is_en { "SSH Password" } else { "SSH 密码" },
        "dialog.profile.empty_host" => if is_en { "Server address cannot be empty" } else { "服务器地址不能为空" },
        "dialog.profile.default_name" => if is_en { "Untitled" } else { "未命名" },
        "dialog.rule.title_new" => if is_en { "Add Rule" } else { "添加规则" },
        "dialog.rule.title_edit" => if is_en { "Edit Rule" } else { "编辑规则" },
        "dialog.rule.pattern" => if is_en { "Domain, IP or CIDR" } else { "域名、IP 或 CIDR" },
        "dialog.rule.kind" => if is_en { "Rule Type" } else { "规则类型" },
        "dialog.rule.action" => if is_en { "Action" } else { "动作" },
        "dialog.rule.empty_pattern" => if is_en { "Please enter domain, IP or CIDR" } else { "请输入域名、IP 或 CIDR" },

        // Tray
        "tray.connect" => if is_en { "Connect" } else { "连接" },
        "tray.disconnect" => if is_en { "Disconnect" } else { "断开连接" },
        "tray.connecting" => if is_en { "Connecting…" } else { "正在连接…" },
        "tray.disconnecting" => if is_en { "Disconnecting…" } else { "正在断开…" },
        "tray.show_window" => if is_en { "Show" } else { "显示" },
        "tray.quit" => if is_en { "Quit" } else { "退出" },

        // Rules View - Tabs
        "rules.tab.apps" => if is_en { "Applications" } else { "应用分流" },
        "rules.tab.domain" => if is_en { "Domains & IPs" } else { "域名与 IP" },
        "rules.tab.blocked" => if is_en { "Blacklist" } else { "黑名单" },

        // Rules View - Applications Tab
        "rules.apps.search" => if is_en { "Search installed apps" } else { "搜索已安装应用" },
        "rules.apps.sort" => if is_en { "Sort" } else { "排序" },
        "rules.apps.sort_name" => if is_en { "By Name" } else { "按名称" },
        "rules.apps.sort_rule" => if is_en { "By Rule" } else { "按分流规则" },
        "rules.apps.group" => if is_en { "Desktop Applications" } else { "桌面应用程序" },

        // Rules View - Domains & IPs Tab
        "rules.domain.default_policy" => if is_en { "Default Routing Policy" } else { "全局默认策略" },
        "rules.domain.unmatched" => if is_en { "Unmatched Traffic" } else { "未匹配流量" },
        "rules.domain.ipv6" => if is_en { "IPv6 Routing" } else { "IPv6 路由分流" },
        "rules.domain.source_addr" => if is_en { "Shadowrocket Subscription URL" } else { "Shadowrocket 规则订阅地址" },
        "rules.domain.remote_group" => if is_en { "Remote Subscriptions" } else { "远程规则订阅" },
        "rules.domain.import_btn" => if is_en { "Import…" } else { "导入…" },
        "rules.domain.custom_group" => if is_en { "Custom Rules" } else { "用户自定义规则" },
        "rules.domain.quick_add" => if is_en { "Add Rule" } else { "添加规则" },
        "rules.domain.custom_list" => if is_en { "Custom Routing List" } else { "自定义分流列表" },
        "rules.domain.source_cfg" => if is_en { "Subscription Config" } else { "订阅配置" },
        "rules.domain.source_provider" => if is_en { "Source" } else { "订阅源" },
        "rules.domain.update_source" => if is_en { "Update Subscription" } else { "更新订阅" },
        "rules.domain.remove_source" => if is_en { "Delete Config" } else { "删除配置" },
        "rules.domain.entries" => if is_en { "Rule Entries" } else { "规则条目" },
        "rules.domain.view_entries" => if is_en { "View Subscription Rules" } else { "查看订阅规则" },
        "rules.domain.imported_title" => if is_en { "Subscription Rules" } else { "订阅规则条目" },
        "rules.domain.imported_search" => if is_en { "Search subscription rules" } else { "搜索订阅规则" },
        "rules.domain.imported_group" => if is_en { "Imported Rules" } else { "已导入条目" },
        "rules.domain.load_more" => if is_en { "Load More" } else { "加载更多" },
        "rules.domain.custom_title" => if is_en { "Custom Routing Rules" } else { "自定义分流规则" },
        "rules.domain.custom_search" => if is_en { "Search custom rules" } else { "搜索自定义规则" },
        "rules.domain.clear_rules" => if is_en { "Clear Rules" } else { "清空规则" },
        "rules.domain.import_omega" => if is_en { "Import SwitchyOmega" } else { "导入 SwitchyOmega" },

        // Rules View - Blocked Tab
        "rules.blocked.proc_group" => if is_en { "Process Blacklist" } else { "按进程拦截" },
        "rules.blocked.proc_add" => if is_en { "Add Process Block" } else { "添加进程拦截" },
        "rules.blocked.target_group" => if is_en { "Target Blacklist" } else { "按目标地址拦截" },
        "rules.blocked.target_add" => if is_en { "Add Target Block" } else { "添加目标拦截" },
        "rules.blocked.app_group" => if is_en { "Quick App Blacklist" } else { "已安装应用快捷拦截" },
        "rules.blocked.app_search" => if is_en { "Search and toggle block" } else { "搜索应用快速拦截" },

        // Traffic View - Tabs
        "traffic.tab.overview" => if is_en { "Overview" } else { "监控总览" },
        "traffic.tab.apps" => if is_en { "App Usage" } else { "应用统计" },
        "traffic.tab.connections" => if is_en { "Active Connections" } else { "实时连接" },

        // Traffic View - Overview
        "traffic.overview.title" => if is_en { "Session &amp; Traffic Overview" } else { "会话与传输总览" },
        "traffic.overview.total" => if is_en { "Total Traffic" } else { "总传输量" },
        "traffic.overview.proxy" => if is_en { "Proxy Traffic" } else { "代理流量" },
        "traffic.overview.direct" => if is_en { "Direct Traffic" } else { "直连流量" },
        "traffic.speed.title" => if is_en { "Real-time Speed" } else { "实时速率" },
        "traffic.speed.download" => if is_en { "Download (↓)" } else { "下载速率 (↓)" },
        "traffic.speed.upload" => if is_en { "Upload (↑)" } else { "上传速率 (↑)" },
        "traffic.speed.now" => if is_en { "Now" } else { "现在" },
        "traffic.session.duration" => if is_en { "Duration" } else { "连接时长" },
        "traffic.session.started" => if is_en { "Started" } else { "始于" },

        // Traffic View - Apps
        "traffic.apps.title" => if is_en { "Process Traffic Statistics" } else { "进程流量统计" },
        "traffic.apps.search" => if is_en { "Search apps or processes" } else { "搜索应用或进程" },
        "traffic.apps.filter.all" => if is_en { "All Traffic" } else { "全部流量" },
        "traffic.apps.filter.proxy" => if is_en { "Proxy Only" } else { "仅代理" },
        "traffic.apps.filter.direct" => if is_en { "Direct & Local" } else { "本地与直连" },
        "traffic.apps.sort.traffic" => if is_en { "Sort by Traffic" } else { "按流量" },
        "traffic.apps.sort.name" => if is_en { "Sort by Name" } else { "按名称" },

        // Traffic View - Connections
        "traffic.conn.title" => if is_en { "Active Connections" } else { "活跃连接监控" },
        "traffic.conn.search" => if is_en { "Search target IP, port or process" } else { "搜索目标 IP、端口或进程" },
        "traffic.conn.rules_dist" => if is_en { "Rule Distribution" } else { "规则策略分布" },
        "traffic.conn.proxy" => if is_en { "Proxy" } else { "代理" },
        "traffic.conn.reject" => if is_en { "Block" } else { "拦截" },
        "traffic.conn.direct" => if is_en { "Direct" } else { "直连" },
        "traffic.conn.local" => if is_en { "Local" } else { "本地" },

        // Logs View
        "logs.tab.all" => if is_en { "All" } else { "全部" },
        "logs.tab.system" => if is_en { "System" } else { "系统" },
        "logs.tab.proxy" => if is_en { "Proxy" } else { "代理" },
        "logs.tab.direct" => if is_en { "Direct" } else { "直连" },
        "logs.btn.copy" => if is_en { "Copy Logs" } else { "复制日志" },
        "logs.btn.clear" => if is_en { "Clear Logs" } else { "清空日志" },

        // Settings View
        "settings.title" => if is_en { "Settings" } else { "应用设置" },
        "settings.appearance.group" => if is_en { "Appearance &amp; Theme" } else { "外观与主题" },
        "settings.appearance.desc" => if is_en { "Personalize application appearance" } else { "个性化客户端外观配色" },
        "settings.theme.title" => if is_en { "Theme Mode" } else { "主题模式" },
        "settings.theme.subtitle" => if is_en { "Follow system appearance or force light/dark" } else { "跟随系统或强制指定浅色/深色" },
        "settings.theme.auto" => if is_en { "Follow System" } else { "跟随系统" },
        "settings.theme.light" => if is_en { "Light" } else { "浅色模式" },
        "settings.theme.dark" => if is_en { "Dark" } else { "深色模式" },

        "settings.language.group" => if is_en { "Language &amp; Region" } else { "语言与区域" },
        "settings.language.desc" => if is_en { "Configure interface display language" } else { "配置界面显示语言" },
        "settings.language.title" => if is_en { "Language" } else { "界面语言" },
        "settings.language.subtitle" => if is_en { "Switch between Chinese and English" } else { "在中英文之间无缝切换" },
        "settings.language.auto" => if is_en { "Follow System" } else { "跟随系统" },
        "settings.language.zh" => if is_en { "简体中文" } else { "简体中文" },
        "settings.language.en" => if is_en { "English" } else { "English" },

        // Tools & Floating HUD
        "floating.menu.hide" => if is_en { "Hide Floating HUD" } else { "关闭悬浮球" },
        "settings.tools.group" => if is_en { "Tools &amp; Plugins" } else { "小工具插件" },
        "settings.tools.desc" => if is_en { "Manage desktop widgets and extensions" } else { "管理桌面小工具与辅助插件" },
        "settings.floating.title" => if is_en { "Desktop Floating HUD" } else { "桌面悬浮监控球" },
        "settings.floating.subtitle" => if is_en { "Show realtime traffic and hardware monitor on desktop" } else { "在桌面展示实时网络与系统硬件监控" },
        "settings.floating.opacity" => if is_en { "Idle Opacity" } else { "移开后虚化透明度" },
        "settings.floating.delay" => if is_en { "Fade Delay (seconds)" } else { "虚化等待时间 (秒)" },
        "settings.floating.decimals" => if is_en { "Speed Decimals" } else { "网速小数位数" },
        "settings.floating.decimals.sub" => if is_en { "Decimal places for traffic speed in HUD (0 ~ 3)" } else { "悬浮球网速显示的小数位数 (0 ~ 3)" },

        _ => key,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_isolation() {
        set_language(Language::Chinese);
        assert_eq!(tr("action.direct"), "直连");
        assert_eq!(tr("action.proxy"), "代理");
        assert_eq!(tr("action.block"), "拦截");
        assert_eq!(tr("settings.theme.auto"), "跟随系统");
        assert_eq!(tr("settings.language.auto"), "跟随系统");

        // Chinese mode must not contain brackets or mixed English
        assert!(!tr("action.direct").contains('('));
        assert!(!tr("action.proxy").contains('('));
        assert!(!tr("action.block").contains('('));

        set_language(Language::English);
        assert_eq!(tr("action.direct"), "Direct");
        assert_eq!(tr("action.proxy"), "Proxy");
        assert_eq!(tr("action.block"), "Block");
        assert_eq!(tr("settings.theme.auto"), "Follow System");
        assert_eq!(tr("settings.language.auto"), "Follow System");
        assert_eq!(tr("settings.appearance.group"), "Appearance &amp; Theme");
        assert_eq!(tr("settings.language.group"), "Language &amp; Region");
        assert_eq!(tr("traffic.overview.title"), "Session &amp; Traffic Overview");

        // English mode must not contain Chinese characters
        assert!(!tr("action.direct").chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        assert!(!tr("action.proxy").chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        assert!(!tr("action.block").chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));

        set_language(Language::Auto);
        assert_eq!(configured_language(), Language::Auto);
    }
}

