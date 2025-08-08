<div align="center">

<p>
  <img src="./brand/logo-no-background.png" alt="ISOtope Logo" width="250" height="auto"/>
</p>

**A flexible, OS-agnostic ISO builder for automated deployments**

[![Rust](https://img.shields.io/badge/built%20with-Rust-orange)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Version](https://img.shields.io/badge/version-0.1.0-brightgreen.svg)](https://github.com/yourusername/isotope)

</div>

---

## 🚀 Overview

ISOtope is a powerful, Rust-based CLI tool that automates the creation of custom live ISOs through VM-based puppet automation. Using a clean, Dockerfile-inspired syntax, ISOtope orchestrates the entire process from OS installation to live system packaging.

```bash
isotope build ubuntu-server.isotope --output custom-ubuntu.iso
```

Think of ISOtope as Docker for operating systems: reproducible, automated, and version-controlled live ISO creation with the power of containerized workflows.

## ✨ Features

### **Multi-Stage Build Pipeline**
- **🤖 Puppet VM Automation**: Fully automated OS installation via keypress sequences
- **🎯 Multi-Stage Architecture**: Separate init, installation, configuration, and packaging stages
- **📋 Dockerfile-like Syntax**: Familiar, clean specification format
- **🔄 Reproducible Builds**: Version-controlled, deterministic ISO generation

### **Cross-Platform VM Support**
- **QEMU**: Full automation with monitor socket control
- **VirtualBox**: Native VBoxManage integration  
- **VMware**: vmrun and VIX API support (planned)
- **Hyper-V**: PowerShell cmdlet integration (planned)

### **Live System Configuration**
- **Command Execution**: Run any command in the live environment
- **File Management**: Copy files, set permissions, create directories
- **Package Installation**: Install software, enable services
- **System Customization**: Configure users, networks, startup scripts

### **Enterprise-Ready**
- **Checksum Verification**: SHA-256/512 validation of source ISOs
- **Template Support**: Environment variable substitution
- **Robust Error Handling**: Comprehensive logging and cleanup
- **Cross-Platform**: Windows, Linux, and macOS support

## 🔧 Installation

```bash
# Install via cargo
cargo install isotope

# Or build from source
git clone https://github.com/OmniCloudOrg/isotope.git
cd isotope
cargo build --release
```

## 📋 Quick Start

### 1. Create an Isotope Specification

Create `ubuntu-docker.isotope`:

```dockerfile
# Ubuntu Server with Docker - Dockerfile-like syntax
FROM ./ubuntu-22.04-server.iso
CHECKSUM sha256:a4acfda10b18da50e2ec50ccaf860d7f20ce1ee42895e3840b57b2b371fc734

LABEL name="ubuntu-docker-server"
LABEL version="1.0.0"
LABEL description="Ubuntu Server 22.04 with Docker pre-installed"

# STAGE init - Configure the puppet VM
STAGE init
VM provider=qemu
VM memory=4G
VM cpus=2
VM disk=20G
VM timeout=30m

# STAGE os_install - Automated OS installation
STAGE os_install
# Language selection
WAIT 30s
PRESS enter

# Network setup
WAIT 5s
PRESS enter

# Disk configuration
WAIT 3s
PRESS down
PRESS enter

# User profile setup
WAIT 10s
TYPE ubuntu
PRESS tab
TYPE ubuntu
PRESS tab
TYPE ubuntu
PRESS enter

# Wait for installation completion
WAIT 30m FOR "Installation complete!"
PRESS enter

# STAGE os_configure - Configure the live system
STAGE os_configure
# Wait for login and authenticate
WAIT 5m FOR login
TYPE ubuntu
PRESS enter
TYPE ubuntu
PRESS enter
WAIT 3s

# Install Docker and configure system
RUN apt-get update
RUN apt-get install -y docker.io curl vim
RUN systemctl enable docker
RUN usermod -aG docker ubuntu

# Copy custom configurations
COPY ./configs/docker-daemon.json /etc/docker/daemon.json
COPY ./scripts/startup.sh /usr/local/bin/startup.sh
RUN chmod +x /usr/local/bin/startup.sh

# STAGE pack - Create the final bootable ISO
STAGE pack
EXPORT ./output/ubuntu-docker-server.iso
FORMAT iso9660
BOOTABLE true
VOLUME_LABEL "Ubuntu Docker Server"
```

### 2. Build Your Custom ISO

```bash
# Build the ISO
isotope build ubuntu-docker.isotope

# Validate specification syntax
isotope validate ubuntu-docker.isotope

# Test VM boot without full build
isotope test ubuntu-docker.isotope
```

### 3. Deploy Your ISO

The generated ISO is a fully bootable live system with all your customizations baked in, ready for deployment on any hardware or VM platform.

## 🏗️ Architecture

ISOtope uses a **puppet VM** approach for maximum automation:

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Source ISO    │───▶│   Puppet VM     │───▶│   Live ISO      │
│                 │    │                 │    │                 │
│ • Ubuntu Server │    │ • Automated     │    │ • Custom Apps   │
│ • Windows 11    │    │   Installation  │    │ • Pre-config    │
│ • Any OS        │    │ • Live Config   │    │ • Bootable      │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

1. **Puppet VM**: Automated installation and configuration in controlled environment
2. **Live Capture**: System snapshot at optimal state
3. **ISO Generation**: Bootable live system with all customizations

## 📚 Documentation

### Command Line Interface

```bash
# Build an ISO from specification
isotope build <spec-file> [--output <path>]

# Validate specification syntax
isotope validate <spec-file>

# Test VM boot process
isotope test <spec-file>

# Convert JSON config to Isotope format
isotope convert <input.json> <output.isotope>
```

### Specification Format

The Isotope specification uses four distinct stages:

#### **STAGE init**
Configure the puppet VM that will build your system:
```dockerfile
STAGE init
VM provider=qemu          # qemu, virtualbox, vmware, hyperv
VM memory=4G              # RAM allocation
VM cpus=2                 # CPU count
VM disk=20G               # Disk size
VM timeout=30m            # Maximum build time
```

#### **STAGE os_install**
Automate the OS installation with keypress sequences:
```dockerfile
STAGE os_install
WAIT 30s                  # Wait for boot
PRESS enter               # Press Enter key
TYPE username             # Type text
WAIT 5m FOR "Complete"    # Wait for condition
```

#### **STAGE os_configure**
Configure the live system:
```dockerfile
STAGE os_configure
RUN apt-get update                    # Execute commands
COPY ./file.sh /usr/bin/script.sh    # Copy files
RUN systemctl enable service         # System configuration
```

#### **STAGE pack**
Package the final ISO:
```dockerfile
STAGE pack
EXPORT ./output/custom.iso    # Output path
FORMAT iso9660                # ISO format
BOOTABLE true                 # Make bootable
VOLUME_LABEL "Custom OS"      # Volume label
```

## 🛠️ Use Cases

### **Enterprise Deployment**
Create standardized OS images with security policies, corporate software, and compliance configurations for consistent deployment across thousands of workstations.

### **Cloud Infrastructure**  
Build custom cloud images optimized for specific workloads, with pre-installed monitoring agents, security tools, and performance tuning.

### **Development Environments**
Generate reproducible development environments with IDEs, runtime environments, and project dependencies pre-configured.

### **Container Runtime Hosts**
Create minimal, hardened OS images specifically designed to run containerized workloads with Docker, Kubernetes, or other container runtimes.

### **OmniCloud Use Case**
At OmniCloud, ISOtope powers our rapid VM provisioning pipeline:

- **90% faster** VM deployment (minutes vs hours)
- **Zero configuration drift** across thousands of instances  
- **On-demand customization** based on customer requirements
- **Automated testing** of new configurations before production
- **Version-controlled infrastructure** with GitOps workflows

## 🔍 Examples

See the `examples/` directory for complete specifications:

- `ubuntu-server.isotope` - Ubuntu Server with Docker
- `windows-11.isotope` - Windows 11 development environment
- `ubuntu-server.json` - Legacy JSON format (for conversion)

## 🏗️ Building from Source

```bash
# Clone the repository
git clone https://github.com/OmniCloudOrg/isotope.git
cd isotope

# Build the project
cargo build --release

# Run tests
cargo test

# Install locally
cargo install --path .
```

### Prerequisites

**Linux/macOS:**
- QEMU (for VM automation)
- `mkisofs` or `genisoimage` (for ISO creation)
- Standard POSIX tools

**Windows:**
- QEMU or VirtualBox
- PowerShell 5.1+ (for ISO handling)
- Windows Subsystem for Linux (recommended)

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Development Priorities

- [ ] VMware provider implementation
- [ ] Hyper-V provider implementation  
- [ ] GUI installer automation improvements
- [ ] Multi-architecture support (ARM64)
- [ ] Container-based builds (Docker-in-Docker)

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 📞 Support

- Create an [Issue](https://github.com/OmniCloudOrg/Isotope/issues)
- Join discussions in our community forums
- Check out the [Wiki](https://github.com/OmniCloudOrg/Isotope/wiki) for advanced usage

---

<div align="center">
  <sub>Built with ❤️ by the OmniCloud Community</sub>
</div>