project(
  'sshuttle-gui', ['vala', 'c'],
  version: '1.0.0',
  license: 'GPL3',
  meson_version: '>= 0.59.0',
  default_options: [
    'warning_level=1',
    'buildtype=debugoptimized',
  ]
)

app_id = 'io.github.idi0t.SshuttleGUI'
version = meson.project_version()

gnome = import('gnome')
i18n = import('i18n')

c_compiler = meson.get_compiler('c')
vala_compiler = meson.get_compiler('vala')

add_project_arguments('-DGETTEXT_PACKAGE="' + meson.project_name() + '"', language: 'c')

gtk_dep = dependency('gtk4', version: '>= 4.10')
adw_dep = dependency('libadwaita-1', version: '>= 1.4')
json_dep = dependency('json-glib-1.0')
posix_dep = vala_compiler.find_library('posix', required: false)

dependencies = [
  dependency('glib-2.0'),
  dependency('gobject-2.0'),
  dependency('gio-2.0'),
  gtk_dep,
  adw_dep,
  json_dep,
  posix_dep,
]

subdir('data')
subdir('src')

gnome.post_install(
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
