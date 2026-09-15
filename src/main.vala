int main (string[] args) {
    if (Posix.geteuid () == 0) {
        stderr.printf ("Run SSH Rocket as your desktop user; connection privileges are requested separately.\n");
        return 1;
    }
    GLib.Environment.set_prgname (Config.APP_ID);
    GLib.Environment.set_application_name ("SSH Rocket");

    var app = new Sshuttle.Application ();
    return app.run (args);
}
