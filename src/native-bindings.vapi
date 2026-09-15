// Vala 0.56 的字符串数组绑定缺少双层 const；在调用边界补齐 C API 的只读类型。
namespace Sshuttle.Native {
    [CCode (cname = "g_subprocess_launcher_spawnv", cheader_filename = "gio/gio.h")]
    public GLib.Subprocess spawnv (GLib.SubprocessLauncher launcher,
        [CCode (array_length = false, array_null_terminated = true, type = "const gchar * const *")] string[] argv) throws GLib.Error;

    [CCode (cname = "gtk_string_list_new", cheader_filename = "gtk/gtk.h")]
    public Gtk.StringList string_list (
        [CCode (array_length = false, array_null_terminated = true, type = "const char * const *")] string[] strings);

    [CCode (cname = "gtk_application_set_accels_for_action", cheader_filename = "gtk/gtk.h")]
    public void set_accels_for_action (Gtk.Application application, string detailed_action_name,
        [CCode (array_length = false, array_null_terminated = true, type = "const char * const *")] string[] accels);
}
