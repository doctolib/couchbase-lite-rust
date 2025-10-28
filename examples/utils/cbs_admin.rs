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
    let url = format!("{CBS_URL}:8093/query/service");
    let query = format!(
        "SELECT META().id, META().deleted FROM `{CBS_BUCKET}` WHERE META().id = '{doc_id}'"
    );
    let body = serde_json::json!({"statement": query});

    let response = reqwest::blocking::Client::new()
        .post(&url)
        .basic_auth(CBS_ADMIN_USER, Some(CBS_ADMIN_PWD))
        .json(&body)
        .send();

    match response {
        Ok(resp) => {
            if let Ok(text) = resp.text() {
                println!("CBS check for {doc_id}: {text}");
            }
        }
        Err(e) => println!("CBS check error: {e}"),
    }
}
