# Isotope - ISO Builder CLI Tool

Always reference these instructions first and fallback to search or bash commands only when you encounter unexpected information that does not match the info here.

## Working Effectively

### Bootstrap and Build
- Install system dependencies first:
  - `sudo apt-get update && sudo apt-get install -y qemu-system-x86 xorriso`
  - VirtualBox is optional: `sudo apt-get install -y virtualbox` (if needed)
- Build the project:
  - Debug build: `cargo build` -- takes 1.5 minutes. NEVER CANCEL. Set timeout to 5+ minutes.
  - Release build: `cargo build --release` -- takes 7.5 minutes first time, 3.5 minutes incremental. NEVER CANCEL. Set timeout to 15+ minutes.
- Run tests: `cargo test` -- takes 1.5 minutes first time, 20 seconds incremental. NEVER CANCEL. Set timeout to 5+ minutes.
- Format code: `cargo fmt` -- takes 1 second. Always run before committing.
- Lint code: `cargo clippy` -- takes 40 seconds. NEVER CANCEL. Set timeout to 2+ minutes.

### Run the CLI Tool
- ALWAYS build first before testing CLI functionality.
- Basic help: `./target/release/isotope --help`
- Validate specifications: `./target/release/isotope validate examples/ubuntu-server.isotope`
- Convert JSON to Isotope format: `./target/release/isotope convert input.json output.isotope`
- Test VM functionality: `./target/release/isotope test examples/ubuntu-server.isotope` (requires internet for OCR models)

### Required Directory Structure for Examples
Create these directories and files before validating examples:
- `mkdir -p configs scripts`
- Create `configs/docker-daemon.json` with valid Docker daemon configuration
- Create `scripts/startup.sh` with executable startup script
- Files are referenced by examples but not included in repository

## Validation

### Build Validation
- ALWAYS run `cargo build --release` and wait for completion before making changes
- Build warnings about missing tools (QEMU, VirtualBox, xorriso) are normal on some systems but don't prevent building
- Build creates binary at `./target/release/isotope`

### CLI Validation Scenarios
- Test basic help: `./target/release/isotope --help` should show all subcommands (build, validate, test, convert)
- Test validation: `./target/release/isotope validate examples/ubuntu-server.isotope` should succeed if example dependencies exist
- Test version: `./target/release/isotope --version` should show current version

### Code Quality Validation
- ALWAYS run `cargo fmt` before committing - the codebase has formatting inconsistencies
- ALWAYS run `cargo clippy` before committing - currently has warnings but should not fail
- Tests are minimal (only 7 unit tests) but should all pass

### Functional Testing Limitations
- Full ISO building requires actual ISO source files (not included in repository)
- VM testing requires internet access for OCR model downloads
- VirtualBox provider requires VirtualBox installation
- Testing against real ISO files is not feasible in CI environment

## Common Tasks

### Building and Testing
```bash
# Full clean build and test cycle
cargo clean
cargo build --release  # 7.5 minutes - NEVER CANCEL
cargo test             # 1.5 minutes first time, 20 seconds incremental - NEVER CANCEL
cargo clippy           # 40 seconds - NEVER CANCEL
cargo fmt              # Always run before commit
```

### Working with Specifications
- Isotope specifications use Dockerfile-like syntax with .isotope extension
- Four main stages: init (VM config), os_install (keypress automation), os_configure (commands), pack (ISO creation)
- Examples in `examples/` directory demonstrate syntax
- Validation checks syntax and file references but not actual ISO building

### Dependency Management
- Core dependencies defined in Cargo.toml
- External tool dependencies: QEMU, xorriso (required), VirtualBox (optional)
- Internet access required for OCR model downloads during testing
- Build script detects available tools but warnings don't prevent building

## Project Structure

### Key Directories
```
src/
├── automation/     # VM automation and OCR logic
├── cli.rs         # Command line interface definitions  
├── config/        # Specification parsing and validation
├── core/          # Core building and testing logic
├── iso/           # ISO creation and packaging
├── utils/         # Utility functions and helpers
├── main.rs        # Application entry point
└── lib.rs         # Library interface
```

### Important Files
- `Cargo.toml` - Project configuration and dependencies
- `build.rs` - Platform detection and build configuration
- `.github/workflows/rust.yml` - CI/CD pipeline
- `examples/ubuntu-server.isotope` - Example specification
- `README.md` - Project documentation and usage examples

### Generated/Build Files
- `target/` - Cargo build artifacts (excluded from git)
- `.isometa` - VM metadata tracking (excluded from git)
- `*.iso`, `*.vdi` - Generated disk images (excluded from git)

## Development Guidelines

### Code Style
- Use `cargo fmt` to maintain consistent formatting
- Address `cargo clippy` warnings when possible
- Current codebase has many unused function warnings - this is normal for early development
- Follow existing patterns in each module

### Testing Strategy
- Unit tests are minimal - focus on integration testing with CLI commands
- Test specifications against validation command before building
- Create mock configuration files for testing examples
- Manual testing required for VM automation features

### Common Issues and Solutions
- **Build warnings about missing tools**: Install QEMU/xorriso or ignore if not doing full ISO builds
- **OCR model download failures**: Requires internet access, skip test command if network restricted
- **Formatting check failures**: Run `cargo fmt` to fix automatically
- **Empty convert output**: Convert feature may be incomplete - validate input JSON format
- **Missing example dependencies**: Create required configs/ and scripts/ directories with placeholder files

## External Dependencies

### Required for Full Functionality
- **QEMU**: `sudo apt-get install -y qemu-system-x86` 
  - Command: `qemu-system-x86_64`
  - Purpose: VM automation backend
- **xorriso**: `sudo apt-get install -y xorriso`
  - Command: `xorriso`  
  - Purpose: ISO creation and manipulation

### Optional Dependencies
- **VirtualBox**: `sudo apt-get install -y virtualbox`
  - Command: `VBoxManage`
  - Purpose: Alternative VM backend
- **Internet access**: Required for OCR model downloads during test operations

### Installation Verification
```bash
which qemu-system-x86_64  # Should return /usr/bin/qemu-system-x86_64
which xorriso             # Should return /usr/bin/xorriso  
qemu-system-x86_64 --version  # Should show QEMU version
xorriso --version         # Should show xorriso version
```