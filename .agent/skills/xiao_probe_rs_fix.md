# XIAO Debug Mate & probe-rs Multiple Probes Issue

## The Problem
When using the **XIAO Debug Mate** (or similar ESP32-S3 based DAPLink debuggers) with `probe-rs`, you may encounter the following error when running `cargo run`:

```text
Error: Did not expect multiple probes
```

### Root Cause
This is a known bug in `probe-rs` ([Issue #3906](https://github.com/probe-rs/probe-rs/issues/3906)). 
The XIAO Debug Mate USB descriptor defines two identical CMSIS-DAP interfaces but fails to provide a unique USB **Serial Number**. 

When the `probe-rs` CLI prompts you to select one of the duplicate interfaces, it uses the selected probe's internal selector (which lacks a serial number) to verify the choice. Because the selector is still ambiguous (due to the missing serial), the core library returns multiple matches again, causing the CLI to panic with `Did not expect multiple probes`.

## The Fix (Local Custom Build)
Until the issue is fixed upstream in `probe-rs` or via a hardware firmware update that adds a proper USB serial number to the Debug Mate, the best workaround for smooth development is to build a locally patched version of `probe-rs`.

### 1. Clone probe-rs
Clone the repository matching your current `probe-rs` version (e.g., `v0.31.0`):
```bash
git clone --depth 1 --branch v0.31.0 https://github.com/probe-rs/probe-rs.git
```

### 2. Apply the Patch
In the cloned repository, edit the file `probe-rs-tools/src/bin/probe-rs/util/cli.rs`.
Locate the `SelectProbeResult::MultipleProbes(_)` match arm inside the interactive selection loop (around line 130).

**Change this:**
```rust
SelectProbeResult::MultipleProbes(_) => {
    anyhow::bail!("Did not expect multiple probes")
}
```

**To this:**
```rust
SelectProbeResult::MultipleProbes(_) => {
    Ok(probe.clone())
}
```
This forces the CLI to blindly accept the user's interactive selection, bypassing the redundant validation check that triggers the panic.

### 3. Build the Patched Binary
```bash
cd probe-rs
cargo build --release --bin probe-rs
```

### 4. Update the Project Runner
Finally, update your project's `.cargo/config.toml` to use your locally compiled binary instead of the system-installed `probe-rs`:

```toml
[target.'cfg(all(target_arch = "arm", target_os = "none"))']
runner = "/absolute/path/to/your/cloned/probe-rs/target/release/probe-rs run --chip <YOUR_CHIP>"
```

Now, when you run `cargo run`, it will prompt you for the interface once (usually `0`), and successfully flash and attach the `defmt` logging console without crashing.
