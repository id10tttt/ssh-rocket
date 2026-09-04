namespace Sshuttle {

    public class LogWindow : Adw.Window {
        private TunnelManager tunnel_manager;
        private Gtk.TextView text_view;
        private Gtk.TextBuffer buffer;
        private ulong log_handler_id = 0;

        public LogWindow (TunnelManager tunnel_manager, Gtk.Window parent) {
            this.tunnel_manager = tunnel_manager;
            this.transient_for = parent;
            this.default_width = 580;
            this.default_height = 420;
            this.title = "Logs";

            var toolbar_view = new Adw.ToolbarView ();
            this.set_content (toolbar_view);

            var header_bar = new Adw.HeaderBar ();
            toolbar_view.add_top_bar (header_bar);

            var copy_btn = new Gtk.Button.from_icon_name ("edit-copy-symbolic");
            copy_btn.tooltip_text = "Copy";
            copy_btn.clicked.connect (this.on_copy_clicked);
            header_bar.pack_start (copy_btn);

            var clear_btn = new Gtk.Button.from_icon_name ("edit-clear-all-symbolic");
            clear_btn.tooltip_text = "Clear";
            clear_btn.clicked.connect (this.on_clear_clicked);
            header_bar.pack_start (clear_btn);

            var scrolled = new Gtk.ScrolledWindow ();
            this.text_view = new Gtk.TextView ();
            this.text_view.editable = false;
            this.text_view.cursor_visible = false;
            this.text_view.monospace = true;
            this.text_view.wrap_mode = Gtk.WrapMode.WORD_CHAR;
            this.text_view.top_margin = 12;
            this.text_view.bottom_margin = 12;
            this.text_view.left_margin = 12;
            this.text_view.right_margin = 12;

            scrolled.set_child (this.text_view);
            toolbar_view.set_content (scrolled);

            this.buffer = this.text_view.get_buffer ();

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

            this.log_handler_id = this.tunnel_manager.log_received.connect ((line) => {
                Gtk.TextIter end_iter;
                this.buffer.get_end_iter (out end_iter);
                this.buffer.insert (ref end_iter, line + "\n", -1);
                this.scroll_to_bottom ();
            });

            this.close_request.connect (() => {
                if (this.log_handler_id != 0) {
                    this.tunnel_manager.disconnect (this.log_handler_id);
                    this.log_handler_id = 0;
                }
                return false;
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
