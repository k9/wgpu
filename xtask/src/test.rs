use std::ffi::OsString;

use anyhow::{bail, Context};
use pico_args::Arguments;
use serde_json::Value;
use xshell::Shell;

use crate::{install_warp, util::flatten_args};

pub fn run_tests(
    shell: Shell,
    mut args: Arguments,
    passthrough_args: Option<Vec<OsString>>,
) -> anyhow::Result<()> {
    let llvm_cov = args.contains("--llvm-cov");
    let list = args.contains("--list");

    // Determine the build profile from arguments
    let is_release = args.contains("--release");
    let custom_profile = args
        .opt_value_from_str::<_, String>("--cargo-profile")
        .ok()
        .flatten();
    let profile = if is_release {
        "release"
    } else if let Some(ref p) = custom_profile {
        p.as_str()
    } else {
        "debug"
    };

    let mut cargo_args = flatten_args(args, passthrough_args);

    // Re-add profile flags that were consumed during argument parsing
    #[expect(clippy::manual_map)] // This is much clearer than using map()
    let profile_arg = if is_release {
        Some(OsString::from("--release"))
    } else if let Some(ref p) = custom_profile {
        Some(OsString::from(format!("--cargo-profile={p}")))
    } else {
        None
    };

    if let Some(ref profile_arg) = profile_arg {
        cargo_args.insert(0, profile_arg.clone());
    }

    // Retries handled by cargo nextest natively

    // Install WARP on Windows for D3D12 testing
    if cfg!(target_os = "windows") {
        let llvm_cov_dir = if llvm_cov {
            "target/llvm-cov-target"
        } else {
            "target"
        };
        let target_dir = format!("{llvm_cov_dir}/{profile}");
        install_warp::install_warp(&shell, &target_dir)?;
    }

    let test_suite_run_flags: &[_] = if llvm_cov {
        &["llvm-cov", "--no-cfg-coverage", "--no-report", "nextest"]
    } else {
        &["nextest", "run"]
    };

    log::info!("Generating .gpuconfig file based on gpus on the system");

    // We use a test to generate the .gpuconfig file instead of using the cli directly
    // as `cargo run --bin wgpu-info` would build a different set of dependencies, causing
    // incremental changes to need to rebuild the wgpu stack twice, one for the tests
    // and once for the cli binary.
    //
    // Needs to be kept in sync with the test in wgpu-info/src/tests.rs
    shell
        .cmd("cargo")
        .args(test_suite_run_flags)
        // Use the same build configuration as the main tests, so that we only build once.
        .args(["--benches", "--tests", "--all-features"])
        // Use the same cargo profile as the main tests.
        .args(profile_arg)
        // We need to tell nextest to filter by binary too, so it doesn't try to enumerate
        // tests on any of the gpu enabled test binaries, as that will fail due to
        // old or missing .gpuconfig files.
        .args(["-E", "binary(wgpu-info)", "generate_gpuconfig_report"])
        // Turn on the env var for saving the .gpuconfig files
        .env("WGPU_INFO_SAVE_GPUCONFIG_REPORT", "1")
        .quiet()
        .run()
        .context("Failed to run tests to generate .gpuconfig")?;

    let gpu_count = shell
        .read_file(".gpuconfig")
        .unwrap()
        .lines()
        .filter(|line| line.contains("name"))
        .count();

    log::info!(
        "Found {} gpu{}",
        gpu_count,
        if gpu_count == 1 { "" } else { "s" }
    );

    if list {
        log::info!("Listing tests");
        shell
            .cmd("cargo")
            .args(["nextest", "list"])
            .args(["-v", "--benches", "--tests", "--all-features"])
            .args(cargo_args)
            .run()
            .context("Failed to list tests")?;
        return Ok(());
    }
    log::info!("Running cargo tests");

    shell
        .cmd("cargo")
        .args(test_suite_run_flags)
        .args(["--benches", "--tests", "--all-features"])
        .args(cargo_args)
        .quiet()
        .run()
        .context("Tests failed")?;

    log::info!("Finished tests");

    Ok(())
}

pub fn run_wasm_tests(
    shell: Shell,
    mut args: Arguments,
    passthrough_args: Option<Vec<OsString>>,
) -> anyhow::Result<()> {
    for file in shell.read_dir("wasm-test/web")? {
        shell.copy_file(file, "wasm-test/dist/")?;
    }

    let args = [
        "build",
        "--target",
        "wasm32-unknown-unknown",
        "--test",
        "wgpu-gpu",
        "--features",
        "wgsl,webgl,web,fragile-send-sync-non-atomic-wasm",
    ];

    let cmd = shell.cmd("cargo").args(args);
    cmd.run()?;

    let build_output = shell
        .cmd("cargo")
        .args(args)
        .args(["--message-format=json", "-q"])
        .output()?;

    let build_output = String::from_utf8(build_output.stdout)?;

    let mut executable_path = None;
    for line in build_output.lines() {
        let line: serde_json::Value =
            serde_json::from_str(line).context("Failed to parse wasm test build output")?;

        if let Some(reason) = line.get("reason") {
            if reason.as_str() == Some("compiler-artifact") {
                if let Some(Value::String(executable)) = line.get("executable").cloned() {
                    if executable.ends_with(".wasm") {
                        executable_path = Some(executable);
                    }
                }
            }
        }
    }

    let Some(executable_path) = executable_path else {
        bail!("Failed to find wasm test binary location");
    };

    shell
        .cmd("wasm-bindgen")
        .args([
            executable_path.as_str(),
            "--out-dir",
            "wasm-test/dist",
            "--out-name",
            "test",
            "--target",
            "web",
        ])
        .run()?;

    Ok(())
}
