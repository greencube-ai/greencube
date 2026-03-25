use bollard::container::{
    Config, CreateContainerOptions, KillContainerOptions, LogOutput, LogsOptions,
    RemoveContainerOptions, StartContainerOptions, WaitContainerOptions,
};
use bollard::models::HostConfig;
use bollard::Docker;
use tokio::time::{timeout, Duration};

use crate::sandbox::{SandboxOptions, SandboxResult};

pub async fn check_docker_available() -> bool {
    match Docker::connect_with_local_defaults() {
        Ok(docker) => docker.ping().await.is_ok(),
        Err(_) => false,
    }
}

pub async fn ensure_image_pulled(docker: &Docker, image: &str) -> anyhow::Result<()> {
    use bollard::image::CreateImageOptions;
    use futures_util::StreamExt;

    // Check if image exists locally first
    if docker.inspect_image(image).await.is_ok() {
        return Ok(());
    }

    tracing::info!(
        "Pulling Docker image: {}. This may take 30-60 seconds on first run...",
        image
    );

    let opts = CreateImageOptions {
        from_image: image,
        ..Default::default()
    };

    let mut stream = docker.create_image(Some(opts), None, None);
    while let Some(result) = stream.next().await {
        result?; // Propagate pull errors
    }

    tracing::info!("Docker image pulled successfully: {}", image);
    Ok(())
}

pub async fn execute_in_sandbox(
    docker: &Docker,
    command: &str,
    options: &SandboxOptions,
) -> anyhow::Result<SandboxResult> {
    // Ensure image is available (pulls on first use)
    ensure_image_pulled(docker, &options.image).await?;

    let container_name = format!("greencube-{}", uuid::Uuid::new_v4());

    // CRITICAL: auto_remove must be false. If true, the container is deleted
    // the instant it exits, and collect_logs will fail with "container not found".
    let config = Config {
        image: Some(options.image.clone()),
        cmd: Some(vec![
            "sh".to_string(),
            "-c".to_string(),
            command.to_string(),
        ]),
        host_config: Some(HostConfig {
            memory: Some((options.memory_limit_mb * 1024 * 1024) as i64),
            nano_cpus: Some((options.cpu_limit_cores * 1_000_000_000.0) as i64),
            network_mode: if options.network_enabled {
                Some("bridge".to_string())
            } else {
                Some("none".to_string())
            },
            auto_remove: Some(false), // MUST be false — we need logs before removal
            ..Default::default()
        }),
        ..Default::default()
    };

    // Create container
    let create_opts = CreateContainerOptions {
        name: container_name.as_str(),
        ..Default::default()
    };
    docker.create_container(Some(create_opts), config).await?;

    // Start container
    docker
        .start_container(&container_name, None::<StartContainerOptions<String>>)
        .await?;

    // Wait for completion with timeout
    let start = std::time::Instant::now();
    let wait_result = timeout(
        Duration::from_secs(options.timeout_seconds),
        wait_for_container(docker, &container_name),
    )
    .await;

    let duration_ms = start.elapsed().as_millis() as u64;

    let result = match wait_result {
        Ok(Ok(exit_code)) => {
            // Collect logs BEFORE removing container
            let (stdout, stderr) = collect_logs(docker, &container_name)
                .await
                .unwrap_or_else(|_| (String::new(), "Failed to collect logs".to_string()));
            Ok(SandboxResult {
                stdout,
                stderr,
                exit_code,
                duration_ms,
                timed_out: false,
            })
        }
        Ok(Err(e)) => {
            let _ = docker
                .kill_container(&container_name, None::<KillContainerOptions<String>>)
                .await;
            Err(e)
        }
        Err(_) => {
            // Timeout — kill container
            let _ = docker
                .kill_container(&container_name, None::<KillContainerOptions<String>>)
                .await;
            Ok(SandboxResult {
                stdout: String::new(),
                stderr: "Execution timed out".to_string(),
                exit_code: -1,
                duration_ms,
                timed_out: true,
            })
        }
    };

    // Always clean up the container (since auto_remove is false)
    let _ = docker
        .remove_container(
            &container_name,
            Some(RemoveContainerOptions {
                force: true,
                ..Default::default()
            }),
        )
        .await;

    result
}

async fn wait_for_container(docker: &Docker, name: &str) -> anyhow::Result<i64> {
    use futures_util::StreamExt;
    let mut stream = docker.wait_container(name, None::<WaitContainerOptions<String>>);
    if let Some(result) = stream.next().await {
        let resp = result?;
        Ok(resp.status_code)
    } else {
        Err(anyhow::anyhow!("container wait stream ended unexpectedly"))
    }
}

async fn collect_logs(docker: &Docker, name: &str) -> anyhow::Result<(String, String)> {
    use futures_util::StreamExt;
    let opts = LogsOptions::<String> {
        stdout: true,
        stderr: true,
        ..Default::default()
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut stream = docker.logs(name, Some(opts));
    while let Some(Ok(output)) = stream.next().await {
        match output {
            LogOutput::StdOut { message } => {
                stdout.push_str(&String::from_utf8_lossy(&message));
            }
            LogOutput::StdErr { message } => {
                stderr.push_str(&String::from_utf8_lossy(&message));
            }
            _ => {}
        }
    }
    Ok((stdout, stderr))
}

pub fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn test_shell_escape_single_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_escape_spaces() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
    }

    #[test]
    fn test_shell_escape_special_chars() {
        assert_eq!(shell_escape("$HOME"), "'$HOME'");
    }

    // Docker-dependent tests are marked #[ignore]
    #[ignore]
    #[tokio::test]
    async fn test_sandbox_echo() {
        let docker = Docker::connect_with_local_defaults().expect("connect");
        let opts = SandboxOptions {
            image: "python:3.12-slim".into(),
            cpu_limit_cores: 0.5,
            memory_limit_mb: 256,
            timeout_seconds: 30,
            network_enabled: false,
        };
        let result = execute_in_sandbox(&docker, "echo hello", &opts)
            .await
            .expect("execute");
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
        assert!(!result.timed_out);
    }

    #[ignore]
    #[tokio::test]
    async fn test_sandbox_exit_code() {
        let docker = Docker::connect_with_local_defaults().expect("connect");
        let opts = SandboxOptions {
            image: "python:3.12-slim".into(),
            cpu_limit_cores: 0.5,
            memory_limit_mb: 256,
            timeout_seconds: 30,
            network_enabled: false,
        };
        let result = execute_in_sandbox(&docker, "exit 42", &opts)
            .await
            .expect("execute");
        assert_eq!(result.exit_code, 42);
    }
}
