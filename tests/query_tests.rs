extern crate couchbase_lite;
extern crate regex;

use couchbase_lite::index::ValueIndexConfiguration;
use regex::Regex;

use self::couchbase_lite::*;

pub mod utils;

#[test]
fn query() {
    utils::with_db(|db| {
        utils::add_doc(db, "doc-1", 1, "one");
        utils::add_doc(db, "doc-2", 2, "two");
        utils::add_doc(db, "doc-3", 3, "three");

        let query = Query::new(
            db,
            QueryLanguage::N1QL,
            "select i, s from _ where i > 1 order by i",
        )
        .expect("create query");
        assert_eq!(query.column_count(), 2);
        assert_eq!(query.column_name(0), Some("i"));
        assert_eq!(query.column_name(1), Some("s"));

        // Step through the iterator manually:
        let mut results = query.execute().expect("execute");
        let mut row = results.next().unwrap(); //FIXME: Do something about the (&results). requirement
        let mut i = row.get(0);
        let mut s = row.get(1);
        assert_eq!(i.as_i64().unwrap(), 2);
        assert_eq!(s.as_string().unwrap(), "two");
        assert_eq!(row.as_dict().to_json(), r#"{"i":2,"s":"two"}"#);

        row = results.next().unwrap();
        i = row.get(0);
        s = row.get(1);
        assert_eq!(i.as_i64().unwrap(), 3);
        assert_eq!(s.as_string().unwrap(), "three");
        assert_eq!(row.as_dict().to_json(), r#"{"i":3,"s":"three"}"#);

        assert!(results.next().is_none());

        // Now try a for...in loop:
        let mut n = 0;
        for row in query.execute().expect("execute") {
            match n {
                0 => {
                    assert_eq!(row.as_array().to_json(), r#"[2,"two"]"#);
                    assert_eq!(row.as_dict().to_json(), r#"{"i":2,"s":"two"}"#);
                }
                1 => {
                    assert_eq!(row.as_array().to_json(), r#"[3,"three"]"#);
                    assert_eq!(row.as_dict().to_json(), r#"{"i":3,"s":"three"}"#);
                }
                _ => {
                    panic!("Too many rows ({})", n);
                }
            }
            n += 1;
        }
        assert_eq!(n, 2);
    });
}

#[test]
fn full_index() {
    utils::with_db(|db| {
        assert!(db
            .create_index(
                "new_index",
                &ValueIndexConfiguration::new(QueryLanguage::JSON, r#"[[".someField"]]"#),
            )
            .unwrap());

        // Check index creation
        let value = db.get_index_names().iter().next().unwrap();
        let name = value.as_string().unwrap();
        assert_eq!(name, "new_index");

        // Check index used
        let query = Query::new(
            db,
            QueryLanguage::N1QL,
            "select _.* from _ where _.someField = 'whatever'",
        )
        .expect("create query");

        let index = Regex::new(r"USING INDEX (\w+) ")
            .unwrap()
            .captures(&query.explain().unwrap())
            .map(|c| c.get(1).unwrap().as_str().to_string())
            .unwrap();

        assert_eq!(index, "new_index");

        // Check index not used
        let query = Query::new(
            db,
            QueryLanguage::N1QL,
            "select _.* from _ where _.notSomeField = 'whatever'",
        )
        .expect("create query");

        let index = Regex::new(r"USING INDEX (\w+) ")
            .unwrap()
            .captures(&query.explain().unwrap())
            .map(|c| c.get(1).unwrap().as_str().to_string());

        assert!(index.is_none());

        // Check index deletion
        assert_eq!(db.get_index_names().count(), 1);

        db.delete_index("idx").unwrap();
        assert_eq!(db.get_index_names().count(), 1);

        db.delete_index("new_index").unwrap();
        assert_eq!(db.get_index_names().count(), 0);
    });
}

#[test]
fn partial_index() {
    utils::with_db(|db| {
        assert!(db
            .create_index(
                "new_index",
                &ValueIndexConfiguration::new(
                    QueryLanguage::JSON,
                    r#"{"WHAT": [[".id"]], "WHERE": ["=", [".someField"], "someValue"]}"#
                ),
            )
            .unwrap());

        // Check index creation
        let value = db.get_index_names().iter().next().unwrap();
        let name = value.as_string().unwrap();
        assert_eq!(name, "new_index");

        // Check index used
        let query = Query::new(
            db,
            QueryLanguage::N1QL,
            "select _.* from _ where _.id = 'id' and _.someField='someValue'",
        )
        .expect("create query");

        let index = Regex::new(r"USING INDEX (\w+) ")
            .unwrap()
            .captures(&query.explain().unwrap())
            .map(|c| c.get(1).unwrap().as_str().to_string())
            .unwrap();

        assert_eq!(index, "new_index");

        // Check index not used
        let query = Query::new(
            db,
            QueryLanguage::N1QL,
            "select _.* from _ where _.id = 'id' and _.someField='notSomeValue'",
        )
        .expect("create query");

        let index = Regex::new(r"USING INDEX (\w+) ")
            .unwrap()
            .captures(&query.explain().unwrap())
            .map(|c| c.get(1).unwrap().as_str().to_string());

        assert!(index.is_none());
    });
}

#[test]
fn parameters() {
    utils::with_db(|db| {
        let mut doc = Document::new_with_id("id1");
        let mut props = doc.mutable_properties();
        props.at("bool").put_bool(true);
        props.at("f64").put_f64(3.1);
        props.at("i64").put_i64(3);
        props.at("string").put_string("allo");
        db.save_document_with_concurency_control(&mut doc, ConcurrencyControl::FailOnConflict)
            .expect("save");

        let query = Query::new(
            db,
            QueryLanguage::N1QL,
            "SELECT _.* FROM _ \
                WHERE _.bool=$bool \
                AND _.f64=$f64 \
                AND _.i64=$i64 \
                AND _.string=$string",
        )
        .expect("create query");

        let mut params = MutableDict::new();
        params.at("bool").put_bool(true);
        params.at("f64").put_f64(3.1);
        params.at("i64").put_i64(3);
        params.at("string").put_string("allo");
        query.set_parameters(&params);

        let params = query.parameters();
        assert_eq!(params.get("bool").as_bool(), Some(true));
        assert_eq!(params.get("f64").as_f64(), Some(3.1));
        assert_eq!(params.get("i64").as_i64(), Some(3));
        assert_eq!(params.get("string").as_string(), Some("allo"));

        assert_eq!(query.execute().unwrap().count(), 1);
    });
}

#[test]
fn array_contains() {
    use std::path::PathBuf;
    use std::time::Instant;

    const DB_NAME: &str = "test_db";

    let base_loc = dirs::data_local_dir()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    let dir = PathBuf::from(format!("{base_loc}/billeo"));

    let cfg = DatabaseConfiguration {
        directory: dir.as_path(),
        encryption_key: None,
    };

    if let Ok(db) = Database::open(DB_NAME, Some(cfg.clone())) {
        db.delete().unwrap();
    }

    let mut db = Database::open(DB_NAME, Some(cfg)).expect("open db");
    assert!(Database::exists(DB_NAME, dir.as_path()));

    // Add documents

    //   - Add Model1

    for i in 0..25000 {
        let mut doc = Document::new_with_id(&format!("id_model1_{i}"));

        let mut props = doc.mutable_properties();
        props.at("type").put_string("Model1");
        props.at("uselessField").put_i64(i);

        let mut model2_ids = MutableArray::new();
        model2_ids
            .append()
            .put_string(&format!("id_model2_{}", 4 * i));
        model2_ids
            .append()
            .put_string(&format!("id_model2_{}", 4 * i + 1));
        model2_ids
            .append()
            .put_string(&format!("id_model2_{}", 4 * i + 2));
        model2_ids
            .append()
            .put_string(&format!("id_model2_{}", 4 * i + 3));
        props.at("model2Ids").put_value(&model2_ids);

        db.save_document_with_concurency_control(&mut doc, ConcurrencyControl::FailOnConflict)
            .expect("save");
    }

    //   - Add Model2

    for i in 0..100000 {
        let mut doc = Document::new_with_id(&format!("id_model2_{i}"));

        let mut props = doc.mutable_properties();
        props.at("type").put_string("Model2");

        db.save_document_with_concurency_control(&mut doc, ConcurrencyControl::FailOnConflict)
            .expect("save");
    }

    // Run query

    let query = Query::new(
        &db,
        QueryLanguage::N1QL,
        "SELECT _.* FROM _ \
            WHERE _.type='Model1' \
            AND ARRAY_CONTAINS(_.model2Ids, $model2Id)",
    )
    .expect("create query");

    fn run_query(use_case: &str, query: &Query) {
        println!(
            "Explain for use case [{}]: {}",
            use_case,
            query.explain().unwrap()
        );

        let start = Instant::now();

        for i in 0..100 {
            let mut params = MutableDict::new();
            params
                .at("model2Id")
                .put_string(&format!("id_model2_{}", i * 100));
            query.set_parameters(&params);

            assert_eq!(query.execute().unwrap().count(), 1);
        }

        let stop = start.elapsed();

        println!(
            "Query average time for use case [{}]: {:?}",
            use_case,
            stop / 100
        );
    }

    //   - No index

    run_query("no index", &query);

    //   - Good index

    assert!(db
        .create_index(
            "good_index",
            &ValueIndexConfiguration::new(QueryLanguage::JSON, r#"[[".type"], [".model2Ids"]]"#),
        )
        .unwrap());

    run_query("good_index", &query);

    //   - Bad index

    assert!(db
        .create_index(
            "bad_index",
            &ValueIndexConfiguration::new(QueryLanguage::JSON, r#"[[".type"], [".uselessField"]]"#),
        )
        .unwrap());

    run_query("bad_index", &query);
}
