#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct LauncherConfig {
    #[serde(default = "default_search_dirs")]
    search_dirs: Vec<String>,
    #[serde(default)]
    executable_name: Option<String>,
}

fn default_search_dirs() -> Vec<String> {
    vec!["versions".to_string(), "app".to_string(), ".".to_string()]
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            search_dirs: default_search_dirs(),
            executable_name: None,
        }
    }
}

enum CandidateType {
    Directory,
    File,
}

struct Candidate {
    version: semver::Version,
    path: PathBuf,
    candidate_type: CandidateType,
}

fn extract_version(name: &str) -> Option<semver::Version> {
    // Find the first ASCII digit
    let start_idx = name.find(|c: char| c.is_ascii_digit())?;
    let possible_ver = &name[start_idx..];

    // Try parsing the entire substring as a SemVer version
    if let Ok(ver) = semver::Version::parse(possible_ver) {
        return Some(ver);
    }

    // If it fails, try parsing up to the first character that is not valid for SemVer.
    // Valid characters include: digits, dots, hyphens, plus signs, and alphanumeric chars.
    let end_idx = possible_ver
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '+')
        .unwrap_or(possible_ver.len());

    let trimmed = &possible_ver[..end_idx];
    if let Ok(ver) = semver::Version::parse(trimmed) {
        return Some(ver);
    }

    None
}

fn show_error(title: &str, message: &str) {
    eprintln!("[{}]: {}", title, message);
    #[cfg(target_os = "windows")]
    {
        use std::ffi::c_void;
        #[link(name = "user32")]
        extern "system" {
            fn MessageBoxW(
                hwnd: *mut c_void,
                lpText: *const u16,
                lpCaption: *const u16,
                uType: u32,
            ) -> i32;
        }

        let lp_text: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
        let lp_caption: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            // MB_OK (0x00000000) | MB_ICONERROR (0x00000010)
            MessageBoxW(std::ptr::null_mut(), lp_text.as_ptr(), lp_caption.as_ptr(), 0x00000010);
        }
    }
}

fn main() {
    if let Err(e) = run_launcher() {
        show_error("启动器错误 / Launcher Error", &e);
        std::process::exit(1);
    }
}

fn run_launcher() -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("无法获取当前程序路径: {}", e))?;
    let current_dir = current_exe
        .parent()
        .ok_or_else(|| "无法获取当前程序所在目录".to_string())?;

    // Canonicalize the launcher executable path to verify matches correctly
    let canonical_current_exe = current_exe.canonicalize().unwrap_or_else(|_| current_exe.clone());

    // Load config if launcher.json exists
    let config_path = current_dir.join("launcher.json");
    let config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)
            .map_err(|e| format!("无法读取配置文件 launcher.json: {}", e))?;
        serde_json::from_str::<LauncherConfig>(&content)
            .map_err(|e| format!("配置文件 launcher.json 格式错误: {}", e))?
    } else {
        LauncherConfig::default()
    };

    let mut candidates = Vec::new();

    // Iterate through all specified search directories
    for search_dir_name in &config.search_dirs {
        let search_dir = current_dir.join(search_dir_name);
        if !search_dir.exists() || !search_dir.is_dir() {
            continue;
        }

        let entries = fs::read_dir(&search_dir)
            .map_err(|e| format!("无法读取搜索目录 {:?}: {}", search_dir, e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            let path = entry.path();
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };

            if path.is_dir() {
                if let Some(version) = extract_version(name) {
                    candidates.push(Candidate {
                        version,
                        path: path.clone(),
                        candidate_type: CandidateType::Directory,
                    });
                }
            } else if path.is_file() {
                // Check if it's an executable and matches the version pattern
                if path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("exe"))
                    .unwrap_or(false)
                {
                    // Skip the launcher itself!
                    if let Ok(canonical_path) = path.canonicalize() {
                        if canonical_path == canonical_current_exe {
                            continue;
                        }
                    } else if path == current_exe {
                        continue;
                    }

                    // Extract version from file stem (e.g. ClearClip_v1.0.0 from ClearClip_v1.0.0.exe)
                    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
                    if let Some(version) = extract_version(stem) {
                        candidates.push(Candidate {
                            version,
                            path: path.clone(),
                            candidate_type: CandidateType::File,
                        });
                    }
                }
            }
        }
    }

    if candidates.is_empty() {
        let scanned_paths_str = config
            .search_dirs
            .iter()
            .map(|d| format!("  - {:?}", current_dir.join(d)))
            .collect::<Vec<String>>()
            .join("\n");
        return Err(format!(
            "未找到任何可用的版本文件夹（如 versions/0.1.0/）或版本化程序文件（如 ClearClip_v0.1.0.exe）。\n\n已检索的路径：\n{}",
            scanned_paths_str
        ));
    }

    // Sort candidates by version descending (newest first)
    candidates.sort_by(|a, b| b.version.cmp(&a.version));

    // Try candidates in order
    for candidate in candidates {
        let exe_path = match candidate.candidate_type {
            CandidateType::File => Some(candidate.path.clone()),
            CandidateType::Directory => find_exe_in_dir(&candidate.path, &config, &current_exe),
        };

        if let Some(exe_path) = exe_path {
            if exe_path.exists() && exe_path.is_file() {
                // Found valid executable! Let's launch it.
                launch_program(&exe_path)?;
                return Ok(());
            }
        }
    }

    Err("找到了版本文件夹/文件，但未在其中找到有效的主程序可执行文件。".to_string())
}

fn find_exe_in_dir(dir: &Path, config: &LauncherConfig, current_exe: &Path) -> Option<PathBuf> {
    // 1. If configured executable_name is provided, use it
    if let Some(ref name) = config.executable_name {
        let path = dir.join(name);
        if path.exists() {
            return Some(path);
        }
        if !name.ends_with(".exe") {
            let path_exe = dir.join(format!("{}.exe", name));
            if path_exe.exists() {
                return Some(path_exe);
            }
        }
    }

    // 2. Look for an executable with the same name as the launcher
    if let Some(launcher_name) = current_exe.file_name() {
        let path = dir.join(launcher_name);
        if path.exists() {
            return Some(path);
        }
    }

    // 3. Look for "tauri-app.exe"
    let tauri_path = dir.join("tauri-app.exe");
    if tauri_path.exists() {
        return Some(tauri_path);
    }

    // 4. Look for the first .exe file (excluding files that match the launcher name)
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("exe"))
                    .unwrap_or(false)
            {
                if let Some(name) = path.file_name() {
                    if Some(name) != current_exe.file_name() {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

fn launch_program(exe_path: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(exe_path);
    cmd.args(std::env::args_os().skip(1));

    // Set current working directory to the directory of the target executable
    if let Some(exe_dir) = exe_path.parent() {
        cmd.current_dir(exe_dir);
    }

    cmd.spawn()
        .map_err(|e| format!("启动主程序失败 (路径: {:?}): {}", exe_path, e))?;

    Ok(())
}
