#!/usr/bin/env bash
# Claude HUD installer for macOS / Linux
set -euo pipefail

REPO="${HUD_REPO:-cuishiying-1/claude-hud}"  # HUD_REPO 可覆盖（开发/测试）
INSTALL_DIR="${HUD_INSTALL_DIR:-${HOME}/.local/bin}"

case "$INSTALL_DIR" in
  *'"'*|*'|'*|*$'\n'*) echo "error: HUD_INSTALL_DIR contains invalid characters" >&2; exit 1 ;;
esac

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)  TARGET="linux-x64" ;;
  Darwin-x86_64) TARGET="macos-x64" ;;
  Darwin-arm64)  TARGET="macos-arm64" ;;
  *) echo "error: unsupported platform $(uname -s)-$(uname -m)" >&2; exit 1 ;;
esac

mkdir -p "$INSTALL_DIR"
echo "Installing Claude HUD (${TARGET}) ..."

if [ -n "${HUD_LOCAL_BIN:-}" ]; then
  # 本地安装模式（开发/CI 冒烟）：不访问网络
  cp "$HUD_LOCAL_BIN" "$INSTALL_DIR/claude-hud"
  chmod +x "$INSTALL_DIR/claude-hud"
else
  LATEST="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)" || true
  [ -n "$LATEST" ] || { echo "error: cannot resolve latest release of ${REPO}" >&2; exit 1; }
  LATEST_DISPLAY="${LATEST#v}"   # 展示用版本号；下载 URL 与 version.txt 保留原始 tag

  if [ -f "$INSTALL_DIR/version.txt" ]; then
    OLD="$(cat "$INSTALL_DIR/version.txt")"
    if [ "$OLD" = "$LATEST" ]; then
      echo "claude-hud v${LATEST_DISPLAY} is up to date"
      exit 0
    fi
    echo "upgrading v${OLD#v} → v${LATEST_DISPLAY}"
  else
    echo "installing claude-hud v${LATEST_DISPLAY}"
  fi

  curl -fsSL "https://github.com/${REPO}/releases/download/${LATEST}/claude-hud-${TARGET}.tar.gz" \
    | tar xz -C "$INSTALL_DIR" claude-hud
  chmod +x "$INSTALL_DIR/claude-hud"
  printf '%s\n' "$LATEST" > "$INSTALL_DIR/version.txt"
fi

if ! echo ":$PATH:" | grep -q ":$INSTALL_DIR:"; then
  case "${SHELL:-}" in
    *zsh*) RC_FILE="${ZDOTDIR:-${HOME}}/.zshrc" ;;
    *)     RC_FILE="${HOME}/.bashrc" ;;
  esac
  if ! grep -qF "export PATH=\"$INSTALL_DIR" "$RC_FILE" 2>/dev/null; then
    printf '\n%s\n' "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$RC_FILE"
  fi
  echo "Added $INSTALL_DIR to PATH in $RC_FILE (restart terminal or source it)"
fi

"$INSTALL_DIR/claude-hud" setup

echo
echo "Done! Verify:"
echo '  echo '"'"'{"model":{"id":"test","display_name":"Test"},"context_window":{"used_percentage":50,"total_input_tokens":1000,"context_window_size":200000},"cost":{"total_cost_usd":0.1,"total_duration_ms":60000}}'"'"' | claude-hud render'
echo '  Restart Claude Code or run /reload-plugins to see the HUD status bar.'
