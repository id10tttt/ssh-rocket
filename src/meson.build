sources = [
  'main.vala',
  'application.vala',
  'config.vapi',
  'models/profile.vala',
  'models/domain-rule.vala',
  'services/command-builder.vala',
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
  'ui/log-view.vala',
  'ui/window.vala',
]

conf = configuration_data()
conf.set_quoted('APP_ID', app_id)
conf.set_quoted('CODE_NAME', meson.project_name())
conf.set_quoted('VERSION', version)
conf.set_quoted('LOCALEDIR', get_option('prefix') / get_option('localedir'))
configure_file(output: 'config.h', configuration: conf)

executable(
  meson.project_name(),
  sources,
  dependencies: dependencies,
  install: true,
)
