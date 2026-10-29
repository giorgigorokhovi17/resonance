# Sets up this PowerShell session for Android builds (docs/android-design.md): the SDK and NDK
# that Android Studio installed, its bundled JDK, and the C compilers the Rust crates need
# (SQLite, libopus). Dot-source it, then build:
#
#   . scripts\android-env.ps1
#   corepack pnpm tauri android build --debug --target aarch64 --apk
#
# Nothing here is saved; open a new window to undo it.

$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { "$env:LOCALAPPDATA\Android\Sdk" }
if (-not (Test-Path $sdk)) { throw "Android SDK not found at $sdk. Install Android Studio, or set ANDROID_HOME." }
$ndk = Get-ChildItem "$sdk\ndk" -Directory | Sort-Object { [version]$_.Name } | Select-Object -Last 1
if (-not $ndk) { throw "No NDK in $sdk\ndk. Install one from Android Studio (SDK Manager → SDK Tools → NDK)." }
$cmake = Get-ChildItem "$sdk\cmake" -Directory | Sort-Object { [version]$_.Name } | Select-Object -Last 1
$jbr = "${env:ProgramFiles}\Android\Android Studio\jbr"

$env:ANDROID_HOME = $sdk
$env:NDK_HOME = $ndk.FullName
$env:ANDROID_NDK_HOME = $ndk.FullName
$env:ANDROID_NDK_ROOT = $ndk.FullName
if (-not $env:JAVA_HOME -and (Test-Path $jbr)) { $env:JAVA_HOME = $jbr }

$bin = "$($ndk.FullName)\toolchains\llvm\prebuilt\windows-x86_64\bin"
$env:CC_aarch64_linux_android = "$bin\aarch64-linux-android34-clang.cmd"
$env:CXX_aarch64_linux_android = "$bin\aarch64-linux-android34-clang++.cmd"
$env:AR_aarch64_linux_android = "$bin\llvm-ar.exe"
$env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = "$bin\aarch64-linux-android34-clang.cmd"
# libopus builds with the SDK's CMake and Ninja (src-tauri/.cargo/config.toml keeps a set CMAKE).
$env:CMAKE = "$($cmake.FullName)\bin\cmake.exe"
$env:CMAKE_GENERATOR = "Ninja"
$env:PATH = "$($cmake.FullName)\bin;$sdk\platform-tools;$env:JAVA_HOME\bin;$env:PATH"

Write-Host "Android build environment: NDK $($ndk.Name), CMake $($cmake.Name), JDK $env:JAVA_HOME"
