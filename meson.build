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

add_project_arguments('--target-glib=2.70', language: 'vala')

add_project_arguments('-DGETTEXT_PACKAGE="' + meson.project_name() + '"', language: 'c')

gtk_dep = dependency('gtk4', version: '>= 4.10')
adw_dep = dependency('libadwaita-1', version: '>= 1.5')
json_dep = dependency('json-glib-1.0')
posix_dep = vala_compiler.find_library('posix', required: false)

dependencies = [
  dependency('glib-2.0', version: '>= 2.70'),
  dependency('gobject-2.0'),
  dependency('gio-2.0'),
  dependency('gio-unix-2.0'),
  gtk_dep,
  adw_dep,
  json_dep,
  posix_dep,
]

# 仅用于 Vala 生成的 C：临时变量、异步结束函数及属性访问器的 const 转换由生成器产生。
# 保留弃用、指针类型、返回值和分配大小检查，避免掩盖可修复的源码问题。
vala_c_args = c_compiler.get_supported_arguments(
  '-Wno-unused-variable',
  '-Wno-unused-but-set-variable',
  '-Wno-unused-function',
  '-Wno-discarded-qualifiers',
)

helper_path = get_option('prefix') / get_option('libexecdir') / 'sshuttle-gui-helper'

subdir('data')
subdir('src')
runtime_tests = executable(
  'runtime-tests',
  'tests/runtime-tests.vala',
  'tests/runtime-fakes.vala',
  'src/helper-main.vala',
  'src/native-bindings.vapi',
  'src/services/runtime-client.vala',
  'src/models/domain-rule.vala',
  dependencies: dependencies,
  vala_args: ['--main=runtime_test_main', '--define=RUNTIME_TESTS'],
  c_args: vala_c_args,
  install: false,
)
test('runtime-session', runtime_tests, timeout: 30)

gnome.post_install(
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
