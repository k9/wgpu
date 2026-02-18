use std::{ffi::OsString, process::Child, thread::sleep, time::Duration};

use anyhow::{bail, Context};
use pico_args::Arguments;
use serde_json::Value;
use xshell::Shell;

struct WasmTestServer(Child);

impl Drop for WasmTestServer {
    // Clean up node processes when parent process ends
    fn drop(&mut self) {
        let _ = self.0.kill();
    }
}

pub fn run_wasm_tests(
    shell: Shell,
    _args: Arguments,
    _passthrough_args: Option<Vec<OsString>>,
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

    let mut server = WasmTestServer(
        std::process::Command::new("node")
            .arg("wasm-test/runner/index.js")
            .spawn()
            .expect("Failed to start wasm test server"),
    );

    loop {
        if ureq::get("http://127.0.0.1:3000/").call().is_ok() {
            break;
        };

        sleep(Duration::from_millis(100));
    }

    let mut response = ureq::get("http://127.0.0.1:3000/gpu_report")
        .call()
        .expect("Failed to get gpu config from browser");

    let gpu_config = response
        .body_mut()
        .read_to_string()
        .expect("Failed to get gpu config from browser");

    std::fs::write(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../.wasmgpuconfig"),
        gpu_config,
    )
    .expect("Failed to write wasm gpu_config");

    shell
        .cmd("cargo")
        .args(["nextest", "run", "-P", "wasm", "--test-threads", "1"])
        .env("TEST_WASM", "true")
        .run()?;

    // server.0.wait()?;
    Ok(())
}
