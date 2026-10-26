---
name: xiao-probe-rs-fix
description: OBSOLETE since probe-rs 0.32.0 (fixed upstream). Only relevant if stuck on probe-rs <= 0.31.x. Fix probe-rs "Error: Did not expect multiple probes" when flashing or running with the XIAO Debug Mate (ESP32-S3 DAPLink, no USB serial number). Use when `cargo run` or `probe-rs run` fails with that error, or when setting up a patched probe-rs runner in .cargo/config.toml.
---

# XIAO Debug Mate & probe-rs Multiple Probes Issue

> **No longer required.** Fixed upstream in [PR #3907](https://github.com/probe-rs/probe-rs/pull/3907) (merged 2026-03-23, closes [#3906](https://github.com/probe-rs/probe-rs/issues/3906)) and released in **probe-rs 0.32.0** (2026-07-22). Upgrade (`cargo install probe-rs-tools --locked --force`) and use the stock `probe-rs` runner. The workaround below only applies if you are stuck on 0.31.x or older.

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
