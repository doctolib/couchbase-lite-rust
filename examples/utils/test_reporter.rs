use crate::utils::docker_manager;
use crate::utils::git_checker::GitInfo;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const GITHUB_REPO_URL: &str = "https://github.com/doctolib/couchbase-lite-rust";

#[derive(Debug, Serialize, Deserialize)]
pub struct Checkpoint {
    pub step: String,
    pub timestamp: String,
    pub elapsed_seconds: u64,
    pub tombstone_state: Option<serde_json::Value>,
    pub notes: Vec<String>,
}

pub struct TestReporter {
    run_dir: PathBuf,
    start_time: Instant,
    start_timestamp: String,
    git_info: GitInfo,
    test_name: String,
    checkpoints: Vec<Checkpoint>,
    console_output: Vec<String>,
}

impl TestReporter {
    pub fn new(test_name: &str, git_info: GitInfo) -> Result<Self, String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let start_timestamp = chrono::DateTime::from_timestamp(timestamp as i64, 0)
            .ok_or("Invalid timestamp")?
            .format("%Y-%m-%d_%H-%M-%S")
            .to_string();

        let run_dir_name = format!("test_run_{}_{}", start_timestamp, git_info.commit_short_sha);

        let run_dir = PathBuf::from("test_results").join(run_dir_name);

        // Create directory
        fs::create_dir_all(&run_dir)
            .map_err(|e| format!("Failed to create test results directory: {}", e))?;

        println!("📊 Test results will be saved to: {}\n", run_dir.display());

        Ok(Self {
            run_dir,
            start_time: Instant::now(),
            start_timestamp,
            git_info,
            test_name: test_name.to_string(),
            checkpoints: Vec::new(),
            console_output: Vec::new(),
        })
    }

    pub fn checkpoint(
        &mut self,
        step: &str,
        tombstone_state: Option<serde_json::Value>,
        notes: Vec<String>,
    ) {
        let elapsed = self.start_time.elapsed().as_secs();
        let timestamp = chrono::DateTime::from_timestamp(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
            0,
        )
        .unwrap()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

        let checkpoint = Checkpoint {
            step: step.to_string(),
            timestamp,
            elapsed_seconds: elapsed,
            tombstone_state,
            notes,
        };

        self.checkpoints.push(checkpoint);
    }

    pub fn log(&mut self, message: &str) {
        self.console_output.push(message.to_string());
        println!("{}", message);
    }

    pub fn finalize(&self) -> Result<(), String> {
        println!("\n📝 Generating test report...");

        // Generate all report files
        self.generate_metadata()?;
        self.generate_readme()?;
        self.generate_tombstone_states()?;
        self.generate_test_output()?;
        self.extract_docker_logs()?;

        println!("✓ Test report generated in: {}\n", self.run_dir.display());
        println!("📂 Report contents:");
        println!("  - README.md: Executive summary");
        println!("  - metadata.json: Test metadata and environment");
        println!("  - tombstone_states.json: Tombstone xattr at each checkpoint");
        println!("  - test_output.log: Complete console output");
        println!("  - cbs_logs.log: Couchbase Server logs");
        println!("  - sgw_logs.log: Sync Gateway logs");

        Ok(())
    }

    fn generate_metadata(&self) -> Result<(), String> {
        let metadata = serde_json::json!({
            "test_name": self.test_name,
            "start_time": self.start_timestamp,
            "duration_seconds": self.start_time.elapsed().as_secs(),
            "git": {
                "commit_sha": self.git_info.commit_sha,
                "commit_short_sha": self.git_info.commit_short_sha,
                "branch": self.git_info.branch,
                "github_link": format!("{}/tree/{}", GITHUB_REPO_URL, self.git_info.commit_sha),
            },
            "environment": {
                "couchbase_server": "7.x",
                "sync_gateway": "4.0.0 EE",
                "couchbase_lite": "3.2.3",
                "enable_shared_bucket_access": true,
            },
        });

        let metadata_path = self.run_dir.join("metadata.json");
        let json = serde_json::to_string_pretty(&metadata)
            .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

        fs::write(&metadata_path, json)
            .map_err(|e| format!("Failed to write metadata.json: {}", e))?;

        Ok(())
    }

    fn generate_readme(&self) -> Result<(), String> {
        let github_link = format!("{}/tree/{}", GITHUB_REPO_URL, self.git_info.commit_sha);

        let mut readme = String::new();
        readme.push_str(&format!("# Test Run: {}\n\n", self.test_name));
        readme.push_str(&format!("**Date**: {}\n", self.start_timestamp));
        readme.push_str(&format!(
            "**Duration**: {} seconds (~{} minutes)\n\n",
            self.start_time.elapsed().as_secs(),
            self.start_time.elapsed().as_secs() / 60
        ));

        readme.push_str("## Environment\n\n");
        readme.push_str(&format!(
            "- **Commit**: {} ([view on GitHub]({}))\n",
            self.git_info.commit_short_sha, github_link
        ));
        readme.push_str(&format!("- **Branch**: {}\n", self.git_info.branch));
        readme.push_str("- **Couchbase Server**: 7.x\n");
        readme.push_str("- **Sync Gateway**: 4.0.0 EE\n");
        readme.push_str("- **Couchbase Lite**: 3.2.3 (Rust)\n");
        readme.push_str("- **enable_shared_bucket_access**: true\n\n");

        readme.push_str("## Test Checkpoints\n\n");
        for checkpoint in &self.checkpoints {
            readme.push_str(&format!(
                "### {} ({}s elapsed)\n",
                checkpoint.step, checkpoint.elapsed_seconds
            ));
            readme.push_str(&format!("**Time**: {}\n\n", checkpoint.timestamp));

            if let Some(ref state) = checkpoint.tombstone_state {
                let flags = state.get("flags").and_then(|f| f.as_i64());
                let tombstoned_at = state.get("tombstoned_at");

                match flags {
                    Some(1) => {
                        readme.push_str("**Status**: 🪦 TOMBSTONE\n");
                        readme.push_str(&format!("- `flags`: 1\n"));
                        if let Some(ts) = tombstoned_at {
                            readme.push_str(&format!("- `tombstoned_at`: {}\n", ts));
                        }
                    }
                    Some(0) | None => {
                        readme.push_str("**Status**: ✅ LIVE DOCUMENT\n");
                        readme.push_str(&format!("- `flags`: {:?}\n", flags.unwrap_or(0)));
                    }
                    _ => {
                        readme.push_str(&format!("**Status**: ❓ UNKNOWN (flags: {:?})\n", flags));
                    }
                }
            } else {
                readme.push_str("**Status**: Document not found or not queried\n");
            }

            if !checkpoint.notes.is_empty() {
                readme.push_str("\n**Notes**:\n");
                for note in &checkpoint.notes {
                    readme.push_str(&format!("- {}\n", note));
                }
            }

            readme.push_str("\n");
        }

        readme.push_str("## Files in This Report\n\n");
        readme.push_str("- `README.md`: This file - executive summary\n");
        readme.push_str("- `metadata.json`: Test metadata (commit, timestamp, environment)\n");
        readme.push_str("- `tombstone_states.json`: Full _sync xattr content at each checkpoint\n");
        readme.push_str("- `test_output.log`: Complete console output from the test\n");
        readme.push_str("- `cbs_logs.log`: Couchbase Server container logs\n");
        readme.push_str("- `sgw_logs.log`: Sync Gateway container logs\n");

        let readme_path = self.run_dir.join("README.md");
        fs::write(&readme_path, readme).map_err(|e| format!("Failed to write README.md: {}", e))?;

        Ok(())
    }

    fn generate_tombstone_states(&self) -> Result<(), String> {
        let states_path = self.run_dir.join("tombstone_states.json");
        let json = serde_json::to_string_pretty(&self.checkpoints)
            .map_err(|e| format!("Failed to serialize checkpoints: {}", e))?;

        fs::write(&states_path, json)
            .map_err(|e| format!("Failed to write tombstone_states.json: {}", e))?;

        Ok(())
    }

    fn generate_test_output(&self) -> Result<(), String> {
        let output_path = self.run_dir.join("test_output.log");
        let mut file = fs::File::create(&output_path)
            .map_err(|e| format!("Failed to create test_output.log: {}", e))?;

        for line in &self.console_output {
            writeln!(file, "{}", line)
                .map_err(|e| format!("Failed to write to test_output.log: {}", e))?;
        }

        Ok(())
    }

    fn extract_docker_logs(&self) -> Result<(), String> {
        println!("  Extracting Docker logs...");

        // CBS logs
        let cbs_logs_path = self.run_dir.join("cbs_logs.log");
        docker_manager::get_docker_logs("cblr-couchbase-server", &cbs_logs_path)?;

        // SGW logs
        let sgw_logs_path = self.run_dir.join("sgw_logs.log");
        docker_manager::get_docker_logs("cblr-sync-gateway", &sgw_logs_path)?;

        Ok(())
    }
}
