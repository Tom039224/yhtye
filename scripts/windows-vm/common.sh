# shellcheck shell=bash disable=SC2034  # sourced by the other scripts, which use these variables
# Shared settings for the Windows 11 test VM scripts. Sourced, not executed.
# Everything that is large or secret (ISO, disk, keys, password) lives in $VM_DIR, outside the repository.

VM_DIR="${WIN11_VM_DIR:-$HOME/VMs/win11}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

VM_NAME=win11
VM_CPUS="${WIN11_VM_CPUS:-8}"
VM_MEM="${WIN11_VM_MEM:-16G}"
VM_DISK_SIZE="${WIN11_VM_DISK_SIZE:-128G}"
SSH_PORT="${WIN11_VM_SSH_PORT:-2222}"
# VNC display number: port = 5900 + display.
VNC_DISPLAY="${WIN11_VM_VNC_DISPLAY:-1}"

ISO="$VM_DIR/win11-enterprise-eval.iso"
DISK="$VM_DIR/disk.qcow2"
UNATTEND_ISO="$VM_DIR/unattend.iso"
OVMF_CODE="${WIN11_VM_OVMF_CODE:-/usr/share/edk2/x64/OVMF_CODE.4m.fd}"
OVMF_VARS_TEMPLATE="${WIN11_VM_OVMF_VARS:-/usr/share/edk2/x64/OVMF_VARS.4m.fd}"
OVMF_VARS="$VM_DIR/OVMF_VARS.4m.fd"
TPM_DIR="$VM_DIR/tpm"
SWTPM_SOCK="$VM_DIR/swtpm.sock"
QMP_SOCK="$VM_DIR/qmp.sock"
QEMU_PID="$VM_DIR/qemu.pid"
SSH_KEY="$HOME/.ssh/yhtye_win11_vm_ed25519"
CONFIG="$VM_DIR/vm.env"

# A distro may ship swtpm / qemu-img linked against a newer libcrypto than the installed one
# (a partial upgrade). Libraries and binaries unpacked into $VM_DIR/tools take precedence.
TOOLS_DIR="$VM_DIR/tools"

die() {
  echo "error: $*" >&2
  exit 1
}

with_tools_env() {
  if [[ -d "$TOOLS_DIR/lib" ]]; then
    LD_LIBRARY_PATH="$TOOLS_DIR/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$@"
  else
    "$@"
  fi
}

find_tool() {
  local name="$1"
  if [[ -x "$TOOLS_DIR/bin/$name" ]]; then
    echo "$TOOLS_DIR/bin/$name"
  elif command -v "$name" >/dev/null 2>&1; then
    command -v "$name"
  else
    return 1
  fi
}

load_config() {
  [[ -f "$CONFIG" ]] || die "$CONFIG is missing; run prepare.sh first"
  # shellcheck source=/dev/null
  source "$CONFIG"
}

qmp() {
  python3 "$SCRIPT_DIR/qmp.py" "$QMP_SOCK" "$@"
}

vm_running() {
  [[ -f "$QEMU_PID" ]] && kill -0 "$(cat "$QEMU_PID")" 2>/dev/null
}
