#!/bin/bash
# Launch iPod Clickwheel Games VM on macOS (modified from original QEMU script)
# Requires QEMU with Hypervisor.framework support (hvf)

set -e

RELEASE_DIR="/Users/kurt/Downloads/iPod Clickwheel Games Preservation Project Release 16"
QEMU_BIN="/opt/homebrew/bin/qemu-system-x86_64"

cd "$RELEASE_DIR"

echo "Starting iPod Clickwheel Games Preservation VM..."
echo "Note: First boot may take several minutes"
echo ""

# Check if we have the qcow2 file
QCOW2="iPod Clickwheel Games Preservation Project.utm/Data/A973B7BF-F17A-44C5-A6D7-B6D819938FDC.qcow2"
if [ ! -f "$QCOW2" ]; then
    echo "ERROR: VM disk not found at $QCOW2"
    exit 1
fi

echo "VM disk: $QCOW2 ($(du -h "$QCOW2" | cut -f1))"

# Check for EFI vars (created on first boot)
EFI_VARS="iPod Clickwheel Games Preservation Project.utm/Data/efi_vars.fd"
if [ ! -f "$EFI_VARS" ]; then
    echo "Creating fresh EFI vars..."
    cp qemu/edk2-x86_64-vars.fd "$EFI_VARS"
fi

# Launch with headless VNC for CLI-only operation
# or use SPICE if available

# Option 1: Headless with VNC (view with vncviewer localhost:5900)
echo ""
echo "Launching VM headless (VNC on port 5900)..."
echo "Connect with: open vnc://localhost:5900"
echo "Or use: vncviewer localhost:5900"
echo ""
echo "Press Ctrl+C to stop the VM"
echo ""

# Modified for macOS hvf acceleration
exec $QEMU_BIN \
    -L qemu \
    -vnc localhost:0 \
    -nodefaults \
    -vga std \
    -smp 2 \
    -machine q35,vmport=off,i8042=off,hpet=off \
    -accel hvf \
    -cpu qemu64 \
    -global PIIX4_PM.disable_s3=1 \
    -global ICH9-LPC.disable_s3=1 \
    -drive if=pflash,format=raw,unit=0,file.filename=qemu/edk2-x86_64-code.fd,file.locking=off,readonly=on \
    -drive "if=pflash,unit=1,file=$EFI_VARS" \
    -m 4096 \
    -usb \
    -device usb-tablet,bus=usb-bus.0 \
    -device usb-mouse,bus=usb-bus.0 \
    -device usb-kbd,bus=usb-bus.0 \
    -device nec-usb-xhci,id=usb-controller-0 \
    -device ide-hd,bus=ide.0,drive=maindisk,bootindex=0 \
    -drive "if=none,media=disk,id=maindisk,file=$QCOW2,discard=unmap,detect-zeroes=unmap,cache=writethrough" \
    -rtc base=localtime \
    -name "iPod Clickwheel Games" \
    -uuid A9FC3197-EE40-4EF9-A948-461B0B194755 \
    -device virtio-rng-pci \
    -netdev user,id=net0,hostfwd=tcp::2222-:22 \
    -device e1000,netdev=net0
