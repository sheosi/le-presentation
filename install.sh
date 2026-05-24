#!/bin/bash
#
# Installation script for Presentation System on Debian base (no GUI)
# Creates a locked-down kiosk system with Cage + Chromium
#
# This script clones and builds from: https://github.com/sheosi/le-presentation
# Internet connection required for installation
#

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
PRESENT_USER="present"
PRESENT_DIR="/home/$PRESENT_USER/presentations"
SERVICE_NAME="presentation-server"
DISPLAY_SERVICE="presentation-display"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Please run as root (use sudo)${NC}"
    exit 1
fi

echo -e "${GREEN}=== Presentation System Installer ===${NC}"
echo ""

# Ask about read-only mode
echo -e "${YELLOW}Read-only filesystem protection:${NC}"
echo "This protects against power outages by making the root filesystem read-only"
echo "with temporary writes stored in RAM (overlayroot)."
echo ""
read -p "Enable read-only mode? (y/N): " readonly_choice

# Update system
echo -e "${GREEN}Updating package lists...${NC}"
apt-get update

# Install core dependencies
echo -e "${GREEN}Installing core dependencies...${NC}"
apt-get install -y \
    curl \
    wget \
    git \
    build-essential \
    pkg-config \
    libssl-dev \
    libasound2-dev \
    libdbus-1-dev \
    libinotifytools0-dev

# Install audio system (PipeWire with WirePlumber)
echo -e "${GREEN}Installing audio system (PipeWire)...${NC}"
apt-get install -y \
    pipewire \
    wireplumber \
    pipewire-pulse \
    pulseaudio-utils

# Enable PipeWire services
systemctl --global enable pipewire.socket wireplumber.service 2>/dev/null || true

# Install document conversion tools
echo -e "${GREEN}Installing document conversion tools...${NC}"
apt-get install -y \
    poppler-utils \
    pdf2svg \
    xdg-utils

# Install Flatpak
echo -e "${GREEN}Installing Flatpak...${NC}"
apt-get install -y flatpak
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo || true

# Install Flatpak apps (LibreOffice, EasyEffects)
echo -e "${GREEN}Installing Flatpak applications...${NC}"
flatpak install -y flathub org.libreoffice.LibreOffice || echo -e "${YELLOW}Warning: LibreOffice installation may have failed${NC}"
flatpak install -y flathub com.github.wwmm.easyeffects || echo -e "${YELLOW}Warning: EasyEffects installation may have failed${NC}"

# Install Chromium and Cage
echo -e "${GREEN}Installing browser (Chromium) and kiosk compositor (Cage)...${NC}"
apt-get install -y \
    chromium \
    cage

# Install NetworkManager (required for embedded-device WiFi hotspot feature)
echo -e "${GREEN}Installing NetworkManager...${NC}"
apt-get install -y \
    network-manager \
    wpasupplicant

# Install Rust (if not present)
if ! command -v cargo &> /dev/null; then
    echo -e "${GREEN}Installing Rust...${NC}"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    # Also copy cargo to /usr/local/bin for other users
    cp "$HOME/.cargo/bin/cargo" /usr/local/bin/ 2>/dev/null || true
    cp "$HOME/.cargo/bin/rustc" /usr/local/bin/ 2>/dev/null || true
fi

# Create dedicated user
echo -e "${GREEN}Creating user account: $PRESENT_USER${NC}"
if ! id "$PRESENT_USER" &>/dev/null; then
    useradd -m -s /bin/bash -G video,audio,netdev,flatpak "$PRESENT_USER"
    # Disable password login (can be changed later if needed)
    passwd -l "$PRESENT_USER"
else
    echo "User $PRESENT_USER already exists"
fi

# Create presentations directory
mkdir -p "$PRESENT_DIR"
chown -R "$PRESENT_USER:$PRESENT_USER" "$PRESENT_DIR"
chmod 755 "$PRESENT_DIR"

# Set up presentations directory symlink for the binary
mkdir -p "/home/$PRESENT_USER/.config"
echo "PRESENTATIONS_DIR=$PRESENT_DIR" > "/home/$PRESENT_USER/.config/presentation.env"
chown -R "$PRESENT_USER:$PRESENT_USER" "/home/$PRESENT_USER/.config"

# Clone and build the presentation server
echo -e "${GREEN}Cloning presentation server from GitHub...${NC}"
if [ -d "/opt/le-presentation" ]; then
    rm -rf /opt/le-presentation
fi

# Clone from GitHub
git clone https://github.com/sheosi/le-presentation.git /opt/le-presentation
cd /opt/le-presentation

echo -e "${GREEN}Building presentation server...${NC}"

# Build with embedded-device feature
if command -v cargo &> /dev/null || [ -f "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
    cargo build --release --features embedded-device
    cp target/release/le-presentation /usr/local/bin/
else
    echo -e "${RED}Rust/Cargo not found. Please install Rust manually.${NC}"
    exit 1
fi

# Create systemd service for presentation server
echo -e "${GREEN}Creating systemd service: $SERVICE_NAME${NC}"
cat > "/etc/systemd/system/${SERVICE_NAME}.service" << 'EOF'
[Unit]
Description=Presentation Server
After=network.target pipewire.service
Wants=network.target

[Service]
Type=simple
User=present
Group=present
Environment=PRESENTATIONS_DIR=/home/present/presentations
Environment=PORT=8080
Environment="PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin"
Environment="XDG_RUNTIME_DIR=/run/user/1000"
WorkingDirectory=/home/present
ExecStart=/usr/local/bin/le-presentation
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

# Create Chromium wrapper script
cat > /usr/local/bin/presentation-browser << 'EOF'
#!/bin/bash
# Wrapper script for Chromium in kiosk mode

export XDG_RUNTIME_DIR=/run/user/1000
export XDG_SESSION_TYPE=wayland

/usr/bin/chromium \
    --kiosk \
    --app=http://localhost:8080/presentation \
    --no-first-run \
    --no-default-browser-check \
    --disable-restore-session-state \
    --disable-infobars \
    --disable-features=TranslateUI \
    --disable-pinch \
    --overscroll-history-navigation=0 \
    --autoplay-policy=no-user-gesture-required \
    --enable-features=WebRTC \
    --window-size=1920,1080 \
    --window-position=0,0 \
    --start-fullscreen \
    --check-for-update-interval=31536000 \
    --disable-component-update
EOF

chmod +x /usr/local/bin/presentation-browser

# Create systemd service for Cage + Chromium display
echo -e "${GREEN}Creating display service: $DISPLAY_SERVICE${NC}"
cat > "/etc/systemd/system/${DISPLAY_SERVICE}.service" << EOF
[Unit]
Description=Presentation Display (Cage + Chromium)
After=presentation-server.service pipewire.service
Requires=presentation-server.service

[Service]
Type=simple
User=$PRESENT_USER
Group=$PRESENT_USER
Environment="WLR_BACKENDS=drm"
Environment="WLR_DRM_DEVICES=/dev/dri/card0"
Environment="XDG_RUNTIME_DIR=/run/user/1000"
Environment="XDG_SESSION_TYPE=wayland"

# Wait for server to be ready
ExecStartPre=/bin/sleep 5

# Start Cage with the wrapper script
ExecStart=/usr/bin/cage -- /usr/local/bin/presentation-browser

Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# Create runtime directory for present user
mkdir -p /run/user/1000
chown present:present /run/user/1000
chmod 700 /run/user/1000

# Enable lingering for present user (allows services to run without login)
loginctl enable-linger present || true

# Reload systemd and enable services
systemctl daemon-reload
systemctl enable "$SERVICE_NAME"
systemctl enable "$DISPLAY_SERVICE"

# Enable NetworkManager
systemctl enable NetworkManager

# Configure read-only filesystem if requested
if [[ "$readonly_choice" =~ ^[Yy]$ ]]; then
    echo -e "${GREEN}Setting up read-only filesystem protection...${NC}"

    # Install overlayroot
    apt-get install -y overlayroot

    # Configure overlayroot
    cat > /etc/overlayroot.conf << 'EOF'
[OverlayRoot]
# Use tmpfs for overlay (RAM-based, resets on reboot)
tmpfs = yes

# Alternative: use a partition for persistent overlay
# lowerdir = /lower
# upperdir = /upper
# workdir = /work
EOF

    # Create initramfs hook to enable overlayroot
    cat > /etc/initramfs-tools/scripts/init-bottom/overlayroot << 'EOF'
#!/bin/sh
PREREQ=""
prereqs()
{
    echo "$PREREQ"
}

case $1 in
    prereqs)
        prereqs
        exit 0
        ;;
esac

. /scripts/functions

# Mount root read-only with overlay
# This is handled by overlayroot package, but we ensure it's enabled
log_success_msg "Overlayroot configured"
EOF
    chmod +x /etc/initramfs-tools/scripts/init-bottom/overlayroot

    # Update initramfs
    update-initramfs -u

    # Create a writable mount point for presentations (separate mount)
    cat > /etc/fstab.d/presentations.mount << EOF
# Writable presentations directory (tmpfs that persists during runtime)
tmpfs $PRESENT_DIR tmpfs defaults,noatime,mode=755,uid=1000,gid=1000 0 0
EOF

    echo -e "${GREEN}Read-only mode configured!${NC}"
    echo -e "${YELLOW}Note: Changes to system files will be lost on reboot.${NC}"
    echo -e "${YELLOW}The $PRESENT_DIR directory is writable but in RAM.${NC}"
    echo -e "${YELLOW}To make permanent changes, run: overlayroot-chroot${NC}"
fi

# Set up autologin for present user on tty1 (optional, for debugging)
mkdir -p /etc/systemd/system/getty@tty1.service.d
cat > /etc/systemd/system/getty@tty1.service.d/override.conf << EOF
[Service]
ExecStart=
ExecStart=-/sbin/agetty --autologin $PRESENT_USER --noclear %I \$TERM
EOF

# Create a README for the user
cat > "/home/$PRESENT_USER/README.txt" << EOF
Presentation System
===================

The presentation system is now installed and configured.

Access Methods:
1. Local display: Connect a monitor - it will show the presentation automatically
2. Remote access: http://$(hostname -I | awk '{print $1}'):8080/

Upload presentations to: $PRESENT_DIR
Supported formats: PNG, JPG, MP4, PDF, PPTX

WiFi Hotspot (if enabled):
SSID: Presentation-Device
Password: present123

Services:
- presentation-server: The web server (port 8080)
- presentation-display: The Cage + Chromium display

Commands:
sudo systemctl start presentation-server    # Start server
sudo systemctl start presentation-display   # Start display
sudo systemctl status presentation-server   # Check status

Logs:
sudo journalctl -u presentation-server -f  # Watch server logs
sudo journalctl -u presentation-display -f # Watch display logs
EOF

chown "$PRESENT_USER:$PRESENT_USER" "/home/$PRESENT_USER/README.txt"

echo ""
echo -e "${GREEN}=== Installation Complete! ===${NC}"
echo ""
echo -e "${GREEN}Services installed:${NC}"
echo "  - presentation-server: Web server on port 8080"
echo "  - presentation-display: Cage + Chromium kiosk"
echo "  - NetworkManager: For WiFi hotspot (embedded-device feature)"
echo ""
echo -e "${GREEN}User created:${NC} $PRESENT_USER"
echo -e "${GREEN}Presentations directory:${NC} $PRESENT_DIR"
echo ""
echo -e "${YELLOW}To start now:${NC}"
echo "  sudo systemctl start $SERVICE_NAME"
echo "  sudo systemctl start $DISPLAY_SERVICE"
echo ""
echo -e "${YELLOW}The system will auto-start on next boot.${NC}"
echo ""

if [[ "$readonly_choice" =~ ^[Yy]$ ]]; then
    echo -e "${YELLOW}Read-only mode is ENABLED.${NC}"
    echo -e "${YELLOW}Reboot to activate read-only filesystem.${NC}"
    echo ""
fi

echo -e "${GREEN}Installation complete!${NC}"
