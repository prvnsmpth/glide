fn main() {
    #[cfg(target_os = "macos")]
    {
        macos_build();
    }

    #[cfg(target_os = "linux")]
    {
        linux_build();
    }
}

#[cfg(target_os = "macos")]
fn macos_build() {
    use std::process::Command;

    // Add rpath for Swift runtime from Command Line Tools
    // This is needed because screencapturekit-rs uses Swift bridging

    // Check if we're using Command Line Tools instead of full Xcode
    if let Ok(output) = Command::new("xcode-select").arg("-p").output() {
        if output.status.success() {
            let xcode_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

            // Add rpath for Command Line Tools Swift runtime
            let swift_lib_path = format!("{}/usr/lib/swift-5.0/macosx", xcode_path);
            println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_lib_path}");

            // Also try the standard swift path
            let swift_lib_path_alt = format!("{}/usr/lib/swift/macosx", xcode_path);
            println!("cargo:rustc-link-arg=-Wl,-rpath,{swift_lib_path_alt}");

            // For full Xcode installations
            let toolchain_path = format!(
                "{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx",
                xcode_path
            );
            println!("cargo:rustc-link-arg=-Wl,-rpath,{toolchain_path}");
        }
    }

    // Add system Swift path as fallback
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
}

#[cfg(target_os = "linux")]
fn linux_build() {
    // Linux build configuration
    // Link XCB libraries for X11 support
    // x11rb handles this via pkg-config, but we can add explicit paths if needed

    // Ensure XCB libraries are available
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
}
