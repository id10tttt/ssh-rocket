use gtk4 as gtk;

/// 初始化全局 UI 样式表
pub fn init_theme() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(
        "
        /* 侧边导航栏微调 */
        .sidebar {
            background-color: alpha(currentColor, 0.03);
        }

        /* 活动连接节点卡片高亮 */
        .active-profile-card {
            border: 2px solid #2ec27e;
            background-color: alpha(#2ec27e, 0.05);
            box-shadow: 0 0 12px alpha(#2ec27e, 0.15);
        }

        /* 状态指示小徽标 */
        .status-dot {
            min-width: 8px;
            min-height: 8px;
            border-radius: 9999px;
        }
        .status-dot-connected {
            background-color: #2ec27e;
            box-shadow: 0 0 6px #2ec27e;
        }
        .status-dot-connecting {
            background-color: #e5a50a;
            box-shadow: 0 0 6px #e5a50a;
        }
        .status-dot-disconnected {
            background-color: #77767b;
        }

        /* 状态胶囊徽标 */
        .status-badge {
            border-radius: 12px;
            padding: 2px 8px;
            font-size: 11px;
            font-weight: bold;
        }
        .status-badge-connected {
            background-color: alpha(#2ec27e, 0.15);
            color: #2ec27e;
        }
        .status-badge-connecting {
            background-color: alpha(#e5a50a, 0.15);
            color: #e5a50a;
        }
        .status-badge-disconnected {
            background-color: alpha(#77767b, 0.15);
            color: #77767b;
        }

        /* 动作与规则标签胶囊 (Badge) */
        .badge-proxy {
            background-color: alpha(#2ec27e, 0.15);
            color: #2ec27e;
            border-radius: 6px;
            padding: 2px 6px;
            font-size: 11px;
            font-weight: 600;
        }
        .badge-direct {
            background-color: alpha(#3584e4, 0.15);
            color: #3584e4;
            border-radius: 6px;
            padding: 2px 6px;
            font-size: 11px;
            font-weight: 600;
        }
        .badge-reject {
            background-color: alpha(#e01b24, 0.15);
            color: #e01b24;
            border-radius: 6px;
            padding: 2px 6px;
            font-size: 11px;
            font-weight: 600;
        }
        .badge-kind {
            background-color: alpha(currentColor, 0.08);
            border-radius: 6px;
            padding: 2px 6px;
            font-size: 11px;
            font-weight: 500;
        }
        .badge-local {
            background-color: alpha(#9141ac, 0.15);
            color: #c061cb;
            border-radius: 6px;
            padding: 2px 6px;
            font-size: 11px;
            font-weight: 600;
        }

        /* 流量图表组件 */
        .chart-track {
            background-color: alpha(currentColor, 0.08);
            border-radius: 6px;
        }
        .chart-fill-direct {
            background-color: #3584e4;
            border-radius: 6px;
        }
        .chart-fill-proxy {
            background-color: #2ec27e;
            border-radius: 6px;
        }
        .chart-fill-reject {
            background-color: #e01b24;
            border-radius: 6px;
        }
        .stat-arrow-up {
            color: #e01b24;
            font-weight: bold;
        }
        .stat-arrow-down {
            color: #2ec27e;
            font-weight: bold;
        }

        /* 终端日志风格 */
        .console-view {
            font-family: monospace;
            font-size: 12px;
            line-height: 1.4;
            padding: 8px;
        }

        /* 横向分段比例条 */
        .distribution-bar {
            background-color: alpha(currentColor, 0.08);
            border-radius: 6px;
            min-height: 12px;
            overflow: hidden;
        }
        .distribution-seg-proxy {
            background-color: #2ec27e;
        }
        .distribution-seg-reject {
            background-color: #e01b24;
        }
        .distribution-seg-direct {
            background-color: #3584e4;
        }

        /* 图例圆点 */
        .legend-dot {
            min-width: 8px;
            min-height: 8px;
            border-radius: 9999px;
        }

        /* 统计卡片与指标排版 */
        .metric-tile {
            padding: 14px 18px;
        }
        .metric-tile + .metric-tile {
            border-left: 1px solid alpha(currentColor, 0.08);
        }
        .metric-title {
            font-size: 12px;
            font-weight: 500;
        }
        .metric-hero {
            font-size: 18px;
            font-weight: bold;
        }
        .process-traffic-total {
            font-weight: 600;
            font-size: 13px;
        }
        ",
    );
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
