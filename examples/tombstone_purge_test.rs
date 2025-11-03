mod utils;

use couchbase_lite::*;
use std::path::Path;
use utils::*;

#[allow(deprecated)]
fn main() {
    println!("=== Tombstone Purge Test (FULL - 1 hour) ===");
    println!("This test validates complete tombstone purge following Thomas's recommendation.");
    println!("Total runtime: ~65-70 minutes\n");

    // SETUP: Check git status
    println!("SETUP: Checking git status...");
    let git_info = match check_git_status() {
        Ok(info) => {
            println!("✓ Git status clean (commit: {})\n", info.commit_short_sha);
            info
        }
        Err(e) => {
            eprintln!("✗ Git check failed:\n{}", e);
            eprintln!("\nPlease commit changes before running this test.");
            std::process::exit(1);
        }
    };

    // SETUP: Rebuild Docker environment
    println!("SETUP: Rebuilding Docker environment with correct configuration...");
    if let Err(e) = ensure_clean_environment() {
        eprintln!("✗ Docker setup failed: {}", e);
        std::process::exit(1);
    }

    // SETUP: Initialize test reporter
    let mut reporter = match TestReporter::new("tombstone_purge_test_full", git_info) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ Failed to initialize reporter: {}", e);
            std::process::exit(1);
        }
    };

    // SETUP: Verify CBS configuration
    reporter.log("SETUP: Verifying CBS metadata purge interval configuration...");
    get_metadata_purge_interval();
    reporter.log("");

    let mut db_cblite = Database::open(
        "tombstone_test_full",
        Some(DatabaseConfiguration {
            directory: Path::new("./"),
            #[cfg(feature = "enterprise")]
            encryption_key: None,
        }),
    )
    .unwrap();

    // Setup user with access to channel1 only
    add_or_update_user("test_user", vec!["channel1".into()]);
    let session_token = get_session("test_user");
    reporter.log(&format!("Sync gateway session token: {session_token}\n"));

    // Setup replicator with auto-purge enabled
    let mut repl = setup_replicator(db_cblite.clone(), session_token)
        .add_document_listener(Box::new(doc_listener));

    repl.start(false);
    std::thread::sleep(std::time::Duration::from_secs(3));

    // STEP 1: Create document in channel1 and replicate
    reporter.log("STEP 1: Creating doc1 in channel1...");
    create_doc(&mut db_cblite, "doc1", "channel1");
    std::thread::sleep(std::time::Duration::from_secs(5));

    assert!(get_doc(&db_cblite, "doc1").is_ok());
    let state1 = get_sync_xattr("doc1");
    reporter.checkpoint(
        "STEP_1_CREATED",
        state1,
        vec!["Document created in channel1 and replicated".to_string()],
    );
    reporter.log("✓ doc1 created and replicated\n");

    // STEP 2: Delete doc1 (creating a tombstone)
    reporter.log("STEP 2: Deleting doc1 (creating tombstone)...");
    let mut doc1 = get_doc(&db_cblite, "doc1").unwrap();
    db_cblite.delete_document(&mut doc1).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(5));

    let state2 = get_sync_xattr("doc1");
    reporter.checkpoint(
        "STEP_2_DELETED",
        state2,
        vec!["Document deleted, tombstone created".to_string()],
    );
    reporter.log("✓ doc1 deleted locally\n");

    // STEP 3: Purge tombstone from SGW
    reporter.log("STEP 3: Purging tombstone from SGW...");
    let mut notes3 = vec![];
    if let Some(tombstone_rev) = get_doc_rev("doc1") {
        purge_doc_from_sgw("doc1", &tombstone_rev);
        notes3.push(format!(
            "Tombstone purged from SGW (rev: {})",
            tombstone_rev
        ));
        reporter.log(&format!(
            "✓ Tombstone purged from SGW (rev: {tombstone_rev})\n"
        ));
    } else {
        notes3.push("Could not get tombstone revision from SGW (404)".to_string());
        notes3.push("Tombstone may not exist in SGW or was auto-purged".to_string());
        reporter.log("⚠ Could not get tombstone revision from SGW");
        reporter
            .log("  This is not blocking - tombstone may not exist in SGW or was auto-purged\n");
    }
    reporter.checkpoint("STEP_3_SGW_PURGE_ATTEMPTED", None, notes3);

    // STEP 4: CBS metadata purge interval should already be configured at bucket creation
    reporter.log("STEP 4: CBS metadata purge interval configuration...");
    reporter.log("  Purge interval was set to 0.04 days (1 hour) at bucket creation.");
    reporter.log("  This ensures tombstones created now are eligible for purge after 1 hour.\n");

    let state4 = get_sync_xattr("doc1");
    reporter.checkpoint(
        "STEP_4_BEFORE_WAIT",
        state4,
        vec![
            "Tombstone state before waiting for purge interval".to_string(),
            "Purge interval: 0.04 days (1 hour)".to_string(),
        ],
    );

    // Check doc in CBS before waiting
    reporter.log("Checking doc1 in CBS before wait...");
    check_doc_in_cbs("doc1");
    reporter.log("");

    // STEP 5: Wait for purge interval + margin
    reporter.log("STEP 5: Waiting 65 minutes for tombstone to be eligible for purge...");
    reporter.log("This is the minimum time required by CBS to purge tombstones.");
    reporter.log("Progress updates every 5 minutes:\n");

    let start_time = std::time::Instant::now();
    for minute in 1..=65 {
        if minute % 5 == 0 || minute == 1 || minute == 65 {
            let elapsed = start_time.elapsed().as_secs() / 60;
            let remaining = 65 - minute;
            reporter.log(&format!(
                "  [{minute}/65] {elapsed} minutes elapsed, {remaining} minutes remaining..."
            ));
        }
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    reporter.log("✓ Wait complete (65 minutes elapsed)\n");

    // STEP 6: Compact CBS bucket
    reporter.log("STEP 6: Compacting CBS bucket...");
    compact_cbs_bucket();
    std::thread::sleep(std::time::Duration::from_secs(5));
    reporter.log("✓ CBS compaction triggered\n");

    // STEP 7: Compact SGW database
    reporter.log("STEP 7: Compacting SGW database...");
    compact_sgw_database();
    std::thread::sleep(std::time::Duration::from_secs(5));
    reporter.log("✓ SGW compaction complete\n");

    // STEP 8: Check if tombstone still exists in CBS
    reporter.log("STEP 8: Checking if tombstone exists in CBS...");
    check_doc_in_cbs("doc1");
    let state8 = get_sync_xattr("doc1");
    let notes8 = if state8
        .as_ref()
        .and_then(|s| s.get("flags"))
        .and_then(|f| f.as_i64())
        == Some(1)
    {
        vec!["Tombstone still present after compaction".to_string()]
    } else if state8.is_none() {
        vec!["Tombstone successfully purged from CBS".to_string()]
    } else {
        vec!["Document is live (unexpected state)".to_string()]
    };
    reporter.checkpoint("STEP_8_AFTER_COMPACTION", state8, notes8);
    reporter.log("  If tombstone was purged, the query should return no results.\n");

    // STEP 9: Re-create doc1 and verify it's treated as new
    reporter.log("STEP 9: Re-creating doc1 with same ID...");
    create_doc(&mut db_cblite, "doc1", "channel1");
    std::thread::sleep(std::time::Duration::from_secs(10));

    let state9 = get_sync_xattr("doc1");
    let notes9 = vec!["Document re-created after tombstone purge test".to_string()];
    reporter.checkpoint("STEP_9_RECREATED", state9, notes9);

    // Verify doc exists locally
    if get_doc(&db_cblite, "doc1").is_ok() {
        reporter.log("✓ doc1 re-created successfully");
        reporter.log("\n=== CRITICAL CHECK ===");
        reporter.log("Review the replication logs above:");
        reporter.log("  - flags=0: Document treated as NEW (tombstone successfully purged) ✓");
        reporter.log("  - flags=1: Document recognized as deleted (tombstone still exists) ✗");
        reporter.log("======================\n");
    } else {
        reporter.log("✗ doc1 could not be re-created\n");
    }

    // Check final state in CBS
    reporter.log("Final CBS state:");
    check_doc_in_cbs("doc1");

    repl.stop(None);

    reporter.log("\n=== Test complete ===");
    reporter.log(&format!(
        "Total runtime: ~{} minutes",
        start_time.elapsed().as_secs() / 60
    ));

    // Generate report
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
            "test_data": "tombstone purge test",
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        })
        .to_string(),
    )
    .unwrap();
    db_cblite.save_document(&mut doc).unwrap();

    println!(
        "  Created doc {id} with content: {}",
        doc.properties_as_json()
    );
}

#[allow(deprecated)]
fn get_doc(db_cblite: &Database, id: &str) -> Result<Document> {
    db_cblite.get_document(id)
}

fn setup_replicator(db_cblite: Database, session_token: String) -> Replicator {
    let repl_conf = ReplicatorConfiguration {
        database: Some(db_cblite.clone()),
        endpoint: Endpoint::new_with_url(SYNC_GW_URL).unwrap(),
        replicator_type: ReplicatorType::PushAndPull,
        continuous: true,
        disable_auto_purge: false, // Auto-purge ENABLED
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

fn doc_listener(direction: Direction, documents: Vec<ReplicatedDocument>) {
    println!("=== Document(s) replicated ===");
    println!("Direction: {direction:?}");
    for document in documents {
        println!("Document: {document:?}");
        if document.flags == 1 {
            println!("  ⚠ flags=1 - Document recognized as deleted/tombstone");
        } else if document.flags == 0 {
            println!("  ✓ flags=0 - Document treated as new");
        }
    }
    println!("===\n");
}
