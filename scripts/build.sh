#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_DIR="$ROOT_DIR/android"
DIST_DIR="$ROOT_DIR/dist"
MACOS_PACKAGING_DIR="$ROOT_DIR/packaging/macos"
DEFAULT_RUSTUP_TOOLCHAIN="stable"
DEFAULT_WINDOWS_HOST_TARGET="x86_64-pc-windows-msvc"
DEFAULT_WINDOWS_CROSS_TARGET="x86_64-pc-windows-gnu"
DEFAULT_ANDROID_PACKAGES=(
  "platform-tools"
  "platforms;android-34"
  "build-tools;34.0.0"
  "ndk;26.3.11579264"
)

log() {
  printf '[build] %s\n' "$*"
}

warn() {
  printf '[build] warning: %s\n' "$*" >&2
}

die() {
  printf '[build] error: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
Usage:
  scripts/build.sh <command>

Commands:
  help               Show this help text.
  android-env        Print detected Android build environment.
  android-setup      Install required Android SDK/NDK packages with sdkmanager.
  android-wrapper    Generate Gradle Wrapper in android/ using local gradle.
  android-debug      Build the Android debug APK.
  android-release    Build the Android release APK.
  android-install    Install the Android debug APK to a connected device.
  windows-env        Print detected Windows build environment.
  windows-setup      Install the configured Windows Rust target with rustup.
  windows-release    Build the Windows release executable and archive it.
  macos-release      Build the macOS release binary, .app bundle, zip, and tar.xz.

Environment:
  CLIPLINK_RUSTUP_TOOLCHAIN
      Optional. When rustup is available, defaults to "stable" for scripted builds.
  ANDROID_HOME / ANDROID_SDK_ROOT
      Optional. Auto-detected when possible.
  ANDROID_NDK_HOME
      Optional. Auto-detected from $ANDROID_HOME/ndk/* when possible.
  CLIPLINK_WINDOWS_TARGET
      Optional. Windows Rust target triple. Defaults to "x86_64-pc-windows-msvc" on Windows hosts.
      On non-Windows hosts, auto-detects "x86_64-pc-windows-gnu" when mingw-w64 is available.

Examples:
  scripts/build.sh android-env
  scripts/build.sh android-setup
  scripts/build.sh android-debug
  scripts/build.sh android-install
  scripts/build.sh windows-env
  scripts/build.sh windows-release
  scripts/build.sh macos-release
EOF
}

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

require_command() {
  command_exists "$1" || die "Missing required command: $1"
}

ensure_rustup_path() {
  local rustup_bin_dir="$HOME/.cargo/bin"
  if [[ -d "$rustup_bin_dir" && ":$PATH:" != *":$rustup_bin_dir:"* ]]; then
    export PATH="$rustup_bin_dir:$PATH"
  fi
}

resolved_rustup_toolchain() {
  ensure_rustup_path
  if command_exists rustup; then
    printf '%s\n' "${CLIPLINK_RUSTUP_TOOLCHAIN:-$DEFAULT_RUSTUP_TOOLCHAIN}"
    return 0
  fi
  return 1
}

selected_rustup_toolchain() {
  resolved_rustup_toolchain 2>/dev/null || true
}

selected_cargo_command() {
  local toolchain
  toolchain="$(selected_rustup_toolchain)"
  if [[ -n "$toolchain" ]]; then
    printf 'rustup run %s cargo\n' "$toolchain"
  else
    printf '%s\n' "$(command -v cargo || printf '<not found>')"
  fi
}

selected_rustc_command() {
  local toolchain
  toolchain="$(selected_rustup_toolchain)"
  if [[ -n "$toolchain" ]]; then
    printf 'rustup run %s rustc\n' "$toolchain"
  else
    printf '%s\n' "$(command -v rustc || printf '<not found>')"
  fi
}

ensure_cargo_available() {
  ensure_rustup_path
  if command_exists cargo || command_exists rustup; then
    return 0
  fi
  die "Neither cargo nor rustup is available."
}

run_rustup() {
  ensure_rustup_path
  command_exists rustup || die "rustup is required for this command."
  rustup "$@"
}

run_cargo() {
  ensure_cargo_available
  local toolchain
  toolchain="$(selected_rustup_toolchain)"
  if [[ -n "$toolchain" ]]; then
    rustup run "$toolchain" cargo "$@"
  else
    cargo "$@"
  fi
}

run_rustc() {
  ensure_cargo_available
  local toolchain
  toolchain="$(selected_rustup_toolchain)"
  if [[ -n "$toolchain" ]]; then
    rustup run "$toolchain" rustc "$@"
  else
    rustc "$@"
  fi
}

is_windows_host() {
  case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
      return 0
      ;;
  esac

  [[ "${OS:-}" == "Windows_NT" ]]
}

read_cargo_package_field() {
  local field="$1"
  awk -F ' *= *' -v key="$field" '
    $0 == "[package]" { in_package=1; next }
    /^\[/ && in_package { exit }
    in_package && $1 == key {
      value = $2
      gsub(/^"/, "", value)
      gsub(/"$/, "", value)
      print value
      exit
    }
  ' "$ROOT_DIR/Cargo.toml"
}

java_major_version() {
  local java_bin="$1"
  local raw
  raw="$("$java_bin" -version 2>&1 | awk -F '"' '/version/ { print $2; exit }')"
  if [[ -z "$raw" ]]; then
    return 1
  fi
  if [[ "$raw" == 1.* ]]; then
    printf '%s\n' "${raw#1.}" | cut -d. -f1
  else
    printf '%s\n' "$raw" | cut -d. -f1
  fi
}

detect_gradle_java_home() {
  local gradle_properties="$ANDROID_DIR/gradle.properties"
  [[ -f "$gradle_properties" ]] || return 1

  awk -F '=' '
    /^[[:space:]]*org\.gradle\.java\.home[[:space:]]*=/ {
      value = substr($0, index($0, "=") + 1)
      sub(/^[[:space:]]+/, "", value)
      sub(/[[:space:]]+$/, "", value)
      gsub(/\\:/, ":", value)
      gsub(/\\\\/, "\\", value)
      print value
      exit
    }
  ' "$gradle_properties"
}

detect_java17_home() {
  local candidate major
  local -a candidates=()

  if [[ -n "${JAVA_HOME:-}" ]]; then
    candidates+=("${JAVA_HOME}")
  fi

  candidate="$(detect_gradle_java_home || true)"
  if [[ -n "$candidate" ]]; then
    candidates+=("$candidate")
  fi

  if [[ "$(uname -s)" == "Darwin" && -x /usr/libexec/java_home ]]; then
    candidate="$(/usr/libexec/java_home -v 17 2>/dev/null || true)"
    if [[ -n "$candidate" ]]; then
      candidates+=("$candidate")
    fi
  fi

  candidates+=(
    "/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home"
    "/usr/local/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home"
    "/opt/homebrew/opt/openjdk@17"
    "/usr/local/opt/openjdk@17"
  )

  for candidate in /Library/Java/JavaVirtualMachines/*17*.jdk/Contents/Home; do
    [[ -d "$candidate" ]] && candidates+=("$candidate")
  done

  for candidate in "${candidates[@]}"; do
    if [[ -n "$candidate" && -x "$candidate/bin/java" ]]; then
      major="$(java_major_version "$candidate/bin/java" || true)"
      if [[ -n "$major" && "$major" -ge 17 ]]; then
        printf '%s\n' "$candidate"
        return 0
      fi
    fi
  done

  return 1
}

ensure_java17() {
  local java_home major

  java_home="$(detect_java17_home || true)"
  if [[ -n "$java_home" && -x "$java_home/bin/java" ]]; then
    export JAVA_HOME="$java_home"
    export PATH="${JAVA_HOME}/bin:${PATH}"
    return 0
  fi

  if command_exists java; then
    major="$(java_major_version "java" || true)"
    if [[ -n "$major" && "$major" -ge 17 ]]; then
      return 0
    fi
  fi

  die "JDK 17 is required for Android builds. Install JDK 17 and ensure JAVA_HOME or android/gradle.properties points to it."
}

detect_android_home() {
  local candidates=(
    "${ANDROID_HOME:-}"
    "${ANDROID_SDK_ROOT:-}"
    "${HOME}/Library/Android/sdk"
    "/opt/homebrew/share/android-sdk"
  )
  local candidate
  for candidate in "${candidates[@]}"; do
    if [[ -n "$candidate" && -d "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

detect_sdkmanager() {
  local android_home="$1"
  local candidates=(
    "$android_home/cmdline-tools/latest/bin/sdkmanager"
    "$android_home/cmdline-tools/bin/sdkmanager"
  )
  local candidate
  for candidate in "${candidates[@]}"; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

detect_android_ndk_home() {
  local android_home="$1"
  local candidate

  if [[ -n "${ANDROID_NDK_HOME:-}" && -d "${ANDROID_NDK_HOME}" ]]; then
    printf '%s\n' "${ANDROID_NDK_HOME}"
    return 0
  fi

  for candidate in "$android_home"/ndk/*; do
    if [[ -d "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

export_android_env() {
  local android_home android_ndk_home
  android_home="$(detect_android_home)" || die "Android SDK not found. Set ANDROID_HOME or ANDROID_SDK_ROOT."
  android_ndk_home="$(detect_android_ndk_home "$android_home")" || die "Android NDK not found under $android_home/ndk. Install it first."

  export ANDROID_HOME="$android_home"
  export ANDROID_SDK_ROOT="$android_home"
  export ANDROID_NDK_HOME="$android_ndk_home"
  export CLIPLINK_RUSTUP_TOOLCHAIN="${CLIPLINK_RUSTUP_TOOLCHAIN:-stable}"
}

print_android_env() {
  local android_home sdkmanager android_ndk_home selected_toolchain detected_java17_home
  android_home="$(detect_android_home || true)"
  sdkmanager=""
  android_ndk_home=""
  selected_toolchain="$(selected_rustup_toolchain)"
  detected_java17_home="$(detect_java17_home || true)"

  if [[ -n "$android_home" ]]; then
    sdkmanager="$(detect_sdkmanager "$android_home" || true)"
    android_ndk_home="$(detect_android_ndk_home "$android_home" || true)"
  fi

  printf 'ROOT_DIR=%s\n' "$ROOT_DIR"
  printf 'ANDROID_DIR=%s\n' "$ANDROID_DIR"
  printf 'ANDROID_HOME=%s\n' "${android_home:-<not found>}"
  printf 'ANDROID_NDK_HOME=%s\n' "${android_ndk_home:-<not found>}"
  printf 'SDKMANAGER=%s\n' "${sdkmanager:-<not found>}"
  printf 'JAVA_HOME=%s\n' "${JAVA_HOME:-<unset>}"
  printf 'JAVA17_HOME=%s\n' "${detected_java17_home:-<not found>}"
  printf 'CLIPLINK_RUSTUP_TOOLCHAIN=%s\n' "${selected_toolchain:-<not using rustup>}"
  printf 'CARGO=%s\n' "$(selected_cargo_command)"
  printf 'RUSTC=%s\n' "$(selected_rustc_command)"
  printf 'GRADLE=%s\n' "$(command -v gradle || printf '<not found>')"
  printf 'GRADLEW=%s\n' "$( [[ -x "$ANDROID_DIR/gradlew" ]] && printf '%s' "$ANDROID_DIR/gradlew" || printf '<not found>' )"
}

cargo_version_supports_edition_2024() {
  local version
  version="$1"
  if [[ -z "$version" ]]; then
    return 1
  fi
  local major minor
  major="${version%%.*}"
  minor="${version#*.}"
  minor="${minor%%.*}"
  [[ "$major" -gt 1 || ( "$major" -eq 1 && "$minor" -ge 85 ) ]]
}

gnu_tool_prefix_for_target() {
  local target="$1"
  case "$target" in
    x86_64-pc-windows-gnu)
      printf 'x86_64-w64-mingw32\n'
      ;;
    *)
      return 1
      ;;
  esac
}

detect_windows_gnu_binary() {
  local tool="$1"
  local target="${2:-$DEFAULT_WINDOWS_CROSS_TARGET}"
  local prefix
  prefix="$(gnu_tool_prefix_for_target "$target" || true)"
  [[ -n "$prefix" ]] || return 1

  local candidate
  for candidate in \
    "${prefix}-${tool}" \
    "/opt/homebrew/bin/${prefix}-${tool}" \
    "/usr/local/bin/${prefix}-${tool}"
  do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

detect_windows_target() {
  if [[ -n "${CLIPLINK_WINDOWS_TARGET:-}" ]]; then
    printf '%s\n' "$CLIPLINK_WINDOWS_TARGET"
    return 0
  fi

  if is_windows_host; then
    printf '%s\n' "$DEFAULT_WINDOWS_HOST_TARGET"
    return 0
  fi

  if detect_windows_gnu_binary gcc >/dev/null 2>&1; then
    printf '%s\n' "$DEFAULT_WINDOWS_CROSS_TARGET"
    return 0
  fi

  return 1
}

windows_target_env_prefix() {
  local target="$1"
  printf '%s' "$target" | tr '[:lower:]-.' '[:upper:]__'
}

windows_target_cc_env_prefix() {
  local target="$1"
  printf '%s' "$target" | tr '[:upper:]-.' '[:lower:]__'
}

configure_windows_gnu_env() {
  local target="$1"
  local env_prefix cc_prefix linker_var ar_var cc_var cxx_var ar_cc_var
  local compiler cxx_compiler archive_tool archive_tool_alt

  env_prefix="$(windows_target_env_prefix "$target")"
  cc_prefix="$(windows_target_cc_env_prefix "$target")"
  linker_var="CARGO_TARGET_${env_prefix}_LINKER"
  ar_var="CARGO_TARGET_${env_prefix}_AR"
  cc_var="CC_${cc_prefix}"
  cxx_var="CXX_${cc_prefix}"
  ar_cc_var="AR_${cc_prefix}"

  if [[ -z "${!linker_var:-}" ]]; then
    compiler="$(detect_windows_gnu_binary gcc "$target" || true)"
    [[ -n "$compiler" ]] && export "${linker_var}=${compiler}"
  fi

  if [[ -z "${!cc_var:-}" ]]; then
    compiler="$(detect_windows_gnu_binary gcc "$target" || true)"
    [[ -n "$compiler" ]] && export "${cc_var}=${compiler}"
  fi

  if [[ -z "${!cxx_var:-}" ]]; then
    cxx_compiler="$(detect_windows_gnu_binary g++ "$target" || true)"
    [[ -n "$cxx_compiler" ]] && export "${cxx_var}=${cxx_compiler}"
  fi

  if [[ -z "${!ar_var:-}" ]]; then
    archive_tool="$(detect_windows_gnu_binary gcc-ar "$target" || true)"
    archive_tool_alt="$(detect_windows_gnu_binary ar "$target" || true)"
    if [[ -n "$archive_tool" ]]; then
      export "${ar_var}=${archive_tool}"
    elif [[ -n "$archive_tool_alt" ]]; then
      export "${ar_var}=${archive_tool_alt}"
    fi
  fi

  if [[ -z "${!ar_cc_var:-}" ]]; then
    archive_tool="$(detect_windows_gnu_binary gcc-ar "$target" || true)"
    archive_tool_alt="$(detect_windows_gnu_binary ar "$target" || true)"
    if [[ -n "$archive_tool" ]]; then
      export "${ar_cc_var}=${archive_tool}"
    elif [[ -n "$archive_tool_alt" ]]; then
      export "${ar_cc_var}=${archive_tool_alt}"
    fi
  fi

  export PKG_CONFIG_ALLOW_CROSS=1
}

ensure_windows_toolchain() {
  local target="$1"
  ensure_cargo_available

  if ! cargo_version_supports_edition_2024 "$(run_cargo -V | awk '{ print $2 }')"; then
    die "The active cargo is too old for edition 2024. Use a newer cargo before building Windows artifacts."
  fi

  local toolchain
  toolchain="$(selected_rustup_toolchain)"
  if [[ -n "$toolchain" ]]; then
    if ! run_rustup target list --installed --toolchain "$toolchain" | grep -qx "$target"; then
      die "Rust target $target is missing. Run: rustup target add $target"
    fi
  fi

  if ! is_windows_host && [[ -z "${CLIPLINK_WINDOWS_TARGET:-}" ]]; then
    die "Windows builds are auto-configured only on Windows hosts. For cross-compilation, set CLIPLINK_WINDOWS_TARGET explicitly and configure the target linker/toolchain yourself."
  fi

  if ! is_windows_host && [[ "$target" == *"-gnu" ]]; then
    configure_windows_gnu_env "$target"
    local env_prefix
    env_prefix="$(windows_target_env_prefix "$target")"
    local linker_var="CARGO_TARGET_${env_prefix}_LINKER"
    if [[ -z "${!linker_var:-}" ]]; then
      die "GNU Windows linker was not found for $target. Install mingw-w64 or set $linker_var manually."
    fi
    return 0
  fi

  if [[ "$target" == *"-msvc" ]] && ! is_windows_host; then
    warn "Target $target usually requires an external Windows linker toolchain on non-Windows hosts."
  fi
}

print_windows_env() {
  local target host linker_var linker_value ar_var ar_value rustup_value suggested_gnu_linker
  target="$(detect_windows_target || true)"
  if command_exists rustc || command_exists rustup; then
    host="$(run_rustc -Vv 2>/dev/null | awk -F ': ' '/^host:/ { value = $2 } END { if (value != "") print value }')"
  else
    host=""
  fi
  linker_var="<not available>"
  linker_value="<unset>"
  ar_var="<not available>"
  ar_value="<unset>"
  suggested_gnu_linker="$(detect_windows_gnu_binary gcc "${target:-$DEFAULT_WINDOWS_CROSS_TARGET}" || true)"

  if [[ -n "$target" ]]; then
    local env_prefix
    env_prefix="$(windows_target_env_prefix "$target")"
    linker_var="CARGO_TARGET_${env_prefix}_LINKER"
    ar_var="CARGO_TARGET_${env_prefix}_AR"
    linker_value="${!linker_var:-<unset>}"
    ar_value="${!ar_var:-<unset>}"
  fi

  rustup_value="$(selected_rustup_toolchain)"

  printf 'ROOT_DIR=%s\n' "$ROOT_DIR"
  printf 'DIST_DIR=%s\n' "$DIST_DIR"
  printf 'HOST_OS=%s\n' "$(uname -s)"
  printf 'RUST_HOST=%s\n' "${host:-<unknown>}"
  printf 'WINDOWS_TARGET=%s\n' "${target:-<not configured>}"
  printf 'CLIPLINK_WINDOWS_TARGET=%s\n' "${CLIPLINK_WINDOWS_TARGET:-<unset>}"
  printf 'CLIPLINK_RUSTUP_TOOLCHAIN=%s\n' "${rustup_value:-<not using rustup>}"
  printf 'CARGO=%s\n' "$(selected_cargo_command)"
  printf 'RUSTC=%s\n' "$(selected_rustc_command)"
  printf 'RUSTUP=%s\n' "$(command -v rustup || printf '<not found>')"
  if [[ "$linker_var" == "<not available>" ]]; then
    printf 'WINDOWS_LINKER_ENV=%s\n' "$linker_var"
    printf 'WINDOWS_AR_ENV=%s\n' "$ar_var"
  else
    printf '%s=%s\n' "$linker_var" "$linker_value"
    printf '%s=%s\n' "$ar_var" "$ar_value"
  fi
  printf 'WINDOWS_GNU_LINKER_DETECTED=%s\n' "${suggested_gnu_linker:-<not found>}"
}

windows_setup() {
  local target toolchain
  toolchain="$(selected_rustup_toolchain)"
  [[ -n "$toolchain" ]] || die "rustup is required to install Windows Rust targets."
  target="$(detect_windows_target)" || die "Unable to determine a Windows target. Set CLIPLINK_WINDOWS_TARGET explicitly."
  log "Installing Rust target $target for toolchain $toolchain"
  run_rustup target add --toolchain "$toolchain" "$target"

  if ! is_windows_host && [[ "$target" == *"-gnu" ]]; then
    configure_windows_gnu_env "$target"
    if ! detect_windows_gnu_binary gcc "$target" >/dev/null 2>&1; then
      warn "GNU Windows linker was not detected. Install mingw-w64 and rerun windows-env before windows-release."
    fi
  fi
}

create_zip_archive() {
  local input_path="$1"
  local output_path="$2"
  rm -f "$output_path"

  if command_exists zip; then
    (
      cd "$(dirname "$input_path")"
      zip -q "$output_path" "$(basename "$input_path")"
    )
    return 0
  fi

  if command_exists pwsh; then
    pwsh -NoProfile -Command \
      "Compress-Archive -Path '$input_path' -DestinationPath '$output_path' -Force" >/dev/null
    return 0
  fi

  if command_exists powershell.exe; then
    powershell.exe -NoProfile -Command \
      "Compress-Archive -Path '$input_path' -DestinationPath '$output_path' -Force" >/dev/null
    return 0
  fi

  warn "Skipping zip archive creation because neither zip nor PowerShell Compress-Archive is available."
  return 1
}

create_tar_xz_archive() {
  local input_path="$1"
  local output_path="$2"
  rm -f "$output_path"

  if command_exists tar; then
    tar -C "$(dirname "$input_path")" -cJf "$output_path" "$(basename "$input_path")"
    return 0
  fi

  warn "Skipping tar.xz archive creation because tar is not available."
  return 1
}

ensure_android_rust_toolchain() {
  local toolchain
  toolchain="$(selected_rustup_toolchain)"
  if [[ -n "$toolchain" ]]; then
    if ! run_rustup target list --installed --toolchain "$toolchain" | grep -qx 'aarch64-linux-android'; then
      die "Rust target aarch64-linux-android is missing for rustup toolchain $toolchain. Run: rustup target add --toolchain $toolchain aarch64-linux-android"
    fi
    if ! cargo_version_supports_edition_2024 "$(run_cargo -V | awk '{ print $2 }')"; then
      die "rustup toolchain $toolchain is too old for edition 2024. Update Rust with: rustup update $toolchain"
    fi
    return 0
  fi

  ensure_cargo_available
  if ! cargo_version_supports_edition_2024 "$(run_cargo -V | awk '{ print $2 }')"; then
    die "The active cargo is too old for edition 2024. Use a newer cargo or install rustup."
  fi
}

gradle_launcher() {
  if [[ -x "$ANDROID_DIR/gradlew" ]]; then
    printf '%s\n' "$ANDROID_DIR/gradlew"
    return 0
  fi
  if command_exists gradle; then
    printf 'gradle\n'
    return 0
  fi
  return 1
}

run_android_gradle() {
  local launcher
  launcher="$(gradle_launcher)" || die "Neither android/gradlew nor gradle is available."
  (
    cd "$ANDROID_DIR"
    "$launcher" "$@"
  )
}

find_android_apk() {
  local build_type="$1"
  local -a candidates=(
    "$ANDROID_DIR/app/build/outputs/apk/$build_type/app-$build_type.apk"
    "$ANDROID_DIR/app/build/outputs/apk/$build_type/app-$build_type-unsigned.apk"
  )
  local candidate
  for candidate in "${candidates[@]}"; do
    if [[ -f "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

android_setup() {
  ensure_java17
  local android_home sdkmanager
  android_home="$(detect_android_home)" || die "Android SDK not found. Set ANDROID_HOME or ANDROID_SDK_ROOT."
  sdkmanager="$(detect_sdkmanager "$android_home")" || die "sdkmanager not found under $android_home/cmdline-tools."
  log "Installing Android SDK packages into $android_home"
  "$sdkmanager" --sdk_root="$android_home" "${DEFAULT_ANDROID_PACKAGES[@]}"
}

android_wrapper() {
  ensure_java17
  if [[ -x "$ANDROID_DIR/gradlew" ]]; then
    log "Gradle Wrapper already exists at $ANDROID_DIR/gradlew"
    return 0
  fi
  require_command gradle
  (
    cd "$ANDROID_DIR"
    gradle wrapper
  )
  log "Gradle Wrapper generated at $ANDROID_DIR/gradlew"
}

android_build() {
  local task="$1"
  ensure_java17
  export_android_env
  ensure_android_rust_toolchain
  run_android_gradle "$task"
}

android_install() {
  ensure_java17
  export_android_env
  ensure_android_rust_toolchain
  require_command adb
  run_android_gradle ":app:installDebug"
}

windows_release() {
  local target package_name binary_name source_exe dist_exe zip_path tar_path

  target="$(detect_windows_target)" || die "Unable to determine a Windows target. Set CLIPLINK_WINDOWS_TARGET explicitly."
  ensure_windows_toolchain "$target"

  package_name="$(read_cargo_package_field "name")"
  [[ -n "$package_name" ]] || die "Failed to read package name from Cargo.toml"
  binary_name="${package_name}.exe"
  source_exe="$ROOT_DIR/target/$target/release/$binary_name"
  dist_exe="$DIST_DIR/${package_name}-windows-$target.exe"
  zip_path="$DIST_DIR/${package_name}-windows-$target.zip"
  tar_path="$DIST_DIR/${package_name}-windows-$target.tar.xz"

  log "Building Windows release binary for $target"
  run_cargo build --release --target "$target"

  [[ -f "$source_exe" ]] || die "Missing Windows executable at $source_exe"

  mkdir -p "$DIST_DIR"
  cp "$source_exe" "$dist_exe"
  create_zip_archive "$dist_exe" "$zip_path" || true
  create_tar_xz_archive "$dist_exe" "$tar_path" || true

  log "Created $dist_exe"
  [[ -f "$zip_path" ]] && log "Created $zip_path"
  [[ -f "$tar_path" ]] && log "Created $tar_path"
}

read_plist_value() {
  local key="$1"
  local plist="$2"
  /usr/libexec/PlistBuddy -c "Print :$key" "$plist"
}

macos_release() {
  ensure_cargo_available
  require_command tar
  require_command ditto
  local info_plist bundle_name executable_name arch bundle_dir zip_path tar_path

  info_plist="$MACOS_PACKAGING_DIR/Info.plist"
  [[ -f "$info_plist" ]] || die "Missing macOS Info.plist at $info_plist"

  bundle_name="$(read_plist_value "CFBundleName" "$info_plist")"
  executable_name="$(read_plist_value "CFBundleExecutable" "$info_plist")"
  arch="$(uname -m)"

  log "Building macOS release binary"
  run_cargo build --release

  mkdir -p "$DIST_DIR"
  bundle_dir="$DIST_DIR/$bundle_name.app"
  zip_path="$DIST_DIR/$bundle_name-macos-$arch.zip"
  tar_path="$DIST_DIR/$bundle_name-macos-$arch.tar.xz"

  rm -rf "$bundle_dir"
  mkdir -p "$bundle_dir/Contents/MacOS"
  cp "$info_plist" "$bundle_dir/Contents/Info.plist"
  cp "$ROOT_DIR/target/release/$executable_name" "$bundle_dir/Contents/MacOS/$executable_name"
  chmod +x "$bundle_dir/Contents/MacOS/$executable_name"

  rm -f "$zip_path" "$tar_path"
  ditto -c -k --sequesterRsrc --keepParent "$bundle_dir" "$zip_path"
  tar -C "$DIST_DIR" -cJf "$tar_path" "$(basename "$bundle_dir")"

  log "Created $bundle_dir"
  log "Created $zip_path"
  log "Created $tar_path"
}

main() {
  local command="${1:-help}"
  case "$command" in
    help|-h|--help)
      usage
      ;;
    android-env)
      print_android_env
      ;;
    android-setup)
      android_setup
      ;;
    android-wrapper)
      android_wrapper
      ;;
    android-debug)
      android_build ":app:assembleDebug"
      log "APK: $(find_android_apk debug || printf '%s' "$ANDROID_DIR/app/build/outputs/apk/debug")"
      ;;
    android-release)
      android_build ":app:assembleRelease"
      log "APK: $(find_android_apk release || printf '%s' "$ANDROID_DIR/app/build/outputs/apk/release")"
      ;;
    android-install)
      android_install
      ;;
    windows-env)
      print_windows_env
      ;;
    windows-setup)
      windows_setup
      ;;
    windows-release)
      windows_release
      ;;
    macos-release)
      macos_release
      ;;
    *)
      usage
      die "Unknown command: $command"
      ;;
  esac
}

main "$@"
