datadir = get_option('datadir')

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
