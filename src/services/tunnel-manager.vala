namespace Sshuttle {

    public class TunnelManager : Object {
        public signal void state_changed (TunnelState state);
        public signal void log_received (string line);
        public signal void profile_changed ();

        public ConfigManager config_manager { get; private set; }
        public TunnelState state { get; private set; default = TunnelState.DISCONNECTED; }
        public GLib.GenericArray<string> log_history { get; private set; }

        private GLib.Subprocess? process = null;
        private GLib.Cancellable? cancellable = null;
        private uint connect_timeout_id = 0;
        private const uint MAX_LOGS = 1000;

        public Profile? active_profile {
            owned get {
                return this.config_manager.get_active_profile ();
            }
        }

        public TunnelManager (ConfigManager config_manager) {
            this.config_manager = config_manager;
            this.log_history = new GLib.GenericArray<string> ();
        }

        public void set_active_profile (string id) {
            this.config_manager.set_active_profile (id);
            this.profile_changed ();
        }

        private void change_state (TunnelState new_state) {
            if (this.state != new_state) {
                this.state = new_state;
                this.state_changed (new_state);
            }
        }

        private void emit_log (string text) {
            this.log_history.add (text);
            if (this.log_history.length > MAX_LOGS) {
                this.log_history.remove_index (0);
            }
            this.log_received (text);
            print ("%s\n", text);
        }

        public void clear_logs () {
            this.log_history.remove_range (0, this.log_history.length);
        }

        public void toggle_connection () {
            if (this.state == TunnelState.CONNECTED || this.state == TunnelState.CONNECTING) {
                this.disconnect_tunnel ();
            } else {
                this.connect_active ();
            }
        }

        public void connect_active () {
            var profile = this.active_profile;
            if (profile == null) {
                this.emit_log ("No profile selected to connect.");
                return;
            }
            this.start_tunnel (profile);
        }

        public void start_tunnel (Profile profile) {
            if (this.state == TunnelState.CONNECTING || this.state == TunnelState.CONNECTED) {
                return;
            }

            this.change_state (TunnelState.CONNECTING);

            try {
                bool use_pkexec = (Posix.geteuid () != 0);
                string[] argv = CommandBuilder.build_argv (profile, use_pkexec);

                string cmd_str = string.joinv (" ", argv);
                this.emit_log (@"Starting tunnel: $(cmd_str)");

                this.cancellable = new GLib.Cancellable ();
                var launcher = new GLib.SubprocessLauncher (
                    GLib.SubprocessFlags.STDOUT_PIPE | GLib.SubprocessFlags.STDERR_PIPE
                );

                this.process = launcher.spawnv (argv);

                var stdout_pipe = this.process.get_stdout_pipe ();
                var stderr_pipe = this.process.get_stderr_pipe ();

                if (stdout_pipe != null) {
                    this.read_stream_async.begin (stdout_pipe);
                }
                if (stderr_pipe != null) {
                    this.read_stream_async.begin (stderr_pipe);
                }

                this.wait_process_async.begin ();

                this.connect_timeout_id = GLib.Timeout.add_seconds (25, () => {
                    this.connect_timeout_id = 0;
                    if (this.state == TunnelState.CONNECTING) {
                        this.emit_log ("Connection timed out after 25 seconds.");
                        this.disconnect_tunnel ();
                        this.change_state (TunnelState.ERROR);
                    }
                    return false;
                });

            } catch (GLib.Error e) {
                this.emit_log (@"Failed to spawn sshuttle: $(e.message)");
                this.change_state (TunnelState.ERROR);
            }
        }

        private async void wait_process_async () {
            try {
                if (this.process != null) {
                    yield this.process.wait_async (this.cancellable);
                }
            } catch (GLib.Error e) {
                this.emit_log (@"Process wait failed: $(e.message)");
            }
            this.on_process_exit ();
        }

        private async void read_stream_async (GLib.InputStream stream) {
            var data_stream = new GLib.DataInputStream (stream);
            try {
                while (true) {
                    size_t length;
                    string? line = yield data_stream.read_line_utf8_async (
                        GLib.Priority.DEFAULT,
                        this.cancellable,
                        out length
                    );
                    if (line == null) {
                        break;
                    }
                    this.on_log_line (line);
                }
            } catch (GLib.Error e) {
                // 读取取消或流关闭
            }
        }

        private void on_log_line (string line) {
            this.emit_log (line);
            string lower = line.down ();
            if ("connected to server" in lower || "c : connected" in lower || "tunnel ready" in lower || (lower.has_prefix ("connected") && !("not connected" in lower))) {
                if (this.state == TunnelState.CONNECTING) {
                    if (this.connect_timeout_id != 0) {
                        GLib.Source.remove (this.connect_timeout_id);
                        this.connect_timeout_id = 0;
                    }
                    this.change_state (TunnelState.CONNECTED);
                }
            }
        }

        public void disconnect_tunnel () {
            if (this.state == TunnelState.DISCONNECTED || this.state == TunnelState.DISCONNECTING) {
                return;
            }

            if (this.connect_timeout_id != 0) {
                GLib.Source.remove (this.connect_timeout_id);
                this.connect_timeout_id = 0;
            }

            if (this.cancellable != null) {
                this.cancellable.cancel ();
            }

            this.change_state (TunnelState.DISCONNECTING);
            this.emit_log ("Disconnecting tunnel...");

            if (this.process != null) {
                this.process.send_signal (Posix.Signal.INT);
                GLib.Timeout.add_seconds (3, () => {
                    if (this.state == TunnelState.DISCONNECTING && this.process != null) {
                        this.process.force_exit ();
                    }
                    return false;
                });
            } else {
                this.change_state (TunnelState.DISCONNECTED);
            }
        }

        private void on_process_exit () {
            if (this.connect_timeout_id != 0) {
                GLib.Source.remove (this.connect_timeout_id);
                this.connect_timeout_id = 0;
            }

            int exit_status = 0;
            if (this.process != null) {
                exit_status = this.process.get_exit_status ();
                this.emit_log (@"Process exited with status $(exit_status)");
            }

            this.process = null;

            if (this.state == TunnelState.DISCONNECTING) {
                this.change_state (TunnelState.DISCONNECTED);
            } else if (this.state != TunnelState.DISCONNECTED) {
                this.change_state (TunnelState.ERROR);
            }
        }
    }
}
