use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

const DOCKER_CONF_DIR: &str = "examples/docker-conf";
const MAX_WAIT_SECONDS: u64 = 120;

pub fn ensure_clean_environment() -> Result<(), String> {
    println!("🐳 Managing Docker environment...\n");

    // Check if docker and docker compose are available
    check_docker_available()?;

    // Navigate to docker-conf directory
    let docker_dir = Path::new(DOCKER_CONF_DIR);
    if !docker_dir.exists() {
        return Err(format!(
            "Docker configuration directory not found: {}",
            DOCKER_CONF_DIR
        ));
    }

    // Stop and remove containers + volumes
    println!("  [1/4] Stopping and removing existing containers...");
    stop_containers()?;

    // Build/pull images
    println!("  [2/4] Building/pulling Docker images...");
    build_images()?;

    // Start containers
    println!("  [3/4] Starting containers...");
    start_containers()?;

    // Wait for services to be healthy
    println!("  [4/4] Waiting for services to be healthy...");
    wait_for_healthy_services()?;

    println!("✓ Docker environment ready\n");
    Ok(())
}

fn check_docker_available() -> Result<(), String> {
    // Check docker
    let docker_check = Command::new("docker").arg("--version").output();

    if docker_check.is_err() {
        return Err(
            "Docker is not installed or not available in PATH. Please install Docker.".to_string(),
        );
    }

    // Check docker compose
    let compose_check = Command::new("docker").args(["compose", "version"]).output();

    if compose_check.is_err() {
        return Err("Docker Compose is not available. Please install Docker Compose.".to_string());
    }

    Ok(())
}

fn stop_containers() -> Result<(), String> {
    let output = Command::new("docker")
        .args(["compose", "down", "-v"])
        .current_dir(DOCKER_CONF_DIR)
        .output()
        .map_err(|e| format!("Failed to stop containers: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Don't fail if containers weren't running
        if !stderr.contains("No such container") && !stderr.is_empty() {
            eprintln!("Warning: docker compose down had errors: {}", stderr);
        }
    }

    Ok(())
}

fn build_images() -> Result<(), String> {
    let output = Command::new("docker")
        .args(["compose", "build"])
        .current_dir(DOCKER_CONF_DIR)
        .stdout(Stdio::null()) // Suppress verbose build output
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("Failed to build images: {}", e))?;

    if !output.status.success() {
        return Err("Docker compose build failed".to_string());
    }

    Ok(())
}

fn start_containers() -> Result<(), String> {
    let output = Command::new("docker")
        .args(["compose", "up", "-d"])
        .current_dir(DOCKER_CONF_DIR)
        .output()
        .map_err(|e| format!("Failed to start containers: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Docker compose up failed: {}", stderr));
    }

    Ok(())
}

fn wait_for_healthy_services() -> Result<(), String> {
    let start = std::time::Instant::now();

    loop {
        if start.elapsed().as_secs() > MAX_WAIT_SECONDS {
            return Err(format!(
                "Services did not become healthy within {} seconds",
                MAX_WAIT_SECONDS
            ));
        }

        // Check if Sync Gateway is responding
        let sgw_ready = reqwest::blocking::get("http://localhost:4985")
            .map(|r| r.status().is_success())
            .unwrap_or(false);

        // Check if CBS is responding
        let cbs_ready = reqwest::blocking::get("http://localhost:8091")
            .map(|r| r.status().is_success())
            .unwrap_or(false);

        if sgw_ready && cbs_ready {
            // Give extra time for full initialization
            thread::sleep(Duration::from_secs(5));
            return Ok(());
        }

        thread::sleep(Duration::from_secs(2));
    }
}

pub fn get_docker_logs(service_name: &str, output_path: &Path) -> Result<(), String> {
    let output = Command::new("docker")
        .args(["compose", "logs", "--no-color", service_name])
        .current_dir(DOCKER_CONF_DIR)
        .output()
        .map_err(|e| format!("Failed to get logs for {}: {}", service_name, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to get logs: {}", stderr));
    }

    std::fs::write(output_path, &output.stdout)
        .map_err(|e| format!("Failed to write logs to file: {}", e))?;

    Ok(())
}
