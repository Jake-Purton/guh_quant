use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

// Default persistence path and decay parameters.
pub const DEFAULT_POINTS_PATH: &str = "points_store.json";
/// Daily multiplicative decay factor applied per elapsed day (exponential decay).
/// Set <1.0 to forget past signals. 0.6 is aggressive (40% retained per day).
const DAILY_DECAY_FACTOR: f64 = 0.6;

/// Volatility bucket names. Keep these stable across runs; change with care.
pub const VOL_LOW: &str = "low";
pub const VOL_MED: &str = "medium";
pub const VOL_HIGH: &str = "high";

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct PointsStore {
	/// Map ticker -> map volatility_bucket -> score
	pub scores: HashMap<String, HashMap<String, f64>>,
	/// Last time (seconds since epoch) the store was updated/decayed.
	#[serde(default)]
	last_updated: u64,
	#[serde(skip)]
	path: String,
}

impl PointsStore {
	/// Load a points store from `path`. If missing or invalid, returns an empty store.
	pub fn load(path: &str) -> Self {
		let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
		match fs::read_to_string(path) {
			Ok(s) => {
				// First, try the new structured format (PointsStore) which includes last_updated.
				if let Ok(mut ps) = serde_json::from_str::<PointsStore>(&s) {
					ps.path = path.to_string();
					// Apply time-based exponential decay based on days elapsed.
					if ps.last_updated > 0 && now > ps.last_updated {
						let elapsed_days = (now - ps.last_updated) as f64 / 86400.0;
						if elapsed_days > 0.0 {
							let factor = DAILY_DECAY_FACTOR.powf(elapsed_days);
							ps.decay_all(factor);
						}
					}
					ps.last_updated = now;
					return ps;
				}
				// Fall back to legacy format: map-only file => adopt now as last_updated
				match serde_json::from_str::<HashMap<String, HashMap<String, f64>>>(&s) {
					Ok(map) => PointsStore { scores: map, last_updated: now, path: path.to_string() },
					Err(e) => {
						eprintln!("[WARN] Could not parse points file '{}': {} - starting fresh", path, e);
						PointsStore { scores: HashMap::new(), last_updated: now, path: path.to_string() }
					}
				}
			}
			Err(_) => PointsStore { scores: HashMap::new(), last_updated: now, path: path.to_string() },
		}
	}

	/// Persist the store to disk. Errors are printed but not returned.
	pub fn save(&self) {
		// Serialize the full struct (scores + last_updated). Use an atomic write (temp file then rename).
		match serde_json::to_string_pretty(&self) {
			Ok(s) => {
				let tmp = format!("{}.tmp", &self.path);
				match File::create(&tmp).and_then(|mut f| f.write_all(s.as_bytes())) {
					Ok(_) => {
						if let Err(e) = fs::rename(&tmp, &self.path) {
							eprintln!("[ERROR] Failed to move temp points file '{}': {}", tmp, e);
						}
					}
					Err(e) => eprintln!("[ERROR] Failed to write temp points file '{}': {}", tmp, e),
				}
			}
			Err(e) => eprintln!("[ERROR] Could not serialize points store: {}", e),
		}
	}

	/// Get the score for a ticker at a volatility bucket. If missing, returns 0.0.
	pub fn get_score(&self, ticker: &str, vol_bucket: &str) -> f64 {
		self.scores
			.get(ticker)
			.and_then(|m| m.get(vol_bucket))
			.cloned()
			.unwrap_or(0.0)
	}

	/// Add (or subtract) points for a ticker at a volatility bucket. Scores are clamped to >= 0.
	pub fn add_score(&mut self, ticker: &str, vol_bucket: &str, delta: f64) {
		let entry = self.scores.entry(ticker.to_string()).or_insert_with(HashMap::new);
		let old = *entry.get(vol_bucket).unwrap_or(&0.0);
		let mut new = old + delta;
		if new < 0.0 { new = 0.0; }
		entry.insert(vol_bucket.to_string(), new);

		// Log when a negative delta was applied or the score decreased
		if delta < 0.0 || new < old {
			eprintln!("[POINTS] Negative update for {}@{}: delta={:.4}, old={:.4} -> new={:.4}", ticker, vol_bucket, delta, old, new);

			// Try to append to a persistent log for later analysis. Ignore failures.
			if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open("negative_points.log") {
				use std::time::{SystemTime, UNIX_EPOCH};
				if let Ok(since) = SystemTime::now().duration_since(UNIX_EPOCH) {
					let ts = since.as_secs();
					let _ = f.write_all(format!("{},{},{},{:.4},{:.4}\n", ts, format!("{}@{}", ticker, vol_bucket), delta, old, new).as_bytes());
				} else {
					let _ = f.write_all(format!("{},{},{:.4},{:.4},{:.4}\n", ticker, vol_bucket, delta, old, new).as_bytes());
				}
			}
		}
	}

	/// Multiply all scores by a decay factor in (0,1] to slowly forget old signals.
	pub fn decay_all(&mut self, factor: f64) {
		if !(0.0..=1.0).contains(&factor) { return; }
		for m in self.scores.values_mut() {
			for v in m.values_mut() {
				*v *= factor;
			}
		}
	}

	/// Ensure the ticker has the three volatility buckets initialized.
	pub fn ensure_buckets(&mut self, ticker: &str) {
		let m = self.scores.entry(ticker.to_string()).or_insert_with(HashMap::new);
		m.entry(VOL_LOW.to_string()).or_insert(0.0);
		m.entry(VOL_MED.to_string()).or_insert(0.0);
		m.entry(VOL_HIGH.to_string()).or_insert(0.0);
	}
}
