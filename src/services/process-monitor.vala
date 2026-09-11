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
        private uint monitor_timer_id = 0;
        private const uint MONITOR_INTERVAL_SEC = 2;

        public ProcessMonitor (CgroupManager cgroup_manager) {
            this.cgroup_manager = cgroup_manager;
            this.target_execs = new GLib.HashTable<string, bool> (GLib.str_hash, GLib.str_equal);
        }

        public void set_targets (string[] exec_names) {
            this.target_execs.remove_all ();
            foreach (var name in exec_names) {
                string trimmed = name.strip ().down ();
                if (trimmed != "") {
                    this.target_execs.insert (trimmed, true);
                }
            }
            // 立即同步一次
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
            if (this.target_execs.size () == 0) {
                // 如果没有任何目标应用，清空代理 cgroup 中的进程
                this.cgroup_manager.cleanup_and_destroy ();
                return;
            }

            if (!this.cgroup_manager.ensure_proxy_cgroup ()) {
                return;
            }

            int[] current_proxy_pids = this.cgroup_manager.get_proxy_pids ();
            var proxy_pids_set = new GLib.HashTable<int, bool> (GLib.direct_hash, GLib.direct_equal);
            foreach (var p in current_proxy_pids) {
                proxy_pids_set.insert (p, true);
            }

            try {
                var proc_dir = GLib.Dir.open ("/proc");
                string? name = null;
                while ((name = proc_dir.read_name ()) != null) {
                    // 仅检测数字目录 (PID)
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

                    bool should_proxy = this.target_execs.contains (proc_exec);

                    if (should_proxy) {
                        if (!proxy_pids_set.contains (pid)) {
                            this.cgroup_manager.move_pid_to_proxy (pid);
                        }
                    } else {
                        // 如果之前在代理里，但目标列表已取消该应用，迁回根 cgroup
                        if (proxy_pids_set.contains (pid)) {
                            this.cgroup_manager.move_pid_to_default (pid);
                        }
                    }
                }
            } catch (GLib.Error e) {
                // 读取 /proc 异常
            }
        }

        private string get_process_name (int pid) {
            // 优先读取 /proc/<pid>/comm (准确短名称)
            string comm_path = @"/proc/$(pid)/comm";
            try {
                string comm;
                GLib.FileUtils.get_contents (comm_path, out comm);
                string trimmed = comm.strip ().down ();
                if (trimmed != "") {
                    // 检查是否匹配
                    if (this.target_execs.contains (trimmed)) {
                        return trimmed;
                    }
                }
            } catch (GLib.Error e) {
            }

            // 检查 /proc/<pid>/exe 软链接指向的文件名
            string exe_path = @"/proc/$(pid)/exe";
            try {
                string link_target = GLib.FileUtils.read_link (exe_path);
                string base_name = GLib.Path.get_basename (link_target).down ();
                if (this.target_execs.contains (base_name)) {
                    return base_name;
                }
            } catch (GLib.Error e) {
            }

            // 检查 /proc/<pid>/cmdline 首个参数
            string cmdline_path = @"/proc/$(pid)/cmdline";
            try {
                string cmdline;
                GLib.FileUtils.get_contents (cmdline_path, out cmdline);
                if (cmdline != null && cmdline.length > 0) {
                    string first_arg = cmdline.split ("\0")[0];
                    string base_name = GLib.Path.get_basename (first_arg).down ();
                    if (this.target_execs.contains (base_name)) {
                        return base_name;
                    }
                }
            } catch (GLib.Error e) {
            }

            return "";
        }
    }
}
