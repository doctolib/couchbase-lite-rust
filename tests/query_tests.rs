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
