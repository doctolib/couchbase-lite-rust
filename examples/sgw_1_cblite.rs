use std::path::Path;

use couchbase_lite::*;

pub const SYNC_GW_URL_ADMIN: &str = "http://localhost:4985/my-db";
pub const SYNC_GW_URL: &str = "ws://localhost:4984/my-db";

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
    let session_token = Some(get_session("great_name")).unwrap();
    print!("Sync gateway session token: {session_token}");

    let repl_conf = ReplicatorConfiguration {
        database: Some(db.clone()),
        endpoint: Endpoint::new_with_url(SYNC_GW_URL).unwrap(),
        replicator_type: ReplicatorType::PushAndPull,
        continuous: true,
        disable_auto_purge: true, // false if we want auto purge when the user loses access to a document
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
        accept_only_self_signed_server_certificate: false,
    };
    let repl_context = ReplicationConfigurationContext::default();
    let mut repl = Replicator::new(repl_conf, Box::new(repl_context)).unwrap();

    repl.start(false);

    std::thread::sleep(std::time::Duration::from_secs(3));

    let mut doc = Document::new_with_id("id1");
    doc.set_properties_as_json(
        &serde_json::json!({
            "name": "allo2"
        })
        .to_string(),
    )
    .unwrap();
    db.save_document(&mut doc).unwrap();

    assert!(db.get_document("id1").is_ok());
    println!("Doc content: {}", doc.properties_as_json());

    std::thread::sleep(std::time::Duration::from_secs(3));

    repl.stop(None);

    // Create new user session: https://docs.couchbase.com/sync-gateway/current/rest_api_admin.html#tag/Session/operation/post_db-_session
}

fn add_or_update_user(name: &str, channels: Vec<String>) {
    let url_admin_sg = format!("{SYNC_GW_URL_ADMIN}/_user/");
    let user_to_post = serde_json::json!({
        "name": name,
        "password": "very_secure",
        "admin_channels": channels
    });
    let result = reqwest::blocking::Client::new()
        .post(url_admin_sg)
        .json(&user_to_post)
        .send();
    println!("{result:?}");
}

fn get_session(name: &str) -> String {
    let url_admin_sg = format!("{SYNC_GW_URL_ADMIN}/_session");
    let to_post = serde_json::json!({
        "name": name,
    });
    let result: serde_json::Value = reqwest::blocking::Client::new()
        .post(url_admin_sg)
        .json(&to_post)
        .send()
        .unwrap()
        .json()
        .unwrap();
    result["session_id"].as_str().unwrap().to_string()
}
