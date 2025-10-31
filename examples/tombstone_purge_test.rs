mod utils;

use std::path::Path;
use couchbase_lite::*;
use utils::*;

fn main() {
    println!("=== Tombstone Purge Test (FULL - 1 hour) ===");
    println!("This test validates complete tombstone purge following Thomas's recommendation.");
    println!("Total runtime: ~65-70 minutes\n");

    let mut db = Database::open(
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
    println!("Sync gateway session token: {session_token}\n");

    // Setup replicator with auto-purge enabled
    let mut repl =
        setup_replicator(db.clone(), session_token).add_document_listener(Box::new(doc_listener));

    repl.start(false);
    std::thread::sleep(std::time::Duration::from_secs(3));

    // STEP 1: Create document in channel1 and replicate
    println!("STEP 1: Creating doc1 in channel1...");
    create_doc(&mut db, "doc1", "channel1");
    std::thread::sleep(std::time::Duration::from_secs(5));

    // Verify doc exists locally
    assert!(get_doc(&db, "doc1").is_ok());
    println!("✓ doc1 created and replicated\n");

    // STEP 2: Delete doc1 (creating a tombstone)
    println!("STEP 2: Deleting doc1 (creating tombstone)...");
    let mut doc1 = get_doc(&db, "doc1").unwrap();
    db.delete_document(&mut doc1).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(5));
    println!("✓ doc1 deleted locally\n");

    // STEP 3: Purge tombstone from SGW
    // Note: This step may fail if SGW doesn't have the tombstone (404).
    // This can happen if:
    // - The tombstone only exists in CBS, not in SGW's cache
    // - SGW auto-purged it very quickly
    // This is not blocking for the test objective (verifying flags=0 on re-create).
    println!("STEP 3: Purging tombstone from SGW...");
    if let Some(tombstone_rev) = get_doc_rev("doc1") {
        purge_doc_from_sgw("doc1", &tombstone_rev);
        println!("✓ Tombstone purged from SGW (rev: {tombstone_rev})\n");
    } else {
        println!("⚠ Could not get tombstone revision from SGW");
        println!("  This is not blocking - tombstone may not exist in SGW or was auto-purged\n");
    }

    // STEP 4: Configure CBS metadata purge interval to 1 hour (minimum allowed)
    println!("STEP 4: Configuring CBS metadata purge interval...");
    let purge_interval_days = 0.04; // 1 hour (CBS minimum)
    let wait_minutes = 65;
    set_metadata_purge_interval(purge_interval_days);
    println!("✓ CBS purge interval set to {purge_interval_days} days (1 hour - CBS minimum)\n");

    // Check doc in CBS before waiting
    println!("Checking doc1 in CBS before wait...");
    check_doc_in_cbs("doc1");
    println!();

    // STEP 5: Wait for purge interval + margin
    println!("STEP 5: Waiting {wait_minutes} minutes for tombstone to be eligible for purge...");
    println!("This is the minimum time required by CBS to purge tombstones.");
    println!("Progress updates every 5 minutes:\n");

    let start_time = std::time::Instant::now();
    for minute in 1..=wait_minutes {
        if minute % 5 == 0 || minute == 1 || minute == wait_minutes {
            let elapsed = start_time.elapsed().as_secs() / 60;
            let remaining = wait_minutes - minute;
            println!(
                "  [{minute}/{wait_minutes}] {elapsed} minutes elapsed, {remaining} minutes remaining..."
            );
        }
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    println!("✓ Wait complete (65 minutes elapsed)\n");

    // STEP 6: Compact CBS bucket
    println!("STEP 6: Compacting CBS bucket...");
    compact_cbs_bucket();
    std::thread::sleep(std::time::Duration::from_secs(5));
    println!("✓ CBS compaction triggered\n");

    // STEP 7: Compact SGW database
    println!("STEP 7: Compacting SGW database...");
    compact_sgw_database();
    std::thread::sleep(std::time::Duration::from_secs(5));
    println!("✓ SGW compaction complete\n");

    // STEP 8: Check if tombstone still exists in CBS
    println!("STEP 8: Checking if tombstone exists in CBS...");
    check_doc_in_cbs("doc1");
    println!("  If tombstone was purged, the query should return no results.");
    println!();

    // STEP 9: Re-create doc1 and verify it's treated as new
    println!("STEP 9: Re-creating doc1 with same ID...");
    create_doc(&mut db, "doc1", "channel1");
    std::thread::sleep(std::time::Duration::from_secs(10));

    // Verify doc exists locally
    if get_doc(&db, "doc1").is_ok() {
        println!("✓ doc1 re-created successfully");
        println!("\n=== CRITICAL CHECK ===");
        println!("Review the replication logs above:");
        println!("  - flags=0: Document treated as NEW (tombstone successfully purged) ✓");
        println!("  - flags=1: Document recognized as deleted (tombstone still exists) ✗");
        println!("======================\n");
    } else {
        println!("✗ doc1 could not be re-created\n");
    }

    // Check final state in CBS
    println!("Final CBS state:");
    check_doc_in_cbs("doc1");

    repl.stop(None);
    println!("\n=== Test complete ===");
    println!(
        "Total runtime: ~{} minutes",
        start_time.elapsed().as_secs() / 60
    );
}

fn create_doc(db: &mut Database, id: &str, channel: &str) {
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
    db.save_document(&mut doc).unwrap();

    println!(
        "  Created doc {id} with content: {}",
        doc.properties_as_json()
    );
}

fn get_doc(db: &Database, id: &str) -> Result<Document> {
    db.get_document(id)
}

fn setup_replicator(db: Database, session_token: String) -> Replicator {
    let repl_conf = ReplicatorConfiguration {
        database: Some(db.clone()),
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
