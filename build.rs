use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "unknown".to_string());
    println!("Building for target OS: {}", target_os);

    if target_os == "windows" {
        setup_windows_build();
    } else if target_os == "linux" || target_os == "macos" || target_os == "freebsd" {
        setup_unix_build();
    } else {
        println!(
            "cargo:warning=Building for unsupported OS: {}. Some features may not work.",
            target_os
        );
    }
}

fn setup_windows_build() {
    // Check for required Windows tools
    let has_powershell = Command::new("where")
        .arg("powershell.exe")
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if !has_powershell {
        println!(
            "cargo:warning=PowerShell not found. Some Windows-specific features may not work."
        );
    }

    // Check for VirtualBox
    let has_virtualbox = Command::new("where")
        .arg("VBoxManage.exe")
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if has_virtualbox {
        println!("cargo:rustc-cfg=feature=\"virtualbox\"");
    } else {
        println!(
            "cargo:warning=VirtualBox not found. VirtualBox-specific features will be disabled."
        );
    }


    // Check for 7-Zip
    let seven_zip_paths = [
        "C:\\Program Files\\7-Zip\\7z.exe",
        "C:\\Program Files (x86)\\7-Zip\\7z.exe",
    ];

    let has_7zip = seven_zip_paths.iter().any(|path| Path::new(path).exists());

    if has_7zip {
        println!("cargo:rustc-cfg=feature=\"7zip\"");
    } else {
        println!("cargo:warning=7-Zip not found. ISO extraction will use PowerShell mount/dismount instead.");
    }
}

fn setup_unix_build() {
    // Check for required Unix tools

    // Check for xorriso
    let has_xorriso = Command::new("which")
        .arg("xorriso")
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if has_xorriso {
        println!("cargo:rustc-cfg=feature=\"xorriso\"");
    } else {
        println!("cargo:warning=xorriso not found. ISO creation/extraction may not work properly.");
    }


    // Check for VirtualBox on Unix
    let has_virtualbox = Command::new("which")
        .arg("VBoxManage")
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if has_virtualbox {
        println!("cargo:rustc-cfg=feature=\"virtualbox\"");
    } else {
        println!(
            "cargo:warning=VirtualBox not found. VirtualBox-specific features will be disabled."
        );
    }

    // Create a config for detected tools
    let config_path = env::var("OUT_DIR").expect("No OUT_DIR") + "/platform_config.rs";
    let mut config_content = String::new();

    config_content.push_str(&format!("pub const HAS_XORRISO: bool = {};\n", has_xorriso));
    config_content.push_str(&format!(
        "pub const HAS_VIRTUALBOX: bool = {};\n",
        has_virtualbox
    ));

    fs::write(config_path, config_content).expect("Failed to write platform config");
}
