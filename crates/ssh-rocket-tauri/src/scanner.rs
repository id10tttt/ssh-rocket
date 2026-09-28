use crate::types::DesktopAppDto;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

static DESKTOP_APPS_CACHE: OnceLock<Vec<DesktopAppDto>> = OnceLock::new();

pub fn scan_desktop_apps() -> Vec<DesktopAppDto> {
    DESKTOP_APPS_CACHE
        .get_or_init(scan_desktop_apps_uncached)
        .clone()
}

#[cfg(target_os = "linux")]
fn scan_desktop_apps_uncached() -> Vec<DesktopAppDto> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }

    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|val| val.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if let Some(app) = parse_desktop_app(&text) {
                if seen.insert(app.executable.clone()) {
                    apps.push(app);
                }
            }
        }
    }
    apps.sort_by_key(|app| app.name.to_lowercase());
    apps
}

#[cfg(target_os = "macos")]
fn scan_desktop_apps_uncached() -> Vec<DesktopAppDto> {
    let dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
    ];
    let mut seen = HashSet::new();
    let mut apps = Vec::new();

    for dir in dirs {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|v| v.to_str()) == Some("app") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    let executable = name.to_lowercase();
                    if seen.insert(executable.clone()) {
                        apps.push(DesktopAppDto {
                            name: name.to_string(),
                            executable,
                            icon: String::new(),
                        });
                    }
                }
            }
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn scan_desktop_apps_uncached() -> Vec<DesktopAppDto> {
    Vec::new()
}

#[cfg(target_os = "linux")]
fn parse_desktop_app(text: &str) -> Option<DesktopAppDto> {
    let mut in_entry = false;
    let mut name = String::new();
    let mut exec = String::new();
    let mut icon = String::new();
    let mut no_display = false;
    let mut app_type = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("Name=") {
            if name.is_empty() {
                name = value.trim().to_string();
            }
        } else if let Some(value) = line.strip_prefix("Exec=") {
            exec = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("Icon=") {
            icon = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("NoDisplay=") {
            no_display = value.eq_ignore_ascii_case("true");
        } else if let Some(value) = line.strip_prefix("Type=") {
            app_type = value.trim().to_string();
        }
    }
    if no_display || (!app_type.is_empty() && app_type != "Application") || exec.is_empty() {
        return None;
    }
    let executable = extract_exec_name(&exec)?;
    Some(DesktopAppDto {
        name: if name.is_empty() {
            executable.clone()
        } else {
            name
        },
        executable,
        icon,
    })
}

#[cfg(target_os = "linux")]
fn extract_exec_name(exec: &str) -> Option<String> {
    let mut parts = exec
        .split_whitespace()
        .filter(|part| !part.starts_with('%'));
    let first = parts.next()?.trim_matches(['\'', '"']);
    let command = if first.ends_with("/env") || first == "env" {
        parts.find(|part| !part.starts_with('-') && !part.contains('='))?
    } else {
        first
    };
    if command.ends_with("flatpak") || command == "flatpak" {
        let args: Vec<_> = exec.split_whitespace().collect();
        if let Some(value) = args.iter().find_map(|arg| arg.strip_prefix("--command=")) {
            return Path::new(value)
                .file_name()
                .map(|value| value.to_string_lossy().to_lowercase());
        }
        if let Some(id) = args
            .iter()
            .rev()
            .find(|arg| !arg.starts_with('-') && **arg != "run")
        {
            return id
                .rsplit('.')
                .find(|part| !matches!(*part, "desktop" | "client" | "app"))
                .map(|value| value.to_lowercase());
        }
    }
    Path::new(command)
        .file_name()
        .map(|value| value.to_string_lossy().to_lowercase())
}
