sources = [
  'main.vala',
  'application.vala',
  'config.vapi',
  'native-bindings.vapi',
  'models/profile.vala',
  'models/domain-rule.vala',
  'services/command-builder.vala',
  'services/runtime-client.vala',
  'services/config-manager.vala',
  'services/tunnel-manager.vala',
  'services/tray-manager.vala',
  'services/cgroup-manager.vala',
  'services/nft-manager.vala',
  'services/app-scanner.vala',
  'services/process-monitor.vala',
  'services/omega-importer.vala',
  'services/dns-proxy.vala',
  'ui/connection-card.vala',
  'ui/profile-editor.vala',
  'ui/rules-view.vala',
  'ui/app-rules-view.vala',
  'ui/domain-rules-view.vala',
  'ui/blacklist-rules-view.vala',
  'ui/log-view.vala',
  'ui/window.vala',
]

conf = configuration_data()
conf.set_quoted('APP_ID', app_id)
conf.set_quoted('HELPER_PATH', helper_path)
conf.set_quoted('NFT_PATH', find_program('nft').full_path())
conf.set_quoted('PKEXEC_PATH', find_program('pkexec').full_path())
conf.set_quoted('SSHUTTLE_PATH', find_program('sshuttle').full_path())
conf.set_quoted('CODE_NAME', meson.project_name())
conf.set_quoted('VERSION', version)
conf.set_quoted('LOCALEDIR', get_option('prefix') / get_option('localedir'))
configure_file(output: 'config.h', configuration: conf)

executable(
  meson.project_name(),
  sources,
  dependencies: dependencies,
  c_args: vala_c_args,
  install: true,
)

executable(
  'sshuttle-gui-helper',
  'helper-main.vala',
  'config.vapi',
  'native-bindings.vapi',
  'models/domain-rule.vala',
  'services/runtime-client.vala',
  'services/cgroup-manager.vala',
  'services/nft-manager.vala',
  dependencies: dependencies,
  c_args: vala_c_args,
  install: true,
  install_dir: get_option('libexecdir'),
)
