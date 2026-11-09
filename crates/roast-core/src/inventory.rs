//! Green coffee inventory: lots and the stock movements that make up their
//! stock.
//!
//! A lot never stores its stock directly. Stock is the sum of its
//! [`StockMovement`]s — purchases add, roasts debit, adjustments correct — so
//! the history is always explainable and a roast that is deleted can take its
//! debit with it.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How the coffee cherries were processed.
///
/// Fine distinctions ("red honey", "anaerobic natural, 72 h") go into
/// [`GreenLot::process_detail`]; the enum keeps the broad family filterable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Process {
    /// Fruit removed before drying (fully washed).
    Washed,
    /// Dried in the whole cherry (dry process).
    Natural,
    /// Skin removed, some mucilage left on while drying (also "pulped natural").
    Honey,
    /// Fermented without oxygen before drying.
    Anaerobic,
    /// Hulled while still wet (Giling Basah, typical for Sumatra).
    WetHulled,
    /// Anything else; describe it in [`GreenLot::process_detail`].
    Other,
}

impl Process {
    /// Every process, in display order.
    pub const ALL: [Process; 6] = [
        Process::Washed,
        Process::Natural,
        Process::Honey,
        Process::Anaerobic,
        Process::WetHulled,
        Process::Other,
    ];

    /// Stable identifier, identical to the serialized form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Process::Washed => "washed",
            Process::Natural => "natural",
            Process::Honey => "honey",
            Process::Anaerobic => "anaerobic",
            Process::WetHulled => "wet_hulled",
            Process::Other => "other",
        }
    }

    /// Parses the serialized form produced by [`Process::as_str`].
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Process::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

impl std::fmt::Display for Process {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Growing altitude in metres above sea level: a single value or a range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Altitude {
    /// Lower bound, or the only value.
    pub min_m: u32,
    /// Upper bound if the producer states a range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_m: Option<u32>,
}

/// A price in a currency.
///
/// Prices are informational (what a kilo cost, roughly what a roast cost);
/// RoastMCP does no accounting, so a binary floating-point amount is adequate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Price {
    /// Amount in the currency's main unit, for example `12.5` for 12.50 €.
    pub amount: f64,
    /// ISO 4217 code, for example `EUR`.
    pub currency: String,
}

/// A lot of green coffee as bought from a supplier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GreenLot {
    pub id: Uuid,
    /// Short name used everywhere in the app, for example "Ethiopia Guji Hambela".
    pub name: String,
    /// Country of origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Producer, farm, washing station or cooperative.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<String>,
    /// Botanical variety, for example "Heirloom" or "Caturra, Castillo".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variety: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<Process>,
    /// Free-text detail of the process.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude: Option<Altitude>,
    /// Harvest as stated by the supplier, for example "2025/26" or "Oct–Jan 2025".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harvest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supplier: Option<String>,
    /// Calendar date of the purchase (a date, not an instant).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purchase_date: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_per_kg: Option<Price>,
    /// Moisture content, percent of the green weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moisture_pct: Option<f64>,
    /// Bulk (free-settled) density, grams per litre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density_g_per_l: Option<f64>,
    /// Screen size as stated, for example "15+" or "17/18".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Hidden from pickers and lists by default (used up or no longer relevant).
    #[serde(default)]
    pub archived: bool,
    pub created_at: DateTime<Utc>,
}

impl GreenLot {
    /// A new, empty lot with a fresh id.
    #[must_use]
    pub fn new(name: impl Into<String>, created_at: DateTime<Utc>) -> Self {
        GreenLot {
            id: Uuid::new_v4(),
            name: name.into(),
            origin_country: None,
            region: None,
            producer: None,
            variety: None,
            process: None,
            process_detail: None,
            altitude: None,
            harvest: None,
            supplier: None,
            purchase_date: None,
            price_per_kg: None,
            moisture_pct: None,
            density_g_per_l: None,
            screen_size: None,
            notes: None,
            archived: false,
            created_at,
        }
    }
}

/// Why the stock of a lot changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StockMovementKind {
    /// Coffee bought (or the opening stock of a lot).
    Purchase,
    /// Green coffee used by a roast.
    RoastDebit { roast_id: Uuid },
    /// Manual correction after weighing, spillage, a sample given away …
    Adjustment,
}

/// One change to the stock of a lot.
///
/// `grams` is signed: positive adds to the stock, negative removes from it. A
/// purchase is positive, a roast debit negative, an adjustment either.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StockMovement {
    pub id: Uuid,
    pub lot_id: Uuid,
    pub at: DateTime<Utc>,
    /// Signed change in grams.
    pub grams: f64,
    #[serde(flatten)]
    pub kind: StockMovementKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl StockMovement {
    /// A purchase of `grams` (taken as a positive amount).
    #[must_use]
    pub fn purchase(lot_id: Uuid, grams: f64, at: DateTime<Utc>) -> Self {
        StockMovement {
            id: Uuid::new_v4(),
            lot_id,
            at,
            grams: grams.abs(),
            kind: StockMovementKind::Purchase,
            note: None,
        }
    }

    /// The debit for a roast that used `green_grams` (stored as a negative amount).
    #[must_use]
    pub fn roast_debit(lot_id: Uuid, roast_id: Uuid, green_grams: f64, at: DateTime<Utc>) -> Self {
        StockMovement {
            id: Uuid::new_v4(),
            lot_id,
            at,
            grams: -green_grams.abs(),
            kind: StockMovementKind::RoastDebit { roast_id },
            note: None,
        }
    }

    /// A signed correction.
    #[must_use]
    pub fn adjustment(lot_id: Uuid, grams: f64, at: DateTime<Utc>, note: Option<String>) -> Self {
        StockMovement {
            id: Uuid::new_v4(),
            lot_id,
            at,
            grams,
            kind: StockMovementKind::Adjustment,
            note,
        }
    }
}

/// Stock in grams: the sum of the movements.
#[must_use]
pub fn stock_g<'a>(movements: impl IntoIterator<Item = &'a StockMovement>) -> f64 {
    movements.into_iter().map(|m| m.grams).sum()
}

/// Why an inventory change was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InventoryError {
    #[error("no green coffee lot {0}")]
    UnknownLot(Uuid),
    #[error("the amount must be a positive number of grams")]
    NotPositive,
    #[error("the amount must be a number of grams other than zero")]
    Zero,
    #[error("a lot needs a name")]
    NoName,
}

/// The whole green coffee inventory: lots and every stock movement.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    #[serde(default)]
    pub lots: Vec<GreenLot>,
    #[serde(default)]
    pub movements: Vec<StockMovement>,
}

impl Inventory {
    /// The lot with this id.
    #[must_use]
    pub fn lot(&self, id: Uuid) -> Option<&GreenLot> {
        self.lots.iter().find(|l| l.id == id)
    }

    /// The lot with this id, to change it.
    pub fn lot_mut(&mut self, id: Uuid) -> Option<&mut GreenLot> {
        self.lots.iter_mut().find(|l| l.id == id)
    }

    /// The lot whose name matches, ignoring case and surrounding spaces;
    /// lots in use come before archived ones.
    #[must_use]
    pub fn lot_by_name(&self, name: &str) -> Option<&GreenLot> {
        let name = name.trim().to_lowercase();
        let mut matches = self
            .lots
            .iter()
            .filter(|l| l.name.trim().to_lowercase() == name);
        let first = matches.next()?;
        Some(if first.archived {
            matches.find(|l| !l.archived).unwrap_or(first)
        } else {
            first
        })
    }

    /// The movements of one lot, oldest first as recorded.
    pub fn movements_of(&self, lot: Uuid) -> impl Iterator<Item = &StockMovement> {
        self.movements.iter().filter(move |m| m.lot_id == lot)
    }

    /// The stock of one lot in grams; negative when more was roasted than
    /// recorded as bought.
    #[must_use]
    pub fn stock_g(&self, lot: Uuid) -> f64 {
        stock_g(self.movements_of(lot))
    }

    /// Adds a lot, with an opening purchase of `purchase_g` grams if given.
    ///
    /// # Errors
    /// When the name is empty or the purchase is not positive.
    pub fn add_lot(
        &mut self,
        lot: GreenLot,
        purchase_g: Option<f64>,
        at: DateTime<Utc>,
    ) -> Result<Uuid, InventoryError> {
        if lot.name.trim().is_empty() {
            return Err(InventoryError::NoName);
        }
        if let Some(g) = purchase_g {
            positive(g)?;
        }
        let id = lot.id;
        self.lots.push(lot);
        if let Some(g) = purchase_g {
            self.movements.push(StockMovement::purchase(id, g, at));
        }
        Ok(id)
    }

    /// Records a purchase of `grams` (positive) for a lot.
    ///
    /// # Errors
    /// When the lot is unknown or the amount is not positive.
    pub fn add_purchase(
        &mut self,
        lot: Uuid,
        grams: f64,
        at: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<&StockMovement, InventoryError> {
        self.known(lot)?;
        positive(grams)?;
        let mut m = StockMovement::purchase(lot, grams, at);
        m.note = note;
        self.movements.push(m);
        Ok(&self.movements[self.movements.len() - 1])
    }

    /// Records a signed correction (weighed less than expected, a sample …).
    ///
    /// # Errors
    /// When the lot is unknown or the amount is zero or not finite.
    pub fn adjust(
        &mut self,
        lot: Uuid,
        grams: f64,
        at: DateTime<Utc>,
        note: Option<String>,
    ) -> Result<&StockMovement, InventoryError> {
        self.known(lot)?;
        if !grams.is_finite() || grams == 0.0 {
            return Err(InventoryError::Zero);
        }
        self.movements
            .push(StockMovement::adjustment(lot, grams, at, note));
        Ok(&self.movements[self.movements.len() - 1])
    }

    /// Makes `roast_id` debit exactly `grams` from `lot`: replaces an earlier
    /// debit of that roast, or removes it when there is no lot or no
    /// positive weight (a roast without a weight debits nothing).
    ///
    /// # Errors
    /// When the lot is unknown.
    pub fn set_roast_debit(
        &mut self,
        roast_id: Uuid,
        lot: Option<Uuid>,
        grams: Option<f64>,
        at: DateTime<Utc>,
    ) -> Result<(), InventoryError> {
        if let Some(l) = lot {
            self.known(l)?;
        }
        self.movements.retain(
            |m| !matches!(m.kind, StockMovementKind::RoastDebit { roast_id: r } if r == roast_id),
        );
        if let (Some(l), Some(g)) = (lot, grams.filter(|g| g.is_finite() && *g > 0.0)) {
            self.movements
                .push(StockMovement::roast_debit(l, roast_id, g, at));
        }
        Ok(())
    }

    fn known(&self, lot: Uuid) -> Result<(), InventoryError> {
        self.lot(lot)
            .map(|_| ())
            .ok_or(InventoryError::UnknownLot(lot))
    }
}

fn positive(grams: f64) -> Result<(), InventoryError> {
    if grams.is_finite() && grams > 0.0 {
        Ok(())
    } else {
        Err(InventoryError::NotPositive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_is_the_sum_of_movements() {
        let lot = Uuid::new_v4();
        let at = DateTime::<Utc>::UNIX_EPOCH;
        let movements = [
            StockMovement::purchase(lot, 1000.0, at),
            StockMovement::roast_debit(lot, Uuid::new_v4(), 200.0, at),
            StockMovement::adjustment(lot, -15.0, at, Some("spilled".into())),
        ];
        assert!((stock_g(&movements) - 785.0).abs() < 1e-9);
    }

    #[test]
    fn movement_serializes_flat() {
        let roast_id = Uuid::nil();
        let m = StockMovement::roast_debit(Uuid::nil(), roast_id, 150.0, DateTime::UNIX_EPOCH);
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["kind"], "roast_debit");
        assert_eq!(json["roast_id"], roast_id.to_string());
        assert_eq!(json["grams"], -150.0);
        let back: StockMovement = serde_json::from_value(json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn purchases_and_roasts_make_the_stock() {
        let at = DateTime::<Utc>::UNIX_EPOCH;
        let mut inv = Inventory::default();
        let lot = inv
            .add_lot(
                GreenLot::new("GL-BR-001 Brasilien Amarelo", at),
                Some(5000.0),
                at,
            )
            .unwrap();
        inv.add_purchase(lot, 1000.0, at, None).unwrap();
        let roast = Uuid::new_v4();
        inv.set_roast_debit(roast, Some(lot), Some(200.0), at)
            .unwrap();
        assert!((inv.stock_g(lot) - 5800.0).abs() < 1e-9);

        // Correcting the green weight replaces the debit, never adds a second.
        inv.set_roast_debit(roast, Some(lot), Some(227.0), at)
            .unwrap();
        assert!((inv.stock_g(lot) - 5773.0).abs() < 1e-9);
        assert_eq!(
            inv.movements_of(lot)
                .filter(|m| matches!(m.kind, StockMovementKind::RoastDebit { .. }))
                .count(),
            1
        );

        // A roast without a weight, or taken off the lot, debits nothing.
        inv.set_roast_debit(roast, Some(lot), None, at).unwrap();
        assert!((inv.stock_g(lot) - 6000.0).abs() < 1e-9);

        inv.adjust(lot, -15.0, at, Some("spilled".into())).unwrap();
        assert!((inv.stock_g(lot) - 5985.0).abs() < 1e-9);
    }

    #[test]
    fn bad_amounts_and_unknown_lots_are_refused() {
        let at = DateTime::<Utc>::UNIX_EPOCH;
        let mut inv = Inventory::default();
        assert_eq!(
            inv.add_lot(GreenLot::new(" ", at), None, at),
            Err(InventoryError::NoName)
        );
        let lot = inv.add_lot(GreenLot::new("A", at), None, at).unwrap();
        assert_eq!(
            inv.add_purchase(lot, -1.0, at, None).err(),
            Some(InventoryError::NotPositive)
        );
        assert_eq!(
            inv.adjust(lot, 0.0, at, None).err(),
            Some(InventoryError::Zero)
        );
        let other = Uuid::new_v4();
        assert_eq!(
            inv.add_purchase(other, 1.0, at, None).err(),
            Some(InventoryError::UnknownLot(other))
        );
        assert_eq!(
            inv.set_roast_debit(Uuid::new_v4(), Some(other), Some(1.0), at),
            Err(InventoryError::UnknownLot(other))
        );
        assert!(inv.movements.is_empty());
    }

    #[test]
    fn lots_are_found_by_name() {
        let at = DateTime::<Utc>::UNIX_EPOCH;
        let mut inv = Inventory::default();
        let mut old = GreenLot::new("Amarelo", at);
        old.archived = true;
        inv.add_lot(old, None, at).unwrap();
        let new = inv.add_lot(GreenLot::new("Amarelo", at), None, at).unwrap();
        assert_eq!(inv.lot_by_name("  amarelo ").map(|l| l.id), Some(new));
        assert!(inv.lot_by_name("Bourbon").is_none());
    }

    #[test]
    fn process_round_trips_through_its_identifier() {
        for p in Process::ALL {
            assert_eq!(Process::parse(p.as_str()), Some(p));
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(json, format!("\"{}\"", p.as_str()));
        }
    }
}
