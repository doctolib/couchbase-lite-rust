use crate::utils::constants::*;

pub fn add_or_update_user(name: &str, channels: Vec<String>) {
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
    println!("Adding user to SGW response: {result:?}");
}

pub fn get_session(name: &str) -> String {
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
    println!("Get session response: {result:?}");
    result["session_id"].as_str().unwrap().to_string()
}

pub fn get_doc_rev(doc_id: &str) -> Option<String> {
    // Try to get the document, including deleted/tombstone versions
    let url = format!("{SYNC_GW_URL_ADMIN}/{doc_id}?deleted=true");
    let result = reqwest::blocking::Client::new().get(&url).send();

    match result {
        Ok(response) => {
            let status = response.status();
            println!("Get doc revision result: status={status}");
            if status.is_success() {
                let json: serde_json::Value = response.json().unwrap();
                let rev = json["_rev"].as_str().unwrap().to_string();
                let is_deleted = json["_deleted"].as_bool().unwrap_or(false);
                println!("get_doc_rev for {doc_id}: found rev {rev} (deleted: {is_deleted})");
                Some(rev)
            } else {
                println!("get_doc_rev for {doc_id}: status {status}, document not found");
                None
            }
        }
        Err(e) => {
            println!("get_doc_rev for {doc_id}: error {e}");
            None
        }
    }
}

pub fn delete_doc_from_sgw(doc_id: &str) -> Option<String> {
    if let Some(rev) = get_doc_rev(doc_id) {
        let url = format!("{SYNC_GW_URL_ADMIN}/{doc_id}?rev={rev}");
        let response = reqwest::blocking::Client::new()
            .delete(&url)
            .send()
            .unwrap();

        let status = response.status();
        let body: serde_json::Value = response.json().unwrap();
        println!("Delete {doc_id} response: status={status}, body={body}");

        if status.is_success() {
            let tombstone_rev = body["rev"].as_str().unwrap().to_string();
            return Some(tombstone_rev);
        }
    }
    println!("Cannot delete {doc_id}");
    None
}

pub fn purge_doc_from_sgw(doc_id: &str, tombstone_rev: &str) {
    let url = format!("{SYNC_GW_URL_ADMIN}/_purge");
    let to_post = serde_json::json!({
        doc_id: [tombstone_rev]
    });
    let result = reqwest::blocking::Client::new()
        .post(&url)
        .json(&to_post)
        .send();
    println!("Purge {doc_id} (tombstone rev {tombstone_rev}) from SGW: {result:?}");
}

pub fn set_sgw_database_offline() {
    let url = format!("{SYNC_GW_URL_ADMIN}/_offline");
    let response = reqwest::blocking::Client::new().post(&url).send();

    match response {
        Ok(resp) => println!("Set SGW database offline: status={}", resp.status()),
        Err(e) => println!("Set SGW database offline error: {e}"),
    }
}

pub fn set_sgw_database_online() {
    let url = format!("{SYNC_GW_URL_ADMIN}/_online");
    let response = reqwest::blocking::Client::new().post(&url).send();

    match response {
        Ok(resp) => println!("Set SGW database online: status={}", resp.status()),
        Err(e) => println!("Set SGW database online error: {e}"),
    }
}

pub fn resync_sgw_database() {
    let url = format!("{SYNC_GW_URL_ADMIN}/_resync?action=start");
    let body = serde_json::json!({
        "regenerate_sequences": true
    });

    let response = reqwest::blocking::Client::new()
        .post(&url)
        .json(&body)
        .send();

    match response {
        Ok(resp) => {
            let status = resp.status();
            if let Ok(body) = resp.text() {
                println!("Resync SGW database: status={status}, body={body}");
            } else {
                println!("Resync SGW database: status={status}");
            }
        }
        Err(e) => println!("Resync SGW database error: {e}"),
    }
}

pub fn compact_sgw_database() {
    let url = format!("{SYNC_GW_URL_ADMIN}/_compact");
    let response = reqwest::blocking::Client::new().post(&url).send();

    match response {
        Ok(resp) => {
            let status = resp.status();
            if let Ok(body) = resp.text() {
                println!("Compact SGW database: status={status}, body={body}");
            } else {
                println!("Compact SGW database: status={status}");
            }
        }
        Err(e) => println!("Compact SGW database error: {e}"),
    }
}
