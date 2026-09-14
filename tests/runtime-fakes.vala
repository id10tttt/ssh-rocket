// 特权服务测试用内存后端，不读写真实防火墙或 cgroup。
namespace Config {
    public const string HELPER_PATH = "/usr/local/libexec/sshuttle-gui-helper";
    public const string SSHUTTLE_PATH = "/proc/self/exe";
    public const string PKEXEC_PATH = "/usr/bin/false";
}

namespace Sshuttle {
    public class CgroupManager : Object {
        public static bool proxy_created;
        public static bool block_created;
        public static int cleanup_count;
        public bool is_cgroup_created () { return proxy_created; }
        public bool is_block_cgroup_created () { return block_created; }
        public bool ensure_proxy_cgroup () { proxy_created = true; return true; }
        public bool ensure_block_cgroup () { block_created = true; return true; }
        public bool move_pid_to_proxy (int pid) { return true; }
        public bool move_pid_to_block (int pid) { return true; }
        public bool move_pid_to_default (int pid) { return true; }
        public void cleanup_and_destroy () {
            proxy_created = false;
            block_created = false;
            cleanup_count++;
        }
    }

    public class NftManager : Object {
        public bool has_active_sshuttle_tables () { return false; }
        public bool base_chains_exist (int port, bool ipv6) { return true; }
        public bool apply_cgroup_filter (int port, bool ipv6, string policy, string[] networks, DomainRule[] rules) {
            return CgroupManager.proxy_created && CgroupManager.block_created;
        }
        public bool apply_blacklist_filter () { return CgroupManager.block_created; }
        public void add_ips_to_set (string[] ips, string action) {}
        public void flush_ip_sets () {}
        public void cleanup_blacklist_filter () {}
        public void cleanup_all_sshuttle_tables (int port) {}
    }
}
