sources = [
  'main.vala',
  'application.vala',
  'config.vapi',
  'models/profile.vala',
  'services/command-builder.vala',
  'services/config-manager.vala',
  'services/tunnel-manager.vala',
  'ui/connection-card.vala',
  'ui/profile-editor.vala',
  'ui/rules-view.vala',
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
