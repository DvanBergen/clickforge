#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

RELEASE_BINARY_PATH="${PROJECT_ROOT}/target/release/clickforge"
ICON_SOURCE_PATH="${PROJECT_ROOT}/assets/clickforge.svg"

BIN_INSTALL_DIR="${XDG_BIN_HOME:-${HOME}/.local/bin}"
DATA_INSTALL_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}"

INSTALLED_BINARY_PATH="${BIN_INSTALL_DIR}/clickforge"
HICOLOR_ICON_DIR="${DATA_INSTALL_DIR}/icons/hicolor"
INSTALLED_ICON_PATH="${HICOLOR_ICON_DIR}/scalable/apps/clickforge.svg"
APPLICATIONS_DIR="${DATA_INSTALL_DIR}/applications"
DESKTOP_ENTRY_PATH="${APPLICATIONS_DIR}/clickforge.desktop"

if [[ ! -f "${ICON_SOURCE_PATH}" ]]; then
    echo "Error: application icon not found at ${ICON_SOURCE_PATH}" >&2
    exit 1
fi

if [[ ! -x "${RELEASE_BINARY_PATH}" ]]; then
    echo "Release binary not found at ${RELEASE_BINARY_PATH}; building with cargo build --release..."
    (cd "${PROJECT_ROOT}" && cargo build --release)
fi

if [[ ! -x "${RELEASE_BINARY_PATH}" ]]; then
    echo "Error: release binary is still missing at ${RELEASE_BINARY_PATH}" >&2
    exit 1
fi

echo "Installing ClickForge binary to ${INSTALLED_BINARY_PATH}..."
install -Dm755 "${RELEASE_BINARY_PATH}" "${INSTALLED_BINARY_PATH}"

echo "Installing ClickForge SVG icon to ${INSTALLED_ICON_PATH}..."
install -Dm644 "${ICON_SOURCE_PATH}" "${INSTALLED_ICON_PATH}"

echo "Writing desktop entry to ${DESKTOP_ENTRY_PATH}..."
mkdir -p "${APPLICATIONS_DIR}"
cat <<EOF > "${DESKTOP_ENTRY_PATH}"
[Desktop Entry]
Type=Application
Name=ClickForge
GenericName=Linux Auto-Clicker and Script Workbench
Comment=Linux-native Rust auto-clicker with visual drag-and-drop blocks and native Rust scripts
Exec=${INSTALLED_BINARY_PATH}
TryExec=${INSTALLED_BINARY_PATH}
Icon=clickforge
Terminal=false
StartupNotify=true
StartupWMClass=clickforge
Categories=Utility;Game;
Keywords=autoclicker;auto clicker;clicker;macro;script;rust;gaming;automation;input;clickforge;
EOF
chmod 644 "${DESKTOP_ENTRY_PATH}"

if command -v desktop-file-validate >/dev/null 2>&1; then
    echo "Validating ${DESKTOP_ENTRY_PATH}..."
    desktop-file-validate "${DESKTOP_ENTRY_PATH}"
fi

if command -v update-desktop-database >/dev/null 2>&1; then
    echo "Updating desktop database in ${APPLICATIONS_DIR}..."
    update-desktop-database "${APPLICATIONS_DIR}"
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1 && [[ -f "${HICOLOR_ICON_DIR}/index.theme" ]]; then
    echo "Updating hicolor icon cache in ${HICOLOR_ICON_DIR}..."
    gtk-update-icon-cache -f -t "${HICOLOR_ICON_DIR}"
fi

echo "ClickForge desktop application installed:"
echo "  Binary:        ${INSTALLED_BINARY_PATH}"
echo "  Icon:          ${INSTALLED_ICON_PATH}"
echo "  Desktop entry: ${DESKTOP_ENTRY_PATH}"
