datadir = get_option('datadir')

rocket_resources = gnome.compile_resources(
  'ssh-rocket-resources',
  'ssh-rocket.gresource.xml',
  source_dir: meson.current_source_dir(),
  c_name: 'ssh_rocket',
)

install_data(
  'icons/ssh-rocket.svg',
  'icons/ssh-rocket-symbolic.svg',
  'icons/ssh-rocket-disconnected-symbolic.svg',
  'icons/ssh-rocket-acquiring-symbolic.svg',
  install_dir: datadir / 'icons/hicolor/scalable/apps',
)

desktop_conf = configuration_data()
desktop_conf.set('GUI_EXECUTABLE', get_option('prefix') / get_option('bindir') / meson.project_name())
configure_file(
  input: 'io.github.idi0t.SshuttleGUI.desktop',
  output: 'io.github.idi0t.SshuttleGUI.desktop',
  configuration: desktop_conf,
  install_dir: datadir / 'applications',
)

install_data(
  'io.github.idi0t.SshuttleGUI.metainfo.xml',
  install_dir: datadir / 'metainfo'
)

policy_conf = configuration_data()
policy_conf.set('HELPER_PATH', helper_path)
configure_file(
  input: 'io.github.idi0t.SshuttleGUI.policy',
  output: 'io.github.idi0t.SshuttleGUI.policy',
  configuration: policy_conf,
  install_dir: '/usr/share/polkit-1/actions',
)
