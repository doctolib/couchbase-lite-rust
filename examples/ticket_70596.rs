mod utils;

use std::path::Path;
use couchbase_lite::*;
use utils::*;

fn main() {
    let mut db = Database::open(
        "test1",
        Some(DatabaseConfiguration {
            directory: Path::new("./"),
            #[cfg(feature = "enterprise")]
            encryption_key: None,
        }),
    )
    .unwrap();

    add_or_update_user("great_name", vec!["channel1".into()]);
    let session_token = get_session("great_name");
    println!("Sync gateway session token: {session_token}");

    let mut repl =
        setup_replicator(db.clone(), session_token).add_document_listener(Box::new(doc_listener));

    repl.start(false);

    std::thread::sleep(std::time::Duration::from_secs(3));

    // Auto-purge test scenario from support ticket https://support.couchbase.com/hc/en-us/requests/70596?page=1
    // Testing if documents pushed to inaccessible channels get auto-purged
    create_doc(&mut db, "doc1", "channel1");
    create_doc(&mut db, "doc2", "channel2");

    std::thread::sleep(std::time::Duration::from_secs(10));
    assert!(get_doc(&db, "doc1").is_ok());
    assert!(get_doc(&db, "doc2").is_ok()); // This looks buggy

    change_channel(&mut db, "doc1", "channel2");

    std::thread::sleep(std::time::Duration::from_secs(10));
    assert!(get_doc(&db, "doc1").is_err());

    repl.stop(None);
}

fn create_doc(db: &mut Database, id: &str, channel: &str) {
    let mut doc = Document::new_with_id(id);
    doc.set_properties_as_json(
        &serde_json::json!({
            "channels": channel,
        })
        .to_string(),
    )
    .unwrap();
    db.save_document(&mut doc).unwrap();

    println!(
        "Created doc {id} with content: {}",
        doc.properties_as_json()
    );
}

fn get_doc(db: &Database, id: &str) -> Result<Document> {
    db.get_document(id)
}

fn change_channel(db: &mut Database, id: &str, channel: &str) {
    let mut doc = get_doc(db, id).unwrap();
    let mut prop = doc.mutable_properties();
    prop.at("channels").put_string(channel);
    let _ = db.save_document(&mut doc);
    println!(
        "Changed doc {id} with content: {}",
        doc.properties_as_json()
    );
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
    println!("=== Document(s) replicated ===");
    println!("Direction: {direction:?}");
    for document in documents {
        println!("Document: {document:?}");
    }
    println!("===");
}
