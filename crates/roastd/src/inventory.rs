//! The green coffee inventory as the daemon keeps it (ADR 0014): lots and
//! stock movements in `inventory.json`, written atomically with the previous
//! version kept as `inventory.json.bak`.
//!
//! A file that cannot be read is never overwritten: the store then refuses
//! every change until the file is fixed or restored from the backup.

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use roast_core::{GreenLot, Inventory, InventoryError, Price, Process, Roast, StockMovement};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::engine::Fail;
use crate::files;

/// A lot with its current stock.
#[derive(Debug, Clone, Serialize)]
pub struct LotView {
    #[serde(flatten)]
    pub lot: GreenLot,
    /// Grams in stock; negative when more was roasted than recorded as bought.
    pub stock_g: f64,
}

/// A new lot, optionally with its opening purchase.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewLot {
    pub name: String,
    #[serde(default)]
    pub origin_country: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub producer: Option<String>,
    #[serde(default)]
    pub variety: Option<String>,
    #[serde(default)]
    pub process: Option<Process>,
    #[serde(default)]
    pub process_detail: Option<String>,
    #[serde(default)]
    pub supplier: Option<String>,
    #[serde(default)]
    pub price_per_kg: Option<Price>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Grams bought now; recorded as the opening purchase.
    #[serde(default)]
    pub purchase_g: Option<f64>,
    /// Day of that purchase; today if left out.
    #[serde(default)]
    pub purchase_date: Option<NaiveDate>,
}

/// Changes to a lot; fields left out stay as they are.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LotPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub origin_country: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub producer: Option<String>,
    #[serde(default)]
    pub variety: Option<String>,
    #[serde(default)]
    pub process: Option<Process>,
    #[serde(default)]
    pub process_detail: Option<String>,
    #[serde(default)]
    pub supplier: Option<String>,
    #[serde(default)]
    pub price_per_kg: Option<Price>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub archived: Option<bool>,
}

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

/// The inventory and the file it lives in.
pub struct InventoryStore {
    path: PathBuf,
    data: Inventory,
    /// Why the file could not be read; changes are refused while set.
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
    /// Loads `path`; a missing file is an empty inventory.
    #[must_use]
    pub fn load(path: PathBuf) -> Self {
        match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Inventory>(&bytes) {
                Ok(data) => InventoryStore {
                    path,
                    data,
                    broken: None,
                },
                Err(e) => {
                    let msg = format!(
                        "{} could not be read ({e}); fix it or restore {}.bak",
                        path.display(),
                        path.display()
                    );
                    tracing::error!("{msg}");
                    InventoryStore {
                        path,
                        data: Inventory::default(),
                        broken: Some(msg),
                    }
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => InventoryStore {
                path,
                data: Inventory::default(),
                broken: None,
            },
            Err(e) => {
                let msg = format!("{} could not be read ({e})", path.display());
                tracing::error!("{msg}");
                InventoryStore {
                    path,
                    data: Inventory::default(),
                    broken: Some(msg),
                }
            }
        }
    }

    /// Why the inventory file could not be read, if it could not.
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
        if let Err(e) = save(&self.path, &self.data) {
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
        lot.origin_country = req.origin_country;
        lot.region = req.region;
        lot.producer = req.producer;
        lot.variety = req.variety;
        lot.process = req.process;
        lot.process_detail = req.process_detail;
        lot.supplier = req.supplier;
        lot.price_per_kg = req.price_per_kg;
        lot.notes = req.notes;
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
            let set = |slot: &mut Option<String>, v: Option<String>| {
                if let Some(v) = v {
                    *slot = Some(v).filter(|s| !s.trim().is_empty());
                }
            };
            set(&mut lot.origin_country, p.origin_country);
            set(&mut lot.region, p.region);
            set(&mut lot.producer, p.producer);
            set(&mut lot.variety, p.variety);
            set(&mut lot.process_detail, p.process_detail);
            set(&mut lot.supplier, p.supplier);
            set(&mut lot.notes, p.notes);
            if p.process.is_some() {
                lot.process = p.process;
            }
            if p.price_per_kg.is_some() {
                lot.price_per_kg = p.price_per_kg;
            }
            if let Some(a) = p.archived {
                lot.archived = a;
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
        let lot = roast.green_lot_id.filter(|_| roast.charge_t().is_some());
        let already = self.data.movements.iter().any(|m| {
            matches!(m.kind, roast_core::StockMovementKind::RoastDebit { roast_id } if roast_id == roast.id)
        });
        if lot.is_none() && !already {
            return Ok(());
        }
        let grams = roast.batch.green_weight_g;
        self.change(|inv| inv.set_roast_debit(roast.id, lot, grams, roast.roasted_at))
    }
}

fn save(path: &Path, inv: &Inventory) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        std::fs::copy(path, PathBuf::from(bak))?;
    }
    let json = serde_json::to_vec_pretty(inv).map_err(std::io::Error::other)?;
    files::write_atomic(path, &json)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use roast_core::{BatchInfo, EventKind, EventOrigin, RoastEvent, RoastOrigin};

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
        let mut store = InventoryStore::load(path.clone());
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

        let again = InventoryStore::load(path.clone());
        assert!((again.lot(lot.lot.id).unwrap().stock_g - 5773.0).abs() < 1e-9);
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        assert!(PathBuf::from(bak).exists());
    }

    #[test]
    fn a_broken_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("inventory.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let mut store = InventoryStore::load(path.clone());
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
        let mut store = InventoryStore::load(dir.path().join("inventory.json"));
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
