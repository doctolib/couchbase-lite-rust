extern crate couchbase_lite;
extern crate regex;

use couchbase_lite::index::{ValueIndexConfiguration, ArrayIndexConfiguration};
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
fn get_index() {
    utils::with_db(|db| {
        // Default collection
        let default_collection = db.default_collection().unwrap().unwrap();
        assert!(
            default_collection
                .create_index(
                    "new_index1",
                    &ValueIndexConfiguration::new(QueryLanguage::JSON, r#"[[".someField"]]"#, ""),
                )
                .unwrap()
        );

        let index1 = default_collection.get_index("new_index1").unwrap();
        assert_eq!(index1.name(), "new_index1");
        assert_eq!(
            index1.collection().full_name(),
            default_collection.full_name()
        );

        // New collection
        let new_coll = db
            .create_collection(String::from("coll"), String::from("scop"))
            .unwrap();

        assert!(
            new_coll
                .create_index(
                    "new_index2",
                    &ValueIndexConfiguration::new(QueryLanguage::JSON, r#"[[".someField2"]]"#, ""),
                )
                .unwrap()
        );

        let index2 = new_coll.get_index("new_index2").unwrap();
        assert_eq!(index2.name(), "new_index2");
        assert_eq!(index2.collection().full_name(), new_coll.full_name());
    })
}

fn get_index_name_from_explain(explain: &str) -> Option<String> {
    Regex::new(r"USING INDEX (\w+) ")
        .unwrap()
        .captures(explain)
        .map(|c| c.get(1).unwrap().as_str().to_string())
}

#[test]
fn full_index() {
    utils::with_db(|db| {
        assert!(
            db.create_index(
                "new_index",
                &ValueIndexConfiguration::new(QueryLanguage::JSON, r#"[[".someField"]]"#, ""),
            )
            .unwrap()
        );

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

        let index = get_index_name_from_explain(&query.explain().unwrap()).unwrap();
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
        assert!(
            db.create_index(
                "new_index",
                &ValueIndexConfiguration::new(
                    QueryLanguage::JSON,
                    r#"{"WHAT": [[".id"]], "WHERE": ["=", [".someField"], "someValue"]}"#,
                    ""
                ),
            )
            .unwrap()
        );

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

        let index = get_index_name_from_explain(&query.explain().unwrap()).unwrap();
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
fn array_index() {
    utils::with_db(|db| {
        let mut default_collection = db.default_collection().unwrap().unwrap();

        // Add one document
        let mut doc = Document::new();
        doc.set_properties_as_json(
            r#"{
            "name":"Sam",
            "contacts":[
              {
                "type":"primary",
                "address":{"street":"1 St","city":"San Pedro","state":"CA"},
                "phones":[
                  {"type":"home","number":"310-123-4567"},
                  {"type":"mobile","number":"310-123-6789"}
                ]
              },
              {
                "type":"secondary",
                "address":{"street":"5 St","city":"Seattle","state":"WA"},
                "phones":[
                  {"type":"home","number":"206-123-4567"},
                  {"type":"mobile","number":"206-123-6789"}
                ]
              }
            ],
            "likes":["soccer","travel"]
          }"#,
        )
        .unwrap();
        default_collection.save_document(&mut doc).unwrap();

        // Index with one level of unnest
        let index_configuration =
            ArrayIndexConfiguration::new(QueryLanguage::N1QL, "likes", "").unwrap();

        let result = default_collection
            .create_array_index("one_level", &index_configuration)
            .unwrap();
        assert!(result);

        let query = Query::new(
            db,
            QueryLanguage::N1QL,
            "SELECT _.name, _like FROM _ UNNEST _.likes as _like WHERE _like = 'travel'",
        )
        .unwrap();

        let index = get_index_name_from_explain(&query.explain().unwrap()).unwrap();
        assert_eq!(index, "one_level");

        let mut result = query.execute().unwrap();
        let row = result.next().unwrap();
        assert_eq!(row.as_array().to_json(), r#"["Sam","travel"]"#);

        assert!(result.next().is_none());

        // Index with two levels of unnest
        let index_configuration =
            ArrayIndexConfiguration::new(QueryLanguage::N1QL, "contacts[].phones", "type").unwrap();

        assert!(
            default_collection
                .create_array_index("two_levels", &index_configuration,)
                .unwrap()
        );

        let query = Query::new(
            db,
            QueryLanguage::N1QL,
            r#"SELECT _.name, contact.type, phone.number
                FROM _
                UNNEST _.contacts as contact
                UNNEST contact.phones as phone
                WHERE phone.type = 'mobile'"#,
        )
        .unwrap();

        let index = get_index_name_from_explain(&query.explain().unwrap()).unwrap();
        assert_eq!(index, "two_levels");

        let mut result = query.execute().unwrap();

        let row = result.next().unwrap();
        assert_eq!(
            row.as_array().to_json(),
            r#"["Sam","primary","310-123-6789"]"#
        );

        let row = result.next().unwrap();
        assert_eq!(
            row.as_array().to_json(),
            r#"["Sam","secondary","206-123-6789"]"#
        );

        assert!(result.next().is_none());
    })
}
