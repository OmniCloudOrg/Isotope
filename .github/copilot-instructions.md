# Isotope - Rust CLI for Automated ISO Building

Always reference these instructions first and fallback to search or additional context gathering only when the information in the instructions is incomplete or found to be in error.

## Working Effectively

### Initial Setup and Build
- Install Rust stable toolchain: Use the version that comes with the system or install via `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- Install required external dependencies:
  ```bash
  sudo apt-get update
  sudo apt-get install -y qemu-system-x86 qemu-utils xorriso genisoimage
  ```
- Optional but recommended: Install VirtualBox for additional VM provider support
- Build the project: `cargo build --verbose` 
  - **NEVER CANCEL: Takes 4-5 minutes for clean build. Set timeout to 600+ seconds.**
  - **NEVER CANCEL: Incremental builds take 1-2 minutes. Set timeout to 300+ seconds.**
- Build release version: `cargo build --release`
  - **NEVER CANCEL: Takes 3-4 minutes. Set timeout to 480+ seconds.**

### Testing and Validation
- Run tests: `cargo test --verbose`
  - **Takes 20-30 seconds. Set timeout to 120+ seconds.**
  - Note: Currently has 0 tests, but command validates the test infrastructure works
- Quick build check: `cargo check`
  - Takes 5-10 seconds for incremental checks
- Validate code formatting: `cargo fmt --check`
  - Takes <1 second, shows formatting issues
- Fix formatting: `cargo fmt`
  - Takes <1 second, automatically fixes formatting
- Run linter: `cargo clippy --verbose`
  - **Takes 30-60 seconds. Set timeout to 120+ seconds.**
- Auto-fix some linter issues: `cargo clippy --fix --allow-dirty --allow-staged`
  - **Takes 30-60 seconds. Set timeout to 120+ seconds.**

### Using the CLI Tool
- Test basic functionality: `./target/debug/isotope --help`
- Check version: `./target/debug/isotope --version`
- Validate isotope specification: `./target/debug/isotope validate examples/ubuntu-server.isotope`
  - Note: Requires referenced files (configs/, scripts/) to exist for full validation
- Build ISO from specification: `./target/debug/isotope build examples/ubuntu-server.isotope`
  - **NEVER CANCEL: This can take 30+ minutes depending on the ISO and VM operations**
- Test specification in VM: `./target/debug/isotope test examples/ubuntu-server.isotope`
  - **NEVER CANCEL: VM operations can take 10-45 minutes**

## Validation Scenarios

After making changes, always run these validation steps:

### Essential Pre-Commit Validation
1. **Format and lint code**: 
   ```bash
   cargo fmt
   cargo clippy --fix --allow-dirty --allow-staged
   ```
2. **Build and test**:
   ```bash
   cargo build --verbose  # NEVER CANCEL: 1-5 minutes
   cargo test --verbose   # NEVER CANCEL: 30+ seconds  
   ```
3. **Validate CLI functionality**:
   ```bash
   ./target/debug/isotope --help
   ./target/debug/isotope validate examples/ubuntu-server.isotope
   ```

### Complete End-to-End Validation
- Test example isotope specification validation
- Ensure required external tools are detected (build output shows QEMU/xorriso found)
- Run CLI help and version commands to verify binary works
- For ISO building features: create required config files (configs/, scripts/) before testing

## Dependency Management

### Required for Core Development
- **Rust toolchain**: Stable version (1.70+)
- **Cargo**: Package manager (comes with Rust)

### Required for Full Functionality  
- **QEMU**: `qemu-system-x86_64` for VM automation
- **xorriso**: For ISO creation and extraction
- **genisoimage/mkisofs**: Alternative ISO manipulation tools

### Optional Dependencies
- **VirtualBox**: `VBoxManage` for VirtualBox VM provider support
- **VMware**: vmrun for VMware provider (planned)
- **Hyper-V**: PowerShell cmdlets for Hyper-V provider (planned)

### Dependency Verification
The build script automatically detects available tools and provides warnings for missing dependencies. Check build output for messages like:
- "✓ QEMU found" / "✗ QEMU not found" 
- "✓ xorriso found" / "✗ xorriso not found"
- "✓ VirtualBox found" / "✗ VirtualBox not found"

## Common Tasks and Navigation

### Project Structure
- `src/main.rs` - CLI entry point with command parsing
- `src/automation/` - VM automation, keypress sequences, OCR, puppet control
- `src/automation/vm/providers/` - VM provider implementations (QEMU, VirtualBox, etc.)
- `src/core/` - Core ISO building logic, builder patterns
- `src/iso/` - ISO creation, extraction, and packaging
- `src/config/` - Isotope specification parsing and validation
- `src/utils/` - Utilities (filesystem, templating, checksums, VM metadata)
- `examples/` - Example isotope specifications
- `build.rs` - Build script that detects platform capabilities

### Key Files to Monitor
- `Cargo.toml` - Dependencies and project configuration
- `examples/ubuntu-server.isotope` - Main example specification
- `.github/workflows/rust.yml` - CI/CD pipeline configuration

### Common File Types
- `*.isotope` - Isotope specification files (Dockerfile-like syntax)
- `*.rs` - Rust source code
- `*.toml` - Configuration files (Cargo.toml, etc.)
- `*.json` - Legacy configuration format (can be converted)

## Build Timing and Requirements

### Expected Build Times (Set Appropriate Timeouts)
- **Clean cargo build**: 4-5 minutes - **NEVER CANCEL, use 600+ second timeout**
- **Incremental cargo build**: 1-2 minutes - **NEVER CANCEL, use 300+ second timeout**  
- **Release build**: 3-4 minutes - **NEVER CANCEL, use 480+ second timeout**
- **cargo test**: 20-30 seconds - **NEVER CANCEL, use 120+ second timeout**
- **cargo clippy**: 30-60 seconds - **NEVER CANCEL, use 120+ second timeout**
- **cargo check**: 5-10 seconds
- **cargo fmt**: <1 second

### Resource Requirements
- RAM: 2GB+ for building (4GB+ recommended for release builds)
- Disk: 1GB+ for dependencies and build artifacts
- CPU: Build is CPU-intensive, more cores = faster builds

## Troubleshooting Common Issues

### Build Failures
- **Missing external tools**: Install qemu-system-x86, xorriso, genisoimage as shown above
- **Rust version issues**: Update to stable Rust 1.70+
- **Out of disk space**: Clean with `cargo clean` if needed
- **Network issues during build**: Dependencies download from crates.io

### Runtime Issues  
- **"VirtualBox not found"**: Expected warning if VBoxManage not installed
- **Validation failures**: Ensure referenced files in isotope specs exist
- **Permission issues**: Ensure user can run qemu and access required files

### CI/CD Integration
- The project uses GitHub Actions with the workflow in `.github/workflows/rust.yml`
- Always run `cargo fmt` and `cargo clippy` before committing
- CI runs `cargo build --verbose` and `cargo test --verbose`

## Development Workflow Tips

### Efficient Development Cycle
1. Use `cargo check` for fast syntax checking during development
2. Use `cargo clippy` to catch common mistakes
3. Use `cargo fmt` to maintain consistent formatting
4. Use `./target/debug/isotope validate` to test specification changes
5. Always test CLI functionality after making changes to core logic

### Performance Considerations
- Use `cargo build` for development (faster compilation, includes debug symbols)
- Use `cargo build --release` for performance testing (optimized, slower compilation)
- VM operations are inherently slow - expect 10-45 minute scenarios for full ISO builds

### Code Quality Standards
- All code must pass `cargo fmt` formatting
- All code should pass `cargo clippy` without warnings where possible
- Maintain comprehensive error handling using `anyhow` and `thiserror`
- Use structured logging with `tracing` for debugging VM automation