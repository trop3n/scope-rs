# Milestone 16: Distribution — GitHub Actions CI/CD

This milestone adds automated building, testing, and release infrastructure using GitHub Actions. Before this, all building and testing happened locally.

## Cargo Release Profiles

Rust's `Cargo.toml` supports `[profile.release]` to configure how release builds are optimized:

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = true
panic = "abort"
```

### What each setting does

**`opt-level = 3`** — Maximum optimization level. The compiler spends more time optimizing to produce faster code. Level 3 enables all optimizations including aggressive inlining and vectorization.

**`lto = "fat"`** — Link-Time Optimization. Normally the compiler optimizes each crate independently. With LTO, the linker sees *all* the code at once and can optimize across crate boundaries — removing unused functions, inlining across crates, and more. "fat" LTO is the most thorough (but slowest to compile).

**`codegen-units = 1`** — By default, Rust splits each crate into multiple "codegen units" that compile in parallel. This speeds up compilation but limits optimization because the compiler can't see across unit boundaries. Setting this to 1 means the entire crate is one unit — slower to compile, but better optimized.

**`strip = true`** — Removes debug symbols and other metadata from the final binary. This significantly reduces binary size (often 50-80% smaller) but makes debugging crash reports harder.

**`panic = "abort"`** — When a panic occurs, immediately abort the process instead of unwinding the stack. This removes all the unwinding machinery from the binary, making it smaller and slightly faster. The trade-off: destructors (`Drop` impls) won't run on panic, and you can't catch panics with `std::panic::catch_unwind`.

### Size impact

These settings together typically reduce binary size by 60-80% compared to a default release build. For scope-rs, this means the binary goes from ~30MB to ~8-10MB.

## GitHub Actions: CI Workflow

The CI workflow (`.github/workflows/ci.yml`) runs on every push and pull request to `main`.

### Matrix builds

GitHub Actions supports **matrix strategies** — running the same job across multiple configurations:

```yaml
strategy:
  fail-fast: false
  matrix:
    os: [ubuntu-latest, windows-latest, macos-latest]
```

This creates 3 parallel jobs, one per platform. Each runs independently.

**`fail-fast: false`** means if one platform fails, the others keep running. This is important because a failure on Windows doesn't mean Linux and macOS are broken — you want to see all results.

### Steps breakdown

1. **Checkout** — `actions/checkout@v4` clones the repository
2. **Rust toolchain** — `dtolnay/rust-toolchain@stable` installs the latest stable Rust with `rustfmt` and `clippy` components
3. **Cache** — `actions/cache@v4` caches `~/.cargo/registry`, `~/.cargo/git`, and `target/` keyed on `Cargo.lock`. This means subsequent CI runs skip downloading and compiling unchanged dependencies
4. **Linux dependencies** — Conditional step (`if: runner.os == 'Linux'`) that installs system libraries needed by our dependencies:
   - `libasound2-dev` — ALSA audio (for `cpal`)
   - `libudev-dev` — Device enumeration
   - `libxkbcommon-dev` — Keyboard handling (for `winit`/`eframe`)
   - `libgtk-3-dev` — File dialogs (for `rfd`)
   - `libgl-dev` — OpenGL (for `eframe`)
5. **Build** — `cargo build --verbose`
6. **Test** — `cargo test --verbose`
7. **Format check** — `cargo fmt -- --check` verifies code matches `rustfmt` style without modifying files
8. **Clippy** — `cargo clippy -- -D warnings` runs Rust's linter and treats all warnings as errors

### Platform dependencies

Different platforms need different system libraries. On Linux, GUI libraries aren't bundled — you install them via the system package manager. On macOS and Windows, the frameworks are part of the OS.

This is why the Linux dependencies step uses `if: runner.os == 'Linux'` — it only runs on the Ubuntu runner.

## GitHub Actions: Release Workflow

The release workflow (`.github/workflows/release.yml`) triggers on tag pushes matching `v*` (e.g., `v0.1.0`).

### Two-job design

**Job 1: `build-release`** — Runs in parallel across 3 platforms using a matrix with `include`:

```yaml
matrix:
  include:
    - os: ubuntu-latest
      target: x86_64-unknown-linux-gnu
      artifact: scope-rs-linux-x86_64
    - os: windows-latest
      target: x86_64-pc-windows-msvc
      artifact: scope-rs-windows-x86_64.exe
    - os: macos-latest
      target: aarch64-apple-darwin
      artifact: scope-rs-macos-aarch64
```

The `include` form (vs a simple list) lets you define different variables per entry. Each job:
1. Installs Rust with the specific target triple
2. Builds with `cargo build --release --target <target>`
3. Renames the binary to the artifact name
4. Uploads it as a GitHub Actions artifact

**Job 2: `create-release`** — Depends on `build-release` (all 3 must succeed). It:
1. Downloads all artifacts
2. Creates a GitHub Release using `softprops/action-gh-release@v2`
3. Attaches all 3 binaries
4. Auto-generates release notes from commits since the last tag

### Rust target triples

Rust uses **target triples** to identify platforms: `<arch>-<vendor>-<os>-<env>`. Examples:
- `x86_64-unknown-linux-gnu` — 64-bit Linux with glibc
- `x86_64-pc-windows-msvc` — 64-bit Windows with MSVC toolchain
- `aarch64-apple-darwin` — ARM64 macOS (Apple Silicon)

You install additional targets with `rustup target add <triple>` and cross-compile with `cargo build --target <triple>`.

## Release process

To create a release:

```bash
# Tag the commit
git tag v0.1.0

# Push the tag (triggers the release workflow)
git push origin v0.1.0
```

GitHub Actions will:
1. Build optimized binaries on all 3 platforms in parallel
2. Create a GitHub Release page with auto-generated notes
3. Attach the binaries for users to download

## Rust concepts covered

- **Cargo profiles** — `[profile.release]` and how each setting affects the binary
- **LTO (Link-Time Optimization)** — Cross-crate optimization at link time
- **Target triples** — How Rust identifies build platforms
- **CI/CD** — Automated testing and release creation
- **Matrix strategies** — Running jobs across multiple configurations in parallel
- **Caching** — Speeding up CI by caching compiled dependencies
- **Conditional steps** — Platform-specific logic in workflows
