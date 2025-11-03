mod utils;

use couchbase_lite::*;
use std::path::Path;
use utils::*;

#[allow(deprecated)]
fn main() {
    println!("=== Test with Reporting Infrastructure ===\n");

    // STEP 0: Check git status
    println!("Step 0: Checking git status...");
    let git_info = match check_git_status() {
        Ok(info) => {
            println!("✓ Git status clean");
            println!("  - Commit: {}", info.commit_short_sha);
            println!("  - Branch: {}\n", info.branch);
            info
        }
        Err(e) => {
            eprintln!("✗ Git check failed:");
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    // STEP 1: Ensure clean Docker environment
    println!("Step 1: Setting up Docker environment...");
    if let Err(e) = ensure_clean_environment() {
        eprintln!("✗ Docker setup failed: {}", e);
        std::process::exit(1);
    }

    // STEP 2: Initialize test reporter
    let mut reporter = match TestReporter::new("test_with_reporting", git_info) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ Failed to initialize reporter: {}", e);
            std::process::exit(1);
        }
    };

    // STEP 3: Run actual test
    reporter.log("=== Starting test ===");

    let mut db_cblite = Database::open(
        "test_reporting",
        Some(DatabaseConfiguration {
            directory: Path::new("./"),
            #[cfg(feature = "enterprise")]
            encryption_key: None,
        }),
    )
    .unwrap();

    add_or_update_user("report_test_user", vec!["channel1".into()]);
    let session_token = get_session("report_test_user");

    let mut repl = setup_replicator(db_cblite.clone(), session_token).add_document_listener(
        Box::new(|_dir, docs| {
            for doc in docs {
                println!("  📡 Replicated: {} (flags={})", doc.id, doc.flags);
            }
        }),
    );

    repl.start(false);
    std::thread::sleep(std::time::Duration::from_secs(3));

    // Create document
    reporter.log("\nSTEP 1: Creating document...");
    create_doc(&mut db_cblite, "test_doc", "channel1");
    std::thread::sleep(std::time::Duration::from_secs(3));

    let state1 = get_sync_xattr("test_doc");
    reporter.checkpoint(
        "CREATED",
        state1.clone(),
        vec!["Document created in channel1".to_string()],
    );
    reporter.log("✓ Document created and replicated");

    // Delete document
    reporter.log("\nSTEP 2: Deleting document...");
    let mut doc = db_cblite.get_document("test_doc").unwrap();
    db_cblite.delete_document(&mut doc).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(3));

    let state2 = get_sync_xattr("test_doc");
    reporter.checkpoint(
        "DELETED",
        state2.clone(),
        vec!["Document deleted, should be tombstone".to_string()],
    );
    reporter.log("✓ Document deleted");

    // Re-create document
    reporter.log("\nSTEP 3: Re-creating document...");
    create_doc(&mut db_cblite, "test_doc", "channel1");
    std::thread::sleep(std::time::Duration::from_secs(3));

    let state3 = get_sync_xattr("test_doc");
    reporter.checkpoint(
        "RECREATED",
        state3.clone(),
        vec!["Document re-created, should be live".to_string()],
    );
    reporter.log("✓ Document re-created");

    repl.stop(None);

    reporter.log("\n=== Test complete ===");

    // Finalize report
    if let Err(e) = reporter.finalize() {
        eprintln!("⚠ Failed to generate report: {}", e);
    }
}

#[allow(deprecated)]
fn create_doc(db_cblite: &mut Database, id: &str, channel: &str) {
    let mut doc = Document::new_with_id(id);
    doc.set_properties_as_json(
        &serde_json::json!({
            "channels": channel,
            "test_data": "reporting test"
        })
        .to_string(),
    )
    .unwrap();
    db_cblite.save_document(&mut doc).unwrap();
}

fn setup_replicator(db_cblite: Database, session_token: String) -> Replicator {
    let repl_conf = ReplicatorConfiguration {
        database: Some(db_cblite.clone()),
        endpoint: Endpoint::new_with_url(SYNC_GW_URL).unwrap(),
        replicator_type: ReplicatorType::PushAndPull,
        continuous: true,
        disable_auto_purge: false,
        max_attempts: 3,
        max_attempt_wait_time: 1,
        heartbeat: 60,
        authenticator: None,
        proxy: None,
        headers: vec![(
            "Cookie".to_string(),
            format!("SyncGatewaySession={session_token}"),
        )]
        .into_iter()
        .collect(),
        pinned_server_certificate: None,
        trusted_root_certificates: None,
        channels: MutableArray::default(),
        document_ids: MutableArray::default(),
        collections: None,
        accept_parent_domain_cookies: false,
        #[cfg(feature = "enterprise")]
        accept_only_self_signed_server_certificate: false,
    };
    let repl_context = ReplicationConfigurationContext::default();
    Replicator::new(repl_conf, Box::new(repl_context)).unwrap()
}
