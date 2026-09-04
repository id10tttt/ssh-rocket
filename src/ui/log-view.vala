namespace Sshuttle {

    public class LogView : Gtk.Box {
        private TunnelManager tunnel_manager;
        private Gtk.TextView text_view;
        private Gtk.TextBuffer buffer;

        public LogView (TunnelManager tunnel_manager) {
            Object (orientation: Gtk.Orientation.VERTICAL, spacing: 0);
            this.tunnel_manager = tunnel_manager;

            // 顶部操作栏
            var action_bar = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 8);
            action_bar.margin_start = 16;
            action_bar.margin_end = 16;
            action_bar.margin_top = 10;
            action_bar.margin_bottom = 10;
            this.append (action_bar);

            var title_lbl = new Gtk.Label ("Real-time Output");
            title_lbl.add_css_class ("dim-label");
            action_bar.append (title_lbl);

            var spacer = new Gtk.Box (Gtk.Orientation.HORIZONTAL, 0);
            spacer.hexpand = true;
            action_bar.append (spacer);

            var copy_btn = new Gtk.Button.from_icon_name ("edit-copy-symbolic");
            copy_btn.tooltip_text = "Copy Log";
            copy_btn.clicked.connect (this.on_copy_clicked);
            action_bar.append (copy_btn);

            var clear_btn = new Gtk.Button.from_icon_name ("edit-clear-all-symbolic");
            clear_btn.tooltip_text = "Clear Log";
            clear_btn.clicked.connect (this.on_clear_clicked);
            action_bar.append (clear_btn);

            // 分割线
            var sep = new Gtk.Separator (Gtk.Orientation.HORIZONTAL);
            this.append (sep);

            // 滚动日志文本区域
            var scrolled = new Gtk.ScrolledWindow ();
            scrolled.vexpand = true;
            this.append (scrolled);

            this.text_view = new Gtk.TextView ();
            this.text_view.editable = false;
            this.text_view.cursor_visible = false;
            this.text_view.monospace = true;
            this.text_view.wrap_mode = Gtk.WrapMode.WORD_CHAR;
            this.text_view.top_margin = 12;
            this.text_view.bottom_margin = 12;
            this.text_view.left_margin = 16;
            this.text_view.right_margin = 16;

            scrolled.set_child (this.text_view);
            this.buffer = this.text_view.get_buffer ();

            // 初始化已有历史
            var hist = this.tunnel_manager.log_history;
            if (hist.length > 0) {
                var sb = new StringBuilder ();
                for (uint i = 0; i < hist.length; i++) {
                    sb.append (hist[i]);
                    sb.append ("\n");
                }
                this.buffer.set_text (sb.str, -1);
                this.scroll_to_bottom ();
            }

            this.tunnel_manager.log_received.connect ((line) => {
                Gtk.TextIter end_iter;
                this.buffer.get_end_iter (out end_iter);
                this.buffer.insert (ref end_iter, line + "\n", -1);
                this.scroll_to_bottom ();
            });
        }

        private void scroll_to_bottom () {
            Gtk.TextIter end_iter;
            this.buffer.get_end_iter (out end_iter);
            var mark = this.buffer.create_mark (null, end_iter, false);
            this.text_view.scroll_to_mark (mark, 0.0, true, 0.0, 1.0);
        }

        private void on_copy_clicked () {
            Gtk.TextIter start_iter, end_iter;
            this.buffer.get_start_iter (out start_iter);
            this.buffer.get_end_iter (out end_iter);
            string text = this.buffer.get_text (start_iter, end_iter, false);
            var display = this.get_display ();
            if (display != null) {
                display.get_clipboard ().set_text (text);
            }
        }

        private void on_clear_clicked () {
            this.tunnel_manager.clear_logs ();
            this.buffer.set_text ("", -1);
        }
    }
}
