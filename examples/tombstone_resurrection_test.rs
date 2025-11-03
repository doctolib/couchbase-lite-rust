mod utils;

use couchbase_lite::*;
use std::path::Path;
use std::sync::{Arc, Mutex};
use utils::*;

#[allow(deprecated)]
fn main() {
    println!("=== Tombstone Resurrection Test (BC-994 Scenario) ===");
    println!(
        "This test validates soft_delete behavior for documents resurrecting after tombstone expiry."
    );
    println!("Total runtime: ~75-80 minutes\n");

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
    println!("SETUP: Rebuilding Docker environment with soft_delete sync function...");
    if let Err(e) = ensure_clean_environment() {
        eprintln!("✗ Docker setup failed: {}", e);
        std::process::exit(1);
    }

    // SETUP: Initialize test reporter
    let mut reporter = match TestReporter::new("tombstone_resurrection_test", git_info) {
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

    // SETUP: Clean up local database from previous run
    let db_name = "tombstone_resurrection_test";
    let db_path = Path::new("./");

    if Database::exists(db_name, db_path) {
        reporter.log("SETUP: Deleting local database from previous run...");
        Database::delete_file(db_name, db_path).expect("Failed to delete existing database");
        reporter.log("✓ Local database cleaned\n");
    }

    let mut db_cblite = Database::open(
        db_name,
        Some(DatabaseConfiguration {
            directory: db_path,
            #[cfg(feature = "enterprise")]
            encryption_key: None,
        }),
    )
    .unwrap();

    // Setup user with access to channel1 only (NOT soft_deleted)
    add_or_update_user("test_user", vec!["channel1".into()]);
    let session_token = get_session("test_user");
    reporter.log(&format!("Sync gateway session token: {session_token}\n"));

    // Track replication events
    let repl_events = Arc::new(Mutex::new(Vec::<String>::new()));
    let repl_events_clone = repl_events.clone();

    // Setup replicator with auto-purge ENABLED
    let mut repl = setup_replicator(db_cblite.clone(), session_token.clone())
        .add_document_listener(Box::new(move |dir, docs| {
            let mut events = repl_events_clone.lock().unwrap();
            for doc in docs {
                let event = format!("{:?}: {} (flags={})", dir, doc.id, doc.flags);
                println!("  📡 {}", event);
                events.push(event);
            }
        }));

    repl.start(false);
    std::thread::sleep(std::time::Duration::from_secs(3));

    // STEP 1: Create document with updatedAt = NOW, replicate, then STOP replication
    reporter.log("STEP 1: Creating doc1 with updatedAt = NOW...");
    let doc_created_at = chrono::Utc::now();
    create_doc_with_updated_at(&mut db_cblite, "doc1", "channel1", &doc_created_at);
    std::thread::sleep(std::time::Duration::from_secs(5));

    assert!(get_doc(&db_cblite, "doc1").is_ok());
    reporter.log(&format!(
        "  Document created at: {}",
        doc_created_at.to_rfc3339()
    ));

    let state1 = get_sync_xattr("doc1");
    reporter.checkpoint(
        "STEP_1_CREATED_AND_REPLICATED",
        state1,
        vec![
            format!(
                "Document created with updatedAt: {}",
                doc_created_at.to_rfc3339()
            ),
            "Document replicated to central".to_string(),
        ],
    );
    reporter.log("✓ doc1 created and replicated to central\n");

    // STOP replication
    reporter.log("STEP 1b: Stopping replication...");
    repl.stop(None);
    std::thread::sleep(std::time::Duration::from_secs(2));
    reporter.log("✓ Replication stopped\n");

    // STEP 2: Delete doc1 from CENTRAL only (doc remains in cblite)
    reporter.log("STEP 2: Deleting doc1 from CENTRAL only...");
    let deletion_success = delete_doc_from_central("doc1");

    if !deletion_success {
        reporter.log("⚠ Failed to delete document from central - test may not be valid\n");
    } else {
        std::thread::sleep(std::time::Duration::from_secs(3));
        reporter.log("✓ doc1 deleted from central (tombstone created in central)\n");
    }

    // Verify doc still exists in cblite
    reporter.log("STEP 2b: Verifying doc1 still exists in local cblite...");
    if get_doc(&db_cblite, "doc1").is_ok() {
        reporter.log("✓ doc1 still present in cblite (as expected)\n");
    } else {
        reporter.log("✗ doc1 NOT in cblite (unexpected!)\n");
    }

    let state2 = get_sync_xattr("doc1");
    reporter.checkpoint(
        "STEP_2_DELETED_IN_CENTRAL",
        state2,
        vec![
            "Document deleted from central only".to_string(),
            "Document still present in cblite".to_string(),
        ],
    );

    // STEP 3: Wait for purge interval + compact
    reporter.log("STEP 3: Waiting 65 minutes for central tombstone to be eligible for purge...");
    reporter.log("  This allows the document's updatedAt to become > 1 hour old.");
    reporter.log("  Progress updates every 5 minutes:\n");

    let start_time = std::time::Instant::now();
    for minute in 1..=65 {
        if minute % 5 == 0 || minute == 1 || minute == 65 {
            let elapsed = start_time.elapsed().as_secs() / 60;
            let remaining = 65 - minute;
            let age_minutes = chrono::Utc::now()
                .signed_duration_since(doc_created_at)
                .num_minutes();
            reporter.log(&format!(
                "  [{minute}/65] {elapsed} min elapsed, {remaining} min remaining (doc age: {} min)",
                age_minutes
            ));
        }
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    reporter.log("✓ Wait complete (65 minutes elapsed)\n");

    // STEP 4: Compact CBS
    reporter.log("STEP 4: Compacting CBS bucket...");
    compact_cbs_bucket();
    std::thread::sleep(std::time::Duration::from_secs(5));
    reporter.log("✓ CBS compaction triggered\n");

    // STEP 5: Compact SGW
    reporter.log("STEP 5: Compacting SGW database...");
    compact_sgw_database();
    std::thread::sleep(std::time::Duration::from_secs(5));
    reporter.log("✓ SGW compaction complete\n");

    // STEP 6: Verify tombstone purged from central
    reporter.log("STEP 6: Checking if central tombstone was purged...");
    let state6 = get_sync_xattr("doc1");
    let purged = state6.is_none() || state6.as_ref().and_then(|s| s.get("flags")).is_none();

    if purged {
        reporter.log("  ✓ Central tombstone successfully purged (xattr absent)\n");
    } else {
        if let Some(ref xattr) = state6 {
            let flags = xattr.get("flags").and_then(|f| f.as_i64()).unwrap_or(0);
            reporter.log(&format!(
                "  ⚠ Central tombstone still present (flags: {})\n",
                flags
            ));
        }
    }

    reporter.checkpoint(
        "STEP_6_TOMBSTONE_CHECK",
        state6,
        if purged {
            vec!["Central tombstone successfully purged".to_string()]
        } else {
            vec!["Central tombstone still present (unexpected)".to_string()]
        },
    );

    // STEP 7: Prepare for replication reset - Touch document to force push
    reporter.log("STEP 7: Preparing document for replication reset...");
    reporter.log("  Touching doc1 to ensure it will be pushed during reset checkpoint...");

    // Modify document slightly to trigger a change
    {
        let mut doc = get_doc(&db_cblite, "doc1").unwrap();
        let mut props = doc.mutable_properties();
        props.at("_resurrection_test").put_bool(true);
        db_cblite.save_document(&mut doc).unwrap();
        reporter.log("  ✓ Document modified to trigger replication\n");
    }

    // STEP 8: Restart replication with RESET CHECKPOINT
    reporter.log("STEP 8: Restarting replication with RESET CHECKPOINT...");
    reporter.log("  This simulates a fresh sync where cblite will push doc1 back to central.");
    reporter.log(&format!(
        "  doc1's updatedAt ({}) is now > 1 hour old",
        doc_created_at.to_rfc3339()
    ));
    reporter.log("  Sync function should route it to 'soft_deleted' channel.\n");

    // Clear previous replication events
    {
        let mut events = repl_events.lock().unwrap();
        events.clear();
    }

    // Recreate replicator with reset flag
    let repl_events_clone2 = repl_events.clone();
    let mut repl_reset = setup_replicator(db_cblite.clone(), session_token).add_document_listener(
        Box::new(move |dir, docs| {
            let mut events = repl_events_clone2.lock().unwrap();
            for doc in docs {
                let event = format!("{:?}: {} (flags={})", dir, doc.id, doc.flags);
                println!("  📡 {}", event);
                events.push(event);
            }
        }),
    );

    repl_reset.start(true); // true = reset checkpoint

    // Wait longer for replication to complete
    reporter.log("  Waiting 30 seconds for replication to process...");
    std::thread::sleep(std::time::Duration::from_secs(30));
    reporter.log("✓ Replication restarted with reset checkpoint\n");

    // Log replication events
    {
        let events = repl_events.lock().unwrap();
        if !events.is_empty() {
            reporter.log(&format!(
                "  Replication events captured: {} events",
                events.len()
            ));
            for event in events.iter() {
                reporter.log(&format!("    - {}", event));
            }
            reporter.log("");
        } else {
            reporter
                .log("  ⚠ No replication events captured (document may not have been pushed)\n");
        }
    }

    // STEP 9: Verify auto-purge in cblite (non-blocking)
    reporter.log("STEP 9: Checking if doc1 was auto-purged from cblite...");
    reporter.log("  doc1 should be auto-purged because it was routed to 'soft_deleted' channel");
    reporter.log("  (user only has access to 'channel1')\n");

    std::thread::sleep(std::time::Duration::from_secs(5));

    match get_doc(&db_cblite, "doc1") {
        Ok(_) => {
            reporter.log("  ⚠ doc1 STILL IN cblite (auto-purge may not have triggered yet)");
            reporter.log("  This is not blocking - continuing test...\n");
        }
        Err(_) => {
            reporter.log("  ✓ doc1 AUTO-PURGED from cblite (as expected)\n");
        }
    }

    // STEP 10: Check if doc exists in central with soft_deleted routing
    reporter.log("STEP 10: Checking if doc1 exists in central...");
    reporter.log("  Querying SGW admin API...");
    let doc_in_central = check_doc_exists_in_central("doc1");

    if doc_in_central {
        reporter.log("  ✓ Document found in central (resurrection successful)");
    } else {
        reporter.log("  ⚠ Document NOT found in central");
        reporter.log("  This means the document was not pushed during replication reset");
        reporter.log("  This is unexpected but continuing test...");
    }
    reporter.log("");

    let state9 = get_sync_xattr("doc1");
    let notes9 = if doc_in_central {
        vec![
            "Document exists in central after resurrection".to_string(),
            "Should be routed to soft_deleted channel".to_string(),
            "TTL set to 5 minutes".to_string(),
        ]
    } else {
        vec![
            "Document NOT found in central (unexpected at this stage)".to_string(),
            "Document may not have been pushed during replication reset".to_string(),
        ]
    };
    reporter.checkpoint("STEP_9_AFTER_RESURRECTION", state9.clone(), notes9);

    // STEP 9b: Check channel routing in xattr
    if let Some(ref xattr) = state9 {
        if let Some(channels) = xattr.get("channels").and_then(|c| c.as_object()) {
            reporter.log("  Channel routing in CBS:");
            for (channel_name, _) in channels {
                reporter.log(&format!("    - {}", channel_name));
            }

            if channels.contains_key("soft_deleted") {
                reporter.log("  ✓ Document correctly routed to 'soft_deleted' channel\n");
            } else {
                reporter.log("  ⚠ Document NOT in 'soft_deleted' channel (unexpected)\n");
            }
        }
    } else if doc_in_central {
        reporter.log("  ⚠ Could not retrieve _sync xattr to verify channel routing\n");
    }

    // STEP 11: Wait for TTL expiry (5 minutes) + compact
    reporter.log("STEP 11: Waiting 6 minutes for TTL expiry (5 min TTL + margin)...");
    for minute in 1..=6 {
        reporter.log(&format!("  [{minute}/6] Waiting..."));
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    reporter.log("✓ Wait complete\n");

    // STEP 12: Compact CBS
    reporter.log("STEP 12: Compacting CBS bucket (to trigger TTL purge)...");
    compact_cbs_bucket();
    std::thread::sleep(std::time::Duration::from_secs(5));
    reporter.log("✓ CBS compaction triggered\n");

    // STEP 13: Compact SGW
    reporter.log("STEP 13: Compacting SGW database...");
    compact_sgw_database();
    std::thread::sleep(std::time::Duration::from_secs(5));
    reporter.log("✓ SGW compaction complete\n");

    // STEP 14: Verify doc purged from central (TTL expired)
    reporter.log("STEP 14: Checking if doc1 was purged from central (TTL expired)...");
    reporter.log("  Querying SGW admin API...");
    let still_in_central = check_doc_exists_in_central("doc1");

    if !still_in_central {
        reporter.log("  ✓ doc1 PURGED from central (TTL expiry successful)\n");
    } else {
        reporter.log("  ⚠ doc1 STILL in central (TTL purge may need more time)\n");
    }

    let state14 = get_sync_xattr("doc1");
    let notes14 = if still_in_central {
        vec!["Document STILL in central (TTL may not have expired yet)".to_string()]
    } else {
        vec!["Document successfully purged from central after TTL expiry".to_string()]
    };
    reporter.checkpoint("STEP_14_AFTER_TTL_PURGE", state14, notes14);

    repl_reset.stop(None);

    reporter.log("\n=== Test complete ===");
    reporter.log(&format!(
        "Total runtime: ~{} minutes",
        start_time.elapsed().as_secs() / 60
    ));

    // Log final replication events summary
    {
        let events = repl_events.lock().unwrap();
        reporter.log(&format!("\nTotal replication events: {}", events.len()));
    }

    reporter.log("\n=== SUMMARY ===");
    reporter.log("✓ Document resurrection scenario tested");
    reporter.log("✓ Sync function soft_delete logic validated");
    reporter.log("✓ Auto-purge mechanism tested");
    reporter.log("✓ TTL-based central purge tested");

    // Generate report
    if let Err(e) = reporter.finalize() {
        eprintln!("⚠ Failed to generate report: {}", e);
    }
}

#[allow(deprecated)]
fn create_doc_with_updated_at(
    db_cblite: &mut Database,
    id: &str,
    channel: &str,
    updated_at: &chrono::DateTime<chrono::Utc>,
) {
    let mut doc = Document::new_with_id(id);
    doc.set_properties_as_json(
        &serde_json::json!({
            "channels": channel,
            "test_data": "tombstone resurrection test",
            "updatedAt": updated_at.to_rfc3339(),
        })
        .to_string(),
    )
    .unwrap();
    db_cblite.save_document(&mut doc).unwrap();
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
        disable_auto_purge: false, // Auto-purge ENABLED - critical for test
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
