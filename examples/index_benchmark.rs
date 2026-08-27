//! Couchbase-Lite-only index migration benchmark (no Sync Gateway / Server).
//!
//! What it does, end to end:
//!  1. Opens (or creates on first run) a persistent, ~100 MB database of production-like
//!     billeo documents (1 UserSettingsModel, many FactureModel, fewer FactureLibreModel,
//!     a lot of EhrEncounterModel, plus the supporting types the queries touch).
//!  2. Drops every index, recreates the current production index set, compacts.
//!  3. Runs every query (each shape/variation) `BENCH_RUNS` times and records timings + the
//!     index each query actually uses (from EXPLAIN).
//!  4. Walks the migration steps; for each: applies the index change, compacts, records the
//!     on-disk size, re-runs every query, and writes a per-step report.
//!  5. Writes an overall report answering: did every query end up faster (and by how much),
//!     did the database shrink, and did any query get slower from one step to the next.
//!
//! Run (release strongly recommended — first run generates the data set):
//!     cargo run --release --example index_benchmark
//! Tunables (env): BENCH_DIR, BENCH_TARGET_MB (default 100), BENCH_RUNS (default 10).
//! A small end-to-end smoke run: BENCH_TARGET_MB=3 BENCH_RUNS=3 cargo run --example index_benchmark

#![allow(deprecated)]

use couchbase_lite::index::{ArrayIndexConfiguration, ValueIndexConfiguration};
use couchbase_lite::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DB_NAME: &str = "index_benchmark";
const META_ID: &str = "benchmark_meta_v2"; // bumped: statut distribution changed → dataset must regenerate

fn bench_dir() -> PathBuf {
    PathBuf::from(std::env::var("BENCH_DIR").unwrap_or_else(|_| "./bench_data".to_string()))
}
fn target_bytes() -> u64 {
    let mb: u64 = std::env::var("BENCH_TARGET_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    mb * 1024 * 1024
}
fn runs() -> usize {
    std::env::var("BENCH_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10)
}
/// Per-query wall-clock budget. A pathological query (e.g. an ARRAY_CONTAINS join that no index
/// can fix) stops early instead of blocking the whole run; we report how many runs completed.
const QUERY_BUDGET: Duration = Duration::from_secs(45);

// ------------------------------------------------------------------ tiny deterministic RNG

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % (n as u64)) as usize
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.next() % 100 < pct
    }
    /// Weighted pick: `xs` is (value, weight); returns a value with probability weight/sum.
    fn pick_weighted<'a>(&mut self, xs: &'a [(&'a str, u32)]) -> &'a str {
        let total: u32 = xs.iter().map(|(_, w)| w).sum();
        let mut r = (self.next() % total as u64) as u32;
        for (v, w) in xs {
            if r < *w {
                return v;
            }
            r -= *w;
        }
        xs[0].0
    }
}

// ------------------------------------------------------------------ value pools

const STATUTS: &[&str] = &[
    "Brouillon",
    "Valide",
    "AFormater",
    "Formatee",
    "Securisee",
    "MiseEnLot",
    "MiseEnLotFormate",
    "Emise",
    "Reemise",
    "Acquittee",
    "ATraiterManuellement",
    "TraiteeManuellement",
    "EnAttente",
    "Annulee",
];
// Production-like FactureModel statut distribution: heavily skewed to the settled/final states
// (Acquittee dominates), with the in-flight statuts a few % each. This is what makes a filter like
// `statut IN ('Securisee','Formatee','AFormater')` genuinely selective (→ elects facture_statut),
// while a filter that includes Acquittee stays a scan (realistically). Weights sum to 100.
const FACTURE_STATUT_WEIGHTS: &[(&str, u32)] = &[
    ("Acquittee", 60),
    ("TraiteeManuellement", 7),
    ("Annulee", 5),
    ("Emise", 4),
    ("Reemise", 2),
    ("MiseEnLot", 3),
    ("MiseEnLotFormate", 2),
    ("Securisee", 4),
    ("Formatee", 3),
    ("AFormater", 2),
    ("Valide", 3),
    ("Brouillon", 3),
    ("ATraiterManuellement", 1),
    ("EnAttente", 1),
];
const ETAT_FACTURE: &[&str] = &["enAttente", "paye", "rejete", "enAnomalie", "partiel"];
// Production-like: most bills are settled/paid; the rejected/anomaly states (what `facture_etat_paiement`
// is for) are rare — so a `= 'rejete'` filter is selective and elects the expression index. Sum = 100.
const ETAT_FACTURE_WEIGHTS: &[(&str, u32)] = &[
    ("paye", 55),
    ("enAttente", 30),
    ("partiel", 8),
    ("rejete", 4),
    ("enAnomalie", 3),
];
const ETAT_ASSURE: &[&str] = &["enAttente", "paye", "rejete"];
const ETAT_PART: &[&str] = &["enAttente", "paye", "rejete", "enAnomalie"];
const MODE_SECU: &[&str] = &["Securise", "Degrade"];
const LOT_TYPES: &[&str] = &["LotDeFSE", "LotDeDRE"];
const LOT_STATUTS: &[&str] = &[
    "ATransmettre",
    "AReemettre",
    "Transmis",
    "ATraiterManuellement",
    "EnAnomalie",
    "Acquitte",
];
const SCOR_STATUTS: &[&str] = &[
    "Emis",
    "Reemis",
    "EmisEnAttente",
    "ReemisEnAttente",
    "Acquitte",
];
const MSG_KINDS: &[&str] = &[
    "accuseDeReceptionLogique",
    "rejetSignalementPaiement",
    "fichierDeFactures",
    "retourNoemie",
];
const MSG_STATUTS: &[&str] = &["Recu", "Traite", "Erreur"];
const PAIEMENT_MODES: &[&str] = &["CB", "Cheque", "Especes", "Virement"];
const ACT_CODES: &[&str] = &["AMI", "AIS", "CS", "APC", "GS", "CCAM"];

// id-space sizes (scaled with the target); keep them large enough to be selective
fn scale() -> f64 {
    (target_bytes() as f64 / (100.0 * 1024.0 * 1024.0)).max(0.02)
}
fn scaled(base: usize) -> usize {
    ((base as f64) * scale()).round() as usize
}
fn n_patients() -> usize {
    scaled(8_000).max(200)
}
fn n_care_plans() -> usize {
    scaled(16_000).max(400)
}
fn n_medical_folders() -> usize {
    scaled(8_000).max(200)
}
fn n_sessions() -> usize {
    scaled(20_000).max(500)
}

fn date_time(seed: u64) -> String {
    let year = 2024 + (seed % 3);
    let month = 1 + (seed / 3) % 12;
    let day = 1 + (seed / 36) % 28;
    let hour = (seed / 1008) % 24;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:00:00")
}
fn date_only(seed: u64) -> String {
    let year = 2024 + (seed % 3);
    let month = 1 + (seed / 3) % 12;
    let day = 1 + (seed / 36) % 28;
    format!("{year:04}-{month:02}-{day:02}")
}

// ------------------------------------------------------------------ document generators

fn prestations(rng: &mut Rng, n: usize) -> Value {
    (0..n)
        .map(|_| json!({ "code": rng.pick(ACT_CODES), "montant": (rng.below(8000) + 500) as i64, "quantite": (rng.below(3) + 1) as i64 }))
        .collect()
}

fn facture_doc(i: usize, rng: &mut Rng) -> (String, String) {
    let p = i % n_patients();
    let cp = i % n_care_plans();
    let has_pj = rng.chance(55);
    let pjs: Vec<String> = if has_pj {
        vec![format!("pji_{}", rng.below(6_000.max(1)))]
    } else {
        vec![]
    };
    let has_dossier = rng.chance(20);
    let dossiers: Vec<String> = if has_dossier {
        vec![format!("dos_{}", rng.below(3_000.max(1)))]
    } else {
        vec![]
    };
    let n_presta = rng.below(4) + 1;
    let v = json!({
        "type": "FactureModel",
        "id": format!("fac_{i}"),
        "statut": rng.pick_weighted(FACTURE_STATUT_WEIGHTS),
        "patientId": format!("pat_{p}"),
        "patient": { "id": format!("pat_{p}"), "internalId": format!("int_{p}") },
        "createdAt": date_time(i as u64),
        "updatedAt": date_time(i as u64 + 7),
        "carePlanId": format!("cp_{cp}"),
        "isQuotation": rng.chance(18),
        "isMobile": rng.chance(30),
        "withoutScor": rng.chance(25),
        "modeSecurisationFacture": rng.pick(MODE_SECU),
        "consultationId": format!("cons_{}", i % (n_patients() * 3).max(1)),
        "typeFacture": if rng.chance(10) { "FSEDRE" } else { "FSE" },
        "piecesJointes": pjs,
        "dossierIds": dossiers,
        "totalFacture": { "totalDesMontantsFactures": (rng.below(15000) + 500) as i64 },
        "facture": { "identFacture": { "numFacture": (100000 + i) as i64, "dateElaborationFacture": date_only(i as u64) } },
        "liquidation": {
            "etatPaiementFacture": rng.pick_weighted(ETAT_FACTURE_WEIGHTS),
            "etatPaiementPartAssure": rng.pick(ETAT_ASSURE),
            "etatPaiementPartAmo": rng.pick(ETAT_PART),
            "etatPaiementPartAmc": rng.pick(ETAT_PART),
            "messagesRsp": [],
        },
        "prestationsDuPanier": prestations(rng, n_presta),
    });
    (format!("fac_{i}"), v.to_string())
}

fn encounter_doc(i: usize, rng: &mut Rng) -> (String, String) {
    let mf = i % n_medical_folders();
    let sess = i % n_sessions();
    let cp = i % n_care_plans();
    let acts: Value = (0..(rng.below(3) + 1))
        .map(|_| {
            json!({
                "care_plan_uuid": format!("cp_{}", rng.below(n_care_plans())),
                "ticked": rng.chance(60),
                "deleted_at": Value::Null,
                "code": rng.pick(ACT_CODES),
                "quantity": (rng.below(3) + 1) as i64,
            })
        })
        .collect();
    // first act is deterministic on this encounter's care plan so the [0] index is exercised
    let mut acts_arr = acts.as_array().unwrap().clone();
    acts_arr[0] = json!({ "care_plan_uuid": format!("cp_{cp}"), "ticked": rng.chance(60), "deleted_at": Value::Null, "code": rng.pick(ACT_CODES), "quantity": 1 });
    let v = json!({
        "type": "EhrEncounterModel",
        "id": format!("enc_{i}"),
        "medical_folder_id": format!("mf_{mf}"),
        "session_id": format!("sess_{sess}"),
        "deleted_at": if rng.chance(5) { Value::String(date_time(i as u64)) } else { Value::Null },
        "started_at": date_time(i as u64),
        "encounter_acts": Value::Array(acts_arr),
        "billed_by": format!("bp_{}", rng.below(500)),
    });
    (format!("enc_{i}"), v.to_string())
}

fn facture_libre_doc(i: usize, rng: &mut Rng) -> (String, String) {
    let p = i % n_patients();
    let v = json!({
        "type": "FactureLibreModel",
        "id": format!("fl_{i}"),
        "patientId": format!("pat_{p}"),
        "date": date_only(i as u64),
        "consultationId": format!("cons_{}", i % (n_patients() * 3).max(1)),
        "numFacture": format!("F{i}"),
        "statut": rng.pick_weighted(FACTURE_STATUT_WEIGHTS),
        "practicienFacturation": format!("pf_{}", rng.below(300)),
        "totalFacture": { "totalDesMontantsFactures": (rng.below(9000) + 300) as i64 },
        "liquidation": { "etatPaiementFacture": rng.pick(ETAT_FACTURE), "etatPaiementPartAssure": rng.pick(ETAT_ASSURE) },
    });
    (format!("fl_{i}"), v.to_string())
}

fn paiement_doc(i: usize, rng: &mut Rng, n_factures: usize) -> (String, String) {
    let f = rng.below(n_factures.max(1));
    let p = f % n_patients();
    let v = json!({
        "type": "PaiementModel",
        "id": format!("pay_{i}"),
        "factureId": [format!("fac_{f}")],
        "patientId": format!("pat_{p}"),
        "paiementMode": rng.pick(PAIEMENT_MODES),
        "montant": (rng.below(15000) + 200) as i64,
        "owner": "owner_1",
    });
    (format!("pay_{i}"), v.to_string())
}

fn paiement_libre_doc(i: usize, rng: &mut Rng, n_fl: usize) -> (String, String) {
    let f = rng.below(n_fl.max(1));
    let v = json!({
        "type": "PaiementForFactureLibreModel",
        "id": format!("payl_{i}"),
        "factureLibreId": format!("fl_{f}"),
        "paiementMode": rng.pick(PAIEMENT_MODES),
        "montant": (rng.below(9000) + 200) as i64,
    });
    (format!("payl_{i}"), v.to_string())
}

fn lot_doc(i: usize, rng: &mut Rng, n_factures: usize) -> (String, String) {
    let n = rng.below(8) + 1;
    let factures: Vec<String> = (0..n)
        .map(|_| format!("fac_{}", rng.below(n_factures.max(1))))
        .collect();
    let arls: Vec<String> = (0..rng.below(3))
        .map(|_| format!("msg_{}", rng.below(5_000.max(1))))
        .collect();
    let v = json!({
        "type": "LotModel",
        "id": format!("lot_{i}"),
        "lotType": rng.pick(LOT_TYPES),
        "statut": rng.pick(LOT_STATUTS),
        "createdAt": date_time(i as u64),
        "numeroLot": format!("NL{i}"),
        "numeroLotReconstitue": format!("NLR{i}"),
        "modeSecurisationFacture": rng.pick(MODE_SECU),
        "factures": factures,
        "messagesArl": arls,
    });
    (format!("lot_{i}"), v.to_string())
}

fn lot_scor_doc(i: usize, rng: &mut Rng) -> (String, String) {
    let v = json!({
        "type": "LotScorModel",
        "id": format!("ls_{i}"),
        "statut": rng.pick(SCOR_STATUTS),
        "lotFse": format!("lf_{}", rng.below(4_000.max(1))),
        "entScanordo": { "temps": (rng.below(1_000_000)) as i64, "identification": format!("ident_{}", rng.below(5000)), "compteur": (rng.below(9999)) as i64 },
        "dossierIds": [format!("dos_{}", rng.below(3_000.max(1)))],
    });
    (format!("ls_{i}"), v.to_string())
}

fn dossier_doc(i: usize, rng: &mut Rng, n_factures: usize) -> (String, String) {
    let v = json!({
        "type": "DossierModel",
        "id": format!("dos_{i}"),
        "statut": if rng.chance(40) { "ATransmettre" } else { rng.pick(SCOR_STATUTS) },
        "factureId": format!("fac_{}", rng.below(n_factures.max(1))),
    });
    (format!("dos_{i}"), v.to_string())
}

fn message_doc(i: usize, rng: &mut Rng) -> (String, String) {
    let v = json!({
        "type": "MessageModel",
        "id": format!("msg_{i}"),
        "messageKind": { "type": rng.pick(MSG_KINDS), "lots": [format!("lot_{}", rng.below(4_000.max(1)))] },
        "statut": rng.pick(MSG_STATUTS),
        "createdAt": date_time(i as u64),
        "sujet": format!("suj_{}", rng.below(2000)),
        "typeFlux": format!("flux_{}", rng.below(8)),
    });
    (format!("msg_{i}"), v.to_string())
}

fn patient_doc(i: usize, _rng: &mut Rng) -> (String, String) {
    let v = json!({
        "type": "PatientModel",
        "id": format!("pat_{i}"),
        "internalId": format!("int_{i}"),
        "name": format!("Patient {i}"),
        "supportsDeDroitsAmo": [],
    });
    (format!("pat_{i}"), v.to_string())
}

fn piece_jointe_doc(i: usize, rng: &mut Rng) -> (String, String) {
    let v = json!({
        "type": "PieceJointeModel",
        "id": format!("pj_{i}"),
        "pieceJointeId": format!("pji_{i}"),
        "owner": "owner_1",
        "isGenerated": rng.chance(30),
    });
    (format!("pj_{i}"), v.to_string())
}

// ------------------------------------------------------------------ generation driver

fn insert(
    db: &mut Database,
    count: usize,
    rng: &mut Rng,
    mut make: impl FnMut(usize, &mut Rng) -> (String, String),
) {
    const BATCH: usize = 5_000;
    let mut i = 0;
    while i < count {
        let end = (i + BATCH).min(count);
        db.in_transaction(|d| {
            for k in i..end {
                let (id, jsonstr) = make(k, rng);
                let mut doc = Document::new_with_id(&id);
                doc.set_properties_as_json(&jsonstr).unwrap();
                d.save_document(&mut doc).unwrap();
            }
            Ok::<(), Error>(())
        })
        .unwrap();
        i = end;
    }
}

fn generate_data(db: &mut Database) {
    let target = target_bytes();
    let mut rng = Rng::new(0xC0FFEE_1234_5678);
    let n_factures = scaled(30_000).max(400);
    let n_fl = scaled(6_000).max(100);

    eprintln!("[gen] target ~{} MB", target / 1024 / 1024);

    // settings (singleton)
    db.in_transaction(|d| {
        let mut doc = Document::new_with_id("settings_1");
        doc.set_properties_as_json(&json!({"type":"UserSettingsModel","id":"settings_1","specialitePs":"Kinesitherapeute","numFacturationPs":"1234567","updatedAt": date_time(1)}).to_string()).unwrap();
        d.save_document(&mut doc).unwrap();
        Ok::<(), Error>(())
    })
    .unwrap();

    eprintln!("[gen] patients: {}", n_patients());
    insert(db, n_patients(), &mut rng, |i, r| patient_doc(i, r));
    eprintln!("[gen] pieces jointes: {}", scaled(6_000).max(100));
    insert(db, scaled(6_000).max(100), &mut rng, |i, r| {
        piece_jointe_doc(i, r)
    });
    eprintln!("[gen] factures: {n_factures}");
    insert(db, n_factures, &mut rng, |i, r| facture_doc(i, r));
    eprintln!("[gen] factures libres: {n_fl}");
    insert(db, n_fl, &mut rng, |i, r| facture_libre_doc(i, r));
    eprintln!("[gen] paiements: {}", scaled(24_000).max(300));
    insert(db, scaled(24_000).max(300), &mut rng, |i, r| {
        paiement_doc(i, r, n_factures)
    });
    insert(db, scaled(4_000).max(100), &mut rng, |i, r| {
        paiement_libre_doc(i, r, n_fl)
    });
    eprintln!("[gen] lots / scor / dossiers / messages");
    insert(db, scaled(4_000).max(100), &mut rng, |i, r| {
        lot_doc(i, r, n_factures)
    });
    insert(db, scaled(2_500).max(50), &mut rng, |i, r| {
        lot_scor_doc(i, r)
    });
    insert(db, scaled(3_000).max(50), &mut rng, |i, r| {
        dossier_doc(i, r, n_factures)
    });
    insert(db, scaled(5_000).max(100), &mut rng, |i, r| {
        message_doc(i, r)
    });

    // Encounters are the bulk ("shit ton"): generate in batches until we reach the target size.
    eprintln!("[gen] encounters until target size...");
    let mut enc = 0usize;
    let max_enc = scaled(600_000).max(20_000);
    loop {
        let base = enc; // global offset so encounter ids stay unique across batches
        insert(db, 5_000, &mut rng, |k, r| encounter_doc(base + k, r));
        enc += 5_000;
        let sz = dir_size(&db.path());
        eprintln!("[gen]   encounters={enc} size={} MB", sz / 1024 / 1024);
        if sz >= target || enc >= max_enc {
            break;
        }
    }

    // meta / sentinel
    db.in_transaction(|d| {
        let mut doc = Document::new_with_id(META_ID);
        doc.set_properties_as_json(&json!({"type":"UserSettingsModel","id":META_ID,"generated":true,"factures":n_factures,"encounters":enc,"targetBytes":target}).to_string()).unwrap();
        d.save_document(&mut doc).unwrap();
        Ok::<(), Error>(())
    })
    .unwrap();

    eprintln!("[gen] done: {} documents", db.count());
}

fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = fs::read_dir(p) {
        for e in rd.flatten() {
            if let Ok(m) = e.metadata() {
                if m.is_dir() {
                    total += dir_size(&e.path());
                } else {
                    total += m.len();
                }
            }
        }
    }
    total
}

// ------------------------------------------------------------------ index model

const WF: &str = r#"["=", [".type"], "FactureModel"]"#;
const WFL: &str = r#"["=", [".type"], "FactureLibreModel"]"#;
const WE: &str = r#"["=", [".type"], "EhrEncounterModel"]"#;
const WL: &str = r#"["=", [".type"], "LotModel"]"#;
const WPAT: &str = r#"["=", [".type"], "PatientModel"]"#;
const WPAY: &str = r#"["=", [".type"], "PaiementModel"]"#;
const WPAYL: &str = r#"["=", [".type"], "PaiementForFactureLibreModel"]"#;
const WMSG: &str = r#"["=", [".type"], "MessageModel"]"#;
const WPJ: &str = r#"["=", [".type"], "PieceJointeModel"]"#;

enum Kind {
    Value,
    Partial(&'static str),
    Array(&'static str, &'static str), // path, expressions
}
struct Idx {
    name: &'static str,
    expr: &'static str,
    kind: Kind,
}
const fn v(name: &'static str, expr: &'static str) -> Idx {
    Idx {
        name,
        expr,
        kind: Kind::Value,
    }
}
const fn p(name: &'static str, expr: &'static str, w: &'static str) -> Idx {
    Idx {
        name,
        expr,
        kind: Kind::Partial(w),
    }
}
const fn a(name: &'static str, path: &'static str, expr: &'static str) -> Idx {
    Idx {
        name,
        expr: "",
        kind: Kind::Array(path, expr),
    }
}

/// The current production index set (storage/src/db/cblite/indexes.rs on master).
fn baseline_indexes() -> Vec<Idx> {
    vec![
        v("id_index", r#"[[".id"]]"#),
        v("statut_index", r#"[[".type"],[".statut"]]"#),
        v(
            "bills_patient_id_and_internal_id_index",
            r#"[[".type"],[".statut"],[".patient.id"],[".patient.internalId"]]"#,
        ),
        v(
            "bills_patient_internal_id_index",
            r#"[[".type"],[".statut"],[".patient.internalId"]]"#,
        ),
        v(
            "bills_careplan_quotation_index",
            r#"[[".type"],[".isQuotation"],[".carePlanId"],[".updatedAt"]]"#,
        ),
        v(
            "bordereaux_fetch_bills",
            r#"[[".type"],[".modeSecurisationFacture"],[".createdAt"],[".statut"]]"#,
        ),
        v("type_created_at_index", r#"[[".type"],[".createdAt"]]"#),
        v(
            "type_date_elaboration_facture_index",
            r#"[[".type"],[".facture.identFacture.dateElaborationFacture"]]"#,
        ),
        v(
            "type_paiement_facture_id_index",
            r#"[[".type"],[".factureId[0]"]]"#,
        ),
        v(
            "bills_overview_counters",
            r#"[[".type"],[".liquidation.etatPaiementFacture"],[".liquidation.messagesRsp"]]"#,
        ),
        v(
            "bills_overview_counters_amo",
            r#"[[".type"],[".liquidation.etatPaiementPartAmo"],[".liquidation.etatPaiementPartAmoForce"],[".liquidation.messagesRsp"],[".createdAt"]]"#,
        ),
        v(
            "bills_overview_counters_amc",
            r#"[[".type"],[".liquidation.etatPaiementPartAmc"],[".liquidation.etatPaiementPartAmcForce"],[".liquidation.messagesRsp"],[".createdAt"]]"#,
        ),
        v(
            "bills_overview_counters_force_amo",
            r#"[[".type"],[".liquidation.etatPaiementPartAmoForce"],[".liquidation.messagesRsp"]]"#,
        ),
        v(
            "bills_overview_counters_force_amc",
            r#"[[".type"],[".liquidation.etatPaiementPartAmcForce"],[".liquidation.messagesRsp"]]"#,
        ),
        v(
            "type_consultation_id_index",
            r#"[[".type"],[".consultationId"]]"#,
        ),
        v(
            "type_num_facture_index",
            r#"[[".type"],[".facture.identFacture.numFacture"]]"#,
        ),
        v(
            "type_num_facture_libre_index",
            r#"[[".type"],[".numFacture"]]"#,
        ),
        v("type_internal_id", r#"[[".type"],[".internalId"]]"#),
        v(
            "search_fds_by_statut",
            r#"[[".type"],[".modeSecurisationFacture"],[".statut"],[".isQuotation"]]"#,
        ),
        v("free_bills_patient_id_2", r#"[[".type"],[".patientId"]]"#),
        v("free_bills_paiements", r#"[[".type"],[".factureLibreId"]]"#),
        v(
            "bills_internalid_careplan",
            r#"[[".type"],[".patient.internalId"],[".carePlanId"],[".isQuotation"]]"#,
        ),
        v("bills_careplan_id", r#"[[".type"],[".carePlanId"]]"#),
        v("bills_piecejointe_id", r#"[[".type"],[".pieceJointeId"]]"#),
        v(
            "payment_error_past_month",
            r#"[[".type"],[".messageKind.type"],[".createdAt"]]"#,
        ),
        v(
            "encounters_by_first_care_plan",
            r#"[[".type"],[".encounter_acts[0].care_plan_uuid"],[".deleted_at"]]"#,
        ),
        v(
            "encounters_by_medical_folder",
            r#"[[".type"],[".medical_folder_id"],[".deleted_at"]]"#,
        ),
        v(
            "find_bills_with_scor_to_bundle",
            r#"[[".type"],[".statut"],[".withoutScor"],[".piecesJointes"],[".dossierIds"]]"#,
        ),
        v(
            "bills_overview_counters_statut",
            r#"[[".type"],[".statut"],[".liquidation.etatPaiementFacture"],[".isQuotation"]]"#,
        ),
        v(
            "search_last_reusable_bill",
            r#"[[".type"],[".patient.internalId"],[".createdAt"],[".isQuotation"],[".statut"],[".isMobile"]]"#,
        ),
        p("encounters_by_session_id", r#"[[".session_id"]]"#, WE),
        p(
            "factures_by_tle_conversation_id",
            r#"[[".tleConversationId"]]"#,
            WF,
        ),
        p(
            "bills_without_dre_bundles",
            r#"[[".typeFacture"],[".statut"],[".createdAt"]]"#,
            WF,
        ),
        p(
            "lots_by_lot_type_created_at",
            r#"[[".lotType"],[".createdAt"]]"#,
            WL,
        ),
        a("get_facture_by_pj_id", "piecesJointes", ""),
        a(
            "find_patient_from_db",
            "supportsDeDroitsAmo",
            "donneesBeneficiaire.donneesBeneficiaire.dateDeNaissanceDuBeneficiaire,donneesBeneficiaire.donneesBeneficiaire.rangNaissance",
        ),
    ]
}

struct Step {
    label: &'static str,
    creates: Vec<Idx>,
    deletes: Vec<&'static str>,
}

/// The migration trains from the plan (each = one release; create-before-delete across trains).
fn steps() -> Vec<Step> {
    vec![
        Step {
            label: "T1_facture_patient",
            creates: vec![p(
                "facture_patient",
                r#"[[".patient.internalId"],[".createdAt"]]"#,
                WF,
            )],
            deletes: vec!["bills_overview_counters", "search_fds_by_statut"],
        },
        Step {
            label: "T2_facture_statut",
            // FULL [type, statut, withoutScor] — NOT partial: CBLite won't elect a partial [statut] index
            // for `type AND statut IN (...)` queries (it scans); the full form with type as a leading
            // indexed column is elected. Still the sole statut-led index, so the tie cluster is gone.
            creates: vec![v(
                "facture_statut",
                r#"[[".type"],[".statut"],[".withoutScor"]]"#,
            )],
            deletes: vec![
                "bills_patient_internal_id_index",
                "bills_patient_id_and_internal_id_index",
                "bills_internalid_careplan",
                "search_last_reusable_bill",
            ],
        },
        Step {
            label: "T3_facture_care_plan",
            creates: vec![p(
                "facture_care_plan",
                r#"[[".carePlanId"],[".isQuotation"],[".updatedAt"]]"#,
                WF,
            )],
            deletes: vec![
                "statut_index",
                "bills_overview_counters_statut",
                "find_bills_with_scor_to_bundle",
            ],
        },
        Step {
            label: "T4_lot_factures",
            creates: vec![a("lot_factures", "factures", "")],
            deletes: vec!["bills_careplan_id", "bills_careplan_quotation_index"],
        },
        // Array-index swap + UNNEST/DISTINCT query rework, counted as one creation (plan T5).
        // Replaces the first-element value index encounters_by_first_care_plan; correctness-driven.
        Step {
            label: "T5_encounters_acts_care_plan",
            creates: vec![a(
                "encounters_acts_care_plan",
                "encounter_acts",
                "care_plan_uuid,ticked",
            )],
            deletes: vec!["encounters_by_first_care_plan"],
        },
        Step {
            label: "T6_facture_etat_paiement",
            creates: vec![p(
                "facture_etat_paiement",
                r#"[["IFMISSINGORNULL()", [".liquidation.etatPaiementFactureForce"], [".liquidation.etatPaiementFacture"]]]"#,
                WF,
            )],
            deletes: vec![],
        },
        Step {
            label: "T7_facture_libre_date",
            creates: vec![p("facture_libre_date", r#"[[".date"]]"#, WFL)],
            deletes: vec![],
        },
        // T8+ — partial-conversion hygiene: convert each remaining full [type, col] value index to a
        // partial [col] WHERE type='X'. Query election and depth are unchanged (same column, same
        // seek), so per-query latency stays flat; the payoff is DB size and write amplification, which
        // is exactly what makes question (b) complete. One conversion = one train (a modification).
        // NOTE: type_date_elaboration_facture_index is deliberately NOT converted to partial and stays
        // full — like facture_statut, the partial FactureModel date index is not elected (queries scan).
        Step {
            label: "T8_facture_num_facture",
            creates: vec![p(
                "facture_num_facture",
                r#"[[".facture.identFacture.numFacture"]]"#,
                WF,
            )],
            deletes: vec!["type_num_facture_index"],
        },
        Step {
            label: "T9_facture_mode_securisation",
            creates: vec![p(
                "facture_mode_securisation",
                r#"[[".modeSecurisationFacture"],[".createdAt"],[".statut"]]"#,
                WF,
            )],
            deletes: vec!["bordereaux_fetch_bills"],
        },
        Step {
            label: "T10_patient_internal_id",
            creates: vec![p("patient_internal_id", r#"[[".internalId"]]"#, WPAT)],
            deletes: vec!["type_internal_id"],
        },
        Step {
            label: "T11_paiement_facture_id",
            creates: vec![p("paiement_facture_id", r#"[[".factureId[0]"]]"#, WPAY)],
            deletes: vec!["type_paiement_facture_id_index"],
        },
        Step {
            label: "T12_paiement_facture_libre_id",
            creates: vec![p(
                "paiement_facture_libre_id",
                r#"[[".factureLibreId"]]"#,
                WPAYL,
            )],
            deletes: vec!["free_bills_paiements"],
        },
        Step {
            label: "T13_message_kind_created_at",
            creates: vec![p(
                "message_kind_created_at",
                r#"[[".messageKind.type"],[".createdAt"]]"#,
                WMSG,
            )],
            deletes: vec!["payment_error_past_month"],
        },
        Step {
            label: "T14_facture_libre_num",
            creates: vec![p("facture_libre_num", r#"[[".numFacture"]]"#, WFL)],
            deletes: vec!["type_num_facture_libre_index"],
        },
        Step {
            label: "T15_piece_jointe_id",
            creates: vec![p("piece_jointe_id", r#"[[".pieceJointeId"]]"#, WPJ)],
            deletes: vec!["bills_piecejointe_id"],
        },
        Step {
            label: "T16_encounters_by_medical_folder_partial",
            creates: vec![p(
                "encounters_by_medical_folder",
                r#"[[".medical_folder_id"],[".deleted_at"]]"#,
                WE,
            )],
            deletes: vec!["encounters_by_medical_folder"], // delete the full value index, recreate as partial
        },
    ]
}

fn create_index(coll: &couchbase_lite::collection::Collection, idx: &Idx) {
    let r = match &idx.kind {
        Kind::Value => coll.create_index(
            idx.name,
            &ValueIndexConfiguration::new(QueryLanguage::JSON, idx.expr, None),
        ),
        Kind::Partial(w) => coll.create_index(
            idx.name,
            &ValueIndexConfiguration::new(QueryLanguage::JSON, idx.expr, Some(w)),
        ),
        Kind::Array(path, expr) => coll.create_array_index(
            idx.name,
            &ArrayIndexConfiguration::new(QueryLanguage::N1QL, path, expr).unwrap(),
        ),
    };
    r.unwrap_or_else(|e| panic!("create index {} failed: {e:?}", idx.name));
}

fn drop_all_indexes(coll: &couchbase_lite::collection::Collection) {
    let names: Vec<String> = coll
        .get_index_names()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_string().map(str::to_string))
        .collect();
    for n in names {
        if !n.is_empty() {
            coll.delete_index(&n).unwrap();
        }
    }
}

fn index_names(coll: &couchbase_lite::collection::Collection) -> Vec<String> {
    let mut names: Vec<String> = coll
        .get_index_names()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_string().map(str::to_string))
        .collect();
    names.sort();
    names
}

fn compact(db: &mut Database) {
    db.perform_maintenance(MaintenanceType::Compact).unwrap();
}

/// Close and reopen the database. This is the production-faithful step: CBLite runs the automatic
/// `Optimize` (a *partial* ANALYZE) on close, which refreshes `sqlite_stat1` and RE-ELECTS every
/// query's plan. Query plans do not change the instant an index is created/deleted — they change
/// on the next close. Reproducing that here is the whole point (forcing `FullOptimize` instead
/// would give deterministic stats and hide the flip behaviour we are trying to observe).
fn reopen(db: Database, dir: &Path) -> Database {
    db.close().expect("close db");
    Database::open(
        DB_NAME,
        Some(DatabaseConfiguration {
            directory: dir,
            #[cfg(feature = "enterprise")]
            encryption_key: None,
        }),
    )
    .expect("reopen db")
}

// ------------------------------------------------------------------ queries

struct Q {
    name: String,
    sql: String,
    params: Value,
    // If set, from this step label onward the query runs `rewrite_sql`/`rewrite_params` instead (the
    // production code rewrite ships in the same train as an index change). Same name → one continuous
    // trajectory; the old (slow) form stops running after that train.
    rewrite_from: Option<&'static str>,
    rewrite_sql: String,
    rewrite_params: Value,
}
fn q(name: &str, sql: &str, params: Value) -> Q {
    Q {
        name: name.to_string(),
        sql: sql.to_string(),
        params,
        rewrite_from: None,
        rewrite_sql: String::new(),
        rewrite_params: Value::Null,
    }
}
fn q_rw(
    name: &str,
    old_sql: &str,
    old_params: Value,
    from_step: &'static str,
    new_sql: &str,
    new_params: Value,
) -> Q {
    Q {
        name: name.to_string(),
        sql: old_sql.to_string(),
        params: old_params,
        rewrite_from: Some(from_step),
        rewrite_sql: new_sql.to_string(),
        rewrite_params: new_params,
    }
}

// statut OR-lists the builders reproduce from billeo (finalized / statut_paiement criteria)
const FINALIZED_STATUTS: &[&str] = &[
    "Formatee",
    "Securisee",
    "MiseEnLot",
    "MiseEnLotFormate",
    "Emise",
    "Reemise",
    "Acquittee",
    "ATraiterManuellement",
    "TraiteeManuellement",
    "EnAttente",
];
const STATUT_PAIEMENT_STATUTS: &[&str] = &["Emise", "Reemise", "Acquittee", "MiseEnLot"];
const REJECTED_ETATS: &[&str] = &["rejete", "enAnomalie"];

/// Distinct named production queries that are NOT one of the two big dynamic builders.
fn core_queries() -> Vec<Q> {
    let dr = ("2025-06-01", "2025-07-01"); // realistic 1-month window (~3% of bills) so date filters are selective
    vec![
        q(
            "has_facture_patient",
            "SELECT createdAt, statut FROM _ WHERE type='FactureModel' AND patientId=$patientId",
            json!({"patientId":"pat_5"}),
        ),
        q(
            "find_last_reusable_bill",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.isQuotation IS NOT TRUE AND (_.statut='Acquittee' OR _.statut='Emise' OR _.statut='Securisee' OR _.statut='Formatee') AND _.patient.internalId=$internalId ORDER BY _.createdAt DESC LIMIT 1",
            json!({"internalId":"int_5"}),
        ),
        q(
            "find_bills_with_scor_to_bundle",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND (_.statut='MiseEnLot' OR _.statut='Emise' OR _.statut='Reemise' OR _.statut='Acquittee') AND ARRAY_LENGTH(_.piecesJointes) > 0 AND ARRAY_LENGTH(_.dossierIds) = 0 AND _.withoutScor=false",
            json!({}),
        ),
        q(
            "overview_stats_sum",
            "SELECT SUM(_.totalFacture.totalDesMontantsFactures) FROM _ WHERE ARRAY_CONTAINS($statuts, _.statut) AND _.type='FactureModel' AND _.isQuotation IS NOT TRUE AND _.facture.identFacture.dateElaborationFacture >= $d1 AND _.facture.identFacture.dateElaborationFacture < $d2",
            json!({"statuts":["Formatee","Securisee","MiseEnLot","Emise","Acquittee"],"d1":dr.0,"d2":dr.1}),
        ),
        q(
            "liste_quotation_for_care_plan",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.carePlanId=$carePlanId AND _.isQuotation=true",
            json!({"carePlanId":"cp_5"}),
        ),
        // added (common UI shapes per the frontend map): quotations by care-plan ids, and last quotation of a patient
        q(
            "liste_quotation_by_care_plan_ids",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.carePlanId IN ('cp_5','cp_6') AND _.isQuotation=true",
            json!({}),
        ),
        q(
            "liste_last_quotation_of_patient",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.patient.internalId=$iid AND _.isQuotation=true ORDER BY _.createdAt DESC LIMIT 1",
            json!({"iid":"int_5"}),
        ),
        q(
            "facture_update_care_plan_lookup",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.carePlanId=$carePlanId",
            json!({"carePlanId":"cp_5"}),
        ),
        q(
            "facture_by_num",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.facture.identFacture.numFacture=$num",
            json!({"num":100123}),
        ),
        q(
            "facture_by_consultation",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.consultationId=$cid",
            json!({"cid":"cons_5"}),
        ),
        q(
            "has_facture_recent",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.createdAt >= $d LIMIT 1",
            json!({"d":"2026-01-01"}),
        ),
        q(
            "count_by_statut_etat",
            "SELECT _.statut, COUNT(*) AS n FROM _ WHERE _.type='FactureModel' AND _.statut IN ('Securisee','Formatee','AFormater') AND (_.isQuotation IS FALSE OR _.isQuotation IS NULL OR _.isQuotation IS MISSING) GROUP BY _.statut",
            json!({}),
        ),
        // Rewrite pairs: old ARRAY_CONTAINS form runs until the train that adds the array index + ships
        // the code rewrite, then the UNNEST form takes over — tracked as one query getting faster.
        q_rw(
            "Lot.find_last_bundles_by_bill_id",
            "SELECT doc.id FROM _ doc WHERE ARRAY_CONTAINS(doc.factures, $billId) AND doc.type='LotModel' ORDER BY doc.createdAt",
            json!({"billId":"fac_10"}),
            "T4_lot_factures",
            "SELECT doc.id FROM _ doc UNNEST doc.factures AS f WHERE doc.type='LotModel' AND f=$billId ORDER BY doc.createdAt",
            json!({"billId":"fac_10"}),
        ),
        q_rw(
            "Facture.find_bills_with_scor_to_transmit",
            "SELECT facture.id FROM _ facture JOIN _ lot ON lot.type='LotModel' AND lot.lotType='LotDeFSE' AND ARRAY_CONTAINS(lot.factures, facture.id) WHERE facture.type='FactureModel' AND facture.statut IN ('MiseEnLot','Emise','Reemise','Acquittee') AND ARRAY_LENGTH(facture.piecesJointes) > 0 AND facture.withoutScor=false",
            json!({}),
            "T4_lot_factures",
            // lot-first UNNEST + join: the containment probe uses lot_factures (verified via BENCH_EXPLAIN)
            "SELECT DISTINCT facture.id FROM _ lot UNNEST lot.factures AS lf JOIN _ facture ON facture.id = lf WHERE lot.type='LotModel' AND lot.lotType='LotDeFSE' AND facture.type='FactureModel' AND facture.statut IN ('MiseEnLot','Emise','Reemise','Acquittee') AND ARRAY_LENGTH(facture.piecesJointes) > 0 AND facture.withoutScor=false",
            json!({}),
        ),
        q(
            "arl_details_join",
            "SELECT m.id FROM _ lots JOIN _ m ON m.type='MessageModel' AND ARRAY_CONTAINS(lots.messagesArl, m.id) WHERE lots.type='LotModel' AND m.messageKind.type='accuseDeReceptionLogique' AND lots.id=$lotId",
            json!({"lotId":"lot_10"}),
        ),
        // no-sort payment-state filter (BillForUi.count style): the only shape that can elect facture_etat_paiement
        q(
            "billforui_count_etat_paiement",
            "SELECT count(*) AS n FROM _ facture WHERE facture.type='FactureModel' AND IFMISSINGORNULL(facture.liquidation.etatPaiementFactureForce, facture.liquidation.etatPaiementFacture) = $etat",
            json!({"etat":"rejete"}),
        ),
        // lots / messages / scor / dossier / other types
        q(
            "bordereau_exists_by_mode_date",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.statut IN ('MiseEnLot','Emise','Reemise','Acquittee','ATraiterManuellement','TraiteeManuellement') AND _.modeSecurisationFacture='Degrade' AND _.createdAt >= $d1 AND _.createdAt < $d2 LIMIT 1",
            json!({"d1":dr.0,"d2":dr.1}),
        ),
        q(
            "bordereau_exists_by_mode_nodate",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.statut IN ('MiseEnLot','Emise','Reemise','Acquittee','ATraiterManuellement','TraiteeManuellement') AND _.modeSecurisationFacture='Degrade' LIMIT 1",
            json!({}),
        ),
        q(
            "lot_by_statut",
            "SELECT _.id FROM _ WHERE _.type='LotModel' AND _.statut=$statut",
            json!({"statut":"AReemettre"}),
        ),
        q(
            "lot_by_num_recent",
            "SELECT _.id FROM _ WHERE _.type='LotModel' AND _.numeroLot=$nl AND _.createdAt > $d",
            json!({"nl":"NL10","d":"2025-06-01"}),
        ),
        q(
            "lot_by_type_created",
            "SELECT _.id FROM _ WHERE _.type='LotModel' AND _.lotType='LotDeFSE' AND _.createdAt > $d",
            json!({"d":"2025-06-01"}),
        ),
        q(
            "message_by_statut",
            "SELECT _.id FROM _ WHERE _.type='MessageModel' AND _.statut=$statut",
            json!({"statut":"Traite"}),
        ),
        q(
            "message_by_kind",
            "SELECT _.id FROM _ WHERE _.type='MessageModel' AND _.messageKind.type=$mk",
            json!({"mk":"accuseDeReceptionLogique"}),
        ),
        q(
            "lotscor_by_statut",
            "SELECT _.id FROM _ WHERE _.type='LotScorModel' AND _.statut=$statut",
            json!({"statut":"Emis"}),
        ),
        q(
            "lotscor_by_identification",
            "SELECT _.id FROM _ WHERE _.type='LotScorModel' AND _.entScanordo.identification=$id AND (_.statut='Emis' OR _.statut='Reemis')",
            json!({"id":"ident_5"}),
        ),
        q(
            "dossier_atransmettre",
            "SELECT _.id FROM _ WHERE _.type='DossierModel' AND _.statut='ATransmettre'",
            json!({}),
        ),
        q(
            "delete_bundle_scor_ids",
            "SELECT _.dossierIds FROM _ WHERE _.type='LotScorModel'",
            json!({}),
        ),
        q(
            "patient_by_internal_id",
            "SELECT _.id FROM _ WHERE _.type='PatientModel' AND _.internalId=$iid",
            json!({"iid":"int_5"}),
        ),
        q(
            "paiement_by_facture",
            "SELECT _.id FROM _ WHERE _.type='PaiementModel' AND _.factureId[0]=$fid",
            json!({"fid":"fac_10"}),
        ),
        q(
            "paiement_by_factures_in",
            "SELECT _.id FROM _ WHERE _.type='PaiementModel' AND _.factureId[0] IN ('fac_10','fac_11','fac_12')",
            json!({}),
        ),
        q(
            "piecejointe_by_id",
            "SELECT _.id FROM _ WHERE _.type='PieceJointeModel' AND _.pieceJointeId=$pid",
            json!({"pid":"pji_10"}),
        ),
        q(
            "facturelibre_by_consultation",
            "SELECT _.id FROM _ WHERE _.type='FactureLibreModel' AND _.consultationId=$cid",
            json!({"cid":"cons_5"}),
        ),
        q(
            "facturelibre_overview_date",
            "SELECT SUM(_.totalFacture.totalDesMontantsFactures) FROM _ WHERE _.type='FactureLibreModel' AND _.date >= $d1 AND _.date < $d2",
            json!({"d1":dr.0,"d2":dr.1}),
        ),
        q(
            "facturelibre_by_num",
            "SELECT _.id FROM _ WHERE _.type='FactureLibreModel' AND _.numFacture=$nf",
            json!({"nf":"F10"}),
        ),
        // encounter first-element bug: old encounter_acts[0] form runs until T5, then the UNNEST rewrite
        // (matches any act; uses encounters_acts_care_plan) takes over — one query, fixed at T5.
        q_rw(
            "Encounter.find_ticked_by_care_plan_id",
            "SELECT _.id FROM _ WHERE _.type='EhrEncounterModel' AND _.encounter_acts[0].care_plan_uuid=$cp AND _.deleted_at IS NULL ORDER BY _.started_at",
            json!({"cp":"cp_5"}),
            "T5_encounters_acts_care_plan",
            "SELECT DISTINCT encounters.id FROM _ encounters UNNEST encounters.encounter_acts AS act WHERE encounters.type='EhrEncounterModel' AND encounters.deleted_at IS NULL AND act.care_plan_uuid=$cp AND act.ticked=true",
            json!({"cp":"cp_5"}),
        ),
        q(
            "encounter_by_medical_folder",
            "SELECT _.id FROM _ WHERE _.type='EhrEncounterModel' AND _.medical_folder_id=$mf AND _.deleted_at IS NULL AND ANY act IN _.encounter_acts SATISFIES act.ticked=true END ORDER BY _.started_at",
            json!({"mf":"mf_5"}),
        ),
        q(
            "encounter_by_session",
            "SELECT _.id FROM _ WHERE _.type='EhrEncounterModel' AND _.session_id=$sid",
            json!({"sid":"sess_5"}),
        ),
        // Paiement.liste_paiement — 4 shapes (join factures)
        q(
            "paiement_liste[patient+facture]",
            "SELECT paiement.id FROM _ paiement JOIN _ factures ON factures.owner=paiement.owner AND paiement.factureId[0]=factures.id WHERE paiement.type='PaiementModel' AND factures.type='FactureModel' AND paiement.patientId=$pid AND factures.id=$fid",
            json!({"pid":"pat_5","fid":"fac_10"}),
        ),
        q(
            "paiement_liste[patient]",
            "SELECT paiement.id FROM _ paiement JOIN _ factures ON factures.owner=paiement.owner AND paiement.factureId[0]=factures.id WHERE paiement.type='PaiementModel' AND factures.type='FactureModel' AND paiement.patientId=$pid",
            json!({"pid":"pat_5"}),
        ),
        q(
            "paiement_liste[facture]",
            "SELECT paiement.id FROM _ paiement JOIN _ factures ON factures.owner=paiement.owner AND paiement.factureId[0]=factures.id WHERE paiement.type='PaiementModel' AND factures.type='FactureModel' AND factures.id=$fid",
            json!({"fid":"fac_10"}),
        ),
        q(
            "list_bills_without_payment[patient]",
            "SELECT _.id FROM _ WHERE _.type='FactureModel' AND _.statut NOT IN ('Annulee','Brouillon','Valide') AND _.patient.internalId IN ('int_5','int_6')",
            json!({}),
        ),
        // LotForUi.find_by shapes
        q(
            "lotforui[date]",
            "SELECT _.id FROM _ WHERE _.type='LotModel' AND _.statut != 'EnAnomalie' AND _.createdAt >= $d1 AND _.createdAt <= $d2 ORDER BY _.createdAt DESC",
            json!({"d1":dr.0,"d2":dr.1}),
        ),
        q(
            "lotforui[lotType+date]",
            "SELECT _.id FROM _ WHERE _.type='LotModel' AND _.statut != 'EnAnomalie' AND _.lotType='LotDeFSE' AND _.createdAt >= $d1 ORDER BY _.createdAt DESC",
            json!({"d1":dr.0}),
        ),
        // MessageForUi.find_by shapes
        q(
            "messageforui[date]",
            "SELECT _.id FROM _ WHERE _.type='MessageModel' AND _.createdAt >= $d1 AND _.createdAt <= $d2 ORDER BY _.createdAt DESC",
            json!({"d1":dr.0,"d2":dr.1}),
        ),
        q(
            "messageforui[kind+date]",
            "SELECT _.id FROM _ WHERE _.type='MessageModel' AND _.messageKind.type=$mk AND _.createdAt >= $d1 ORDER BY _.createdAt DESC",
            json!({"mk":"accuseDeReceptionLogique","d1":dr.0}),
        ),
        // FactureLibre.find_by_criteria shapes
        q(
            "facturelibre_criteria[date]",
            "SELECT _.id FROM _ WHERE _.type='FactureLibreModel' AND _.date >= $d1 AND _.date <= $d2",
            json!({"d1":dr.0,"d2":dr.1}),
        ),
        q(
            "facturelibre_criteria[patient]",
            "SELECT _.id FROM _ WHERE _.type='FactureLibreModel' AND _.patientId IN ('pat_5','pat_6')",
            json!({}),
        ),
        q(
            "facturelibre_criteria[num]",
            "SELECT _.id FROM _ WHERE _.type='FactureLibreModel' AND _.numFacture=$nf",
            json!({"nf":"F10"}),
        ),
        // type-only listings
        q(
            "liste_patients_all",
            "SELECT _.id FROM _ WHERE _.type='PatientModel'",
            json!({}),
        ),
        q(
            "liste_lots_all",
            "SELECT _.id FROM _ WHERE _.type='LotModel'",
            json!({}),
        ),
        q(
            "liste_lots_scor_all",
            "SELECT _.id FROM _ WHERE _.type='LotScorModel'",
            json!({}),
        ),
        q(
            "liste_messages_all",
            "SELECT _.id FROM _ WHERE _.type='MessageModel'",
            json!({}),
        ),
        q(
            "settings_get",
            "SELECT _.id FROM _ WHERE _.type='UserSettingsModel' ORDER BY _.updatedAt DESC LIMIT 1",
            json!({}),
        ),
    ]
}

// --- Facture.liste_factures (requete_liste_factures) faithful clause builder ---

#[derive(Default, Clone)]
struct Lf {
    care_plan_id: Option<&'static str>,
    care_plan_ids: Option<&'static [&'static str]>,
    mode_secu: Option<&'static str>,
    created_before: Option<&'static str>,
    caisse: Option<&'static str>,
    num_facture: Option<i64>,
    nir_assure: Option<&'static str>,
    is_mobile: Option<bool>,
    patient_ids: Option<&'static [&'static str]>,
    date_debut: Option<&'static str>,
    date_fin: Option<&'static str>,
    statut: Option<&'static [&'static str]>,
    montant_min: Option<i64>,
    montant_max: Option<i64>,
    etat_pf: Option<&'static [&'static str]>,
    etat_assure: Option<&'static str>,
    finalized: bool,
    statut_paiement: bool,
    with_paiement: bool,
}

fn build_lf(hint: &str, c: &Lf) -> Q {
    let mut w: Vec<String> = vec![
        "facture.type='FactureModel'".into(),
        "facture.isQuotation IS NOT TRUE".into(),
    ];
    let mut params = serde_json::Map::new();
    let mut patient_added = false;
    let mut created_added = false;
    let mut statut_present = false;

    if let Some(ids) = c.patient_ids {
        let ph: Vec<String> = (0..ids.len()).map(|i| format!("$pid{i}")).collect();
        w.push(format!("facture.patient.internalId IN ({})", ph.join(",")));
        for (i, v) in ids.iter().enumerate() {
            params.insert(format!("pid{i}"), json!(v));
        }
        patient_added = true; // when patient set, date_debut/date_fin are NOT added (post-filtered)
    } else {
        if let Some(d) = c.date_debut {
            w.push("facture.createdAt >= $dd".into());
            params.insert("dd".into(), json!(d));
            created_added = true;
        }
        if let Some(d) = c.date_fin {
            w.push("facture.createdAt <= $df".into());
            params.insert("df".into(), json!(d));
            created_added = true;
        }
    }
    if let Some(d) = c.created_before {
        w.push("facture.createdAt < $cb".into());
        params.insert("cb".into(), json!(d));
        created_added = true;
    }
    if let Some(cp) = c.care_plan_id {
        w.push("facture.carePlanId=$cp".into());
        params.insert("cp".into(), json!(cp));
    }
    if let Some(cps) = c.care_plan_ids {
        let ph: Vec<String> = (0..cps.len()).map(|i| format!("$cpid{i}")).collect();
        w.push(format!("facture.carePlanId IN ({})", ph.join(",")));
        for (i, v) in cps.iter().enumerate() {
            params.insert(format!("cpid{i}"), json!(v));
        }
    }
    if let Some(m) = c.mode_secu {
        w.push("facture.modeSecurisationFacture=$ms".into());
        params.insert("ms".into(), json!(m));
    }
    if let Some(cg) = c.caisse {
        w.push("facture.organismeAmo.caisseGestionnaire=$caisse".into());
        params.insert("caisse".into(), json!(cg));
    }
    if let Some(n) = c.num_facture {
        w.push("facture.facture.identFacture.numFacture=$num".into());
        params.insert("num".into(), json!(n));
    }
    if let Some(nir) = c.nir_assure {
        w.push("facture.identBeneficiaire.nir=$nir".into());
        params.insert("nir".into(), json!(nir));
    }
    if let Some(b) = c.is_mobile {
        w.push("facture.isMobile=$mob".into());
        params.insert("mob".into(), json!(b));
    }
    if let Some(mn) = c.montant_min {
        w.push("facture.totalFacture.totalDesMontantsFactures >= $mmin".into());
        params.insert("mmin".into(), json!(mn));
    }
    if let Some(mx) = c.montant_max {
        w.push("facture.totalFacture.totalDesMontantsFactures <= $mmax".into());
        params.insert("mmax".into(), json!(mx));
    }
    if let Some(e) = c.etat_pf {
        w.push("ARRAY_CONTAINS($etatPF, facture.liquidation.etatPaiementFacture)".into());
        params.insert("etatPF".into(), json!(e));
    }
    if let Some(ea) = c.etat_assure {
        w.push("facture.liquidation.etatPaiementPartAssure=$ea".into());
        params.insert("ea".into(), json!(ea));
    }
    // statut criterion (createdAt present => de-indexed ARRAY_CONTAINS, BC-2730)
    if let Some(st) = c.statut {
        if created_added {
            w.push("ARRAY_CONTAINS($statuts, facture.statut)".into());
            params.insert("statuts".into(), json!(st));
        } else {
            let lits: Vec<String> = st.iter().map(|s| format!("'{s}'")).collect();
            w.push(format!("facture.statut IN ({})", lits.join(",")));
        }
        statut_present = true;
    } else if c.finalized {
        let ors = FINALIZED_STATUTS
            .iter()
            .map(|s| format!("facture.statut='{s}'"))
            .collect::<Vec<_>>()
            .join(" OR ");
        w.push(format!("({ors})"));
        statut_present = true;
    } else if c.statut_paiement {
        let ors = STATUT_PAIEMENT_STATUTS
            .iter()
            .map(|s| format!("facture.statut='{s}'"))
            .collect::<Vec<_>>()
            .join(" OR ");
        w.push(format!("({ors})"));
        statut_present = true;
    }
    // BUGS-25737 safety net; #9213: indexable IN only when a patient clause is present
    if !statut_present {
        if patient_added {
            let lits: Vec<String> = STATUTS.iter().map(|s| format!("'{s}'")).collect();
            w.push(format!("facture.statut IN ({})", lits.join(",")));
        } else {
            w.push("ARRAY_CONTAINS($allStatuts, facture.statut)".into());
            params.insert("allStatuts".into(), json!(STATUTS));
        }
    }
    // production's requete_liste_factures always appends `AND 1=1` (also sidesteps a CBLite parser
    // quirk where a trailing `IN (...)` immediately before ORDER BY fails to parse)
    let where_clause = format!("{} AND 1=1", w.join(" AND "));
    let sql = if c.with_paiement {
        format!(
            "SELECT facture.id FROM _ facture JOIN _ paiement ON paiement.factureId[0]=facture.id WHERE paiement.type='PaiementModel' AND {where_clause}"
        )
    } else {
        format!("SELECT facture.id FROM _ facture WHERE {where_clause}")
    };
    q(
        &format!("Facture.liste_factures[{hint}]"),
        &sql,
        Value::Object(params),
    )
}

fn liste_factures_shapes() -> Vec<Q> {
    const PIDS: &[&str] = &["int_5", "int_6"];
    // a realistic "to transmit" filter: the in-flight statuts, WITHOUT the dominant settled Acquittee,
    // so it is genuinely selective (~9% of bills) and elects facture_statut rather than a full scan.
    const STS: &[&str] = &["Securisee", "Formatee", "AFormater"];
    const D1: &str = "2025-06-01";
    const D2: &str = "2025-07-01";
    let mut v = vec![];
    v.push(build_lf("default", &Lf::default()));
    v.push(build_lf(
        "statut",
        &Lf {
            statut: Some(STS),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "period",
        &Lf {
            date_debut: Some(D1),
            date_fin: Some(D2),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "period+statut",
        &Lf {
            date_debut: Some(D1),
            date_fin: Some(D2),
            statut: Some(STS),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "patient",
        &Lf {
            patient_ids: Some(PIDS),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "patient+statut",
        &Lf {
            patient_ids: Some(PIDS),
            statut: Some(STS),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "patient+period",
        &Lf {
            patient_ids: Some(PIDS),
            date_debut: Some(D1),
            date_fin: Some(D2),
            ..Default::default()
        },
    ));
    // pruned (not UI-reachable, per the FR billing frontend map): patient & care plan never co-occur,
    // and care plan is only ever searched with isQuotation=true — see the liste_quotation_* core queries.
    // Plain carePlanId election is still covered by facture_update_care_plan_lookup.
    v.push(build_lf(
        "num_facture",
        &Lf {
            num_facture: Some(100123),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "num_facture+statut",
        &Lf {
            num_facture: Some(100123),
            statut: Some(STS),
            ..Default::default()
        },
    ));
    // pruned (not UI-reachable): modeDeSecurisation is not set on any liste_factures UI path (not even
    // the advanced console form); modeSecu index election is covered by the bordereau_* core queries.
    v.push(build_lf(
        "created_before",
        &Lf {
            created_before: Some(D2),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "created_before+statut",
        &Lf {
            created_before: Some(D2),
            statut: Some(STS),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "montant_range",
        &Lf {
            montant_min: Some(1000),
            montant_max: Some(9000),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "montant+statut",
        &Lf {
            montant_min: Some(1000),
            statut: Some(STS),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "etat_paiement_facture",
        &Lf {
            etat_pf: Some(&["rejete", "enAnomalie"]),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "etat_pf+statut",
        &Lf {
            etat_pf: Some(&["rejete"]),
            statut: Some(STS),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "etat_assure",
        &Lf {
            etat_assure: Some("paye"),
            ..Default::default()
        },
    ));
    // pruned (not UI-reachable): isMobile is never set by any UI call (the engine infers it from the session).
    v.push(build_lf(
        "caisse",
        &Lf {
            caisse: Some("101"),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "nir_assure",
        &Lf {
            nir_assure: Some("199057512345678"),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "finalized",
        &Lf {
            finalized: true,
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "statut_paiement",
        &Lf {
            statut_paiement: true,
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "with_paiement+statut",
        &Lf {
            with_paiement: true,
            statut: Some(STS),
            ..Default::default()
        },
    ));
    v.push(build_lf(
        "with_paiement+patient",
        &Lf {
            with_paiement: true,
            patient_ids: Some(PIDS),
            ..Default::default()
        },
    ));
    v
}

// --- BillForUi.sql_clauses faithful builder ---

#[derive(Clone, Copy)]
enum Bfil {
    ToTeletransmit,
    RejectedAtTeletransmission,
    RejectedAtPayment,
    Rejected,
    Finalized,
    WaitingForPatientPaiement,
    WaitingForThirdParty,
    PaidPatientPart,
    PutOnHold,
    Draft,
}

#[derive(Default, Clone)]
struct Bf {
    patient: bool,
    date: bool,
    paiement_mode: bool,
    etat_paiement: bool,
    num_regexp: bool,
    care_plan_ids: bool,
    filter: Option<Bfil>,
}

fn build_bf(hint: &str, c: &Bf) -> Q {
    let mut w: Vec<String> = vec!["facture.type='FactureModel'".into(), "(facture.isQuotation IS FALSE OR facture.isQuotation IS NULL OR facture.isQuotation IS MISSING)".into()];
    let mut params = serde_json::Map::new();
    if c.patient {
        w.push("facture.patient.internalId IN ('int_5','int_6')".into());
    }
    if c.date {
        w.push("facture.createdAt >= $d1".into());
        w.push("facture.createdAt <= $d2".into());
        params.insert("d1".into(), json!("2025-06-01"));
        params.insert("d2".into(), json!("2025-07-01"));
    }
    if c.paiement_mode {
        w.push("paiements.paiementMode=$pm".into());
        params.insert("pm".into(), json!("CB"));
    }
    if c.etat_paiement {
        w.push("IFMISSINGORNULL(facture.liquidation.etatPaiementFactureForce, facture.liquidation.etatPaiementFacture) = $etat".into());
        params.insert("etat".into(), json!("rejete"));
    }
    if c.num_regexp {
        w.push("REGEXP_CONTAINS(TOSTRING(facture.facture.identFacture.numFacture), $re)".into());
        params.insert("re".into(), json!("123"));
    }
    if c.care_plan_ids {
        w.push("facture.carePlanId IN ('cp_5','cp_6')".into());
    }
    if let Some(f) = c.filter {
        match f {
            Bfil::ToTeletransmit => w.push("facture.statut IN ('Securisee','Formatee','AFormater')".into()),
            Bfil::RejectedAtTeletransmission => w.push("facture.statut='ATraiterManuellement'".into()),
            Bfil::RejectedAtPayment => w.push("IFMISSINGORNULL(facture.liquidation.etatPaiementFactureForce, facture.liquidation.etatPaiementFacture)='rejete'".into()),
            Bfil::Rejected => {
                w.push("(ARRAY_CONTAINS($rej, IFMISSINGORNULL(facture.liquidation.etatPaiementPartAmoForce, facture.liquidation.etatPaiementPartAmo)) OR ARRAY_CONTAINS($rej, IFMISSINGORNULL(facture.liquidation.etatPaiementPartAmcForce, facture.liquidation.etatPaiementPartAmc)))".into());
                params.insert("rej".into(), json!(REJECTED_ETATS));
            }
            Bfil::Finalized => {
                let lits: Vec<String> = FINALIZED_STATUTS.iter().map(|s| format!("'{s}'")).collect();
                w.push(format!("facture.statut IN ({})", lits.join(",")));
            }
            Bfil::WaitingForPatientPaiement => w.push("facture.liquidation.etatPaiementPartAssure='enAttente'".into()),
            Bfil::WaitingForThirdParty => {
                w.push("(ARRAY_CONTAINS($wait, IFMISSINGORNULL(facture.liquidation.etatPaiementPartAmoForce, facture.liquidation.etatPaiementPartAmo)) OR ARRAY_CONTAINS($wait, IFMISSINGORNULL(facture.liquidation.etatPaiementPartAmcForce, facture.liquidation.etatPaiementPartAmc)))".into());
                params.insert("wait".into(), json!(["enAttente"]));
            }
            Bfil::PaidPatientPart => w.push("facture.liquidation.etatPaiementPartAssure='paye'".into()),
            Bfil::PutOnHold => w.push("facture.statut='EnAttente'".into()),
            Bfil::Draft => w.push("facture.statut='Brouillon'".into()),
        }
    }
    // `AND 1=1` before ORDER BY: matches production and avoids the CBLite `IN (...) ORDER BY` parse quirk
    let where_clause = format!("{} AND 1=1", w.join(" AND "));
    let sql = if c.paiement_mode {
        format!(
            "SELECT facture.id FROM _ facture LEFT JOIN _ paiements ON paiements.factureId[0]=facture.id AND paiements.type='PaiementModel' WHERE {where_clause} ORDER BY facture.createdAt DESC LIMIT 50"
        )
    } else {
        format!(
            "SELECT facture.id FROM _ facture WHERE {where_clause} ORDER BY facture.createdAt DESC LIMIT 50"
        )
    };
    q(
        &format!("BillForUi.find_by[{hint}]"),
        &sql,
        Value::Object(params),
    )
}

fn billforui_shapes() -> Vec<Q> {
    let f = |name: &str, fil: Bfil| {
        build_bf(
            name,
            &Bf {
                filter: Some(fil),
                ..Default::default()
            },
        )
    };
    vec![
        build_bf("default", &Bf::default()),
        f("filter=ToTeletransmit", Bfil::ToTeletransmit),
        f(
            "filter=RejectedAtTeletransmission",
            Bfil::RejectedAtTeletransmission,
        ),
        f("filter=RejectedAtPayment", Bfil::RejectedAtPayment),
        f("filter=Rejected", Bfil::Rejected),
        f("filter=Finalized", Bfil::Finalized),
        f(
            "filter=WaitingForPatientPaiement",
            Bfil::WaitingForPatientPaiement,
        ),
        f("filter=WaitingForThirdParty", Bfil::WaitingForThirdParty),
        f("filter=PaidPatientPart", Bfil::PaidPatientPart),
        f("filter=PutOnHold", Bfil::PutOnHold),
        f("filter=Draft", Bfil::Draft),
        build_bf(
            "patient",
            &Bf {
                patient: true,
                ..Default::default()
            },
        ),
        build_bf(
            "patient+ToTeletransmit",
            &Bf {
                patient: true,
                filter: Some(Bfil::ToTeletransmit),
                ..Default::default()
            },
        ),
        build_bf(
            "patient+Finalized",
            &Bf {
                patient: true,
                filter: Some(Bfil::Finalized),
                ..Default::default()
            },
        ),
        build_bf(
            "period",
            &Bf {
                date: true,
                ..Default::default()
            },
        ),
        build_bf(
            "period+Finalized",
            &Bf {
                date: true,
                filter: Some(Bfil::Finalized),
                ..Default::default()
            },
        ),
        build_bf(
            "paiement_mode(join)",
            &Bf {
                paiement_mode: true,
                ..Default::default()
            },
        ),
        build_bf(
            "etat_paiement",
            &Bf {
                etat_paiement: true,
                ..Default::default()
            },
        ),
        build_bf(
            "num_regexp",
            &Bf {
                num_regexp: true,
                ..Default::default()
            },
        ),
        build_bf(
            "care_plan_ids",
            &Bf {
                care_plan_ids: true,
                ..Default::default()
            },
        ),
        build_bf(
            "care_plan_ids+ToTeletransmit",
            &Bf {
                care_plan_ids: true,
                filter: Some(Bfil::ToTeletransmit),
                ..Default::default()
            },
        ),
    ]
}

fn queries() -> Vec<Q> {
    let mut all = core_queries();
    all.extend(liste_factures_shapes());
    all.extend(billforui_shapes());
    all
}

fn json_to_params(v: &Value) -> MutableDict {
    let mut d = MutableDict::new();
    if let Value::Object(m) = v {
        for (k, val) in m {
            match val {
                Value::String(s) => d.at(k).put_string(s),
                Value::Bool(b) => d.at(k).put_bool(*b),
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        d.at(k).put_i64(i);
                    } else {
                        d.at(k).put_f64(n.as_f64().unwrap());
                    }
                }
                Value::Array(arr) => {
                    let mut a = MutableArray::new();
                    for e in arr {
                        match e {
                            Value::String(s) => a.append().put_string(s),
                            Value::Bool(b) => a.append().put_bool(*b),
                            Value::Number(n) => {
                                if let Some(i) = n.as_i64() {
                                    a.append().put_i64(i);
                                } else {
                                    a.append().put_f64(n.as_f64().unwrap());
                                }
                            }
                            _ => {}
                        }
                    }
                    d.at(k).put_value(&a);
                }
                _ => {}
            }
        }
    }
    d
}

/// All `USING INDEX <name>` occurrences (joins have more than one; the first-match-only parse
/// in production is a known blind spot, so we capture them all here).
fn indexes_from_explain(explain: &str) -> String {
    let mut found = vec![];
    for part in explain.split("USING INDEX ").skip(1) {
        let name: String = part
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            found.push(name);
        }
    }
    if found.is_empty() {
        // a bare table scan
        if explain.contains("SCAN") {
            "SCAN".to_string()
        } else {
            "-".to_string()
        }
    } else {
        found.join("+")
    }
}

#[derive(Clone)]
struct Measure {
    median_ms: f64,
    mean_ms: f64,
    min_ms: f64,
    max_ms: f64,
    rows: usize,
    runs: usize,
    index_used: String,
    error: Option<String>,
}

fn run_query(db: &Database, sql: &str, params_json: &Value, n_runs: usize) -> Measure {
    let query = match Query::new(db, QueryLanguage::N1QL, sql) {
        Ok(q) => q,
        Err(e) => {
            return Measure {
                median_ms: f64::NAN,
                mean_ms: f64::NAN,
                min_ms: f64::NAN,
                max_ms: f64::NAN,
                rows: 0,
                runs: 0,
                index_used: "-".into(),
                error: Some(format!("compile: {e:?}")),
            };
        }
    };
    let params = json_to_params(params_json);
    query.set_parameters(&params);
    let index_used = query
        .explain()
        .map(|e| indexes_from_explain(&e))
        .unwrap_or_else(|_| "-".into());

    let mut times = vec![];
    let mut rows = 0;
    let started = Instant::now();
    for r in 0..n_runs {
        let t0 = Instant::now();
        match query.execute() {
            Ok(rs) => {
                let c = rs.count();
                if r == 0 {
                    rows = c;
                }
            }
            Err(e) => {
                return Measure {
                    median_ms: f64::NAN,
                    mean_ms: f64::NAN,
                    min_ms: f64::NAN,
                    max_ms: f64::NAN,
                    rows: 0,
                    runs: r,
                    index_used,
                    error: Some(format!("exec: {e:?}")),
                };
            }
        }
        times.push(t0.elapsed().as_secs_f64() * 1000.0);
        let _ = r;
        // Bound total wall time per query: once the cumulative budget is spent, stop (keeps a
        // pathological ARRAY_CONTAINS join from dominating the run). Fast queries never hit this
        // and complete all n_runs.
        if started.elapsed() > QUERY_BUDGET {
            break;
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = times.len();
    let median = times[n / 2];
    let mean = times.iter().sum::<f64>() / n as f64;
    Measure {
        median_ms: median,
        mean_ms: mean,
        min_ms: times[0],
        max_ms: times[n - 1],
        rows,
        runs: n,
        index_used,
        error: None,
    }
}

fn run_all(
    db: &Database,
    qs: &[Q],
    n_runs: usize,
    cur_ord: usize,
    step_labels: &[String],
) -> BTreeMap<String, Measure> {
    let mut out = BTreeMap::new();
    for q in qs {
        // A rewrite-pair query runs its OLD sql up to (but not including) the train where the rewrite
        // ships, then its NEW sql from that train on — recorded under the same name, so the timeline
        // reads as one query getting faster at that train (and the slow old form stops running).
        let use_rw = q
            .rewrite_from
            .map(|l| {
                cur_ord
                    >= step_labels
                        .iter()
                        .position(|s| s == l)
                        .unwrap_or(usize::MAX)
            })
            .unwrap_or(false);
        let (sql, params, tag) = if use_rw {
            (&q.rewrite_sql, &q.rewrite_params, " [rewritten]")
        } else {
            (&q.sql, &q.params, "")
        };
        let m = run_query(db, sql, params, n_runs);
        eprintln!(
            "    {:<44} {:>9.2} ms  rows={:<7} idx={}{}",
            q.name, m.median_ms, m.rows, m.index_used, tag
        );
        out.insert(q.name.clone(), m);
    }
    out
}

struct StepData {
    label: String,
    size_bytes: u64,
    indexes: Vec<String>,
    results: BTreeMap<String, Measure>,
}

// ------------------------------------------------------------------ reporting

/// The code-rewrite queries are now modeled as rewrite pairs (old form until the rewrite train, new
/// UNNEST form after), so they show up as one query getting faster and belong in the aggregate.
/// Nothing is excluded any more.
fn needs_code_rewrite(_name: &str) -> bool {
    false
}

fn pct_change(base: f64, new: f64) -> f64 {
    if base <= 0.0 || base.is_nan() || new.is_nan() {
        return f64::NAN;
    }
    (new - base) / base * 100.0 // positive = slower, negative = faster
}

const NOISE_PCT: f64 = 15.0;
const NOISE_MS: f64 = 1.0;

fn classify(prev: f64, new: f64) -> &'static str {
    if prev.is_nan() || new.is_nan() {
        return "n/a";
    }
    let d = new - prev;
    if d.abs() < NOISE_MS || pct_change(prev, new).abs() < NOISE_PCT {
        "flat"
    } else if d < 0.0 {
        "faster"
    } else {
        "SLOWER"
    }
}

fn write_step_report(dir: &Path, prev: &StepData, cur: &StepData, step: &Step, n_runs: usize) {
    let mut s = String::new();
    s.push_str(&format!("# Step {} — index change report\n\n", cur.label));
    s.push_str(&format!(
        "- DB size: {:.1} MB (was {:.1} MB, {:+.1} MB)\n",
        mb(cur.size_bytes),
        mb(prev.size_bytes),
        mb(cur.size_bytes) - mb(prev.size_bytes)
    ));
    s.push_str(&format!(
        "- Indexes created: {}\n",
        step.creates
            .iter()
            .map(|i| i.name)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    s.push_str(&format!(
        "- Indexes deleted: {}\n",
        if step.deletes.is_empty() {
            "(none)".to_string()
        } else {
            step.deletes.join(", ")
        }
    ));
    s.push_str(&format!("- Index count now: {}\n\n", cur.indexes.len()));

    s.push_str("| query | prev ms | now ms | change | verdict | index before | index after |\n");
    s.push_str("|---|--:|--:|--:|---|---|---|\n");
    let mut regressions = vec![];
    for (name, m) in &cur.results {
        let pm = prev.results.get(name);
        let (pms, pidx) = pm
            .map(|x| (x.median_ms, x.index_used.clone()))
            .unwrap_or((f64::NAN, "-".into()));
        let verdict = classify(pms, m.median_ms);
        if verdict == "SLOWER" {
            regressions.push((
                name.clone(),
                pms,
                m.median_ms,
                pidx.clone(),
                m.index_used.clone(),
            ));
        }
        s.push_str(&format!(
            "| {} | {:.2} | {:.2} | {:+.1}% | {} | {} | {} |\n",
            name,
            pms,
            m.median_ms,
            pct_change(pms, m.median_ms),
            verdict,
            pidx,
            m.index_used
        ));
    }
    s.push_str("\n");
    if regressions.is_empty() {
        s.push_str("**No query got slower at this step.**\n");
    } else {
        s.push_str("**Queries that got SLOWER at this step:**\n\n");
        for (n, a, b, ia, ib) in &regressions {
            s.push_str(&format!(
                "- `{}` {:.2} → {:.2} ms ({:+.1}%), index {} → {}\n",
                n,
                a,
                b,
                pct_change(*a, *b),
                ia,
                ib
            ));
        }
    }
    let capped = capped_in(cur, n_runs);
    if !capped.is_empty() {
        s.push_str(&format!(
            "\n_Budget-capped (fewer than {n_runs} runs): {}_\n",
            capped.join(", ")
        ));
    }
    let errs = errors_in(cur);
    if !errs.is_empty() {
        s.push_str(&format!("\n**Query errors:** {}\n", errs.join("; ")));
    }
    fs::write(dir.join(format!("report_{}.md", cur.label)), s).unwrap();
    write_step_csv(dir, cur);
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

/// Full raw timings for the step (every recorded field), for deeper offline analysis.
fn write_step_csv(dir: &Path, cur: &StepData) {
    let mut s = String::from("query,median_ms,mean_ms,min_ms,max_ms,rows,runs,index_used,error\n");
    for (name, m) in &cur.results {
        s.push_str(&format!(
            "{},{:.3},{:.3},{:.3},{:.3},{},{},{},{}\n",
            name,
            m.median_ms,
            m.mean_ms,
            m.min_ms,
            m.max_ms,
            m.rows,
            m.runs,
            m.index_used,
            m.error.clone().unwrap_or_default()
        ));
    }
    fs::write(dir.join(format!("timings_{}.csv", cur.label)), s).unwrap();
}

fn errors_in(cur: &StepData) -> Vec<String> {
    cur.results
        .iter()
        .filter_map(|(n, m)| m.error.as_ref().map(|e| format!("{n}: {e}")))
        .collect()
}

fn capped_in(cur: &StepData, n_runs: usize) -> Vec<String> {
    cur.results
        .iter()
        .filter(|(_, m)| m.error.is_none() && m.runs < n_runs)
        .map(|(n, m)| format!("{n} ({} runs, {:.0} ms each)", m.runs, m.median_ms))
        .collect()
}

fn write_overall_report(dir: &Path, history: &[StepData]) {
    let base = &history[0];
    let last = history.last().unwrap();
    let mut s = String::new();
    s.push_str("# Overall benchmark report\n\n");
    s.push_str(&format!(
        "Steps: {}\n\n",
        history
            .iter()
            .map(|h| h.label.clone())
            .collect::<Vec<_>>()
            .join(" → ")
    ));

    // ---- size per step ----
    s.push_str("## Database size per step\n\n| step | size MB | indexes |\n|---|--:|--:|\n");
    for h in history {
        s.push_str(&format!(
            "| {} | {:.1} | {} |\n",
            h.label,
            mb(h.size_bytes),
            h.indexes.len()
        ));
    }
    s.push_str(&format!(
        "\n**(b) Database size: {:.1} MB → {:.1} MB ({:+.1} MB, {:+.1}%).** {}\n\n",
        mb(base.size_bytes),
        mb(last.size_bytes),
        mb(last.size_bytes) - mb(base.size_bytes),
        pct_change(base.size_bytes as f64, last.size_bytes as f64),
        if last.size_bytes < base.size_bytes {
            "Smaller. ✅"
        } else {
            "Not smaller. ⚠️"
        }
    ));

    // ---- (a) end vs baseline per query ----
    s.push_str("## (a) Final vs baseline — per query\n\n");
    s.push_str("Positive % = faster at the end than at baseline. Verdict uses the noise guard ");
    s.push_str(&format!(
        "(a change counts only if it moves > {NOISE_MS:.0} ms AND > {NOISE_PCT:.0}%).\n\n"
    ));
    s.push_str("| query | baseline ms | final ms | speedup% | verdict | baseline idx | final idx |\n|---|--:|--:|--:|---|---|---|\n");

    let mut faster = 0usize;
    let mut flat = 0usize;
    let mut slower = vec![];
    let mut errored = vec![];
    let mut rewrite_rows = vec![];
    let mut base_sum = 0.0; // index-addressable queries only (exclude the code-rewrite joins)
    let mut last_sum = 0.0;
    for (name, bm) in &base.results {
        let lm = last.results.get(name).unwrap();
        let speedup = -pct_change(bm.median_ms, lm.median_ms);
        let verdict = if bm.error.is_some() || lm.error.is_some() {
            "ERR"
        } else {
            classify(bm.median_ms, lm.median_ms)
        };
        s.push_str(&format!(
            "| {} | {:.2} | {:.2} | {:+.1}% | {} | {} | {} |\n",
            name, bm.median_ms, lm.median_ms, speedup, verdict, bm.index_used, lm.index_used
        ));
        if verdict == "ERR" {
            errored.push(name.clone());
            continue;
        }
        if needs_code_rewrite(name) {
            rewrite_rows.push((name.clone(), bm.median_ms, lm.median_ms, speedup));
            continue; // not index-addressable — kept out of the headline aggregate
        }
        base_sum += bm.median_ms;
        last_sum += lm.median_ms;
        match verdict {
            "faster" => faster += 1,
            "SLOWER" => slower.push((
                name.clone(),
                bm.median_ms,
                lm.median_ms,
                bm.index_used.clone(),
                lm.index_used.clone(),
            )),
            _ => flat += 1,
        }
    }
    let overall = if last_sum > 0.0 {
        base_sum / last_sum
    } else {
        f64::NAN
    };
    s.push_str(&format!(
        "\n**(a) Of the index-addressable queries: {faster} faster, {flat} already-fast/flat, {} slower.** Aggregate time {:.1} ms → {:.1} ms — {:.2}× overall.\n\n",
        slower.len(),
        base_sum,
        last_sum,
        overall
    ));
    if slower.is_empty() {
        s.push_str("No index-addressable query ended up slower than baseline. ✅\n\n");
    } else {
        s.push_str("**Index-addressable queries that ended up SLOWER than baseline (need attention):**\n\n");
        for (n, b, l, ib, il) in &slower {
            s.push_str(&format!(
                "- `{}` {:.2} → {:.2} ms ({:+.1}%), index {} → {}\n",
                n,
                b,
                l,
                pct_change(*b, *l),
                ib,
                il
            ));
        }
        s.push_str("\n");
    }
    if !rewrite_rows.is_empty() {
        s.push_str("**Queries fixed by a code rewrite, not an index (ARRAY_CONTAINS-on-parameter containment, or a first-element `array[0]` access replaced by UNNEST + an array index). Shown for context, excluded from the aggregate — their rewritten UNNEST counterparts are in the table above:**\n\n");
        for (n, b, l, sp) in &rewrite_rows {
            s.push_str(&format!("- `{}` {:.2} → {:.2} ms ({:+.1}%)\n", n, b, l, sp));
        }
        s.push_str("\n");
    }
    if !errored.is_empty() {
        s.push_str(&format!(
            "**Queries that errored (see CSV for message): {}**\n\n",
            errored.join(", ")
        ));
    }

    // ---- (c) step-to-step regressions ----
    s.push_str("## (c) Step-to-step regressions\n\n");
    let mut any = false;
    for w in history.windows(2) {
        let (prev, cur) = (&w[0], &w[1]);
        for (name, m) in &cur.results {
            if let Some(pm) = prev.results.get(name) {
                if classify(pm.median_ms, m.median_ms) == "SLOWER" {
                    any = true;
                    s.push_str(&format!(
                        "- `{}` at {}: {:.2} → {:.2} ms ({:+.1}%), index {} → {}\n",
                        name,
                        cur.label,
                        pm.median_ms,
                        m.median_ms,
                        pct_change(pm.median_ms, m.median_ms),
                        pm.index_used,
                        m.index_used
                    ));
                }
            }
        }
    }
    if !any {
        s.push_str("**No query got slower from one step to the next.** ✅\n");
    } else {
        s.push_str("\n(Each line is a query that regressed at that step — investigate before shipping that train.)\n");
    }

    s.push_str("\n---\n\nPer-step tables are in the `report_T*.md` files in this directory.\n");
    fs::write(dir.join("report_OVERALL.md"), &s).unwrap();
    println!("\n{s}");
}

// ------------------------------------------------------------------ main

fn main() {
    if std::env::var("BENCH_DUMP_SQL").is_ok() {
        for q in queries() {
            println!("### {}\n{}\n", q.name, q.sql);
        }
        return;
    }
    if std::env::var("BENCH_EXPLAIN").is_ok() {
        // Diagnostic — production-faithful (only close/reopen, i.e. the on-close partial Optimize; never
        // FullOptimize). Shows that the statut-only / date queries won't elect the PARTIAL FactureModel
        // indexes (they full-scan), but DO elect an equivalent FULL [type,col] index. Leaves bench_data as-is.
        let dir = bench_dir();
        let mut db = Database::open(
            DB_NAME,
            Some(DatabaseConfiguration {
                directory: &dir,
                #[cfg(feature = "enterprise")]
                encryption_key: None,
            }),
        )
        .unwrap();
        let cands: Vec<(&str, String)> = vec![
            ("liste_factures[statut] (selective statut IN, no group)", "SELECT facture.id FROM _ facture WHERE facture.type='FactureModel' AND facture.isQuotation IS NOT TRUE AND facture.statut IN ('Securisee','Formatee','AFormater') AND 1=1".to_string()),
            ("count_by_statut_etat (same statut IN, GROUP BY) — control", "SELECT _.statut, COUNT(*) AS n FROM _ WHERE _.type='FactureModel' AND _.statut IN ('Securisee','Formatee','AFormater') AND (_.isQuotation IS FALSE OR _.isQuotation IS NULL OR _.isQuotation IS MISSING) GROUP BY _.statut".to_string()),
            ("overview_stats NARROW date (one month)", "SELECT SUM(_.totalFacture.totalDesMontantsFactures) FROM _ WHERE ARRAY_CONTAINS(['Securisee','Formatee'], _.statut) AND _.type='FactureModel' AND _.facture.identFacture.dateElaborationFacture >= '2025-06-01' AND _.facture.identFacture.dateElaborationFacture < '2025-07-01'".to_string()),
        ];
        let show = |db: &Database, phase: &str| {
            println!("--- {phase} ---");
            for (label, sql) in &cands {
                match Query::new(db, QueryLanguage::N1QL, sql) {
                    Ok(qq) => println!(
                        "   {:<52} idx={}",
                        label,
                        indexes_from_explain(&qq.explain().unwrap_or_default())
                    ),
                    Err(e) => println!("   {label}: ERR {e:?}"),
                }
            }
        };
        show(
            &db,
            "with the plan's PARTIAL indexes (current bench_data state)",
        );
        // Add a FULL [type,statut,withoutScor] index and see if the statut-only query elects it (baseline
        // used a full statut index). Then remove it — bench_data is left unchanged.
        {
            let coll = db.default_collection_or_error().unwrap();
            create_index(
                &coll,
                &v(
                    "zz_full_statut",
                    r#"[[".type"],[".statut"],[".withoutScor"]]"#,
                ),
            );
        }
        db = reopen(db, &dir);
        show(&db, "with a FULL [type,statut,withoutScor] index added");
        {
            let coll = db.default_collection_or_error().unwrap();
            coll.delete_index("zz_full_statut").unwrap();
        }
        let _ = reopen(db, &dir);
        return;
    }
    let dir = bench_dir();
    fs::create_dir_all(&dir).unwrap();
    let n_runs = runs();

    let mut db = Database::open(
        DB_NAME,
        Some(DatabaseConfiguration {
            directory: &dir,
            #[cfg(feature = "enterprise")]
            encryption_key: None,
        }),
    )
    .expect("open db");

    // 1. create-or-reuse the data set
    if db.get_document(META_ID).is_err() {
        eprintln!("[setup] no existing data set — generating (one-time)...");
        generate_data(&mut db);
    } else {
        eprintln!(
            "[setup] reusing existing data set: {} documents",
            db.count()
        );
    }

    // 2. drop all indexes, recreate production baseline, compact, then close+reopen (plan election)
    eprintln!("[setup] resetting to production baseline index set...");
    {
        let coll = db.default_collection_or_error().unwrap();
        drop_all_indexes(&coll);
        for idx in baseline_indexes() {
            create_index(&coll, &idx);
        }
    }
    compact(&mut db);
    db = reopen(db, &dir);

    // ordered labels: baseline=0, then T1..Tn — used to resolve rewrite-pair old/new per step
    let step_labels: Vec<String> = std::iter::once("baseline".to_string())
        .chain(steps().iter().map(|s| s.label.to_string()))
        .collect();

    // 3. baseline measurement
    let mut history: Vec<StepData> = vec![];
    {
        let (size, names) = {
            let coll = db.default_collection_or_error().unwrap();
            (dir_size(&db.path()), index_names(&coll))
        };
        eprintln!(
            "\n=== baseline (production indexes) — size {:.1} MB, {} indexes ===",
            mb(size),
            names.len()
        );
        let results = run_all(&db, &queries(), n_runs, 0, &step_labels);
        let base = StepData {
            label: "baseline".into(),
            size_bytes: size,
            indexes: names,
            results,
        };
        write_step_csv(&dir, &base);
        history.push(base);
    }

    // 4. walk the migration steps
    let qs = queries();
    for (i, step) in steps().into_iter().enumerate() {
        let cur_ord = i + 1; // baseline is ordinal 0
        eprintln!("\n=== step {} ===", step.label);
        {
            let coll = db.default_collection_or_error().unwrap();
            for name in &step.deletes {
                coll.delete_index(name).unwrap();
            }
            for idx in &step.creates {
                create_index(&coll, idx);
            }
        }
        compact(&mut db);
        // close+reopen: the auto-Optimize on close is what re-elects query plans in production
        db = reopen(db, &dir);
        let (size, names) = {
            let coll = db.default_collection_or_error().unwrap();
            (dir_size(&db.path()), index_names(&coll))
        };
        eprintln!("  size {:.1} MB, {} indexes", mb(size), names.len());
        let results = run_all(&db, &qs, n_runs, cur_ord, &step_labels);
        let cur = StepData {
            label: step.label.into(),
            size_bytes: size,
            indexes: names,
            results,
        };
        write_step_report(&dir, history.last().unwrap(), &cur, &step, n_runs);
        history.push(cur);
    }

    // 5. overall report
    write_overall_report(&dir, &history);
    eprintln!("\nReports written to {}/report_*.md", dir.display());
}
