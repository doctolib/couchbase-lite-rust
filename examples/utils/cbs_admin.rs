use crate::utils::constants::*;

pub fn purge_doc_from_cbs(doc_id: &str) {
    let url = format!("{CBS_URL}/pools/default/buckets/{CBS_BUCKET}/docs/{doc_id}");
    let response = reqwest::blocking::Client::new()
        .delete(&url)
        .basic_auth(CBS_ADMIN_USER, Some(CBS_ADMIN_PWD))
        .send();

    match response {
        Ok(resp) => {
            let status = resp.status();
            if let Ok(body) = resp.text() {
                println!("Purge {doc_id} from CBS: status={status}, body={body}");
            } else {
                println!("Purge {doc_id} from CBS: status={status}");
            }
        }
        Err(e) => println!("Purge {doc_id} from CBS error: {e}"),
    }
}

pub fn compact_cbs_bucket() {
    let url = format!("{CBS_URL}/pools/default/buckets/{CBS_BUCKET}/controller/compactBucket");
    let response = reqwest::blocking::Client::new()
        .post(&url)
        .basic_auth(CBS_ADMIN_USER, Some(CBS_ADMIN_PWD))
        .send();

    match response {
        Ok(resp) => {
            let status = resp.status();
            if let Ok(body) = resp.text() {
                println!("Compact CBS bucket: status={status}, body={body}");
            } else {
                println!("Compact CBS bucket: status={status}");
            }
        }
        Err(e) => println!("Compact CBS bucket error: {e}"),
    }
}

pub fn check_doc_in_cbs(doc_id: &str) {
    // Use port 8093 for Query service (not 8091 which is admin/REST API)
    // Query XATTRs to see tombstones in shared bucket access mode
    // The _sync xattr contains Sync Gateway metadata including deleted status
    //
    // WARNING: Querying _sync xattr directly is UNSUPPORTED in production per Sync Gateway docs
    // This is only for testing/debugging purposes. The _sync structure can change between versions.
    // Reference: https://docs.couchbase.com/sync-gateway/current/shared-bucket-access.html
    let url = "http://localhost:8093/query/service";

    // Query the entire _sync xattr to see its structure
    // This helps debug what fields are actually available
    let query = format!(
        "SELECT META().id, META().xattrs._sync as sync_metadata FROM `{CBS_BUCKET}` USE KEYS ['{doc_id}']"
    );
    let body = serde_json::json!({"statement": query});

    let response = reqwest::blocking::Client::new()
        .post(url)
        .basic_auth(CBS_ADMIN_USER, Some(CBS_ADMIN_PWD))
        .json(&body)
        .send();

    match response {
        Ok(resp) => {
            let status = resp.status();
            if let Ok(text) = resp.text() {
                // Parse the response to show results more clearly
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    if let Some(results) = json["results"].as_array() {
                        if results.is_empty() {
                            println!(
                                "CBS check for {doc_id}: ✓ Document not found (completely purged)"
                            );
                        } else {
                            println!("CBS check for {doc_id}: Found {} result(s)", results.len());
                            for result in results {
                                // Display the full sync_metadata to understand its structure
                                if let Some(sync_meta) = result.get("sync_metadata") {
                                    if sync_meta.is_null() {
                                        println!(
                                            "  ⚠ sync_metadata is NULL - may lack permissions to read system xattrs"
                                        );
                                        println!(
                                            "  💡 System xattrs (starting with _) may require special RBAC roles"
                                        );
                                    } else {
                                        println!("  📦 Full _sync xattr content:");
                                        println!(
                                            "{}",
                                            serde_json::to_string_pretty(sync_meta).unwrap()
                                        );

                                        // Detect tombstone status from _sync.flags field
                                        // flags == 1 indicates a deleted/tombstone document
                                        // Other indicators: tombstoned_at field, channels.*.del == true
                                        let flags = sync_meta
                                            .get("flags")
                                            .and_then(|v| v.as_i64())
                                            .unwrap_or(0);

                                        let has_tombstoned_at =
                                            sync_meta.get("tombstoned_at").is_some();

                                        let is_tombstone = flags == 1 || has_tombstoned_at;

                                        if is_tombstone {
                                            println!("\n  ✓ Document is TOMBSTONE");
                                            println!("     - flags: {}", flags);
                                            if has_tombstoned_at {
                                                println!(
                                                    "     - tombstoned_at: {}",
                                                    sync_meta["tombstoned_at"]
                                                );
                                            }
                                        } else {
                                            println!("\n  ✓ Document is LIVE");
                                            println!("     - flags: {}", flags);
                                        }
                                    }
                                } else {
                                    println!("  ⚠ No sync_metadata field in result");
                                    println!(
                                        "  Full result: {}",
                                        serde_json::to_string_pretty(result).unwrap()
                                    );
                                }
                            }
                        }
                    } else {
                        println!("CBS check for {doc_id}: status={status}, response={text}");
                    }
                } else {
                    println!("CBS check for {doc_id}: status={status}, response={text}");
                }
            } else {
                println!("CBS check for {doc_id}: status={status}, could not read response");
            }
        }
        Err(e) => println!("CBS check error: {e}"),
    }
}

pub fn set_metadata_purge_interval(days: f64) {
    const MIN_PURGE_INTERVAL_DAYS: f64 = 0.04; // 1 hour minimum per CBS spec

    if days < MIN_PURGE_INTERVAL_DAYS {
        println!(
            "⚠ Warning: CBS metadata purge interval minimum is {MIN_PURGE_INTERVAL_DAYS} days (1 hour)."
        );
        println!(
            "  Requested: {days} days (~{:.1} minutes)",
            days * 24.0 * 60.0
        );
        println!("  CBS may not enforce purge before the minimum interval.");
        println!("  Proceeding with requested value for testing purposes...\n");
    }

    let url = format!("{CBS_URL}/pools/default/buckets/{CBS_BUCKET}");
    let params = [("metadataPurgeInterval", days.to_string())];

    let response = reqwest::blocking::Client::new()
        .post(&url)
        .basic_auth(CBS_ADMIN_USER, Some(CBS_ADMIN_PWD))
        .form(&params)
        .send();

    match response {
        Ok(resp) => {
            let status = resp.status();
            if let Ok(body) = resp.text() {
                println!(
                    "Set metadata purge interval to {days} days: status={status}, body={body}"
                );
            } else {
                println!("Set metadata purge interval to {days} days: status={status}");
            }
        }
        Err(e) => println!("Set metadata purge interval error: {e}"),
    }
}
