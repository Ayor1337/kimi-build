#!/usr/bin/env bash
# Kimi Build installer for macOS, Linux, Git Bash, MSYS2, and Cygwin.

set -e

REPOSITORY="${KAMI_REPOSITORY:-Ayor1337/kimi-build}"
RELEASES_URL="https://github.com/${REPOSITORY}/releases"
VERSION="${1:-${KAMI_VERSION:-}}"
KAMI_HOME="${KAMI_HOME:-$HOME/.kami}"
DOWNLOAD_DIR="$KAMI_HOME/downloads"
BIN_DIR="${KAMI_BIN_DIR:-$KAMI_HOME/bin}"

if command -v curl >/dev/null 2>&1; then
    download() { curl -fsSL --retry 3 -o "$2" "$1"; }
    download_stdout() { curl -fsSL --retry 3 "$1"; }
elif command -v wget >/dev/null 2>&1; then
    download() { wget -q -O "$2" "$1"; }
    download_stdout() { wget -q -O - "$1"; }
else
    echo "Error: curl or wget is required." >&2
    exit 1
fi

case "$(uname -s)" in
    Darwin) os="macos" ;;
    Linux) os="linux" ;;
    MINGW* | MSYS* | CYGWIN*) os="windows" ;;
    *) echo "Error: unsupported operating system: $(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
    x86_64 | amd64 | AMD64) arch="x86_64" ;;
    arm64 | aarch64 | ARM64) arch="aarch64" ;;
    *) echo "Error: unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac

if [ "$os" = "windows" ] && [ "$arch" != "x86_64" ]; then
    echo "Error: Windows ARM64 releases are not available yet." >&2
    exit 1
fi

if [ -z "$VERSION" ]; then
    echo "Fetching latest Kimi Build version..." >&2
    VERSION="$(download_stdout "$RELEASES_URL/latest/download/version.txt" | tr -d '\r\n[:space:]')"
fi

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9._]+)?$ ]]; then
    echo "Error: invalid version '$VERSION' (expected X.Y.Z or X.Y.Z-suffix)." >&2
    exit 1
fi

platform="${os}-${arch}"
suffix=""
[ "$os" = "windows" ] && suffix=".exe"
asset="kami-${VERSION}-${platform}${suffix}"
url="$RELEASES_URL/download/v${VERSION}/${asset}"

mkdir -p "$DOWNLOAD_DIR" "$BIN_DIR"
binary="$DOWNLOAD_DIR/$asset"
temporary="$binary.tmp.$$"
trap 'rm -f "$temporary"' EXIT

echo "Installing Kimi Build $VERSION ($platform)..." >&2
download "$url" "$temporary"
chmod +x "$temporary"
if ! "$temporary" --version </dev/null >/dev/null 2>&1; then
    echo "Error: downloaded binary failed its version check; the current install is unchanged." >&2
    exit 1
fi
mv -f "$temporary" "$binary"
trap - EXIT

if [ "$os" = "windows" ]; then
    for name in kami.exe grok.exe agent.exe; do
        cp -f "$binary" "$BIN_DIR/$name"
    done
else
    if [ "$(dirname "$BIN_DIR")" = "$(dirname "$DOWNLOAD_DIR")" ]; then
        link_target="../$(basename "$DOWNLOAD_DIR")/$asset"
    else
        link_target="$binary"
    fi
    for name in kami grok agent; do
        ln -sfn "$link_target" "$BIN_DIR/$name"
    done
fi

# Record the updater backend without disturbing unrelated settings.
config_file="$KAMI_HOME/config.toml"
if [ ! -f "$config_file" ]; then
    printf '[cli]\ninstaller = "gh-release"\n' > "$config_file"
elif grep -q '^\[cli\][[:space:]]*$' "$config_file"; then
    config_tmp="$config_file.tmp.$$"
    awk '
        /^\[cli\][[:space:]]*$/ { print; print "installer = \"gh-release\""; in_cli=1; next }
        /^\[/ { in_cli=0 }
        in_cli && /^[[:space:]]*installer[[:space:]]*=/ { next }
        { print }
    ' "$config_file" > "$config_tmp" && mv "$config_tmp" "$config_file"
else
    printf '\n[cli]\ninstaller = "gh-release"\n' >> "$config_file"
fi

# Generate completions when the binary supports them.
mkdir -p "$KAMI_HOME/completions/bash" "$KAMI_HOME/completions/zsh"
"$BIN_DIR/kami" completions bash > "$KAMI_HOME/completions/bash/kami.bash" 2>/dev/null || true
"$BIN_DIR/kami" completions zsh > "$KAMI_HOME/completions/zsh/_kami" 2>/dev/null || true
if mkdir -p "$HOME/.config/fish/completions" 2>/dev/null; then
    "$BIN_DIR/kami" completions fish > "$HOME/.config/fish/completions/kami.fish" 2>/dev/null || true
fi

path_has_dir() {
    case ":$PATH:" in *":$1:"*) return 0 ;; *) return 1 ;; esac
}

shell_name="$(basename "${SHELL:-}")"
shell_config=""
case "$shell_name" in
    bash) shell_config="$HOME/.bashrc" ;;
    zsh) shell_config="$HOME/.zshrc" ;;
    fish) shell_config="$HOME/.config/fish/config.fish" ;;
esac

if [ -n "$shell_config" ] && ! path_has_dir "$BIN_DIR"; then
    mkdir -p "$(dirname "$shell_config")"
    path_value="$KAMI_HOME/bin"
    if [ "$shell_name" = "fish" ]; then
        path_line="fish_add_path $path_value"
    else
        path_line="export PATH=\"$path_value:\$PATH\""
    fi
    if ! grep -Fq "$path_line" "$shell_config" 2>/dev/null; then
        printf '\n# Kimi Build\n%s\n' "$path_line" >> "$shell_config"
    fi
fi

echo "Kimi Build $VERSION installed to $BIN_DIR/kami${suffix}." >&2
if path_has_dir "$BIN_DIR"; then
    echo "Run 'kami' to get started." >&2
elif [ -n "$shell_config" ]; then
    echo "Restart your terminal, then run 'kami'." >&2
else
    echo "Add $BIN_DIR to PATH, then run 'kami'." >&2
fi
