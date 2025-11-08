use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;

/// Simple persistent points store used to bias stock selection.
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct PointsStore {
    pub scores: HashMap<String, f64>,
    #[serde(skip)]
    path: String,
}

impl PointsStore {
    /// Load a points store from `path`. If missing or invalid, returns an empty store.
    pub fn load(path: &str) -> Self {
        match fs::read_to_string(path) {
            Ok(s) => match serde_json::from_str::<HashMap<String, f64>>(&s) {
                Ok(map) => PointsStore { scores: map, path: path.to_string() },
                Err(e) => {
                    eprintln!("[WARN] Could not parse points file '{}': {} - starting fresh", path, e);
                    PointsStore { scores: HashMap::new(), path: path.to_string() }
                }
            },
            Err(_) => PointsStore { scores: HashMap::new(), path: path.to_string() },
        }
    }

    /// Persist the store to disk. Errors are printed but not returned.
    pub fn save(&self) {
        match serde_json::to_string_pretty(&self.scores) {
            Ok(s) => {
                if let Err(e) = fs::OpenOptions::new().create(true).write(true).truncate(true).open(&self.path)
                    .and_then(|mut f| f.write_all(s.as_bytes()))
                {
                    eprintln!("[ERROR] Failed to write points file '{}': {}", self.path, e);
                }
            }
            Err(e) => eprintln!("[ERROR] Could not serialize points store: {}", e),
        }
    }

    /// Get the score for a ticker (0.0 if missing)
    pub fn get_score(&self, ticker: &str) -> f64 {
        *self.scores.get(ticker).unwrap_or(&0.0)
    }

    /// Add (or subtract) points for a ticker. Scores are clamped to >= 0.
    pub fn add_score(&mut self, ticker: &str, delta: f64) {
        let entry = self.scores.entry(ticker.to_string()).or_insert(0.0);
        let old = *entry;
        let mut new = old + delta;
        if new < 0.0 { new = 0.0; }
        *entry = new;

        // Log when a negative delta was applied or the score decreased
        if delta < 0.0 || new < old {
            eprintln!("[POINTS] Negative update for {}: delta={:.4}, old={:.4} -> new={:.4}", ticker, delta, old, new);

            // Try to append to a persistent log for later analysis. Ignore failures.
            if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open("negative_points.log") {
                use std::time::{SystemTime, UNIX_EPOCH};
                if let Ok(since) = SystemTime::now().duration_since(UNIX_EPOCH) {
                    let ts = since.as_secs();
                    let _ = f.write_all(format!("{},{},{:.4},{:.4},{:.4}\n", ts, ticker, delta, old, new).as_bytes());
                } else {
                    let _ = f.write_all(format!("{}, {:.4}, {:.4}, {:.4}\n", ticker, delta, old, new).as_bytes());
                }
            }
        }
    }

    /// Multiply all scores by a decay factor in (0,1] to slowly forget old signals.
    pub fn decay_all(&mut self, factor: f64) {
        if !(0.0..=1.0).contains(&factor) { return; }
        for v in self.scores.values_mut() {
            *v *= factor;
        }
    }
}
