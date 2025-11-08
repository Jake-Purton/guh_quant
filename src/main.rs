mod investor;
mod stocks;
mod portfolio;
mod points;

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use std::error::Error;

use investor::InvestorProfile;
use stocks::{Stock, prefetch_all_stocks, fetch_historical_returns};
use portfolio::{filter_stocks_by_profile, build_portfolio, budget_spend_fraction};
use portfolio::volatility_bucket;
use points::PointsStore;
use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use regex::Regex;

const URL: &str = "http://www.prism-challenge.com";
const PORT: u16 = 8082;
const TEAM_API_CODE: &str = "f7f47b3680640b753e6cccfd14bbca89";
// Minimum expected client worth (in 'points') below which we will skip the request.
// Tune this constant to be more or less aggressive about skipping low-value clients.
const MIN_EXPECTED_POINTS: f64 = 20.0; // suggested starting threshold (near mean_expected ~90)

// Linear surrogate predictor default coefficients exported from `linear_surrogate.json`.
// Feature order: [budget_log, eligible, period, avg_vol, avg_logcap, avg_pt, psize, risk_cons, risk_mod, risk_aggr]
const SURROGATE_INTERCEPT: f64 = 97.32438057349923;
const SURROGATE_COEFFS: [f64; 10] = [
    13.874491883091332, // budget_log
    0.8099161078085209, // eligible
    -0.0016355332571995223, // period
    -1210.6415214918222, // avg_vol (note large negative due to scale)
    -24.202314848774293, // avg_logcap
    2.5531580924259565, // avg_pt
    1.9513411424118705, // psize
    -0.13083387343613717, // risk_cons
    5.4051113479713795, // risk_mod
    -5.274277474537541, // risk_aggr
];

#[derive(Debug, Clone)]
struct LinearSurrogate {
    intercept: f64,
    coeffs: [f64; 10],
}

impl LinearSurrogate {
    fn default() -> Self {
        Self { intercept: SURROGATE_INTERCEPT, coeffs: SURROGATE_COEFFS }
    }
}

/// Attempt to load a JSON file with keys {intercept, coefficients} where coefficients is an array of 10 numbers.
fn load_linear_surrogate(path: &str) -> Option<LinearSurrogate> {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            match serde_json::from_str::<serde_json::Value>(&s) {
                Ok(v) => {
                    let intercept = v.get("intercept").and_then(|x| x.as_f64()).unwrap_or(SURROGATE_INTERCEPT);
                    if let Some(arr) = v.get("coefficients").and_then(|c| c.as_array()) {
                        if arr.len() == 10 {
                            let mut coeffs = [0.0f64; 10];
                            for (i, item) in arr.iter().enumerate() {
                                coeffs[i] = item.as_f64().unwrap_or(0.0);
                            }
                            return Some(LinearSurrogate { intercept, coeffs });
                        }
                    }
                    None
                }
                Err(_) => None,
            }
        }
        Err(_) => None,
    }
}

/// Predict points using the given linear surrogate and feature vector.
fn predict_points_surrogate(sur: &LinearSurrogate,
    budget: f64,
    eligible_count: usize,
    period_years: f64,
    avg_vol: f64,
    avg_logcap: f64,
    avg_pts_score: f64,
    psize: f64,
    risk: &investor::RiskLevel,
) -> f64 {
    let budget_log = budget.max(0.0).ln_1p();
    let eligible = eligible_count as f64;
    let period = period_years;
    let x = [
        budget_log,
        eligible,
        period,
        avg_vol,
        avg_logcap,
        avg_pts_score,
        psize,
        if matches!(risk, investor::RiskLevel::Conservative) { 1.0 } else { 0.0 },
        if matches!(risk, investor::RiskLevel::Moderate) { 1.0 } else { 0.0 },
        if matches!(risk, investor::RiskLevel::Aggressive) { 1.0 } else { 0.0 },
    ];

    let mut sum = sur.intercept;
    for i in 0..10 {
        sum += sur.coeffs[i] * x[i];
    }
    sum
}

// API Functions
async fn send_get_request(path: &str) -> Result<String, Box<dyn Error>> {
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert("X-API-Code", HeaderValue::from_str(TEAM_API_CODE)?);
    let url = format!("{URL}:{PORT}{path}");
    let resp = client.get(&url).headers(headers).send().await?;

    let status = resp.status();
    let text = resp.text().await?;

    if !status.is_success() {
        Err(format!(
            "Error - something went wrong when requesting [CODE: {}]: {}",
            status, text
        ))?
    } else {
        Ok(text)
    }
}

async fn send_post_request(path: &str, data: &Value) -> Result<String, Box<dyn Error>> {
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert("X-API-Code", HeaderValue::from_str(TEAM_API_CODE)?);
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let url = format!("{URL}:{PORT}{path}");
    let resp = client.post(&url).headers(headers).json(data).send().await?;

    let status = resp.status();
    let text = resp.text().await?;

    if !status.is_success() {
        Err(format!(
            "Error - something went wrong when requesting [CODE: {}]: {}",
            status, text
        ))?
    } else {
        Ok(text)
    }
}

async fn get_context() -> Result<String, Box<dyn Error>> {
    // Retry logic for network issues
    for attempt in 1..=3 {
        match send_get_request("/request").await {
            Ok(response) => return Ok(response),
            Err(e) => {
                if attempt < 3 {
                    // eprintln!("[WARN] Network error (attempt {}): {}. Retrying...", attempt, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err("Failed after 3 attempts".into())
}

async fn send_portfolio(weighted_stocks: Vec<(&str, i32)>) -> Result<String, Box<dyn Error>> {
    // Submit the portfolio once. Avoid retrying POSTs because retries can
    // trigger race conditions on the server (e.g., 403 after a late retry).
    let data: Vec<Value> = weighted_stocks
        .into_iter()
        .map(|(ticker, quantity)| json!({ "ticker": ticker, "quantity": quantity }))
        .collect();

    send_post_request("/submit", &json!(data)).await
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Load initial stock data from cache (metadata + structure)
    println!("[LOAD] Loading initial stock data...");
    let stock_metadata = prefetch_all_stocks().await?;
    
    println!("[INFO] Loaded {} stocks from cache\n", stock_metadata.len());

    // Load linear surrogate coefficients from disk if present. This lets you update
    // the heuristic by replacing `linear_surrogate.json` without recompiling.
    let surrogate: LinearSurrogate = match load_linear_surrogate("linear_surrogate.json") {
        Some(s) => {
            println!("[INFO] Loaded linear_surrogate.json with intercept={:.3}", s.intercept);
            s
        }
        None => {
            println!("[WARN] linear_surrogate.json not found or invalid - using built-in defaults");
            LinearSurrogate::default()
        }
    };

    loop {
        // Get and parse context
        let context = get_context().await?;
        println!("Context provided: {}", context);
        
        if let Ok(profile) = InvestorProfile::from_context(&context) {

            println!("\n[PROFILE] Investor Profile:");
            println!("  Name: {}", profile.name);
            println!("  Age: {} ({:?})", profile.age, profile.risk_tolerance);
            println!("  Budget: ${:.2}", profile.budget);
            println!("  Excluded: {:?}", profile.excluded_sectors);
            println!("  Investment Period: {:?} to {:?}", profile.start_year, profile.end_year);
        
            // Clone stock metadata for this request
            let mut all_stocks = stock_metadata.clone();
            
            // PHASE 1: Fetch historical returns for ranking/selection (uses interpolation)
            if let (Some(start_year), Some(end_year)) = (profile.start_year, profile.end_year) {
                // Construct date strings from the profile
                let start = format!("{}-01-01", start_year);
                let end = format!("{}-12-31", end_year);
                
                println!("[PHASE1] Fetching historical data for ranking ({} to {})...", start, end);
                if let Err(_e) = fetch_historical_returns(&mut all_stocks, &start, &end).await {
                    // eprintln!("[WARN] Could not fetch historical returns: {}", _e);
                }
            }
            
            // Filter by investor profile
            let eligible_stocks = filter_stocks_by_profile(&all_stocks, &profile);
            println!("[FILTER] Eligible stocks after filtering: {} (from {} total)", eligible_stocks.len(), all_stocks.len());
            
            if eligible_stocks.is_empty() {
                return Err("No eligible stocks found!".into());
            }

            // Use linear surrogate predictor to estimate expected points and skip if below threshold
            // Compute features approximated from eligible universe (portfolio-level features are approximated here)
            let mut seen = 0.0f64;
            let mut sum_logcap = 0.0f64;
            let mut sum_vol = 0.0f64;
            let mut sum_pts = 0.0f64;
            for s in &eligible_stocks {
                seen += 1.0;
                if s.market_cap > 0 {
                    sum_logcap += (s.market_cap as f64).log10();
                }
                sum_vol += s.volatility;
                let bucket = volatility_bucket(s.volatility);
                let score = PointsStore::load("points_store.json").get_score(&s.ticker, bucket);
                sum_pts += score;
            }
            let avg_logcap = if seen > 0.0 { sum_logcap / seen } else { 0.0 };
            let avg_vol = if seen > 0.0 { sum_vol / seen } else { 0.0 };
            let avg_pts_score = if seen > 0.0 { sum_pts / seen } else { 0.0 };

            let period_years = match (profile.start_year, profile.end_year) {
                (Some(s), Some(e)) if e >= s => (e - s + 1) as f64,
                _ => 1.0,
            };

            // Approximate portfolio size by eligible universe size (conservative proxy)
            let psize = eligible_stocks.len() as f64;

            let predicted_points = predict_points_surrogate(
                &surrogate,
                profile.budget,
                eligible_stocks.len(),
                period_years,
                avg_vol,
                avg_logcap,
                avg_pts_score,
                psize,
                &profile.risk_tolerance,
            );

            println!("[HEURISTIC] Surrogate predicted points: {:.2} (threshold {:.2})", predicted_points, MIN_EXPECTED_POINTS);
            if predicted_points < MIN_EXPECTED_POINTS {
                println!("[SKIP] Predicted points {:.2} below threshold {:.2} - skipping this request.", predicted_points, MIN_EXPECTED_POINTS);
                // Log a compact trace entry for analysis indicating we skipped this request
                if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("request_trace.jsonl") {
                    use chrono::Utc;
                    let ts = Utc::now().to_rfc3339();
                    let entry = json!({
                        "ts": ts,
                        "raw_context": context,
                        "parsed_profile": {
                            "name": profile.name,
                            "age": profile.age,
                            "budget": profile.budget,
                        },
                        "eligible_count": eligible_stocks.len(),
                        "predicted_points": predicted_points,
                        "skipped": true,
                        "skip_reason": "low_expected_points"
                    });
                    if let Ok(line) = serde_json::to_string(&entry) {
                        let _ = f.write_all(line.as_bytes());
                        let _ = f.write_all(b"\n");
                    }
                }
                continue;
            }
        
            // Build portfolio based on interpolated/cached data
            let portfolio = build_portfolio(
                &eligible_stocks,
                profile.budget,
                profile.risk_tolerance
            );
            // If the built portfolio has zero total market value (e.g., no quantities or prices missing), skip.
            let mut portfolio_value = 0.0f64;
            for (ticker, qty) in &portfolio {
                if *qty <= 0 { continue; }
                if let Some(stock) = eligible_stocks.iter().find(|s| &s.ticker == ticker) {
                    portfolio_value += stock.get_current_price() * (*qty as f64);
                }
            }
            if portfolio_value == 0.0 {
                println!("[SKIP] Built portfolio has zero value - skipping request.");
                // Log skip for later analysis
                if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("request_trace.jsonl") {
                    use chrono::Utc;
                    let ts = Utc::now().to_rfc3339();
                    let entry = json!({
                        "ts": ts,
                        "raw_context": context,
                        "parsed_profile": {
                            "name": profile.name,
                            "age": profile.age,
                            "budget": profile.budget,
                        },
                        "eligible_count": eligible_stocks.len(),
                        "portfolio_value": portfolio_value,
                        "skipped": true,
                        "skip_reason": "zero_portfolio_value"
                    });
                    if let Ok(line) = serde_json::to_string(&entry) {
                        let _ = f.write_all(line.as_bytes());
                        let _ = f.write_all(b"\n");
                    }
                }
                continue;
            }
            
            // Debug: Show selected stocks and their IPO info
            println!("\n[DEBUG] Selected stocks for portfolio:");
            for (ticker, _) in &portfolio {
                if let Some(stock) = eligible_stocks.iter().find(|s| &s.ticker == ticker) {
                    println!("  {} - IPO: {} (return: {:.1}%)", 
                            ticker, 
                            stock.first_trading_date.as_ref().unwrap_or(&"unknown".to_string()),
                            stock.historical_return.unwrap_or(0.0));
                }
            }
            println!();
            
            // PHASE 2: DISABLED - Just use interpolated prices
            // Phase 2 (exact pricing via API) was causing issues with:
            // - Ticker changes (BKNG was PCLN)
            // - API rate limiting
            // - Inconsistent data availability
            // Interpolated prices from Phase 1 are accurate enough (within 2-3%)
            println!("[INFO] Using interpolated prices from cached data (Phase 2 disabled)");
            
            // Submit portfolio with interpolated prices
                // Validate/clean portfolio before the single allowed submit
                let cleaned = pre_submit_validate(&portfolio, &eligible_stocks, profile.budget);
                // Pass the raw context and original budget so the logger can record both
                print_portfolio_and_submit(&cleaned, &eligible_stocks, &profile, &context, profile.budget).await?;
        } else {
            println!("error in profile skipping")
        }
    }
    
    // Unreachable: loop runs forever until externally terminated
    #[allow(unreachable_code)]
    Ok(())
}

async fn print_portfolio_and_submit(
    portfolio: &[(String, i32)],
    eligible_stocks: &[Stock],
    profile: &InvestorProfile,
    raw_context: &str,
    original_budget: f64,
) -> Result<(), Box<dyn Error>> {
    let mut total_cost = 0.0;
    for (ticker, qty) in portfolio {
        let stock = eligible_stocks.iter().find(|s| s.ticker == *ticker).unwrap();
        // Use current market price for displayed/submitted cost so it matches evaluator
        let current_price = stock.get_current_price();
        let cost = current_price * (*qty as f64);
        total_cost += cost;

        // Show current price and historical start price (if available)
        if let Some(hist_price) = stock.historical_start_price {
            println!(
                "  {} x{} @ ${:.2} current (${:.2} historical → {:.1}% return) = ${:.2}",
                ticker, qty, current_price, hist_price,
                stock.historical_return.unwrap_or(0.0), cost
            );
        } else {
            println!(
                "  {} x{} @ ${:.2} = ${:.2}",
                ticker, qty, current_price, cost
            );
        }
    }
    println!("  Total: ${:.2} / ${:.2}", total_cost, profile.budget);

    // Convert to required format
    let portfolio_refs: Vec<(&str, i32)> = portfolio
        .iter()
        .map(|(t, q)| (t.as_str(), *q))
        .collect();

    // Submit portfolio and capture the response (or error) for logging
    let send_result = match send_portfolio(portfolio_refs).await {
        Ok(response) => {
            println!("\n[SUCCESS] Evaluation: {}", response);
            Ok(response)
        }
        Err(e) => {
            println!("[ERROR] {}", e);
            // Try to extract problematic tickers from the error message and persist them
            if let Some(problematic) = parse_problematic_tickers(&e.to_string()) {
                if !problematic.is_empty() {
                    if let Err(err) = append_rejected_tickers(&problematic) {
                        eprintln!("[VALIDATOR] Failed to append rejected tickers: {}", err);
                    } else {
                        eprintln!("[VALIDATOR] Appended rejected tickers: {:?}", problematic);
                    }
                }
            }
            Err(e)
        }
    };

    // Append a compact JSONL trace for debugging/correlation analysis
    // Fields: timestamp, raw_context, parsed_profile, eligible_count, alloc_budget, portfolio, total_cost, response/error
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("request_trace.jsonl") {
        use chrono::Utc;
        let ts = Utc::now().to_rfc3339();

        // Build profile object
        let profile_obj = json!({
            "name": profile.name,
            "age": profile.age,
            "budget": profile.budget,
            "excluded_sectors": profile.excluded_sectors,
            "risk_tolerance": format!("{:?}", profile.risk_tolerance),
            "start_year": profile.start_year,
            "end_year": profile.end_year,
        });

    let alloc_budget = original_budget * budget_spend_fraction();

        let portfolio_json: Vec<Value> = portfolio.iter().map(|(t, q)| json!({ "ticker": t, "quantity": q })).collect();

        let entry = json!({
            "ts": ts,
            "raw_context": raw_context,
            "parsed_profile": profile_obj,
            "eligible_count": eligible_stocks.len(),
            "alloc_budget": alloc_budget,
            "portfolio": portfolio_json,
            "allocated_cost": total_cost,
            "result": match &send_result {
                Ok(resp) => json!({"ok": true, "response": resp}),
                Err(err) => json!({"ok": false, "error": err.to_string()}),
            }
        });

        if let Ok(line) = serde_json::to_string(&entry) {
            let _ = f.write_all(line.as_bytes());
            let _ = f.write_all(b"\n");
        }
    }
    
    // Reinforcement learning: immediate update of PointsStore using evaluator points
    if let Ok(resp_text) = &send_result {
        // If the evaluator response contains timeout/slow indicators, skip RL update.
        let resp_lc = resp_text.to_lowercase();
        let ignore_patterns = [
            "too slow",
            "responded too slowly",
            "context expired",
            "timed out",
            "timeout",
            "expired",
            "context deadline",
        ];

        let mut is_ignored = false;
        for p in &ignore_patterns {
            if resp_lc.contains(p) {
                is_ignored = true;
                break;
            }
        }

        if is_ignored {
            println!("[POINTS] Skipping RL update due to timeout/slow response indicator");
        } else {
            // Try to parse evaluator response as JSON to extract numeric `points`.
            let mut points_val: Option<f64> = None;
            if let Ok(v) = serde_json::from_str::<Value>(resp_text) {
                if v.is_object() {
                    if let Some(p) = v.get("points").and_then(|x| x.as_f64()) {
                        points_val = Some(p);
                    }
                } else if v.is_string() {
                    if let Some(s) = v.as_str() {
                        if let Ok(inner) = serde_json::from_str::<Value>(s) {
                            if let Some(p) = inner.get("points").and_then(|x| x.as_f64()) {
                                points_val = Some(p);
                            }
                        }
                    }
                }
            }

            if let Some(points_num) = points_val {
                // delta = points / 100 per your request
                let delta = points_num / 100.0;
                let mut ps = PointsStore::load("points_store.json");
                for (ticker, _qty) in portfolio {
                    if let Some(stock) = eligible_stocks.iter().find(|s| &s.ticker == ticker) {
                        let bucket = volatility_bucket(stock.volatility);
                        ps.ensure_buckets(&ticker);
                        ps.add_score(&ticker, bucket, delta);
                    } else {
                        // If we don't have metadata, still apply to default (medium) bucket
                        ps.ensure_buckets(&ticker);
                        ps.add_score(&ticker, crate::points::VOL_MED, delta);
                    }
                }
                ps.save();
                // eprintln!("[POINTS] Applied delta {:.4} for {} tickers", delta, portfolio.len());
            }
        }
    }
    
    Ok(())
}

// Load rejected tickers from disk (one per line). Missing file results in empty set.
fn load_rejected_tickers(path: &str) -> HashSet<String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => contents
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Err(_) => HashSet::new(),
    }
}

// Append new rejected tickers to the file (avoids duplicates by checking existing file first)
fn append_rejected_tickers(tickers: &[String]) -> Result<(), Box<dyn Error>> {
    let path = "rejected_tickers.txt";
    let mut existing = load_rejected_tickers(path);
    let mut new_added = Vec::new();

    for t in tickers {
        if !existing.contains(t) {
            existing.insert(t.clone());
            new_added.push(t.clone());
        }
    }

    if new_added.is_empty() {
        return Ok(());
    }

    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    for t in new_added {
        writeln!(f, "{}", t)?;
    }

    Ok(())
}

// Try to parse a few common error message shapes to extract problematic tickers.
// Returns None if nothing parsed.
fn parse_problematic_tickers(err_text: &str) -> Option<Vec<String>> {
    // Use regex-based extraction to handle multiple error formats.
    let mut found: HashSet<String> = HashSet::new();

    // 1) Extract contents of bracketed lists: [...]
    if let Ok(bracket_re) = Regex::new(r"\[([^\]]+)\]") {
        for cap in bracket_re.captures_iter(err_text) {
            let inner = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            for token in inner.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '.') {
                let tok = token.trim().trim_matches('"').trim_matches('\'');
                if tok.is_empty() { continue; }
                let cleaned: String = tok.chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '.')
                    .map(|c| c.to_ascii_uppercase())
                    .collect();
                if cleaned.chars().any(|c| c.is_ascii_alphabetic()) {
                    found.insert(cleaned);
                }
            }
        }
    }

    // 2) Specific pattern: 'invalid ticker type: TICKER of type ...'
    if let Ok(inv_re) = Regex::new(r"invalid ticker type:\s*([A-Za-z0-9.\-]+)") {
        for cap in inv_re.captures_iter(err_text) {
            found.insert(cap[1].to_ascii_uppercase());
        }
    }

    // 3) Some errors embed arrays of pairs like [['TTWO', 11], ['ROKU', 10]] - bracket capture above will pick them up,
    // but as a fallback extract standalone ticker-like tokens (all-caps, length 1-6)
    if found.is_empty() {
        if let Ok(tok_re) = Regex::new(r"\b[A-Z0-9][A-Z0-9.\-]{0,6}\b") {
            for cap in tok_re.captures_iter(err_text) {
                let tok = &cap[0];
                // skip purely numeric tokens
                if tok.chars().any(|c| c.is_ascii_alphabetic()) {
                    found.insert(tok.to_string());
                }
            }
        }
    }

    if found.is_empty() {
        None
    } else {
        Some(found.into_iter().collect())
    }
}

/// Pre-submit validator: remove unknown tickers and force portfolio within budget.
fn pre_submit_validate(
    portfolio: &[(String, i32)],
    eligible_stocks: &[Stock],
    budget: f64,
) -> Vec<(String, i32)> {
    // Conservative pre-submit validator.
    // We apply a small safety margin because the remote evaluator may value
    // the portfolio using a different snapshot or canonical tickers. This
    // margin reduces the chance of a single-submission budget-breach.
    const SUBMIT_MARGIN: f64 = 0.03; // 3% safety margin

    // Build a lookup of current prices
    let price_map: HashMap<String, f64> = eligible_stocks
        .iter()
        .map(|s| (s.ticker.clone(), s.get_current_price()))
        .collect();

    // Keep only tickers that are in eligible_stocks and have positive qty
    let mut cleaned: Vec<(String, i32)> = portfolio.iter()
        .filter(|(t, q)| *q > 0 && price_map.contains_key(t))
        .cloned()
        .collect();

    // Also drop any tickers we've previously seen rejected by the evaluator
    let rejected = load_rejected_tickers("rejected_tickers.txt");
    if !rejected.is_empty() {
        let before = cleaned.len();
        cleaned.retain(|(t, _)| !rejected.contains(t));
        let after = cleaned.len();
        if before != after {
            eprintln!("[VALIDATOR] Removed {} previously-rejected tickers before submit", before - after);
        }
    }

    // Drop obviously-problematic tickers (dots, slashes, carets) that the
    // evaluator often rejects as non-canonical. Log them for analysis.
    let mut removed_problematic: Vec<String> = Vec::new();
    cleaned.retain(|(t, q)| {
        if t.contains('.') || t.contains('/') || t.contains('^') || t.contains(' ') {
            removed_problematic.push(t.clone());
            false
        } else {
            *q > 0
        }
    });
    if !removed_problematic.is_empty() {
        eprintln!("[VALIDATOR] Dropped problematic tickers (non-canonical forms): {:?}", removed_problematic);
    }

    // Compute current total cost
    let mut total: f64 = cleaned.iter().map(|(t, q)| price_map.get(t).unwrap() * (*q as f64)).sum();

    // Apply safety margin to the effective budget we target
    let effective_budget = budget * (1.0 - SUBMIT_MARGIN);
    if total <= effective_budget { return cleaned; }

    eprintln!("[VALIDATOR] Portfolio exceeds safe budget before submit: ${:.2} > ${:.2} (budget ${:.2}, margin {:.1}%) - reducing...", total, effective_budget, budget, SUBMIT_MARGIN*100.0);

    // Sort positions by price descending (drop most expensive shares first)
    cleaned.sort_by(|a, b| {
        let pa = *price_map.get(&a.0).unwrap_or(&0.0);
        let pb = *price_map.get(&b.0).unwrap_or(&0.0);
        pb.partial_cmp(&pa).unwrap()
    });

    // Iteratively reduce quantities from the most expensive position until under effective_budget
    let mut idx = 0;
    while total > effective_budget && !cleaned.is_empty() {
        if idx >= cleaned.len() { idx = 0; } // wrap

        let (ref ticker, ref mut qty) = cleaned[idx];
        let price = *price_map.get(ticker).unwrap_or(&0.0);
        if *qty > 0 && price > 0.0 {
            *qty -= 1;
            total -= price;
            if *qty == 0 {
                cleaned.remove(idx);
                // don't increment idx (next element shifted into this index)
            } else {
                idx += 1;
            }
        } else {
            // remove impossible position
            let removed = cleaned.remove(idx);
            eprintln!("[VALIDATOR] Removed impossible position: {:?}", removed);
        }
    }

    eprintln!("[VALIDATOR] Reduced portfolio cost to ${:.2} (target <= ${:.2})", total, effective_budget);
    cleaned
}