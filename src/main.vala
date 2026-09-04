int main (string[] args) {
    GLib.Environment.set_prgname (Config.APP_ID);
    GLib.Environment.set_application_name ("SShuttle");

    var app = new Sshuttle.Application ();
    return app.run (args);
}
