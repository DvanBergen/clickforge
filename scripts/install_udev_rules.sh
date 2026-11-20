#!/usr/bin/env bash
set -euo pipefail

UDEV_RULE_PATH="/etc/udev/rules.d/99-clickforge-uinput.rules"
MODULE_LOAD_PATH="/etc/modules-load.d/clickforge-uinput.conf"
TARGET_USER="${SUDO_USER:-$USER}"

echo "ClickForge Linux Input Permissions Setup"
echo "========================================"
echo "This script installs a udev rule granting members of the 'input' group"
echo "read/write access to /dev/uinput without running ClickForge as root."
echo ""

if [[ "${EUID}" -ne 0 ]]; then
    echo "Please run this script with sudo:"
    echo "  sudo $0"
    exit 1
fi

echo "1. Writing udev rule to ${UDEV_RULE_PATH}..."
cat <<'EOF' > "${UDEV_RULE_PATH}"
# ClickForge virtual mouse and keyboard injection permissions
KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput", TAG+="uaccess"
EOF

echo "2. Ensuring the uinput kernel module loads at boot (${MODULE_LOAD_PATH})..."
echo "uinput" > "${MODULE_LOAD_PATH}"
modprobe uinput || true

echo "3. Reloading udev rules..."
udevadm control --reload-rules
udevadm trigger --subsystem-match=misc --attr-match=name=uinput || true

echo "4. Checking 'input' group membership for user '${TARGET_USER}'..."
if id -nG "${TARGET_USER}" | tr ' ' '\n' | grep -qx "input"; then
    echo "   User '${TARGET_USER}' already belongs to the 'input' group."
else
    echo "   Adding '${TARGET_USER}' to the 'input' group..."
    usermod -aG input "${TARGET_USER}"
    echo "   Group membership updated. Log out and log back in for the change to take effect."
fi

echo ""
echo "Current /dev/uinput permissions:"
ls -la /dev/uinput || true
echo ""
echo "Setup complete."
