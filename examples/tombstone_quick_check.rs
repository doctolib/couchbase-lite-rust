mod utils;

use couchbase_lite::*;
use std::path::Path;
use utils::*;

#[allow(deprecated)]
fn main() {
    println!("=== Tombstone Quick Check (30 seconds) ===");
    println!("This is a rapid validation test for tombstone detection via XATTRs.\n");

    let mut db = Database::open(
        "tombstone_quick_check",
        Some(DatabaseConfiguration {
            directory: Path::new("./"),
            #[cfg(feature = "enterprise")]
            encryption_key: None,
        }),
    )
    .unwrap();

    // Setup user with access to channel1 only
    add_or_update_user("quick_test_user", vec!["channel1".into()]);
    let session_token = get_session("quick_test_user");
    println!("Session token: {session_token}\n");

    // Setup replicator with auto-purge enabled
    let mut repl =
        setup_replicator(db.clone(), session_token).add_document_listener(Box::new(doc_listener));

    repl.start(false);
    std::thread::sleep(std::time::Duration::from_secs(3));

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("TEST 1: Create document and check CBS state");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    create_doc(&mut db, "quick_doc", "channel1");
    std::thread::sleep(std::time::Duration::from_secs(3));

    println!("\n📊 CBS State after creation:");
    check_doc_in_cbs("quick_doc");
    println!("✓ Expected: Document exists as LIVE document\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("TEST 2: Delete document and check CBS state");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let mut doc = db.get_document("quick_doc").unwrap();
    db.delete_document(&mut doc).unwrap();
    println!("Document deleted locally");
    std::thread::sleep(std::time::Duration::from_secs(3));

    println!("\n📊 CBS State after deletion:");
    check_doc_in_cbs("quick_doc");
    println!("✓ Expected: Document exists as TOMBSTONE (deleted: true)\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("TEST 3: Re-create document and check CBS state");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    create_doc(&mut db, "quick_doc", "channel1");
    std::thread::sleep(std::time::Duration::from_secs(3));

    println!("\n📊 CBS State after re-creation:");
    check_doc_in_cbs("quick_doc");
    println!("✓ Expected: Document exists as LIVE document\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("TEST 4: Check replication flags");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("Review the replication logs above:");
    println!("  - Initial creation: should have flags=0 (new)");
    println!("  - After deletion: should have flags=1 (deleted)");
    println!("  - After re-creation: should have flags=0 (new) ✓\n");

    repl.stop(None);
    println!("=== Quick check complete ===");
}

fn create_doc(db: &mut Database, id: &str, channel: &str) {
    let mut doc = Document::new_with_id(id);
    doc.set_properties_as_json(
        &serde_json::json!({
            "channels": channel,
            "test_data": "quick check",
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        })
        .to_string(),
    )
    .unwrap();
    db.save_document(&mut doc).unwrap();
    println!("  Created doc {id}");
}

fn setup_replicator(db: Database, session_token: String) -> Replicator {
    let repl_conf = ReplicatorConfiguration {
        database: Some(db.clone()),
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

fn doc_listener(direction: Direction, documents: Vec<ReplicatedDocument>) {
    for document in documents {
        let flag_meaning = match document.flags {
            0 => "NEW",
            1 => "DELETED",
            _ => "OTHER",
        };
        println!(
            "  📡 Replicated [{:?}]: {} (flags={} - {})",
            direction, document.id, document.flags, flag_meaning
        );
    }
}
