#!/usr/bin/env bash
# Claude HUD uninstaller for macOS / Linux
set -euo pipefail

INSTALL_DIR="${HUD_INSTALL_DIR:-${HOME}/.local/bin}"
BIN="$INSTALL_DIR/claude-hud"

case "$INSTALL_DIR" in
  *'"'*|*'|'*|*$'\n'*) echo "error: HUD_INSTALL_DIR contains invalid characters" >&2; exit 1 ;;
esac

# 1. 先摘掉 statusLine 并删除配置目录（二进制内置逻辑），
#    避免 Claude Code 每 5 秒调用已删除的命令
if [ -x "$BIN" ]; then
  "$BIN" uninstall || echo "warning: claude-hud uninstall reported an issue" >&2
fi

# 2. 移除安装脚本追加的 PATH 行（精确匹配，仅删该行）
for RC_FILE in "${HOME}/.bashrc" "${ZDOTDIR:-${HOME}}/.zshrc"; do
  if [ -f "$RC_FILE" ]; then
    sed -i.bak "\|export PATH=\"${INSTALL_DIR}:\$PATH\"|d" "$RC_FILE" || true
    rm -f "$RC_FILE.bak"
  fi
done

# 3. 删除二进制与版本标记
rm -f "$BIN" "$INSTALL_DIR/version.txt"
if [ -f "$BIN" ] || [ -f "$INSTALL_DIR/version.txt" ]; then
  echo "warning: some files could not be removed — delete them manually" >&2
else
  echo "Removed $BIN"
fi

echo "Claude HUD uninstalled."
