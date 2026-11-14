//! The green coffee inventory as the daemon keeps it (ADR 0014): lots and
//! stock movements in the database (ADR 0003), every change all or nothing.
//!
//! An inventory that cannot be read is never overwritten: the store then
//! refuses every change until that is fixed.

use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, Utc};
use roast_core::{GreenLot, Inventory, InventoryError, LotDetails, Roast, StockMovement};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use roast_store::Store;

use crate::engine::Fail;

/// A lot with its current stock.
#[derive(Debug, Clone, Serialize)]
pub struct LotView {
    #[serde(flatten)]
    pub lot: GreenLot,
    /// Grams in stock; negative when more was roasted than recorded as bought.
    pub stock_g: f64,
}

/// A new lot, optionally with its opening purchase. The offer-sheet details
/// sit next to the name, as in the stored lot.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NewLot {
    pub name: String,
    #[serde(flatten)]
    pub details: LotDetails,
    /// Grams bought now; recorded as the opening purchase.
    #[serde(default)]
    pub purchase_g: Option<f64>,
    /// Day of that purchase; today if left out.
    #[serde(default)]
    pub purchase_date: Option<NaiveDate>,
    /// Short code for its roasts' batch numbers.
    #[serde(default)]
    pub batch_code: Option<String>,
}

/// Changes to a lot. `details`, when given, replaces all offer-sheet details
/// (the edit form sends them whole); fields left out stay as they are.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LotPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub details: Option<LotDetails>,
    #[serde(default)]
    pub archived: Option<bool>,
    /// A new batch code; an empty one removes it.
    #[serde(default)]
    pub batch_code: Option<String>,
    /// The note for the next roast; an empty one removes it.
    #[serde(default)]
    pub next_roast: Option<String>,
}

/// The longest note for the next roast, in characters.
const NOTE_MAX: usize = 500;

/// A purchase of more of a lot.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PurchaseReq {
    pub grams: f64,
    /// Day of the purchase; today if left out.
    #[serde(default)]
    pub date: Option<NaiveDate>,
    #[serde(default)]
    pub note: Option<String>,
}

/// A signed correction of a lot's stock.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustReq {
    pub grams: f64,
    #[serde(default)]
    pub note: Option<String>,
}

/// The inventory and the database it lives in.
pub struct InventoryStore {
    store: Arc<Store>,
    data: Inventory,
    /// Why the inventory could not be read; changes are refused while set.
    broken: Option<String>,
}

fn day_to_instant(day: Option<NaiveDate>) -> DateTime<Utc> {
    day.and_then(|d| d.and_hms_opt(12, 0, 0))
        .map_or_else(Utc::now, |dt| dt.and_utc())
}

fn refused(e: &InventoryError) -> Fail {
    match e {
        InventoryError::UnknownLot(_) => Fail::not_found(e.to_string()),
        _ => Fail::bad(e.to_string()),
    }
}

impl InventoryStore {
    /// The inventory in `store`. An `inventory.json` of the time before the
    /// database (`legacy`) is taken over first.
    #[must_use]
    pub fn open(store: Arc<Store>, legacy: &Path) -> Self {
        let loaded = store
            .inventory()
            .map_err(|e| e.to_string())
            .and_then(|inv| {
                if !inv.lots.is_empty() {
                    return Ok(inv);
                }
                match crate::legacy::read::<Inventory>(legacy)? {
                    Some(old) => {
                        store.save_inventory(&old).map_err(|e| {
                            format!("{} could not be taken over: {e}", legacy.display())
                        })?;
                        crate::legacy::retire(legacy);
                        Ok(old)
                    }
                    None => Ok(inv),
                }
            });
        match loaded {
            Ok(data) => InventoryStore {
                store,
                data,
                broken: None,
            },
            Err(msg) => {
                tracing::error!("{msg}");
                InventoryStore {
                    store,
                    data: Inventory::default(),
                    broken: Some(msg),
                }
            }
        }
    }

    /// Why the inventory could not be read, if it could not.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.broken.as_deref()
    }

    /// The whole inventory.
    #[must_use]
    pub fn data(&self) -> &Inventory {
        &self.data
    }

    fn view(&self, lot: &GreenLot) -> LotView {
        LotView {
            lot: lot.clone(),
            stock_g: self.data.stock_g(lot.id),
        }
    }

    /// Lots with their stock, newest first; archived ones only if asked.
    #[must_use]
    pub fn lots(&self, include_archived: bool) -> Vec<LotView> {
        let mut out: Vec<LotView> = self
            .data
            .lots
            .iter()
            .filter(|l| include_archived || !l.archived)
            .map(|l| self.view(l))
            .collect();
        out.sort_by_key(|v| std::cmp::Reverse(v.lot.created_at));
        out
    }

    /// One lot with its stock.
    ///
    /// # Errors
    /// When there is no such lot.
    pub fn lot(&self, id: Uuid) -> Result<LotView, Fail> {
        self.data
            .lot(id)
            .map(|l| self.view(l))
            .ok_or_else(|| Fail::not_found(format!("no green coffee lot {id}")))
    }

    /// The movements of one lot, newest first.
    ///
    /// # Errors
    /// When there is no such lot.
    pub fn movements(&self, id: Uuid) -> Result<Vec<StockMovement>, Fail> {
        self.lot(id)?;
        let mut out: Vec<StockMovement> = self.data.movements_of(id).cloned().collect();
        out.sort_by_key(|m| std::cmp::Reverse(m.at));
        Ok(out)
    }

    /// Applies `f` and saves; on any failure the inventory stays as it was.
    fn change<T>(
        &mut self,
        f: impl FnOnce(&mut Inventory) -> Result<T, InventoryError>,
    ) -> Result<T, Fail> {
        if let Some(msg) = &self.broken {
            return Err(Fail::conflict(msg.clone(), None));
        }
        let before = self.data.clone();
        let out = f(&mut self.data).map_err(|e| refused(&e))?;
        if let Err(e) = self.store.save_inventory(&self.data) {
            self.data = before;
            return Err(Fail::internal(format!(
                "the inventory could not be saved: {e}"
            )));
        }
        Ok(out)
    }

    /// Adds a lot, with its opening purchase.
    ///
    /// # Errors
    /// A missing name, a non-positive purchase, or a failed save.
    pub fn add_lot(&mut self, req: NewLot) -> Result<LotView, Fail> {
        let at = day_to_instant(req.purchase_date);
        let mut lot = GreenLot::new(req.name.trim(), Utc::now());
        lot.details = clean(req.details);
        lot.batch_code = req.batch_code.as_deref().and_then(batch_code);
        lot.purchase_date = req
            .purchase_g
            .map(|_| req.purchase_date.unwrap_or_else(|| Utc::now().date_naive()));
        let id = self.change(|inv| inv.add_lot(lot, req.purchase_g, at))?;
        self.lot(id)
    }

    /// Changes a lot's details.
    ///
    /// # Errors
    /// An unknown lot, an empty name, or a failed save.
    pub fn update_lot(&mut self, id: Uuid, p: LotPatch) -> Result<LotView, Fail> {
        self.change(|inv| {
            let lot = inv.lot_mut(id).ok_or(InventoryError::UnknownLot(id))?;
            if let Some(n) = p.name {
                if n.trim().is_empty() {
                    return Err(InventoryError::NoName);
                }
                lot.name = n.trim().to_owned();
            }
            if let Some(d) = p.details {
                lot.details = clean(d);
            }
            if let Some(a) = p.archived {
                lot.archived = a;
            }
            if let Some(c) = p.batch_code {
                lot.batch_code = batch_code(&c);
            }
            if let Some(n) = p.next_roast {
                let n = n.trim();
                lot.next_roast = (!n.is_empty()).then(|| n.chars().take(NOTE_MAX).collect());
            }
            Ok(())
        })?;
        self.lot(id)
    }

    /// Records a purchase of more of a lot.
    ///
    /// # Errors
    /// An unknown lot, a non-positive amount, or a failed save.
    pub fn add_purchase(&mut self, id: Uuid, req: PurchaseReq) -> Result<LotView, Fail> {
        let at = day_to_instant(req.date);
        self.change(|inv| inv.add_purchase(id, req.grams, at, req.note).map(|_| ()))?;
        self.lot(id)
    }

    /// Records a correction of a lot's stock.
    ///
    /// # Errors
    /// An unknown lot, a zero amount, or a failed save.
    pub fn adjust(&mut self, id: Uuid, req: AdjustReq) -> Result<LotView, Fail> {
        self.change(|inv| inv.adjust(id, req.grams, Utc::now(), req.note).map(|_| ()))?;
        self.lot(id)
    }

    /// Makes a saved roast debit its green weight from its lot, once: only a
    /// roast that reached CHARGE debits, and a changed weight replaces the
    /// earlier debit.
    ///
    /// # Errors
    /// An unknown lot or a failed save.
    pub fn sync_roast_debit(&mut self, roast: &Roast) -> Result<(), Fail> {
        // A roast in the simulator never touches the real stock.
        let simulated = matches!(roast.origin, roast_core::RoastOrigin::Simulated);
        let lot = roast
            .green_lot_id
            .filter(|_| roast.charge_t().is_some() && !simulated);
        let already = self.data.movements.iter().any(|m| {
            matches!(m.kind, roast_core::StockMovementKind::RoastDebit { roast_id } if roast_id == roast.id)
        });
        if lot.is_none() && !already {
            return Ok(());
        }
        let grams = roast.batch.green_weight_g;
        self.change(|inv| inv.set_roast_debit(roast.id, lot, grams, roast.roasted_at))
    }

    /// Takes back the debit of a deleted roast: its green coffee is in stock
    /// again. Returns the grams credited back.
    ///
    /// # Errors
    /// A failed save.
    pub fn remove_roast_debit(&mut self, roast_id: Uuid) -> Result<f64, Fail> {
        let grams: f64 = self
            .data
            .movements
            .iter()
            .filter(|m| matches!(m.kind, roast_core::StockMovementKind::RoastDebit { roast_id: r } if r == roast_id))
            .map(|m| -m.grams)
            .sum();
        if grams == 0.0 {
            return Ok(0.0);
        }
        self.change(|inv| inv.set_roast_debit(roast_id, None, None, Utc::now()))?;
        Ok(grams)
    }

    /// Gives a lot without a batch code the code of a batch number used for
    /// it (`BRYB-002` → `BRYB`), so the next roast continues the series.
    pub fn adopt_batch_code(&mut self, lot: Uuid, batch_no: &str) {
        let Some(code) = batch_no.rsplit_once('-').and_then(|(c, _)| batch_code(c)) else {
            return;
        };
        if self.data.lot(lot).is_some_and(|l| l.batch_code.is_none()) {
            let _ = self.change(|inv| {
                if let Some(l) = inv.lot_mut(lot) {
                    l.batch_code = Some(code);
                }
                Ok(())
            });
        }
    }
}

/// A batch code as stored: letters and digits only, upper case, at most 8;
/// `None` when nothing is left.
fn batch_code(raw: &str) -> Option<String> {
    let code: String = raw
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .take(8)
        .collect();
    (!code.is_empty()).then_some(code)
}

/// Trims text fields and drops empty ones, so a cleared field is gone rather
/// than an empty string.
fn clean(mut d: LotDetails) -> LotDetails {
    for slot in [
        &mut d.origin_country,
        &mut d.region,
        &mut d.producer,
        &mut d.process_detail,
        &mut d.harvest,
        &mut d.grade,
        &mut d.screen_size,
        &mut d.flavor_notes,
        &mut d.supplier,
        &mut d.supplier_lot,
        &mut d.storage_location,
        &mut d.notes,
    ] {
        *slot = slot
            .take()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
    }
    d.varieties = d
        .varieties
        .into_iter()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .collect();
    if d.process != Some(roast_core::Process::Honey) {
        d.honey = None;
    }
    d
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use roast_core::{BatchInfo, EventKind, EventOrigin, RoastEvent, RoastOrigin};

    fn db(dir: &Path) -> Arc<Store> {
        Arc::new(Store::open(&dir.join("roastmcp.sqlite")).unwrap())
    }

    fn roast(lot: Uuid, grams: f64, charged: bool) -> Roast {
        Roast {
            id: Uuid::new_v4(),
            title: "t".into(),
            roasted_at: Utc::now(),
            machine: None,
            sampling_interval_s: 1.5,
            batch: BatchInfo {
                green_weight_g: Some(grams),
                roasted_weight_g: None,
                beans: None,
                batch_no: None,
            },
            ambient: None,
            roast_degree: None,
            green_lot_id: Some(lot),
            plan: None,
            profile_version_id: None,
            experiment_id: None,
            origin: RoastOrigin::Recorded,
            samples: Vec::new(),
            events: if charged {
                vec![RoastEvent {
                    t: 1.0,
                    kind: EventKind::Charge,
                    origin: EventOrigin::Human,
                    bt: None,
                }]
            } else {
                Vec::new()
            },
            control_log: Vec::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn lots_purchases_and_debits_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.json");
        let mut store = InventoryStore::open(db(dir.path()), &path);
        let lot = store
            .add_lot(NewLot {
                name: "GL-BR-001 Brasilien Amarelo".into(),
                purchase_g: Some(5000.0),
                ..NewLot::default()
            })
            .unwrap();
        store
            .add_purchase(
                lot.lot.id,
                PurchaseReq {
                    grams: 1000.0,
                    date: None,
                    note: None,
                },
            )
            .unwrap();
        let mut r = roast(lot.lot.id, 200.0, true);
        store.sync_roast_debit(&r).unwrap();
        r.batch.green_weight_g = Some(227.0);
        store.sync_roast_debit(&r).unwrap();
        // A recording stopped before CHARGE debits nothing.
        store
            .sync_roast_debit(&roast(lot.lot.id, 200.0, false))
            .unwrap();

        let again = InventoryStore::open(db(dir.path()), &path);
        assert!((again.lot(lot.lot.id).unwrap().stock_g - 5773.0).abs() < 1e-9);
    }

    #[test]
    fn the_note_for_the_next_roast_is_kept_trimmed_and_removed_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.json");
        let mut store = InventoryStore::open(db(dir.path()), &path);
        let lot = store
            .add_lot(NewLot {
                name: "Yellow Bourbon".into(),
                ..NewLot::default()
            })
            .unwrap();
        let note = |n: &str| LotPatch {
            next_roast: Some(n.into()),
            ..LotPatch::default()
        };
        let v = store
            .update_lot(lot.lot.id, note("  charge 10 °C hotter "))
            .unwrap();
        assert_eq!(v.lot.next_roast.as_deref(), Some("charge 10 °C hotter"));
        let again = InventoryStore::open(db(dir.path()), &path);
        assert_eq!(
            again.lot(lot.lot.id).unwrap().lot.next_roast.as_deref(),
            Some("charge 10 °C hotter")
        );
        // Other changes leave it alone; an empty note removes it.
        let v = store
            .update_lot(
                lot.lot.id,
                LotPatch {
                    archived: Some(false),
                    ..LotPatch::default()
                },
            )
            .unwrap();
        assert!(v.lot.next_roast.is_some());
        let v = store.update_lot(lot.lot.id, note(" ")).unwrap();
        assert!(v.lot.next_roast.is_none());
    }

    #[test]
    fn an_inventory_file_of_an_earlier_version_is_taken_over_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.json");
        let lot = GreenLot::new("Yellow Bourbon", Utc::now());
        let old = Inventory {
            movements: vec![StockMovement::purchase(lot.id, 5000.0, Utc::now())],
            lots: vec![lot.clone()],
        };
        std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();

        let store = InventoryStore::open(db(dir.path()), &path);
        assert!(store.problem().is_none());
        assert!((store.lot(lot.id).unwrap().stock_g - 5000.0).abs() < 1e-9);
        assert!(!path.exists(), "the file is no longer live");
        assert!(dir.path().join("inventory.json.migrated").exists());

        let again = InventoryStore::open(db(dir.path()), &path);
        assert_eq!(again.data(), &old);
    }

    #[test]
    fn details_are_stored_cleaned_and_replaced_whole() {
        use roast_core::{Fermentation, HoneyGrade, Process};
        let dir = tempfile::tempdir().unwrap();
        let mut store = InventoryStore::open(db(dir.path()), &dir.path().join("inventory.json"));
        let req: NewLot = serde_json::from_value(serde_json::json!({
            "name": " 88graines Castillo Washed ",
            "purchase_g": 2000,
            "origin_country": "Kolumbien",
            "region": "  ",
            "altitude": {"min_m": 1950, "max_m": 2080},
            "varieties": ["Castillo", " "],
            "process": "washed",
            "honey": "red",
            "fermentation": ["anaerobic"],
            "supplier": "88graines",
            "certifications": ["organic"]
        }))
        .unwrap();
        let lot = store.add_lot(req).unwrap();
        assert_eq!(lot.lot.name, "88graines Castillo Washed");
        let d = &lot.lot.details;
        assert_eq!(d.region, None, "blank text is dropped");
        assert_eq!(d.varieties, ["Castillo"]);
        assert_eq!(
            d.honey, None,
            "a honey grade only goes with a honey process"
        );
        assert_eq!(d.fermentation, [Fermentation::Anaerobic]);
        assert!((lot.stock_g - 2000.0).abs() < 1e-9);

        let mut details = lot.lot.details.clone();
        details.process = Some(Process::Honey);
        details.honey = Some(HoneyGrade::Red);
        details.supplier = None;
        let lot = store
            .update_lot(
                lot.lot.id,
                LotPatch {
                    details: Some(details),
                    ..LotPatch::default()
                },
            )
            .unwrap();
        assert_eq!(lot.lot.details.honey, Some(HoneyGrade::Red));
        assert_eq!(lot.lot.details.supplier, None);
    }

    #[test]
    fn a_broken_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let mut store = InventoryStore::open(db(dir.path()), &path);
        assert!(store.problem().is_some());
        let err = store
            .add_lot(NewLot {
                name: "A".into(),
                ..NewLot::default()
            })
            .unwrap_err();
        assert_eq!(err.status, 409);
        assert_eq!(std::fs::read(&path).unwrap(), b"{ not json");
    }

    #[test]
    fn refusals_change_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = InventoryStore::open(db(dir.path()), &dir.path().join("inventory.json"));
        let lot = store
            .add_lot(NewLot {
                name: "A".into(),
                ..NewLot::default()
            })
            .unwrap();
        let err = store
            .add_purchase(
                lot.lot.id,
                PurchaseReq {
                    grams: -5.0,
                    date: None,
                    note: None,
                },
            )
            .unwrap_err();
        assert_eq!(err.status, 400);
        assert_eq!(store.lot(Uuid::new_v4()).unwrap_err().status, 404);
        assert!(store.data().movements.is_empty());
    }
}
