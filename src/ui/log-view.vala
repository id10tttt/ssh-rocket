namespace Sshuttle {

    public class LogView : Gtk.Box {
        private TunnelManager tunnel_manager;

        private Adw.ViewStack stack;
        private Gtk.TextView app_text_view;
        private Gtk.TextBuffer app_buffer;
        private Gtk.StringList app_filter_model;
        private Gtk.DropDown app_dropdown;

        private Gtk.TextView proxy_text_view;
        private Gtk.TextBuffer proxy_buffer;

        public LogView (TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            this.tunnel_manager = tunnel_manager;

            // Tab 切换条
            var switcher_box = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0);
            switcher_box.halign = Gtk.Align.CENTER;
            switcher_box.margin_top = 8;
            switcher_box.margin_bottom = 8;

            this.stack = new Adw.ViewStack ();
            this.stack.vexpand = true;

            var switcher = new Adw.ViewSwitcher ();
            switcher.stack = this.stack;
            switcher.policy = Adw.ViewSwitcherPolicy.WIDE;
            switcher_box.append (switcher);
            this.append (switcher_box);
            this.append (this.stack);

            // Tab 1: 软件日志 (App Logs)
            var app_tab = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            this.setup_app_logs_tab (app_tab);
            var app_page = this.stack.add_named (app_tab, "app_logs");
            app_page.title = "App Logs";
            app_page.icon_name = "application-x-executable-symbolic";

            // Tab 2: 代理日志 (Proxy Logs)
            var proxy_tab = new Gtk.Box (Gtk.Orientation.VERTICAL, 0);
            this.setup_proxy_logs_tab (proxy_tab);
            var proxy_page = this.stack.add_named (proxy_tab, "proxy_logs");
            proxy_page.title = "Proxy Logs";
            proxy_page.icon_name = "network-vpn-symbolic";
        }

        private void setup_app_logs_tab (Gtk.Box container) {
            // 操作栏：筛选下拉菜单 + 复制 + 清空
            var action_bar = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            action_bar.margin_start = 16;
            action_bar.margin_end = 16;
            action_bar.margin_top = 4;
            action_bar.margin_bottom = 8;
            container.append (action_bar);

            var filter_label = new Gtk.Label ("Filter App:");
            filter_label.add_css_class ("dim-label");
            action_bar.append (filter_label);

            this.app_filter_model = new Gtk.StringList (new string[] { "All Applications", "System" });
            this.app_dropdown = new Gtk.DropDown (this.app_filter_model, null);
            this.app_dropdown.notify["selected"].connect (this.refresh_app_logs);
            action_bar.append (this.app_dropdown);

            var spacer = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0);
            spacer.hexpand = true;
            action_bar.append (spacer);

            var copy_btn = new Gtk.Button.from_icon_name ("edit-copy-symbolic");
            copy_btn.add_css_class ("flat");
            copy_btn.tooltip_text = "Copy App Logs";
            copy_btn.clicked.connect (this.on_copy_app_logs);
            action_bar.append (copy_btn);

            var clear_btn = new Gtk.Button.from_icon_name ("edit-clear-all-symbolic");
            clear_btn.add_css_class ("flat");
            clear_btn.tooltip_text = "Clear App Logs";
            clear_btn.clicked.connect (this.on_clear_app_logs);
            action_bar.append (clear_btn);

            var sep = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            container.append (sep);

            // 滚动文本区域
            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;
            container.append (scrolled);

            this.app_text_view = new Gtk.TextView ();
            this.app_text_view.editable = false;
            this.app_text_view.cursor_visible = false;
            this.app_text_view.monospace = true;
            this.app_text_view.wrap_mode = Gtk.WrapMode.WORD_CHAR;
            this.app_text_view.top_margin = 12;
            this.app_text_view.bottom_margin = 12;
            this.app_text_view.left_margin = 16;
            this.app_text_view.right_margin = 16;
            this.app_text_view.add_css_class ("dim-label");

            scrolled.set_child (this.app_text_view);
            this.app_buffer = this.app_text_view.get_buffer ();

            // 初始化已有数据
            this.refresh_app_logs ();

            // 监听新日志
            this.tunnel_manager.app_log_received.connect ((entry) => {
                this.ensure_app_in_filter (entry.app_name);
                string selected_app = this.get_selected_app_filter ();
                if (selected_app == "All Applications" || selected_app == entry.app_name) {
                    Gtk.TextIter end_iter;
                    this.app_buffer.get_end_iter (out end_iter);
                    this.app_buffer.insert (ref end_iter, @"[$(entry.timestamp)] [$(entry.app_name)] $(entry.message)\n", -1);
                    this.scroll_to_bottom (this.app_text_view, this.app_buffer);
                }
            });
        }

        private void setup_proxy_logs_tab (Gtk.Box container) {
            var action_bar = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            action_bar.margin_start = 16;
            action_bar.margin_end = 16;
            action_bar.margin_top = 4;
            action_bar.margin_bottom = 8;
            container.append (action_bar);

            var spacer = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0);
            spacer.hexpand = true;
            action_bar.append (spacer);

            var copy_btn = new Gtk.Button.from_icon_name ("edit-copy-symbolic");
            copy_btn.add_css_class ("flat");
            copy_btn.tooltip_text = "Copy Proxy Logs";
            copy_btn.clicked.connect (this.on_copy_proxy_logs);
            action_bar.append (copy_btn);

            var clear_btn = new Gtk.Button.from_icon_name ("edit-clear-all-symbolic");
            clear_btn.add_css_class ("flat");
            clear_btn.tooltip_text = "Clear Proxy Logs";
            clear_btn.clicked.connect (this.on_clear_proxy_logs);
            action_bar.append (clear_btn);

            var sep = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            container.append (sep);

            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;
            container.append (scrolled);

            this.proxy_text_view = new Gtk.TextView ();
            this.proxy_text_view.editable = false;
            this.proxy_text_view.cursor_visible = false;
            this.proxy_text_view.monospace = true;
            this.proxy_text_view.wrap_mode = Gtk.WrapMode.WORD_CHAR;
            this.proxy_text_view.top_margin = 12;
            this.proxy_text_view.bottom_margin = 12;
            this.proxy_text_view.left_margin = 16;
            this.proxy_text_view.right_margin = 16;
            this.proxy_text_view.add_css_class ("dim-label");

            scrolled.set_child (this.proxy_text_view);
            this.proxy_buffer = this.proxy_text_view.get_buffer ();

            // 初始化代理日志
            var p_logs = this.tunnel_manager.proxy_logs;
            if (p_logs.length > 0) {
                var sb = new StringBuilder ();
                for (uint i = 0; i < p_logs.length; i++) {
                    sb.append (p_logs[i]);
                    sb.append ("\n");
                }
                this.proxy_buffer.set_text (sb.str, -1);
                this.scroll_to_bottom (this.proxy_text_view, this.proxy_buffer);
            }

            this.tunnel_manager.proxy_log_received.connect ((line) => {
                Gtk.TextIter end_iter;
                this.proxy_buffer.get_end_iter (out end_iter);
                this.proxy_buffer.insert (ref end_iter, line + "\n", -1);
                this.scroll_to_bottom (this.proxy_text_view, this.proxy_buffer);
            });
        }

        private string get_selected_app_filter () {
            uint idx = this.app_dropdown.selected;
            if (idx < this.app_filter_model.get_n_items ()) {
                return this.app_filter_model.get_string (idx);
            }
            return "All Applications";
        }

        private void ensure_app_in_filter (string app_name) {
            uint n = this.app_filter_model.get_n_items ();
            for (uint i = 0; i < n; i++) {
                if (this.app_filter_model.get_string (i) == app_name) {
                    return;
                }
            }
            this.app_filter_model.append (app_name);
        }

        private void refresh_app_logs () {
            string filter = this.get_selected_app_filter ();
            var logs = this.tunnel_manager.app_logs;
            var sb = new StringBuilder ();

            for (uint i = 0; i < logs.length; i++) {
                var entry = logs[i];
                this.ensure_app_in_filter (entry.app_name);
                if (filter == "All Applications" || filter == entry.app_name) {
                    sb.append (@"[$(entry.timestamp)] [$(entry.app_name)] $(entry.message)\n");
                }
            }
            this.app_buffer.set_text (sb.str, -1);
            this.scroll_to_bottom (this.app_text_view, this.app_buffer);
        }

        private void scroll_to_bottom (Gtk.TextView tv, Gtk.TextBuffer buf) {
            Gtk.TextIter end_iter;
            buf.get_end_iter (out end_iter);
            var mark = buf.create_mark (null, end_iter, false);
            tv.scroll_to_mark (mark, 0.0, true, 0.0, 1.0);
        }

        private void on_copy_app_logs () {
            Gtk.TextIter start_iter, end_iter;
            this.app_buffer.get_start_iter (out start_iter);
            this.app_buffer.get_end_iter (out end_iter);
            string text = this.app_buffer.get_text (start_iter, end_iter, false);
            var display = this.get_display ();
            if (display != null) {
                display.get_clipboard ().set_text (text);
            }
        }

        private void on_clear_app_logs () {
            this.tunnel_manager.clear_app_logs ();
            this.app_buffer.set_text ("", -1);
        }

        private void on_copy_proxy_logs () {
            Gtk.TextIter start_iter, end_iter;
            this.proxy_buffer.get_start_iter (out start_iter);
            this.proxy_buffer.get_end_iter (out end_iter);
            string text = this.proxy_buffer.get_text (start_iter, end_iter, false);
            var display = this.get_display ();
            if (display != null) {
                display.get_clipboard ().set_text (text);
            }
        }

        private void on_clear_proxy_logs () {
            this.tunnel_manager.clear_proxy_logs ();
            this.proxy_buffer.set_text ("", -1);
        }
    }
}
