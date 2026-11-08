//! Databases written by [`Writer`] and read back by [`Database`]: random
//! rows and values (NULL, integers at their limits, floats, any text,
//! blobs), payloads around the overflow limits, trees several levels
//! deep, the schema, and what happens when the card is full.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{COLUMNS, Memory, PAGE, read_rows, table_entry, text, write_table};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sqlite::{Database, Error, Value, Writer, columns, decode_record, encode_record};
use proptest::prelude::*;

/// Integers where their stored size changes, and the limits.
const EDGES: [i64; 18] = [
    i64::MIN,
    i64::MIN + 1,
    -140_737_488_355_329,
    -140_737_488_355_328,
    -2_147_483_649,
    -8_388_609,
    -32_769,
    -129,
    -1,
    0,
    1,
    2,
    127,
    128,
    32_768,
    8_388_608,
    140_737_488_355_328,
    i64::MAX,
];

fn value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<i64>().prop_map(Value::Int),
        prop::sample::select(&EDGES[..]).prop_map(Value::Int),
        any::<f64>().prop_map(Value::Real),
        any::<String>().prop_map(Value::Text),
        prop::collection::vec(any::<u8>(), 0..40).prop_map(Value::Blob),
        // Long ones, into overflow pages.
        (0usize..12_000).prop_map(|n| Value::Blob((0..n).map(|i| (i % 251) as u8).collect())),
        (0usize..9_000).prop_map(|n| Value::Text("ü".repeat(n / 2))),
    ]
}

fn row() -> impl Strategy<Value = Vec<Value>> {
    prop::collection::vec(value(), 0..=COLUMNS.len())
}

/// Rows with distinct ids in order.
fn rows(max: usize) -> impl Strategy<Value = Vec<(i64, Vec<Value>)>> {
    prop::collection::btree_set(
        prop_oneof![any::<i64>(), -500i64..500, prop::sample::select(&EDGES[..])],
        0..max,
    )
    .prop_flat_map(|ids| {
        let n = ids.len();
        (Just(ids), prop::collection::vec(row(), n))
    })
    .prop_map(|(ids, rows)| ids.into_iter().zip(rows).collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(64)))]

    /// Whatever rows go in come out, in order and byte for byte.
    #[test]
    fn rows_read_back_as_written(rows in rows(300)) {
        let storage = Memory::default();
        let root = write_table(&storage, &rows);
        let db = Database::open(&storage, "db").unwrap();
        let read = read_rows(&db, root).unwrap();
        let written: Vec<(i64, Vec<u8>)> =
            rows.iter().map(|(id, v)| (*id, encode_record(v))).collect();
        prop_assert_eq!(read, written);
        prop_assert!(storage.get("db.part").is_none());
    }

    /// A record decodes to the values it was made of (compared as bytes,
    /// so NaN and -0.0 count too).
    #[test]
    fn records_round_trip(values in prop::collection::vec(value(), 0..20)) {
        let record = encode_record(&values);
        let decoded = decode_record(&record).unwrap();
        prop_assert_eq!(decoded.len(), values.len());
        prop_assert_eq!(encode_record(&decoded), record);
    }

    /// Records with a header of sensible size never panic the decoder,
    /// whatever follows.
    #[test]
    fn decoding_any_bytes_after_a_small_header_does_not_panic(
        header in 0u8..0x80,
        rest in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let mut data = vec![header];
        data.extend_from_slice(&rest);
        let _ = decode_record(&data);
    }

    /// Column names are read from any text without panicking.
    #[test]
    fn columns_of_any_text_do_not_panic(sql in any::<String>()) {
        let _ = columns(&sql).unwrap_or_default();
    }
}

/// Payloads just below, at and above the size a cell keeps, and over
/// one and two overflow pages exactly.
#[test]
fn payloads_around_the_overflow_limits_read_back() {
    let sizes = (4030..4110).chain(8150..8210).chain(12_240..12_300);
    let rows: Vec<(i64, Vec<Value>)> = sizes
        .enumerate()
        .map(|(i, n)| {
            let blob = (0..n).map(|k| (k * 7 % 256) as u8).collect();
            (i as i64 + 1, vec![Value::Int(i as i64), Value::Blob(blob)])
        })
        .collect();
    let storage = Memory::default();
    let root = write_table(&storage, &rows);
    let db = Database::open(&storage, "db").unwrap();
    let read = read_rows(&db, root).unwrap();
    assert_eq!(read.len(), rows.len());
    for ((id, values), (read_id, record)) in rows.iter().zip(&read) {
        assert_eq!(id, read_id);
        assert_eq!(&encode_record(values), record, "row {id}");
    }
}

/// A table of three levels: the root and its children are interior
/// pages, and every row is found in order.
#[test]
fn a_table_three_levels_deep_reads_back() {
    let rows: Vec<(i64, Vec<Value>)> = (1..=3000)
        .map(|i| (i * 3, vec![Value::Int(i), text(&"r".repeat(1000))]))
        .collect();
    let storage = Memory::default();
    let root = write_table(&storage, &rows);
    let file = storage.get("db").unwrap();
    let page = |n: u32| &file[(n as usize - 1) * PAGE..n as usize * PAGE];
    assert_eq!(page(root)[0], 0x05, "the root is an interior page");
    let first = u32::from_be_bytes(page(root)[8..12].try_into().unwrap());
    assert_eq!(page(first)[0], 0x05, "so is its right-most child");
    let db = Database::open(&storage, "db").unwrap();
    let read = read_rows(&db, root).unwrap();
    assert_eq!(read.len(), 3000);
    assert!(read.windows(2).all(|w| w[0].0 < w[1].0));
    assert_eq!(read[1234].1, encode_record(&rows[1234].1));
}

/// Ids from the smallest to the largest, over many leaves: interior keys
/// are negative too.
#[test]
fn extreme_row_ids_keep_their_order_across_pages() {
    let step = u64::MAX / 1999;
    let rows: Vec<(i64, Vec<Value>)> = (0..2000u64)
        .map(|i| {
            let id = (i64::MIN as i128 + (i * step) as i128) as i64;
            (id, vec![text(&format!("row {i:04} {}", "x".repeat(40)))])
        })
        .chain([(i64::MAX, vec![text("last")])])
        .collect();
    let storage = Memory::default();
    let root = write_table(&storage, &rows);
    let db = Database::open(&storage, "db").unwrap();
    let ids: Vec<i64> = read_rows(&db, root).unwrap().iter().map(|r| r.0).collect();
    let expected: Vec<i64> = rows.iter().map(|r| r.0).collect();
    assert_eq!(ids.first(), Some(&i64::MIN));
    assert_eq!(ids.last(), Some(&i64::MAX));
    assert_eq!(ids, expected);
}

#[test]
fn an_empty_table_reads_as_empty() {
    let storage = Memory::default();
    let root = write_table(&storage, &[]);
    let db = Database::open(&storage, "db").unwrap();
    assert_eq!(read_rows(&db, root).unwrap(), []);
    assert_eq!(
        db.table("t").unwrap(),
        (COLUMNS.map(String::from).to_vec(), root)
    );
}

/// A scan asked to stop sees no further row, also across pages.
#[test]
fn a_scan_stops_when_asked() {
    let rows: Vec<(i64, Vec<Value>)> = (1..=2000)
        .map(|i| (i, vec![text(&"s".repeat(100))]))
        .collect();
    let storage = Memory::default();
    let root = write_table(&storage, &rows);
    let db = Database::open(&storage, "db").unwrap();
    let mut seen = Vec::new();
    db.scan(root, &mut |id, _| {
        seen.push(id);
        seen.len() < 150
    })
    .unwrap();
    assert_eq!(seen, (1..=150).collect::<Vec<i64>>());
}

#[test]
fn schema_and_user_version_read_back() {
    let storage = Memory::default();
    let mut writer = Writer::new(&storage, "stats.db");
    let a = writer.table([(1, encode_record(&[text("one")]))]).unwrap();
    let b = writer.table([]).unwrap();
    let schema = [
        table_entry("Book", &["title"], a),
        table_entry("page_stat_data", &["id_book", "page"], b),
        paperpaw_sqlite::SchemaEntry {
            kind: "view".to_string(),
            name: "page_stat".to_string(),
            table: "page_stat".to_string(),
            root: 0,
            sql: "CREATE VIEW page_stat AS SELECT * FROM page_stat_data".to_string(),
        },
    ];
    writer.finish(&schema, 20_221_111).unwrap();
    let db = Database::open(&storage, "stats.db").unwrap();
    assert_eq!(db.user_version, 20_221_111);
    assert_eq!(db.schema().unwrap(), schema);
    // Table names are found without regard to ASCII case.
    assert_eq!(db.table("BOOK").unwrap(), (vec!["title".to_string()], a));
    assert_eq!(
        db.table("page_stat"),
        Err(Error::Missing),
        "a view is no table"
    );
    assert_eq!(db.table("nothing"), Err(Error::Missing));
}

/// A schema too large for the first page is refused, before the file is
/// written.
#[test]
fn a_schema_too_large_for_the_first_page_is_refused() {
    let storage = Memory::default();
    let mut writer = Writer::new(&storage, "db");
    let root = writer.table([]).unwrap();
    let long = table_entry("t", &[&"c".repeat(5000)], root);
    assert_eq!(writer.finish(&[long], 1), Err(Error::SchemaTooLarge));
    assert!(storage.get("db").is_none());

    let storage = Memory::default();
    let mut writer = Writer::new(&storage, "db");
    let root = writer.table([]).unwrap();
    // Each fits, all together do not.
    let many: Vec<_> = (0..10)
        .map(|i| table_entry(&format!("t{i}"), &[&"c".repeat(500)], root))
        .collect();
    assert_eq!(writer.finish(&many, 1), Err(Error::SchemaTooLarge));
    assert!(storage.get("db").is_none());
}

/// The temporary pages of a refused schema are removed, as they are
/// when the storage fails.
#[test]
fn a_refused_schema_leaves_no_part_file() {
    let storage = Memory::default();
    let mut writer = Writer::new(&storage, "db");
    let rows = (1..=100).map(|i| (i, encode_record(&[text(&"p".repeat(200))])));
    let root = writer.table(rows).unwrap();
    let long = table_entry("t", &[&"c".repeat(5000)], root);
    assert_eq!(writer.finish(&[long], 1), Err(Error::SchemaTooLarge));
    assert_eq!(storage.paths(), Vec::<String>::new());
}

/// A card that fills up while the database is put together (its pages
/// were written): the error comes back, the database there before is
/// left as it was and no part of the new one or its pages remain.
#[test]
fn a_write_that_fails_part_way_leaves_the_old_database() {
    let storage = Memory::default();
    storage.put("db", b"the database before".to_vec());
    let mut writer = Writer::new(&storage, "db");
    let rows = (1..=100).map(|i| (i, encode_record(&[Value::Int(i)])));
    let root = writer.table(rows).unwrap();
    storage.full.set(true);
    assert_eq!(
        writer.finish(&[table_entry("t", &["a"], root)], 1),
        Err(Error::Storage(StorageError::Full))
    );
    assert_eq!(storage.paths(), ["db"]);
    assert_eq!(storage.get("db").unwrap(), b"the database before");
}

/// A storage that cannot move files: the write is refused and the database
/// there before is left as it was (no copy over it that could fail part
/// way), with no temporary file.
#[test]
fn a_storage_that_cannot_move_files_keeps_the_old_database() {
    let storage = Memory::default();
    storage.put("db", b"the database before".to_vec());
    storage.fixed.set(true);
    let mut writer = Writer::new(&storage, "db");
    let rows = (1..=100).map(|i| (i, encode_record(&[Value::Int(i)])));
    let root = writer.table(rows).unwrap();
    assert_eq!(
        writer.finish(&[table_entry("t", &["a"], root)], 1),
        Err(Error::Storage(StorageError::Unsupported))
    );
    assert_eq!(storage.paths(), ["db"]);
    assert_eq!(storage.get("db").unwrap(), b"the database before");
}

/// A card that fills up while the pages are written: the error comes
/// back, no database and no temporary file are left.
#[test]
fn a_full_card_is_reported_and_leaves_nothing() {
    let storage = Memory::default();
    let mut writer = Writer::new(&storage, "db");
    storage.full.set(true);
    let rows = (1..=100).map(|i| (i, encode_record(&[Value::Int(i)])));
    assert_eq!(writer.table(rows), Err(Error::Storage(StorageError::Full)));
    assert_eq!(
        writer.finish(&[table_entry("t", &["a"], 2)], 1),
        Err(Error::Storage(StorageError::Full))
    );
    assert!(storage.get("db").is_none());
    assert!(storage.get("db.part").is_none());
}

/// A database written over an older one replaces it whole.
#[test]
fn writing_again_replaces_the_old_database() {
    let storage = Memory::default();
    let big: Vec<(i64, Vec<Value>)> = (1..=500)
        .map(|i| (i, vec![text(&"o".repeat(300))]))
        .collect();
    write_table(&storage, &big);
    let old_len = storage.size("db").unwrap();
    let root = write_table(&storage, &[(7, vec![text("new")])]);
    assert!(storage.size("db").unwrap() < old_len);
    let db = Database::open(&storage, "db").unwrap();
    assert_eq!(
        read_rows(&db, root).unwrap(),
        [(7, encode_record(&[text("new")]))]
    );
}

#[test]
fn columns_come_from_odd_create_statements() {
    // No parentheses, or the wrong way round: no columns, no panic.
    assert!(columns("CREATE TABLE t").unwrap().is_empty());
    assert!(columns(") broken (").unwrap().is_empty());
    assert!(columns("").unwrap().is_empty());
    // Quoted and bracketed names, constraints, nested parentheses and
    // comments, names in other scripts.
    let sql = "CREATE TABLE \"données\" (\n  \"prénom\" TEXT CHECK (length(\"prénom\") > (1)), -- the name\n  [日付] INTEGER,\n  `x` DEFAULT (1, 2),\n  CONSTRAINT pk PRIMARY KEY (x),\n  CHECK (x > 0)\n)";
    assert_eq!(columns(sql).unwrap(), ["prénom", "日付", "x"]);
    // Empty parts are skipped.
    assert_eq!(columns("CREATE TABLE t(a, , b,)").unwrap(), ["a", "b"]);
}

/// SQLite keeps a table's statement as it was written, comments and
/// default texts included.
#[test]
fn commas_in_comments_and_texts_make_no_columns() {
    let sql =
        "CREATE TABLE t (\n  a TEXT, -- the title, as shown\n  b TEXT DEFAULT 'x, y',\n  c\n)";
    assert_eq!(columns(sql).unwrap(), ["a", "b", "c"]);
}
