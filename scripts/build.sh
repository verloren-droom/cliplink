#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DEFAULT_BUILD_CONFIG_PATH="$SCRIPT_DIR/build.env"
SELECTED_BUILD_CONFIG_PATH="${BUILD_CONFIG:-$DEFAULT_BUILD_CONFIG_PATH}"
LOADED_BUILD_CONFIG_PATH=""

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

read_var() {
  local var_name="$1"
  printf '%s\n' "${!var_name:-}"
}

read_var_or_default() {
  local var_name="$1"
  local default_value="$2"
  local value="${!var_name:-}"
  printf '%s\n' "${value:-$default_value}"
}

read_existing_dir_var() {
  local var_name="$1"
  local value="${!var_name:-}"
  [[ -n "$value" && -d "$value" ]] || return 1
  printf '%s\n' "$value"
}

read_first_existing_dir_var() {
  local var_name
  local value

  for var_name in "$@"; do
    value="${!var_name:-}"
    if [[ -n "$value" && -d "$value" ]]; then
      printf '%s\n' "$value"
      return 0
    fi
  done

  return 1
}

build_config_path_for_messages() {
  if [[ -n "$LOADED_BUILD_CONFIG_PATH" ]]; then
    printf '%s\n' "$LOADED_BUILD_CONFIG_PATH"
  else
    printf '%s\n' "$SELECTED_BUILD_CONFIG_PATH"
  fi
}

load_build_config_file() {
  if [[ -n "${BUILD_CONFIG:-}" && ! -f "$SELECTED_BUILD_CONFIG_PATH" ]]; then
    die "BUILD_CONFIG points to a missing file: $SELECTED_BUILD_CONFIG_PATH"
  fi

  if [[ -f "$SELECTED_BUILD_CONFIG_PATH" ]]; then
    # shellcheck source=/dev/null
    source "$SELECTED_BUILD_CONFIG_PATH"
    LOADED_BUILD_CONFIG_PATH="$SELECTED_BUILD_CONFIG_PATH"
  fi
}

load_build_config_file

ANDROID_DIR="$(read_var_or_default ANDROID_DIR "$ROOT_DIR/android")"
DIST_DIR="$(read_var_or_default DIST_DIR "$ROOT_DIR/dist")"
PACKAGING_DIR="$(read_var_or_default PACKAGING_DIR "$ROOT_DIR/packaging")"
DEFAULT_RUSTUP_TOOLCHAIN="stable"
WINDOWS_HOST_TARGET="$(read_var_or_default WINDOWS_HOST_TARGET "x86_64-pc-windows-msvc")"
WINDOWS_CROSS_TARGET="$(read_var_or_default WINDOWS_CROSS_TARGET "x86_64-pc-windows-gnu")"

if declare -p ANDROID_SDK_PACKAGES >/dev/null 2>&1; then
  DEFAULT_ANDROID_PACKAGES=("${ANDROID_SDK_PACKAGES[@]}")
else
  DEFAULT_ANDROID_PACKAGES=(
    "platform-tools"
    "platforms;android-34"
    "build-tools;34.0.0"
    "ndk;26.3.11579264"
  )
fi

usage() {
  cat <<'EOF'
Usage:
  scripts/build.sh <command>

Commands:
  help
      Show this help text.

  clean
      Output:
        Removes local build outputs and caches:
        target/, dist/, android/.gradle, android/build, android/app/build

  android-env
      Print resolved Android inputs: SDK / NDK paths, Java, rustup toolchain,
      cargo/rustc, Gradle / Gradle Wrapper.

  android-setup
      Input:
        ANDROID_HOME or ANDROID_SDK_ROOT
      Output:
        Installs required Android SDK packages into the detected SDK directory.

  android-wrapper
      Input:
        JDK 17, local gradle
      Output:
        Generates android/gradlew and wrapper files.

  android-debug
      Input:
        Android SDK / NDK, JDK 17, Rust Android target, cargo-ndk
        adb, connected device
      Output:
        android/app/build/outputs/apk/debug/app-debug.apk
        Installs the Android debug app on the current device user.

  android-release
      Input:
        Android SDK / NDK, JDK 17, Rust Android target, cargo-ndk
        Optional local install: adb, connected device
        Optional signing: ANDROID_KEYSTORE_PATH, ANDROID_KEYSTORE_PASSWORD,
        ANDROID_KEY_ALIAS, ANDROID_KEY_PASSWORD
      Output:
        dist/<package>-<version>-android-<abi>.apk
        Installs the signed Android release app on the current device user
        when adb and a connected device are available.
        or dist/<package>-<version>-android-<abi>-unsigned.apk

  windows-env
      Print resolved Windows inputs: target triple, rust host, rustup toolchain,
      linker / archiver environment variables, detected GNU linker.

  windows-setup
      Input:
        rustup, resolved Windows target
      Output:
        Installs the configured Rust target and reports linker readiness.

  windows-release
      Input:
        Windows Rust target and toolchain
      Output:
        dist/<package>-<version>-windows-<target>.exe

  macos-release
      Input:
        Local Rust toolchain, Cargo package metadata, app identity constants
        Optional signing: MACOS_SIGNING_IDENTITY, MACOS_ENTITLEMENTS_PATH
      Output:
        dist/<bundle-name>.app

Environment:
  BUILD_CONFIG
      Optional. Path to a local shell config file. If unset, the script will
      try scripts/build.env.
  JAVA_HOME
      Required for Android builds. The config file value is used first, then the system environment.
  ANDROID_HOME / ANDROID_SDK_ROOT
      Required for Android builds. The config file value is used first, then the system environment.
  ANDROID_NDK_HOME
      Required for Android builds. The config file value is used first, then the system environment.
  WINDOWS_TARGET
      Optional. Windows Rust target triple. Defaults to the host target on Windows.
      On non-Windows hosts, auto-detects the configured GNU cross target when mingw-w64 is available.

  RUSTUP_TOOLCHAIN
      Optional. When rustup is available, defaults to "stable" for scripted builds.
  ANDROID_DIR / DIST_DIR / PACKAGING_DIR
      Optional. Override default repository-relative directories.
  ANDROID_KEYSTORE_PATH / ANDROID_KEYSTORE_PASSWORD / ANDROID_KEY_ALIAS / ANDROID_KEY_PASSWORD
      Optional. Enable a signed Android release APK when all four are set.
      If ANDROID_KEYSTORE_PATH is unset, the script will try packaging/android/release.keystore
      and packaging/android/release.jks.
  MACOS_SIGNING_IDENTITY / MACOS_ENTITLEMENTS_PATH
      Optional. Enable codesign for the macOS app bundle when a signing identity is available.
      If MACOS_ENTITLEMENTS_PATH is unset, the script will try packaging/macos/entitlements.plist.
  MACOS_BUNDLE_NAME / MACOS_BUNDLE_IDENTIFIER / MACOS_EXECUTABLE_NAME
      Optional. Override generated macOS Info.plist identity fields.
  MACOS_BUNDLE_VERSION / MACOS_BUILD_VERSION / MACOS_DEVELOPMENT_REGION / MACOS_MINIMUM_SYSTEM_VERSION
      Optional. Override generated macOS Info.plist version and platform fields.
  WINDOWS_HOST_TARGET / WINDOWS_CROSS_TARGET
      Optional. Override the default Windows target triples.
  ANDROID_SDK_PACKAGES
      Optional. In a sourced config file only, override the Android SDK package
      list as a bash array.
  CARGO_TARGET_<WINDOWS_TARGET>_LINKER / CARGO_TARGET_<WINDOWS_TARGET>_AR
      Optional. Override linker and archiver when cross-compiling Windows targets.
EOF
}

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

require_command() {
  command_exists "$1" || die "Missing required command: $1"
}

remove_dir_if_exists() {
  local dir_path="$1"
  [[ -n "$dir_path" ]] || return 0
  [[ -d "$dir_path" ]] || return 0

  case "$dir_path" in
    /|"$ROOT_DIR")
      die "Refusing to remove unsafe path: $dir_path"
      ;;
  esac

  rm -rf "$dir_path"
  log "Removed $dir_path"
}

first_existing_file_path() {
  local file_path

  for file_path in "$@"; do
    if [[ -f "$file_path" ]]; then
      printf '%s\n' "$file_path"
      return 0
    fi
  done

  return 1
}

resolve_cargo_target_dir() {
  read_var_or_default CARGO_TARGET_DIR "$ROOT_DIR/target"
}

find_generated_release_build_output() {
  local file_name="$1"
  local target_dir
  target_dir="$(resolve_cargo_target_dir)"

  [[ -d "$target_dir/release/build" ]] || return 1
  find "$target_dir/release/build" -path "*/out/$file_name" -type f | head -n 1
}

copy_file_to_dist() {
  local source_path="$1"
  local target_name="$2"
  local target_path="$DIST_DIR/$target_name"

  [[ -f "$source_path" ]] || die "Missing source file: $source_path"

  mkdir -p "$DIST_DIR"
  rm -f "$target_path"
  cp "$source_path" "$target_path"

  printf '%s\n' "$target_path"
}

print_common_build_context() {
  printf 'ROOT_DIR=%s\n' "$ROOT_DIR"
  printf 'BUILD_CONFIG=%s\n' "${LOADED_BUILD_CONFIG_PATH:-<not loaded>}"
  printf 'ANDROID_DIR=%s\n' "$ANDROID_DIR"
  printf 'DIST_DIR=%s\n' "$DIST_DIR"
  printf 'PACKAGING_DIR=%s\n' "$PACKAGING_DIR"
}

ensure_rustup_path() {
  local rustup_bin_dir="$HOME/.cargo/bin"
  if [[ -d "$rustup_bin_dir" && ":$PATH:" != *":$rustup_bin_dir:"* ]]; then
    export PATH="$rustup_bin_dir:$PATH"
  fi
}

resolve_rustup_toolchain() {
  ensure_rustup_path
  if command_exists rustup; then
    read_var_or_default RUSTUP_TOOLCHAIN "$DEFAULT_RUSTUP_TOOLCHAIN"
    return 0
  fi
  return 1
}

active_rustup_toolchain() {
  resolve_rustup_toolchain 2>/dev/null || true
}

describe_toolchain_binary() {
  local binary="$1"
  local toolchain

  toolchain="$(active_rustup_toolchain)"
  if [[ -n "$toolchain" ]]; then
    printf 'rustup run %s %s\n' "$toolchain" "$binary"
  else
    printf '%s\n' "$(command -v "$binary" || printf '<not found>')"
  fi
}

describe_cargo_binary() {
  describe_toolchain_binary cargo
}

describe_rustc_binary() {
  describe_toolchain_binary rustc
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

run_with_active_toolchain() {
  local binary="$1"
  shift

  ensure_cargo_available
  local toolchain
  toolchain="$(active_rustup_toolchain)"
  if [[ -n "$toolchain" ]]; then
    rustup run "$toolchain" "$binary" "$@"
  else
    "$binary" "$@"
  fi
}

run_cargo() {
  run_with_active_toolchain cargo "$@"
}

run_rustc() {
  run_with_active_toolchain rustc "$@"
}

require_edition_2024_cargo() {
  local error_message="$1"

  ensure_cargo_available
  if ! cargo_supports_edition_2024 "$(run_cargo -V | awk '{ print $2 }')"; then
    die "$error_message"
  fi
}

ensure_rustup_target_installed() {
  local toolchain="$1"
  local target="$2"
  local error_message="$3"

  [[ -n "$toolchain" ]] || return 0
  if ! run_rustup target list --installed --toolchain "$toolchain" | grep -qx "$target"; then
    die "$error_message"
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

read_cargo_package_value() {
  local field_name="$1"
  awk -F ' *= *' -v key="$field_name" '
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

read_cargo_package_name() {
  read_cargo_package_value name
}

read_cargo_package_version() {
  read_cargo_package_value version
}

resolve_release_artifact_prefix() {
  local package_name package_version
  package_name="$(read_cargo_package_name)"
  [[ -n "$package_name" ]] || die "Failed to read package name from Cargo.toml"
  package_version="$(read_cargo_package_version)"
  [[ -n "$package_version" ]] || die "Failed to read package version from Cargo.toml"
  printf '%s-%s\n' "$package_name" "$package_version"
}

read_rust_string_constant() {
  local constant_name="$1"
  awk -F '"' -v name="$constant_name" '
    $0 ~ ("const[[:space:]]+" name "[[:space:]]*:[[:space:]]*&str[[:space:]]*=") {
      print $2
      exit
    }
  ' "$ROOT_DIR/src/constants.rs"
}

read_android_application_id() {
  awk -F '= *' '
    /^[[:space:]]*applicationId[[:space:]]*=/ {
      value = $2
      gsub(/^[[:space:]]*"/, "", value)
      gsub(/"[[:space:]]*$/, "", value)
      print value
      exit
    }
  ' "$ANDROID_DIR/app/build.gradle.kts"
}

read_java_major_version() {
  local java_bin="$1"
  local version_text
  version_text="$("$java_bin" -version 2>&1 | awk -F '"' '/version/ { print $2; exit }')"
  if [[ -z "$version_text" ]]; then
    return 1
  fi
  if [[ "$version_text" == 1.* ]]; then
    printf '%s\n' "${version_text#1.}" | cut -d. -f1
  else
    printf '%s\n' "$version_text" | cut -d. -f1
  fi
}

detect_java17_home() {
  local java_home_candidate major
  java_home_candidate="$(read_var JAVA_HOME)"
  [[ -n "$java_home_candidate" ]] || return 1
  [[ -x "$java_home_candidate/bin/java" ]] || return 1

  major="$(read_java_major_version "$java_home_candidate/bin/java" || true)"
  [[ -n "$major" && "$major" -ge 17 ]] || return 1

  printf '%s\n' "$java_home_candidate"
}

require_java17() {
  local java_home

  java_home="$(detect_java17_home || true)"
  if [[ -z "$java_home" ]]; then
    die "JAVA_HOME is required and must point to JDK 17 or newer. Set JAVA_HOME in $(build_config_path_for_messages) or the environment."
  fi

  export JAVA_HOME="$java_home"
  export PATH="${JAVA_HOME}/bin:${PATH}"
}

detect_android_sdk_dir() {
  read_first_existing_dir_var ANDROID_HOME ANDROID_SDK_ROOT
}

detect_sdkmanager_path() {
  local android_sdk_dir="$1"
  local -a candidate_paths=(
    "$android_sdk_dir/cmdline-tools/latest/bin/sdkmanager"
    "$android_sdk_dir/cmdline-tools/bin/sdkmanager"
  )
  local candidate_path

  for candidate_path in "${candidate_paths[@]}"; do
    if [[ -x "$candidate_path" ]]; then
      printf '%s\n' "$candidate_path"
      return 0
    fi
  done

  for candidate_path in "$android_sdk_dir"/cmdline-tools/*/bin/sdkmanager; do
    if [[ -x "$candidate_path" ]]; then
      printf '%s\n' "$candidate_path"
      return 0
    fi
  done

  candidate_path="$(command -v sdkmanager 2>/dev/null || true)"
  if [[ -n "$candidate_path" ]]; then
    printf '%s\n' "$candidate_path"
    return 0
  fi

  return 1
}

detect_android_ndk_dir() {
  read_existing_dir_var ANDROID_NDK_HOME
}

require_android_sdk_dir() {
  detect_android_sdk_dir || die "ANDROID_HOME or ANDROID_SDK_ROOT is required and must point to an existing SDK directory. Set it in $(build_config_path_for_messages) or the environment."
}

require_android_ndk_dir() {
  detect_android_ndk_dir || die "ANDROID_NDK_HOME is required and must point to an existing NDK directory. Set it in $(build_config_path_for_messages) or the environment."
}

resolve_android_keystore_path() {
  local configured_path
  configured_path="$(read_var ANDROID_KEYSTORE_PATH)"

  if [[ -n "$configured_path" ]]; then
    [[ -f "$configured_path" ]] || die "ANDROID_KEYSTORE_PATH points to a missing file: $configured_path"
    printf '%s\n' "$configured_path"
    return 0
  fi

  first_existing_file_path \
    "$PACKAGING_DIR/android/release.keystore" \
    "$PACKAGING_DIR/android/release.jks"
}

export_var_if_set() {
  local var_name="$1"
  local var_value
  var_value="$(read_var "$var_name")"
  if [[ -n "$var_value" ]]; then
    export "${var_name}=${var_value}"
  fi
}

export_android_build_env() {
  local android_sdk_dir android_ndk_dir android_keystore_path
  android_sdk_dir="$(require_android_sdk_dir)"
  android_ndk_dir="$(require_android_ndk_dir)"
  android_keystore_path="$(resolve_android_keystore_path || true)"

  export ANDROID_HOME="$android_sdk_dir"
  export ANDROID_SDK_ROOT="$android_sdk_dir"
  export ANDROID_NDK_HOME="$android_ndk_dir"
  export RUSTUP_TOOLCHAIN="$(read_var_or_default RUSTUP_TOOLCHAIN "$DEFAULT_RUSTUP_TOOLCHAIN")"

  if [[ -n "$android_keystore_path" ]]; then
    export ANDROID_KEYSTORE_PATH="$android_keystore_path"
  fi

  export_var_if_set ANDROID_KEYSTORE_PASSWORD
  export_var_if_set ANDROID_KEY_ALIAS
  export_var_if_set ANDROID_KEY_PASSWORD
}

print_android_build_env() {
  local android_sdk_dir sdkmanager_path android_ndk_dir active_toolchain android_keystore_path
  android_sdk_dir="$(detect_android_sdk_dir || true)"
  sdkmanager_path=""
  android_ndk_dir=""
  active_toolchain="$(active_rustup_toolchain)"
  android_keystore_path="$(resolve_android_keystore_path || true)"

  if [[ -n "$android_sdk_dir" ]]; then
    sdkmanager_path="$(detect_sdkmanager_path "$android_sdk_dir" || true)"
    android_ndk_dir="$(detect_android_ndk_dir || true)"
  fi

  print_common_build_context
  printf 'ANDROID_HOME=%s\n' "${android_sdk_dir:-<not found>}"
  printf 'ANDROID_NDK_HOME=%s\n' "${android_ndk_dir:-<not found>}"
  printf 'SDKMANAGER=%s\n' "${sdkmanager_path:-<not found>}"
  printf 'JAVA_HOME=%s\n' "$(read_var_or_default JAVA_HOME '<unset>')"
  printf 'ANDROID_KEYSTORE_PATH=%s\n' "${android_keystore_path:-<not found>}"
  printf 'ANDROID_RELEASE_SIGNING=%s\n' "$(android_release_signing_configured && printf 'enabled' || printf 'disabled')"
  printf 'RUSTUP_TOOLCHAIN=%s\n' "${active_toolchain:-<not using rustup>}"
  printf 'CARGO=%s\n' "$(describe_cargo_binary)"
  printf 'RUSTC=%s\n' "$(describe_rustc_binary)"
  printf 'GRADLE=%s\n' "$(command -v gradle || printf '<not found>')"
  printf 'GRADLEW=%s\n' "$( [[ -x "$ANDROID_DIR/gradlew" ]] && printf '%s' "$ANDROID_DIR/gradlew" || printf '<not found>' )"
}

cargo_supports_edition_2024() {
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

detect_windows_gnu_tool() {
  local tool_name="$1"
  local target="${2:-$WINDOWS_CROSS_TARGET}"
  local prefix
  prefix="$(gnu_tool_prefix_for_target "$target" || true)"
  [[ -n "$prefix" ]] || return 1

  local tool_path
  tool_path="$(command -v "${prefix}-${tool_name}" 2>/dev/null || true)"
  [[ -n "$tool_path" ]] || return 1
  printf '%s\n' "$tool_path"
}

export_windows_tool_if_unset() {
  local target="$1"
  local env_var_name="$2"
  shift 2

  [[ -z "${!env_var_name:-}" ]] || return 0

  local tool_name
  local tool_path
  for tool_name in "$@"; do
    tool_path="$(detect_windows_gnu_tool "$tool_name" "$target" || true)"
    if [[ -n "$tool_path" ]]; then
      export "${env_var_name}=${tool_path}"
      return 0
    fi
  done

  return 1
}

resolve_windows_target() {
  local configured_target
  configured_target="$(read_var WINDOWS_TARGET)"
  if [[ -n "$configured_target" ]]; then
    printf '%s\n' "$configured_target"
    return 0
  fi

  if is_windows_host; then
    printf '%s\n' "$WINDOWS_HOST_TARGET"
    return 0
  fi

  if detect_windows_gnu_tool gcc >/dev/null 2>&1; then
    printf '%s\n' "$WINDOWS_CROSS_TARGET"
    return 0
  fi

  return 1
}

require_windows_target() {
  resolve_windows_target || die "Unable to determine a Windows target. Set WINDOWS_TARGET explicitly."
}

windows_target_upper_env_suffix() {
  local target="$1"
  printf '%s' "$target" | tr '[:lower:]-.' '[:upper:]__'
}

windows_target_lower_env_suffix() {
  local target="$1"
  printf '%s' "$target" | tr '[:upper:]-.' '[:lower:]__'
}

export_windows_gnu_env() {
  local target="$1"
  local cargo_env_suffix cc_env_suffix cargo_linker_var cargo_archiver_var cc_var cxx_var cc_archiver_var

  cargo_env_suffix="$(windows_target_upper_env_suffix "$target")"
  cc_env_suffix="$(windows_target_lower_env_suffix "$target")"
  cargo_linker_var="CARGO_TARGET_${cargo_env_suffix}_LINKER"
  cargo_archiver_var="CARGO_TARGET_${cargo_env_suffix}_AR"
  cc_var="CC_${cc_env_suffix}"
  cxx_var="CXX_${cc_env_suffix}"
  cc_archiver_var="AR_${cc_env_suffix}"

  export_windows_tool_if_unset "$target" "$cargo_linker_var" gcc
  export_windows_tool_if_unset "$target" "$cc_var" gcc
  export_windows_tool_if_unset "$target" "$cxx_var" g++
  export_windows_tool_if_unset "$target" "$cargo_archiver_var" gcc-ar ar
  export_windows_tool_if_unset "$target" "$cc_archiver_var" gcc-ar ar

  export PKG_CONFIG_ALLOW_CROSS=1
}

ensure_windows_build_toolchain() {
  local target="$1"
  require_edition_2024_cargo "The active cargo is too old for edition 2024. Use a newer cargo before building Windows artifacts."

  local toolchain
  toolchain="$(active_rustup_toolchain)"
  ensure_rustup_target_installed "$toolchain" "$target" "Rust target $target is missing. Run: rustup target add $target"

  if ! is_windows_host && [[ "$target" == *"-gnu" ]]; then
    export_windows_gnu_env "$target"
    local cargo_env_suffix
    cargo_env_suffix="$(windows_target_upper_env_suffix "$target")"
    local cargo_linker_var="CARGO_TARGET_${cargo_env_suffix}_LINKER"
    if [[ -z "${!cargo_linker_var:-}" ]]; then
      die "GNU Windows linker was not found for $target. Install mingw-w64 or set $cargo_linker_var manually."
    fi
    return 0
  fi

  if [[ "$target" == *"-msvc" ]] && ! is_windows_host; then
    warn "Target $target usually requires an external Windows linker toolchain on non-Windows hosts."
  fi
}

print_windows_build_env() {
  local windows_target rust_host cargo_linker_var cargo_linker_value cargo_archiver_var cargo_archiver_value rustup_toolchain detected_gnu_linker
  windows_target="$(resolve_windows_target || true)"
  if command_exists rustc || command_exists rustup; then
    rust_host="$(run_rustc -Vv 2>/dev/null | awk -F ': ' '/^host:/ { value = $2 } END { if (value != "") print value }')"
  else
    rust_host=""
  fi
  cargo_linker_var="<not available>"
  cargo_linker_value="<unset>"
  cargo_archiver_var="<not available>"
  cargo_archiver_value="<unset>"
  detected_gnu_linker="$(detect_windows_gnu_tool gcc "${windows_target:-$WINDOWS_CROSS_TARGET}" || true)"

  if [[ -n "$windows_target" ]]; then
    local cargo_env_suffix
    cargo_env_suffix="$(windows_target_upper_env_suffix "$windows_target")"
    cargo_linker_var="CARGO_TARGET_${cargo_env_suffix}_LINKER"
    cargo_archiver_var="CARGO_TARGET_${cargo_env_suffix}_AR"
    cargo_linker_value="${!cargo_linker_var:-<unset>}"
    cargo_archiver_value="${!cargo_archiver_var:-<unset>}"
  fi

  rustup_toolchain="$(active_rustup_toolchain)"

  print_common_build_context
  printf 'HOST_OS=%s\n' "$(uname -s)"
  printf 'RUST_HOST=%s\n' "${rust_host:-<unknown>}"
  printf 'RESOLVED_WINDOWS_TARGET=%s\n' "${windows_target:-<not configured>}"
  printf 'WINDOWS_TARGET=%s\n' "$(read_var_or_default WINDOWS_TARGET '<unset>')"
  printf 'RUSTUP_TOOLCHAIN=%s\n' "${rustup_toolchain:-<not using rustup>}"
  printf 'CARGO=%s\n' "$(describe_cargo_binary)"
  printf 'RUSTC=%s\n' "$(describe_rustc_binary)"
  printf 'RUSTUP=%s\n' "$(command -v rustup || printf '<not found>')"
  if [[ "$cargo_linker_var" == "<not available>" ]]; then
    printf 'WINDOWS_LINKER_ENV=%s\n' "$cargo_linker_var"
    printf 'WINDOWS_AR_ENV=%s\n' "$cargo_archiver_var"
  else
    printf '%s=%s\n' "$cargo_linker_var" "$cargo_linker_value"
    printf '%s=%s\n' "$cargo_archiver_var" "$cargo_archiver_value"
  fi
  printf 'WINDOWS_GNU_LINKER_DETECTED=%s\n' "${detected_gnu_linker:-<not found>}"
}

clean_build_outputs() {
  log "Cleaning local build outputs and caches"
  remove_dir_if_exists "$ROOT_DIR/target"
  remove_dir_if_exists "$DIST_DIR"
  remove_dir_if_exists "$ANDROID_DIR/.gradle"
  remove_dir_if_exists "$ANDROID_DIR/.kotlin"
  remove_dir_if_exists "$ANDROID_DIR/build"
  remove_dir_if_exists "$ANDROID_DIR/app/build"
  remove_dir_if_exists "$ANDROID_DIR/app/.cxx"
  remove_dir_if_exists "$ANDROID_DIR/app/.externalNativeBuild"
}

windows_setup() {
  local windows_target toolchain gnu_linker_path
  toolchain="$(active_rustup_toolchain)"
  [[ -n "$toolchain" ]] || die "rustup is required to install Windows Rust targets."
  windows_target="$(require_windows_target)"
  log "Installing Rust target $windows_target for toolchain $toolchain"
  run_rustup target add --toolchain "$toolchain" "$windows_target"

  if ! is_windows_host && [[ "$windows_target" == *"-gnu" ]]; then
    export_windows_gnu_env "$windows_target"
    gnu_linker_path="$(detect_windows_gnu_tool gcc "$windows_target" || true)"
    if [[ -z "$gnu_linker_path" ]]; then
      warn "GNU Windows linker was not detected. Install mingw-w64 and rerun windows-env before windows-release."
    else
      log "Detected GNU Windows linker for $windows_target: $gnu_linker_path"
    fi
  fi
}

ensure_android_rust_build_toolchain() {
  local toolchain
  toolchain="$(active_rustup_toolchain)"
  if [[ -n "$toolchain" ]]; then
    ensure_rustup_target_installed \
      "$toolchain" \
      "aarch64-linux-android" \
      "Rust target aarch64-linux-android is missing for rustup toolchain $toolchain. Run: rustup target add --toolchain $toolchain aarch64-linux-android"
    require_edition_2024_cargo "rustup toolchain $toolchain is too old for edition 2024. Update Rust with: rustup update $toolchain"
    return 0
  fi

  require_edition_2024_cargo "The active cargo is too old for edition 2024. Use a newer cargo or install rustup."
}

prepare_android_build_context() {
  require_java17
  export_android_build_env
  ensure_android_rust_build_toolchain
}

resolve_gradle_command() {
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
  local gradle_command
  gradle_command="$(resolve_gradle_command)" || die "Neither android/gradlew nor gradle is available."
  (
    cd "$ANDROID_DIR"
    "$gradle_command" "$@"
  )
}

android_release_signing_configured() {
  [[ -n "${ANDROID_KEYSTORE_PATH:-}" ]] \
    && [[ -n "${ANDROID_KEYSTORE_PASSWORD:-}" ]] \
    && [[ -n "${ANDROID_KEY_ALIAS:-}" ]] \
    && [[ -n "${ANDROID_KEY_PASSWORD:-}" ]]
}

find_android_apk() {
  local build_type="$1"
  local -a candidate_paths=(
    "$ANDROID_DIR/app/build/outputs/apk/$build_type/app-$build_type.apk"
    "$ANDROID_DIR/app/build/outputs/apk/$build_type/app-$build_type-unsigned.apk"
  )
  local candidate_path
  for candidate_path in "${candidate_paths[@]}"; do
    if [[ -f "$candidate_path" ]]; then
      printf '%s\n' "$candidate_path"
      return 0
    fi
  done
  return 1
}

read_android_apk_abis() {
  local apk_path="$1"

  [[ -f "$apk_path" ]] || return 1
  command_exists unzip || return 1

  unzip -Z1 "$apk_path" 2>/dev/null \
    | awk -F/ 'NF == 3 && $1 == "lib" { print $2 }' \
    | sort -u
}

resolve_android_release_abi_label() {
  local source_apk_path="$1"
  local abi_label

  abi_label="$(read_android_apk_abis "$source_apk_path" | paste -sd+ - || true)"
  printf '%s\n' "${abi_label:-universal}"
}

resolve_android_release_asset_name() {
  local source_apk_path="$1"
  local artifact_prefix abi_label
  artifact_prefix="$(resolve_release_artifact_prefix)"
  abi_label="$(resolve_android_release_abi_label "$source_apk_path")"

  if [[ "$source_apk_path" == *"-unsigned.apk" ]]; then
    printf '%s-android-%s-unsigned.apk\n' "$artifact_prefix" "$abi_label"
  else
    printf '%s-android-%s.apk\n' "$artifact_prefix" "$abi_label"
  fi
}

stage_android_release_apk() {
  local source_apk_path="$1"
  local target_name artifact_prefix
  artifact_prefix="$(resolve_release_artifact_prefix)"
  target_name="$(resolve_android_release_asset_name "$source_apk_path")"

  rm -f "$DIST_DIR/${artifact_prefix}-android-"*.apk
  copy_file_to_dist "$source_apk_path" "$target_name"
}

read_android_current_user() {
  adb shell am get-current-user 2>/dev/null | tr -d '\r' | tr -d '\n'
}

ensure_android_package_available_for_current_user() {
  local application_id="$1"
  local current_user="$2"

  if [[ -n "$current_user" && -n "$application_id" ]]; then
    adb shell cmd package install-existing --user "$current_user" "$application_id" >/dev/null 2>&1 || true
    log "Ensured package $application_id is available for Android user $current_user"
  fi
}

has_connected_android_device() {
  command_exists adb || return 1
  adb devices | awk '
    NR > 1 && $2 == "device" { found = 1; exit }
    END { exit(found ? 0 : 1) }
  '
}

install_android_apk_on_connected_device() {
  local apk_path="$1"
  local current_user application_id install_output install_status

  require_command adb
  [[ -f "$apk_path" ]] || die "Missing Android APK: $apk_path"

  application_id="$(read_android_application_id)"
  log "Installing Android APK on connected device: $apk_path"

  set +e
  install_output="$(adb install -r "$apk_path" 2>&1)"
  install_status=$?
  set -e

  [[ -n "$install_output" ]] && printf '%s\n' "$install_output"

  if [[ "$install_status" -ne 0 ]]; then
    if [[ "$install_output" == *"INSTALL_FAILED_UPDATE_INCOMPATIBLE"* ]]; then
      die "adb install failed because $application_id is already installed with a different signing certificate. Uninstall the existing app first with: adb uninstall $application_id, or rebuild with the same keystore used by the installed app."
    fi
    die "adb install failed for $application_id."
  fi

  current_user="$(read_android_current_user)"
  ensure_android_package_available_for_current_user "$application_id" "$current_user"
}

maybe_install_android_release_apk() {
  local apk_path="$1"

  if [[ "$apk_path" == *"-unsigned.apk" ]]; then
    warn "Release APK is unsigned; skipping adb install."
    return 0
  fi

  if ! command_exists adb; then
    warn "adb is not available; skipping release APK install."
    return 0
  fi

  if ! has_connected_android_device; then
    warn "No connected Android device detected; skipping release APK install."
    return 0
  fi

  install_android_apk_on_connected_device "$apk_path"
}

print_android_apk_path() {
  local build_type="$1"
  log "APK: $(find_android_apk "$build_type" || printf '%s' "$ANDROID_DIR/app/build/outputs/apk/$build_type")"
}

android_setup() {
  require_java17
  local android_sdk_dir sdkmanager_path
  android_sdk_dir="$(require_android_sdk_dir)"
  sdkmanager_path="$(detect_sdkmanager_path "$android_sdk_dir")" || die "sdkmanager was not found under $android_sdk_dir. Install Android command-line tools first."
  log "Installing Android SDK packages into $android_sdk_dir"
  log "Using sdkmanager: $sdkmanager_path"
  log "Packages: ${DEFAULT_ANDROID_PACKAGES[*]}"
  "$sdkmanager_path" --sdk_root="$android_sdk_dir" "${DEFAULT_ANDROID_PACKAGES[@]}"
}

android_wrapper() {
  require_java17
  if [[ -x "$ANDROID_DIR/gradlew" ]]; then
    log "Gradle Wrapper already exists at $ANDROID_DIR/gradlew"
    return 0
  fi
  require_command gradle
  log "Generating Gradle Wrapper with $(command -v gradle)"
  (
    cd "$ANDROID_DIR"
    gradle wrapper
  )
  log "Gradle Wrapper generated at $ANDROID_DIR/gradlew"
}

android_build() {
  local task="$1"
  prepare_android_build_context
  log "Running Android Gradle task $task"
  if [[ "$task" == ":app:assembleRelease" ]]; then
    if android_release_signing_configured; then
      log "Android release signing is enabled via environment variables"
    else
      log "Android release signing is not configured; Gradle may produce an unsigned release APK"
    fi
  fi
  run_android_gradle "$task"
}

android_debug() {
  prepare_android_build_context
  require_command adb
  local current_user application_id
  log "Running Android Gradle task :app:installDebug"
  run_android_gradle ":app:installDebug"
  current_user="$(read_android_current_user)"
  application_id="$(read_android_application_id)"
  ensure_android_package_available_for_current_user "$application_id" "$current_user"
  print_android_apk_path debug
}

android_release() {
  local source_apk_path release_apk_path

  android_build ":app:assembleRelease"
  source_apk_path="$(find_android_apk release)" || die "Missing Android release APK."
  release_apk_path="$(stage_android_release_apk "$source_apk_path")"
  log "Source APK: $source_apk_path"
  log "Created $release_apk_path"
  maybe_install_android_release_apk "$release_apk_path"
}

windows_release() {
  local windows_target package_name executable_name source_executable dist_executable artifact_prefix

  windows_target="$(require_windows_target)"
  ensure_windows_build_toolchain "$windows_target"

  package_name="$(read_cargo_package_name)"
  [[ -n "$package_name" ]] || die "Failed to read package name from Cargo.toml"
  artifact_prefix="$(resolve_release_artifact_prefix)"
  executable_name="${package_name}.exe"
  source_executable="$ROOT_DIR/target/$windows_target/release/$executable_name"
  dist_executable="$DIST_DIR/${artifact_prefix}-windows-$windows_target.exe"

  log "Building Windows release binary for $windows_target"
  run_cargo build --release --target "$windows_target"

  [[ -f "$source_executable" ]] || die "Missing Windows executable at $source_executable"

  dist_executable="$(copy_file_to_dist "$source_executable" "$(basename "$dist_executable")")"

  log "Source executable: $source_executable"
  log "Created $dist_executable"
}

macos_signing_configured() {
  [[ -n "${MACOS_SIGNING_IDENTITY:-}" ]]
}

resolve_macos_bundle_name() {
  local bundle_name
  bundle_name="$(read_var MACOS_BUNDLE_NAME)"
  [[ -n "$bundle_name" ]] || bundle_name="$(read_rust_string_constant APP_NAME)"
  [[ -n "$bundle_name" ]] || bundle_name="$(read_cargo_package_name)"
  [[ -n "$bundle_name" ]] || die "Failed to resolve macOS bundle name."
  printf '%s\n' "$bundle_name"
}

resolve_macos_bundle_identifier() {
  local bundle_identifier
  bundle_identifier="$(read_var MACOS_BUNDLE_IDENTIFIER)"
  [[ -n "$bundle_identifier" ]] || bundle_identifier="$(read_rust_string_constant APP_BUNDLE_ID)"
  [[ -n "$bundle_identifier" ]] || die "Failed to resolve macOS bundle identifier."
  printf '%s\n' "$bundle_identifier"
}

resolve_macos_executable_name() {
  local executable_name
  executable_name="$(read_var MACOS_EXECUTABLE_NAME)"
  [[ -n "$executable_name" ]] || executable_name="$(read_cargo_package_name)"
  [[ -n "$executable_name" ]] || die "Failed to resolve macOS executable name."
  printf '%s\n' "$executable_name"
}

resolve_macos_bundle_version() {
  local bundle_version
  bundle_version="$(read_var MACOS_BUNDLE_VERSION)"
  [[ -n "$bundle_version" ]] || bundle_version="$(read_cargo_package_version)"
  [[ -n "$bundle_version" ]] || die "Failed to resolve macOS bundle version."
  printf '%s\n' "$bundle_version"
}

resolve_macos_build_version() {
  local build_version
  build_version="$(read_var MACOS_BUILD_VERSION)"
  [[ -n "$build_version" ]] || build_version="$(resolve_macos_bundle_version)"
  printf '%s\n' "$build_version"
}

resolve_macos_development_region() {
  read_var_or_default MACOS_DEVELOPMENT_REGION "zh_CN"
}

resolve_macos_minimum_system_version() {
  read_var_or_default MACOS_MINIMUM_SYSTEM_VERSION "12.0"
}

resolve_macos_entitlements_path() {
  local configured_path
  configured_path="$(read_var MACOS_ENTITLEMENTS_PATH)"

  if [[ -n "$configured_path" ]]; then
    [[ -f "$configured_path" ]] || die "MACOS_ENTITLEMENTS_PATH points to a missing file: $configured_path"
    printf '%s\n' "$configured_path"
    return 0
  fi

  first_existing_file_path "$PACKAGING_DIR/macos/entitlements.plist"
}

write_macos_info_plist() {
  local output_path="$1"
  local bundle_name="$2"
  local executable_name="$3"
  local bundle_identifier="$4"
  local bundle_version="$5"
  local build_version="$6"
  local development_region="$7"
  local minimum_system_version="$8"

  cat > "$output_path" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>${development_region}</string>
    <key>CFBundleDisplayName</key>
    <string>${bundle_name}</string>
    <key>CFBundleExecutable</key>
    <string>${executable_name}</string>
    <key>CFBundleIdentifier</key>
    <string>${bundle_identifier}</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>${bundle_name}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${bundle_version}</string>
    <key>CFBundleVersion</key>
    <string>${build_version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>${minimum_system_version}</string>
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
EOF
}

create_macos_bundle_icon() {
  local resources_dir="$1"
  local source_icon_path icon_file_path

  source_icon_path="$(find_generated_release_build_output "cliplink-macos-bundle-icon.icns" || true)"
  [[ -n "$source_icon_path" ]] || die "Missing generated macOS bundle icon. Re-run the release build after confirming resources/icon.svg exists."

  icon_file_path="$resources_dir/AppIcon.icns"

  rm -f "$icon_file_path"
  cp "$source_icon_path" "$icon_file_path"

  printf '%s\n' "$icon_file_path"
}

macos_release() {
  ensure_cargo_available
  local bundle_name executable_name bundle_identifier bundle_version build_version development_region minimum_system_version macos_entitlements_path bundle_dir info_plist_path resources_dir app_icon_path

  bundle_name="$(resolve_macos_bundle_name)"
  executable_name="$(resolve_macos_executable_name)"
  bundle_identifier="$(resolve_macos_bundle_identifier)"
  bundle_version="$(resolve_macos_bundle_version)"
  build_version="$(resolve_macos_build_version)"
  development_region="$(resolve_macos_development_region)"
  minimum_system_version="$(resolve_macos_minimum_system_version)"
  macos_entitlements_path="$(resolve_macos_entitlements_path || true)"

  log "Building macOS release binary"
  log "Bundle name: $bundle_name"
  log "Executable name: $executable_name"
  log "Bundle identifier: $bundle_identifier"
  log "Bundle version: $bundle_version"
  run_cargo build --release

  mkdir -p "$DIST_DIR"
  bundle_dir="$DIST_DIR/$bundle_name.app"
  info_plist_path="$bundle_dir/Contents/Info.plist"
  resources_dir="$bundle_dir/Contents/Resources"

  rm -rf "$bundle_dir"
  mkdir -p "$bundle_dir/Contents/MacOS"
  mkdir -p "$resources_dir"
  app_icon_path="$(create_macos_bundle_icon "$resources_dir")"
  write_macos_info_plist \
    "$info_plist_path" \
    "$bundle_name" \
    "$executable_name" \
    "$bundle_identifier" \
    "$bundle_version" \
    "$build_version" \
    "$development_region" \
    "$minimum_system_version"
  cp "$ROOT_DIR/target/release/$executable_name" "$bundle_dir/Contents/MacOS/$executable_name"
  chmod +x "$bundle_dir/Contents/MacOS/$executable_name"

  if command_exists plutil; then
    plutil -lint "$info_plist_path" >/dev/null
  fi

  if macos_signing_configured; then
    local -a codesign_args=(
      --force
      --deep
      --sign "$MACOS_SIGNING_IDENTITY"
      --timestamp
    )

    if [[ -n "$macos_entitlements_path" ]]; then
      codesign_args+=(--entitlements "$macos_entitlements_path")
    fi

    require_command codesign
    log "Signing macOS app bundle with identity: $MACOS_SIGNING_IDENTITY"
    codesign "${codesign_args[@]}" "$bundle_dir"
    codesign --verify --deep --strict --verbose=2 "$bundle_dir"
  else
    log "macOS signing is not configured; produced an unsigned app bundle"
  fi

  log "Generated macOS app icon: $app_icon_path"
  log "Generated Info.plist: $info_plist_path"
  log "Created $bundle_dir"
}

main() {
  local command_name="${1:-help}"
  case "$command_name" in
    help|-h|--help)
      usage
      ;;
    clean)
      clean_build_outputs
      ;;
    android-env)
      print_android_build_env
      ;;
    android-setup)
      android_setup
      ;;
    android-wrapper)
      android_wrapper
      ;;
    android-debug)
      android_debug
      ;;
    android-release)
      android_release
      ;;
    windows-env)
      print_windows_build_env
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
      die "Unknown command: $command_name"
      ;;
  esac
}

main "$@"
