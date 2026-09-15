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
        public const string BLOCK_CGROUP_NAME = "sshuttle-block";
        public const string BLOCK_CGROUP_PATH = "/sys/fs/cgroup/sshuttle-block";
        public const string ROOT_PROCS_PATH = "/sys/fs/cgroup/cgroup.procs";
        public const string PROXY_PROCS_PATH = "/sys/fs/cgroup/sshuttle-proxy/cgroup.procs";
        public const string BLOCK_PROCS_PATH = "/sys/fs/cgroup/sshuttle-block/cgroup.procs";
        private GLib.HashTable<int, string> original_cgroups;
        private bool runtime_created;

        public CgroupManager () {
            this.original_cgroups = new GLib.HashTable<int, string> (GLib.direct_hash, GLib.direct_equal);
        }

        public bool ensure_runtime_cgroup () {
            runtime_created = Posix.mkdir ("/sys/fs/cgroup/sshrocket-runtime", 0755) == 0;
            return runtime_created;
        }

        public bool is_runtime_cgroup_created () {
            return GLib.FileUtils.test ("/sys/fs/cgroup/sshrocket-runtime", GLib.FileTest.IS_DIR);
        }

        public bool is_cgroup_created () {
            return GLib.FileUtils.test (PROXY_CGROUP_PATH, GLib.FileTest.IS_DIR);
        }

        public bool is_block_cgroup_created () {
            return GLib.FileUtils.test (BLOCK_CGROUP_PATH, GLib.FileTest.IS_DIR);
        }

        /**
         * 创建代理专用的 cgroup v2 目录
         */
        public bool ensure_proxy_cgroup () {
            if (Posix.geteuid () != 0) {
                return this.remote_operation ("ensure-proxy", 0);
            }
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
            if (Posix.geteuid () != 0) {
                return this.remote_operation ("proxy", pid);
            }
            if (!this.ensure_proxy_cgroup ()) {
                return false;
            }

            // 检查进程是否仍然存活
            string proc_dir = @"/proc/$(pid)";
            if (!GLib.FileUtils.test (proc_dir, GLib.FileTest.IS_DIR)) {
                return false;
            }

            this.remember_original_cgroup (pid);
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
            if (Posix.geteuid () != 0) {
                return this.remote_operation ("default", pid);
            }
            string proc_dir = @"/proc/$(pid)";
            if (!GLib.FileUtils.test (proc_dir, GLib.FileTest.IS_DIR)) {
                return false;
            }

            string target_procs_path = ROOT_PROCS_PATH;
            string? original_path = this.original_cgroups.lookup (pid);
            if (original_path != null && original_path != "" && original_path != "/") {
                string relative_path = original_path.has_prefix ("/")
                    ? original_path.substring (1)
                    : original_path;
                string candidate = GLib.Path.build_filename (CGROUP_BASE, relative_path, "cgroup.procs");
                if (GLib.FileUtils.test (candidate, GLib.FileTest.EXISTS)) {
                    target_procs_path = candidate;
                }
            }

            try {
                var file = GLib.File.new_for_path (target_procs_path);
                var os = file.append_to (GLib.FileCreateFlags.NONE);
                string pid_str = @"$(pid)\n";
                os.write (pid_str.data);
                os.close ();
                this.original_cgroups.remove (pid);
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

            int length = (int) result.length;
            if (length <= 0) {
                return new int[0];
            }
            var arr = new int[length];
            for (int i = 0; i < length; i++) {
                arr[i] = result[i];
            }
            return arr;
        }

        /**
         * 创建黑名单专用的 cgroup v2 目录
         */
        public bool ensure_block_cgroup () {
            if (Posix.geteuid () != 0) {
                return this.remote_operation ("ensure-block", 0);
            }
            if (this.is_block_cgroup_created ()) {
                return true;
            }

            if (!GLib.FileUtils.test (ROOT_PROCS_PATH, GLib.FileTest.EXISTS)) {
                return false;
            }

            int res = Posix.mkdir (BLOCK_CGROUP_PATH, 0755);
            if (res != 0 && Posix.errno != Posix.EEXIST) {
                return false;
            }

            return true;
        }

        /**
         * 将指定 PID 移入黑名单 (禁止联网) cgroup
         */
        public bool move_pid_to_block (int pid) {
            if (Posix.geteuid () != 0) {
                return this.remote_operation ("block", pid);
            }
            if (!this.ensure_block_cgroup ()) {
                return false;
            }

            string proc_dir = @"/proc/$(pid)";
            if (!GLib.FileUtils.test (proc_dir, GLib.FileTest.IS_DIR)) {
                return false;
            }

            this.remember_original_cgroup (pid);
            try {
                var file = GLib.File.new_for_path (BLOCK_PROCS_PATH);
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
         * 获取当前处于黑名单 cgroup 中的所有 PID
         */
        public int[] get_block_pids () {
            var result = new GLib.GenericArray<int> ();
            if (!this.is_block_cgroup_created ()) {
                return new int[0];
            }

            try {
                string content;
                GLib.FileUtils.get_contents (BLOCK_PROCS_PATH, out content);
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
            }

            int length = (int) result.length;
            if (length <= 0) {
                return new int[0];
            }
            var arr = new int[length];
            for (int i = 0; i < length; i++) {
                arr[i] = result[i];
            }
            return arr;
        }

        /**
         * 彻底销毁代理与黑名单 cgroup：将所有剩余进程移回根 cgroup，然后 rmdir
         * 确保系统无任何残余。
         */
        public void cleanup_and_destroy () {
            if (Posix.geteuid () != 0) {
                return;
            }
            if (runtime_created) {
                Posix.rmdir ("/sys/fs/cgroup/sshrocket-runtime");
                runtime_created = false;
            }
            // 1. 清理代理 cgroup
            if (this.is_cgroup_created ()) {
                int[] pids = this.get_proxy_pids ();
                foreach (var pid in pids) {
                    this.move_pid_to_default (pid);
                }
                Posix.rmdir (PROXY_CGROUP_PATH);
            }

            // 2. 清理黑名单 cgroup
            if (this.is_block_cgroup_created ()) {
                int[] block_pids = this.get_block_pids ();
                foreach (var pid in block_pids) {
                    this.move_pid_to_default (pid);
                }
                Posix.rmdir (BLOCK_CGROUP_PATH);
            }
            this.original_cgroups.remove_all ();
        }

        private bool remote_operation (string operation, int pid) {
            if (RuntimeClient.proxy == null) {
                return false;
            }
            try {
                return RuntimeClient.proxy.cgroup (operation, pid);
            } catch (GLib.Error e) {
                warning ("Cgroup operation %s failed for PID %d: %s", operation, pid, e.message);
                return false;
            }
        }

        private void remember_original_cgroup (int pid) {
            try {
                string content;
                GLib.FileUtils.get_contents (@"/proc/$(pid)/cgroup", out content);
                foreach (var line in content.split ("\n")) {
                    string trimmed = line.strip ();
                    if (!trimmed.has_prefix ("0::")) {
                        continue;
                    }

                    string cgroup_path = trimmed.substring (3);
                    if (cgroup_path == @"/$(PROXY_CGROUP_NAME)" || cgroup_path == @"/$(BLOCK_CGROUP_NAME)") {
                        return;
                    }
                    // PID 可能在长时间运行中被系统复用，以当前进程的实际归属覆盖旧记录。
                    this.original_cgroups.insert (pid, cgroup_path);
                    return;
                }
            } catch (GLib.Error e) {
            }
        }
    }
}
