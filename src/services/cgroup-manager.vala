namespace Sshuttle {

    /**
     * CgroupManager
     * 管理 cgroup v2 临时代理切片 (/sys/fs/cgroup/sshuttle-proxy/)
     * 纯运行时管理，断开与退出时必须完全清理，不留残余。
     */
    public class CgroupManager : Object {
        public const string CGROUP_BASE = "/sys/fs/cgroup";
        public const string PROXY_CGROUP_NAME = "sshuttle-proxy";
        public const string PROXY_CGROUP_PATH = "/sys/fs/cgroup/sshuttle-proxy";
        public const string ROOT_PROCS_PATH = "/sys/fs/cgroup/cgroup.procs";
        public const string PROXY_PROCS_PATH = "/sys/fs/cgroup/sshuttle-proxy/cgroup.procs";

        public bool is_cgroup_created () {
            return GLib.FileUtils.test (PROXY_CGROUP_PATH, GLib.FileTest.IS_DIR);
        }

        /**
         * 创建代理专用的 cgroup v2 目录
         */
        public bool ensure_proxy_cgroup () {
            if (this.is_cgroup_created ()) {
                return true;
            }

            // 检查系统是否启用了 cgroup v2
            if (!GLib.FileUtils.test (ROOT_PROCS_PATH, GLib.FileTest.EXISTS)) {
                warning ("cgroup v2 is not mounted at %s", CGROUP_BASE);
                return false;
            }

            int res = Posix.mkdir (PROXY_CGROUP_PATH, 0755);
            if (res != 0 && Posix.errno != Posix.EEXIST) {
                warning ("Failed to create cgroup directory %s: %s", PROXY_CGROUP_PATH, Posix.strerror (Posix.errno));
                return false;
            }

            return true;
        }

        /**
         * 将指定 PID 移入代理 cgroup
         */
        public bool move_pid_to_proxy (int pid) {
            if (!this.ensure_proxy_cgroup ()) {
                return false;
            }

            // 检查进程是否仍然存活
            string proc_dir = @"/proc/$(pid)";
            if (!GLib.FileUtils.test (proc_dir, GLib.FileTest.IS_DIR)) {
                return false;
            }

            try {
                var file = GLib.File.new_for_path (PROXY_PROCS_PATH);
                var os = file.append_to (GLib.FileCreateFlags.NONE);
                string pid_str = @"$(pid)\n";
                os.write (pid_str.data);
                os.close ();
                return true;
            } catch (GLib.Error e) {
                // 进程可能在写入时恰好退出，忽略此类常见错误
                return false;
            }
        }

        /**
         * 将指定 PID 移回根 cgroup
         */
        public bool move_pid_to_default (int pid) {
            string proc_dir = @"/proc/$(pid)";
            if (!GLib.FileUtils.test (proc_dir, GLib.FileTest.IS_DIR)) {
                return false;
            }

            try {
                var file = GLib.File.new_for_path (ROOT_PROCS_PATH);
                var os = file.append_to (GLib.FileCreateFlags.NONE);
                string pid_str = @"$(pid)\n";
                os.write (pid_str.data);
                os.close ();
                return true;
            } catch (GLib.Error e) {
                return false;
            }
        }

        /**
         * 获取当前处于代理 cgroup 中的所有 PID
         */
        public int[] get_proxy_pids () {
            var result = new GLib.GenericArray<int> ();
            if (!this.is_cgroup_created ()) {
                return new int[0];
            }

            try {
                string content;
                GLib.FileUtils.get_contents (PROXY_PROCS_PATH, out content);
                string[] lines = content.split ("\n");
                foreach (var line in lines) {
                    string trimmed = line.strip ();
                    if (trimmed != "") {
                        int pid = int.parse (trimmed);
                        if (pid > 0) {
                            result.add (pid);
                        }
                    }
                }
            } catch (GLib.Error e) {
                // 读取失败或为空
            }

            var arr = new int[result.length];
            for (uint i = 0; i < result.length; i++) {
                arr[i] = result[i];
            }
            return arr;
        }

        /**
         * 彻底销毁代理 cgroup：将所有剩余进程移回根 cgroup，然后 rmdir
         * 确保系统无任何残余。
         */
        public void cleanup_and_destroy () {
            if (!this.is_cgroup_created ()) {
                return;
            }

            int[] pids = this.get_proxy_pids ();
            foreach (var pid in pids) {
                this.move_pid_to_default (pid);
            }

            // 给内核微小的时间同步迁移状态
            int res = Posix.rmdir (PROXY_CGROUP_PATH);
            if (res != 0 && Posix.errno != Posix.ENOENT) {
                // 如果还有子线程或延迟，再次迁移尝试清理
                pids = this.get_proxy_pids ();
                foreach (var pid in pids) {
                    this.move_pid_to_default (pid);
                }
                Posix.rmdir (PROXY_CGROUP_PATH);
            }
        }
    }
}
