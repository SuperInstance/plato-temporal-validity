//! plato-temporal-validity — Temporal Validity for Tiles
//!
//! Tiles have a shelf life. This crate provides time-aware scoring
//! that decays tiles based on age, recency of evidence, and epoch.
//!
//! ## Why
//! A tile from 6 months ago about "Python 3.11 features" is less
//! useful than one from yesterday. Without temporal decay, stale
//! tiles get equal ranking as fresh ones.
//!
//! ## API
//! ```rust
//! let tv = TemporalValidity::new(chrono_now());
//! tv.is_valid();          // still within validity window?
//! tv.decay_factor();      // 0.0-1.0 multiplier for scoring
//! tv.refresh();           // extend validity window
//! ```

/// Temporal validity metadata for a tile.
#[derive(Debug, Clone)]
pub struct TemporalValidity {
    /// When this tile was created (epoch seconds).
    pub created_at: u64,
    /// When this tile was last refreshed (epoch seconds).
    pub refreshed_at: u64,
    /// Last evidence tick (epoch seconds).
    pub last_evidence_at: u64,
    /// Validity window in seconds (tile is fully valid for this long).
    pub validity_window: u64,
    /// Grace period in seconds (tile decays but isn't expired).
    pub grace_period: u64,
    /// Current epoch seconds.
    pub now: u64,
}

impl TemporalValidity {
    pub fn new(now: u64) -> Self {
        Self {
            created_at: now,
            refreshed_at: now,
            last_evidence_at: now,
            validity_window: 7 * 24 * 3600,  // 1 week
            grace_period: 30 * 24 * 3600,     // 1 month
            now,
        }
    }

    pub fn with_window(mut self, validity: u64, grace: u64) -> Self {
        self.validity_window = validity;
        self.grace_period = grace;
        self
    }

    /// Age in seconds since last refresh (or creation if never refreshed).
    pub fn age(&self) -> u64 {
        self.now.saturating_sub(self.refreshed_at)
    }

    /// Age in seconds since last evidence.
    pub fn evidence_age(&self) -> u64 {
        self.now.saturating_sub(self.last_evidence_at)
    }

    /// Is this tile fully valid (within validity window)?
    pub fn is_valid(&self) -> bool {
        self.age() <= self.validity_window
    }

    /// Is this tile in the grace period (decaying but not expired)?
    pub fn in_grace(&self) -> bool {
        let age = self.age();
        age > self.validity_window && age <= self.validity_window + self.grace_period
    }

    /// Is this tile expired (beyond grace period)?
    pub fn is_expired(&self) -> bool {
        self.age() > self.validity_window + self.grace_period
    }

    /// Decay factor: 1.0 = fresh, 0.0 = expired.
    /// Linear decay during grace period.
    pub fn decay_factor(&self) -> f64 {
        if self.is_valid() { return 1.0; }
        if self.is_expired() { return 0.0; }
        let grace_elapsed = self.age() - self.validity_window;
        let ratio = grace_elapsed as f64 / self.grace_period as f64;
        1.0 - ratio
    }

    /// Evidence recency bonus: 1.0 = evidence just now, 0.0 = no evidence ever.
    /// Decays with half-life of validity_window.
    pub fn evidence_bonus(&self) -> f64 {
        if self.last_evidence_at == 0 { return 0.0; }
        let evidence_age = self.evidence_age() as f64;
        let half_life = self.validity_window as f64;
        // Exponential decay: bonus = e^(-age * ln(2) / half_life)
        let lambda = 0.693 / half_life; // ln(2)
        (-lambda * evidence_age).exp()
    }

    /// Combined score: decay_factor * (1.0 + evidence_bonus * 0.5).
    /// Evidence can boost a decaying tile up to 1.5x.
    pub fn temporal_score(&self) -> f64 {
        self.decay_factor() * (1.0 + self.evidence_bonus() * 0.5)
    }

    /// Refresh: extend validity window to current time.
    pub fn refresh(&mut self, now: u64) {
        self.refreshed_at = now;
        self.last_evidence_at = now;
    }

    /// Record new evidence (updates last_evidence_at).
    pub fn record_evidence(&mut self, now: u64) {
        self.last_evidence_at = now;
    }

    /// Advance time by given seconds.
    pub fn advance(&mut self, seconds: u64) {
        self.now = self.now.saturating_add(seconds);
    }

    /// Validity state description.
    pub fn state(&self) -> ValidityState {
        if self.is_valid() { ValidityState::Valid }
        else if self.in_grace() { ValidityState::Grace }
        else { ValidityState::Expired }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidityState {
    Valid,
    Grace,
    Expired,
}

/// Tile with temporal validity.
#[derive(Debug, Clone)]
pub struct TemporalTile {
    pub id: String,
    pub content: String,
    pub validity: TemporalValidity,
    pub base_score: f64,
}

impl TemporalTile {
    pub fn new(id: &str, content: &str, now: u64) -> Self {
        Self {
            id: id.to_string(),
            content: content.to_string(),
            validity: TemporalValidity::new(now),
            base_score: 1.0,
        }
    }

    /// Score adjusted by temporal decay.
    pub fn scored(&self) -> f64 {
        self.base_score * self.validity.temporal_score()
    }
}

/// Temporal tile store with automatic eviction.
pub struct TemporalStore {
    tiles: Vec<TemporalTile>,
}

impl TemporalStore {
    pub fn new() -> Self { Self { tiles: Vec::new() } }

    pub fn add(&mut self, tile: TemporalTile) {
        self.tiles.push(tile);
    }

    pub fn get(&self, id: &str) -> Option<&TemporalTile> {
        self.tiles.iter().find(|t| t.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut TemporalTile> {
        self.tiles.iter_mut().find(|t| t.id == id)
    }

    /// Top-k tiles by temporal score.
    pub fn top_k(&self, k: usize) -> Vec<&TemporalTile> {
        let mut sorted: Vec<_> = self.tiles.iter().collect();
        sorted.sort_by(|a, b| b.scored().partial_cmp(&a.scored()).unwrap());
        sorted.truncate(k);
        sorted
    }

    /// Evict all expired tiles. Returns count.
    pub fn evict_expired(&mut self) -> usize {
        let before = self.tiles.len();
        self.tiles.retain(|t| !t.validity.is_expired());
        before - self.tiles.len()
    }

    /// Advance all tiles' clocks.
    pub fn advance_all(&mut self, seconds: u64) {
        for tile in &mut self.tiles {
            tile.validity.advance(seconds);
        }
    }

    pub fn len(&self) -> usize { self.tiles.len() }
    pub fn is_empty(&self) -> bool { self.tiles.is_empty() }
}

impl Default for TemporalStore {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> u64 { 1_000_000 }

    #[test]
    fn test_new_tile_is_valid() {
        let tv = TemporalValidity::new(now());
        assert!(tv.is_valid());
        assert!(!tv.is_expired());
        assert_eq!(tv.decay_factor(), 1.0);
    }

    #[test]
    fn test_tile_decays_in_grace() {
        let mut tv = TemporalValidity::new(now())
            .with_window(100, 100);
        tv.advance(150); // past validity, in grace
        assert!(!tv.is_valid());
        assert!(tv.in_grace());
        assert!(!tv.is_expired());
        let factor = tv.decay_factor();
        assert!(factor < 1.0 && factor > 0.0);
    }

    #[test]
    fn test_tile_expires_after_grace() {
        let mut tv = TemporalValidity::new(now())
            .with_window(100, 100);
        tv.advance(300); // past grace
        assert!(tv.is_expired());
        assert_eq!(tv.decay_factor(), 0.0);
    }

    #[test]
    fn test_refresh_resets_validity() {
        let mut tv = TemporalValidity::new(now())
            .with_window(100, 100);
        tv.advance(150);
        assert!(tv.in_grace());
        tv.refresh(tv.now);
        assert!(tv.is_valid()); // refreshed, back to valid
    }

    #[test]
    fn test_evidence_bonus_decay() {
        let mut tv = TemporalValidity::new(now())
            .with_window(100, 100);
        tv.advance(100); // one half-life
        let bonus = tv.evidence_bonus();
        assert!(bonus > 0.3 && bonus < 0.7, "bonus should be ~0.5 at half-life");
    }

    #[test]
    fn test_temporal_score_with_evidence() {
        let mut tv = TemporalValidity::new(now())
            .with_window(100, 100);
        tv.advance(50);
        tv.record_evidence(tv.now);
        let score = tv.temporal_score();
        assert!(score > 1.0, "fresh evidence should boost above 1.0");
    }

    #[test]
    fn test_temporal_tile_scored() {
        let mut tile = TemporalTile::new("t1", "hello", now());
        tile.validity = TemporalValidity::new(now()).with_window(100, 100);
        assert!(tile.scored() >= 1.0, "fresh tile should score at least 1.0, got {}", tile.scored());
        tile.validity.advance(201);
        assert!(tile.scored() < 1.0);
    }

    #[test]
    fn test_temporal_store_top_k() {
        let mut store = TemporalStore::new();
        let mut t1 = TemporalTile::new("fresh", "new", now());
        t1.validity = TemporalValidity::new(now()).with_window(1000, 1000);
        let mut t2 = TemporalTile::new("stale", "old", now());
        t2.validity = TemporalValidity::new(now()).with_window(100, 100);
        t2.validity.advance(150);
        store.add(t1);
        store.add(t2);
        let top = store.top_k(1);
        assert_eq!(top[0].id, "fresh");
    }

    #[test]
    fn test_evict_expired() {
        let mut store = TemporalStore::new();
        let mut t1 = TemporalTile::new("alive", "x", now());
        t1.validity = TemporalValidity::new(now()).with_window(1000, 1000);
        let mut t2 = TemporalTile::new("dead", "y", now());
        t2.validity = TemporalValidity::new(now()).with_window(10, 10);
        t2.validity.advance(100);
        store.add(t1);
        store.add(t2);
        let evicted = store.evict_expired();
        assert_eq!(evicted, 1);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn test_advance_all() {
        let mut store = TemporalStore::new();
        store.add(TemporalTile::new("a", "b", now()));
        store.advance_all(500);
        assert!(store.get("a").unwrap().validity.age() == 500);
    }

    #[test]
    fn test_state_enum() {
        let mut tv = TemporalValidity::new(now()).with_window(100, 100);
        assert_eq!(tv.state(), ValidityState::Valid);
        tv.advance(150);
        assert_eq!(tv.state(), ValidityState::Grace);
        tv.advance(200);
        assert_eq!(tv.state(), ValidityState::Expired);
    }

    #[test]
    fn test_evidence_age_no_evidence() {
        let tv = TemporalValidity::new(now());
        assert_eq!(tv.evidence_age(), 0);
    }

    #[test]
    fn test_validity_state_debug() {
        let tv = TemporalValidity::new(now());
        let state = tv.state();
        assert_eq!(state, ValidityState::Valid);
    }
}
