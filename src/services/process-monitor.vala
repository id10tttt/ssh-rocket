namespace Sshuttle {

    /**
     * ProcessMonitor
     * 监控系统中属于被代理 App 的进程，自动将它们移入 sshuttle-proxy cgroup。
     * 当应用被取消代理时，将现有进程迁回根 cgroup。
     * 停止监控时全量迁移恢复。
     */
    public class ProcessMonitor : Object {
        private CgroupManager cgroup_manager;
        private GLib.HashTable<string, bool> target_execs;
        private GLib.HashTable<string, bool> block_execs;
        private uint monitor_timer_id = 0;
        private const uint MONITOR_INTERVAL_SEC = 2;

        public signal void process_migrated (string app_name, int pid, string cgroup_type);

        public ProcessMonitor (CgroupManager cgroup_manager) {
            this.cgroup_manager = cgroup_manager;
            this.target_execs = new GLib.HashTable<string, bool> (GLib.str_hash, GLib.str_equal);
            this.block_execs = new GLib.HashTable<string, bool> (GLib.str_hash, GLib.str_equal);
        }

        public void set_targets (string[] exec_names) {
            this.target_execs.remove_all ();
            foreach (var name in exec_names) {
                string trimmed = name.strip ().down ();
                if (trimmed != "") {
                    this.target_execs.insert (trimmed, true);
                }
            }
            this.scan_and_migrate ();
        }

        public void set_block_targets (string[] block_names) {
            this.block_execs.remove_all ();
            foreach (var name in block_names) {
                string trimmed = name.strip ().down ();
                if (trimmed != "") {
                    this.block_execs.insert (trimmed, true);
                }
            }
            this.scan_and_migrate ();
        }

        public void start () {
            if (this.monitor_timer_id != 0) {
                return;
            }

            this.scan_and_migrate ();
            this.monitor_timer_id = GLib.Timeout.add_seconds (MONITOR_INTERVAL_SEC, () => {
                this.scan_and_migrate ();
                return true;
            });
        }

        public void stop () {
            if (this.monitor_timer_id != 0) {
                GLib.Source.remove (this.monitor_timer_id);
                this.monitor_timer_id = 0;
            }
            this.cgroup_manager.cleanup_and_destroy ();
        }

        public void force_sync () {
            this.scan_and_migrate ();
        }

        private void scan_and_migrate () {
            if (this.target_execs.size () == 0 && this.block_execs.size () == 0) {
                this.cgroup_manager.cleanup_and_destroy ();
                return;
            }

            if (this.target_execs.size () > 0) {
                this.cgroup_manager.ensure_proxy_cgroup ();
            }
            if (this.block_execs.size () > 0) {
                this.cgroup_manager.ensure_block_cgroup ();
            }

            int[] current_proxy_pids = this.cgroup_manager.get_proxy_pids ();
            var proxy_pids_set = new GLib.HashTable<int, bool> (GLib.direct_hash, GLib.direct_equal);
            foreach (var p in current_proxy_pids) {
                proxy_pids_set.insert (p, true);
            }

            int[] current_block_pids = this.cgroup_manager.get_block_pids ();
            var block_pids_set = new GLib.HashTable<int, bool> (GLib.direct_hash, GLib.direct_equal);
            foreach (var p in current_block_pids) {
                block_pids_set.insert (p, true);
            }

            try {
                var proc_dir = GLib.Dir.open ("/proc");
                string? name = null;
                while ((name = proc_dir.read_name ()) != null) {
                    if (name.length == 0 || !name[0].isdigit ()) {
                        continue;
                    }

                    int pid = int.parse (name);
                    if (pid <= 1) {
                        continue;
                    }

                    string proc_exec = this.get_process_name (pid);
                    if (proc_exec == "") {
                        continue;
                    }

                    bool should_block = this.block_execs.contains (proc_exec);
                    bool should_proxy = this.target_execs.contains (proc_exec);

                    // 黑名单优先级最高：若被黑名单则移入 block cgroup
                    if (should_block) {
                        if (!block_pids_set.contains (pid)) {
                            this.cgroup_manager.move_pid_to_block (pid);
                            this.process_migrated (proc_exec, pid, "block");
                        }
                    } else if (should_proxy) {
                        if (!proxy_pids_set.contains (pid)) {
                            this.cgroup_manager.move_pid_to_proxy (pid);
                            this.process_migrated (proc_exec, pid, "proxy");
                        }
                    } else {
                        if (proxy_pids_set.contains (pid) || block_pids_set.contains (pid)) {
                            this.cgroup_manager.move_pid_to_default (pid);
                        }
                    }
                }
            } catch (GLib.Error e) {
            }
        }

        private string get_process_name (int pid) {
            string comm_path = @"/proc/$(pid)/comm";
            try {
                string comm;
                GLib.FileUtils.get_contents (comm_path, out comm);
                string trimmed = comm.strip ().down ();
                if (trimmed != "") {
                    if (this.target_execs.contains (trimmed) || this.block_execs.contains (trimmed)) {
                        return trimmed;
                    }
                }
            } catch (GLib.Error e) {
            }

            string exe_path = @"/proc/$(pid)/exe";
            try {
                string link_target = GLib.FileUtils.read_link (exe_path);
                string base_name = GLib.Path.get_basename (link_target).down ();
                if (this.target_execs.contains (base_name) || this.block_execs.contains (base_name)) {
                    return base_name;
                }
            } catch (GLib.Error e) {
            }

            string cmdline_path = @"/proc/$(pid)/cmdline";
            try {
                uint8[] data;
                GLib.FileUtils.get_data (cmdline_path, out data);
                if (data != null && data.length > 0) {
                    // cmdline 中以 \0 分隔各个参数，遍历所有参数寻找目标可执行文件名
                    int start = 0;
                    for (int i = 0; i < data.length; i++) {
                        if (data[i] == 0) {
                            if (i > start) {
                                string arg = ((string) data).substring (start, i - start);
                                string base_name = GLib.Path.get_basename (arg).down ();
                                if (this.target_execs.contains (base_name) || this.block_execs.contains (base_name)) {
                                    return base_name;
                                }
                            }
                            start = i + 1;
                        }
                    }
                }
            } catch (GLib.Error e) {
            }

            // 针对 Flatpak / 沙盒应用，检查 /proc/$(pid)/cgroup 中的应用标识
            string cgroup_path = @"/proc/$(pid)/cgroup";
            try {
                string cgroup_content;
                GLib.FileUtils.get_contents (cgroup_path, out cgroup_content);
                string cgroup_lower = cgroup_content.down ();
                if ("app-flatpak-" in cgroup_lower || "flatpak" in cgroup_lower) {
                    var iter = GLib.HashTableIter<string, bool> (this.target_execs);
                    string key;
                    while (iter.next (out key, null)) {
                        if (key.length >= 3 && key in cgroup_lower) {
                            return key;
                        }
                    }
                    var biter = GLib.HashTableIter<string, bool> (this.block_execs);
                    while (biter.next (out key, null)) {
                        if (key.length >= 3 && key in cgroup_lower) {
                            return key;
                        }
                    }
                }
            } catch (GLib.Error e) {
            }

            return "";
        }
    }
}
