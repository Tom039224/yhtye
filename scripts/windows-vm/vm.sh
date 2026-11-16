#!/usr/bin/env bash
# Run and drive the Windows 11 test VM (QEMU/KVM, UEFI + swtpm TPM 2.0). See README.md next to this file.
#
#   vm.sh install            first boot from the install ISO + unattend.iso (unattended install)
#   vm.sh start              boot the installed VM (headless; VNC and SSH on 127.0.0.1)
#   vm.sh stop [SECONDS]     ACPI shutdown, wait (default 180 s), then force off
#   vm.sh kill               force off immediately
#   vm.sh status             running or not, ports, guest state
#   vm.sh ssh [COMMAND...]   SSH as the admin user (PowerShell); no COMMAND = interactive
#   vm.sh wait-ssh [SECONDS] wait until SSH answers (default 1800 s)
#   vm.sh screenshot [FILE]  PNG of the screen (default $VM_DIR/screenshots/<time>.png)
#   vm.sh key KEYS           send keys, e.g. `vm.sh key ctrl-alt-delete` or `vm.sh key ret`
#   vm.sh vnc                print the VNC address and open a viewer if one is installed
#   vm.sh qmp COMMAND [JSON] raw QMP command
set -euo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

ssh_opts=(-i "$SSH_KEY" -p "$SSH_PORT" -o IdentitiesOnly=yes -o StrictHostKeyChecking=no
  -o UserKnownHostsFile="$VM_DIR/known_hosts" -o ConnectTimeout=5 -o LogLevel=ERROR)

start_swtpm() {
  # --terminate: swtpm exits when QEMU closes the socket, so nothing is left behind after shutdown.
  rm -f "$SWTPM_SOCK"
  with_tools_env swtpm socket --tpm2 --tpmstate dir="$TPM_DIR" \
    --ctrl type=unixio,path="$SWTPM_SOCK" --terminate --daemon \
    --log file="$VM_DIR/swtpm.log",level=1 --pid file="$VM_DIR/swtpm.pid"
  for _ in $(seq 50); do [[ -S "$SWTPM_SOCK" ]] && return 0; sleep 0.1; done
  die "swtpm did not create $SWTPM_SOCK (see $VM_DIR/swtpm.log)"
}

start_vm() {
  local install="$1"
  vm_running && die "already running (pid $(cat "$QEMU_PID"))"
  [[ -f "$DISK" && -f "$OVMF_VARS" ]] || die "run prepare.sh first"
  local media=()
  if [[ "$install" == 1 ]]; then
    [[ -f "$ISO" ]] || die "$ISO is missing (download-iso.sh)"
    [[ -f "$UNATTEND_ISO" ]] || die "$UNATTEND_ISO is missing (prepare.sh)"
    # shellcheck disable=SC2054  # commas are QEMU option syntax
    media=(-drive file="$ISO",media=cdrom,if=none,id=cd0,readonly=on -device ide-cd,drive=cd0,bus=ide.1,bootindex=0
      -drive file="$UNATTEND_ISO",media=cdrom,if=none,id=cd1,readonly=on -device ide-cd,drive=cd1,bus=ide.2)
  fi
  start_swtpm
  rm -f "$QMP_SOCK"
  # SATA (AHCI) disk and e1000e NIC: Windows has the drivers in the box, no virtio ISO needed.
  qemu-system-x86_64 \
    -name "$VM_NAME" \
    -machine q35,accel=kvm \
    -cpu host,hv_relaxed,hv_spinlocks=0x1fff,hv_vapic,hv_time \
    -smp "$VM_CPUS",sockets=1,cores="$VM_CPUS",threads=1 \
    -m "$VM_MEM" \
    -rtc base=localtime \
    -drive if=pflash,format=raw,unit=0,file="$OVMF_CODE",readonly=on \
    -drive if=pflash,format=raw,unit=1,file="$OVMF_VARS" \
    -chardev socket,id=chrtpm,path="$SWTPM_SOCK" -tpmdev emulator,id=tpm0,chardev=chrtpm -device tpm-tis,tpmdev=tpm0 \
    -drive file="$DISK",if=none,id=disk0,format=qcow2,discard=unmap,cache=writeback \
    -device ide-hd,drive=disk0,bus=ide.0,bootindex=1 \
    "${media[@]}" \
    -netdev user,id=net0,hostfwd=tcp:127.0.0.1:"$SSH_PORT"-:22 -device e1000e,netdev=net0 \
    -device VGA,vgamem_mb=64,xres=1600,yres=900 \
    -device qemu-xhci -device usb-tablet \
    -display vnc=127.0.0.1:"$VNC_DISPLAY" \
    -qmp unix:"$QMP_SOCK",server=on,wait=off \
    -monitor none -serial none \
    -pidfile "$QEMU_PID" -daemonize
  echo "started: VNC 127.0.0.1:$((5900 + VNC_DISPLAY)), SSH 127.0.0.1:$SSH_PORT, QMP $QMP_SOCK"
  if [[ "$install" == 1 ]]; then
    # Get past "Press any key to boot from CD or DVD..." (otherwise firmware falls through to the empty disk).
    qmp spam-key ret 20
    echo "installing unattended; follow it with 'vm.sh screenshot' or VNC, then 'vm.sh wait-ssh'"
  fi
}

stop_vm() {
  local timeout="${1:-180}"
  vm_running || { echo "not running"; return 0; }
  qmp system_powerdown
  for _ in $(seq "$timeout"); do vm_running || { echo "stopped"; return 0; }; sleep 1; done
  echo "no shutdown after ${timeout}s; forcing off"
  qmp quit || true
}

cmd="${1:-status}"
shift || true
case "$cmd" in
  install) start_vm 1 ;;
  start) start_vm 0 ;;
  stop) stop_vm "$@" ;;
  kill) vm_running && qmp quit; echo "stopped" ;;
  status)
    if vm_running; then
      echo "running (pid $(cat "$QEMU_PID")): VNC 127.0.0.1:$((5900 + VNC_DISPLAY)), SSH 127.0.0.1:$SSH_PORT"
      qmp query-status
    else
      echo "not running"
    fi
    ;;
  ssh)
    load_config
    exec ssh "${ssh_opts[@]}" "$ADMIN_USER@127.0.0.1" "$@"
    ;;
  wait-ssh)
    load_config
    deadline=$((SECONDS + ${1:-1800}))
    until ssh "${ssh_opts[@]}" -o BatchMode=yes "$ADMIN_USER@127.0.0.1" 'exit 0' 2>/dev/null; do
      ((SECONDS < deadline)) || die "SSH did not come up"
      vm_running || die "VM is not running"
      sleep 10
    done
    echo "SSH is up"
    ;;
  screenshot)
    file="${1:-$VM_DIR/screenshots/$(date +%Y%m%d-%H%M%S).png}"
    mkdir -p "$(dirname "$file")"
    file="$(realpath -m "$file")"
    qmp screendump "{\"filename\": \"$file\", \"format\": \"png\"}"
    echo "$file"
    ;;
  key) qmp hmp sendkey "$@" ;;
  vnc)
    addr="127.0.0.1:$((5900 + VNC_DISPLAY))"
    echo "VNC: $addr (no password; listening on localhost only)"
    for viewer in vncviewer gvncviewer remmina krdc; do
      if command -v "$viewer" >/dev/null; then
        case "$viewer" in
          remmina) exec remmina -c "vnc://$addr" ;;
          krdc) exec krdc "vnc://$addr" ;;
          gvncviewer) exec gvncviewer "127.0.0.1:$VNC_DISPLAY" ;;
          *) exec "$viewer" "$addr" ;;
        esac
      fi
    done
    echo "no VNC viewer found (e.g. install tigervnc and run: vncviewer $addr)"
    ;;
  qmp) qmp "$@" ;;
  *) sed -n '2,15p' "$0"; exit 2 ;;
esac
