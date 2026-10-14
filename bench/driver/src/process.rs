//! Building, starting, versioning and stopping one implementation from its
//! manifest entry. `PORT` and `WORKLOADS` tell the server where to listen and
//! where the workload files are.

use crate::manifest::Implementation;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

pub struct Running {
    child: Child,
}

fn shell(implementation: &Implementation, command: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command).current_dir(&implementation.cwd);
    cmd
}

pub async fn build(implementation: &Implementation) {
    let Some(build) = &implementation.build else {
        return;
    };
    eprintln!("[{}] {build}", implementation.name);
    let status = shell(implementation, build)
        .status()
        .await
        .unwrap_or_else(|e| panic!("{}: build: {e}", implementation.name));
    assert!(
        status.success(),
        "{}: build failed: {status}",
        implementation.name
    );
}

pub async fn version(implementation: &Implementation) -> String {
    let output = shell(implementation, &implementation.version)
        .stderr(Stdio::inherit())
        .output()
        .await
        .unwrap_or_else(|e| panic!("{}: version: {e}", implementation.name));
    assert!(
        output.status.success(),
        "{}: version command failed",
        implementation.name
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Starts the server and waits until it answers `{ __typename }`.
pub async fn start(
    implementation: &Implementation,
    workloads: &Path,
    http: &reqwest::Client,
) -> Running {
    eprintln!(
        "[{}] {} on port {}",
        implementation.name, implementation.run, implementation.port
    );
    let child = shell(implementation, &format!("exec {}", implementation.run))
        .env("PORT", implementation.port.to_string())
        .env("WORKLOADS", workloads)
        .stdout(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap_or_else(|e| panic!("{}: run: {e}", implementation.name));
    let mut running = Running { child };
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(response) = http
            .post(implementation.url())
            .header("content-type", "application/json")
            .body(r#"{"query":"{ __typename }"}"#)
            .send()
            .await
            && response.status().is_success()
        {
            return running;
        }
        if let Some(status) = running.child.try_wait().expect("poll child") {
            panic!(
                "{}: exited before becoming ready: {status}",
                implementation.name
            );
        }
        assert!(
            Instant::now() < deadline,
            "{}: not ready after 60 s",
            implementation.name
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

impl Running {
    pub async fn stop(mut self) {
        let _ = self.child.kill().await;
    }
}
