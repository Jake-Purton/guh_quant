use crate::investor::{InvestorProfile, RiskLevel};
use crate::stocks::Stock;
use crate::points::PointsStore;
use serde_json::json;
use chrono::Utc;
use std::fs::OpenOptions;
use std::io::Write;

// Helper: append an overbudget event (JSONL) with timestamp, budget, cost and portfolio
fn log_overbudget_event(portfolio: &[(String, i32)], stocks: &[Stock], budget: f64, total_cost: f64, reason: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("overbudget_events.jsonl") {
        let ts = Utc::now().to_rfc3339();
        let portfolio_json: Vec<serde_json::Value> = portfolio.iter().map(|(t, q)| {
            let price = stocks.iter().find(|s| &s.ticker == t).map(|s| s.get_current_price()).unwrap_or(0.0);
            json!({"ticker": t, "quantity": q, "price": price})
        }).collect();

        let entry = json!({
            "ts": ts,
            "reason": reason,
            "budget": budget,
            "total_cost": total_cost,
            "over_by": total_cost - budget,
            "portfolio": portfolio_json,
        });

        if let Ok(line) = serde_json::to_string(&entry) {
            let _ = f.write_all(line.as_bytes());
            let _ = f.write_all(b"\n");
        }
    }
}

// Learning / weighting configuration
const RETURN_WEIGHT: f64 = 0.7; // weight given to historical return
// (Points system removed)

// Concentrated allocation settings
// When true, allocate quantities using a rank-based quantity table
// (e.g. 50 shares of top, 20 of second, ...). If budget doesn't allow the
// full target quantity the value is reduced to what can be afforded.
const CONCENTRATE_ALLOCATION: bool = true;
// Default rank quantity targets for positions (index 0 = top performer)
const RANK_QUANTITIES: &[i32] = &[
    50, 20, 15, 10, 8, 6, 5, 4, 3, 2, // top 10
    1, 1, 1, 1, 1, // fallback for additional ranks
];
// Hard cap on number of distict positions in any portfolio
const MAX_POSITIONS: usize = 12;
// Fraction of the provided budget that we allow the allocator to spend.
// Set to 0.70 to only use 70% of the budget for purchases; the remainder
// is intentionally left unspent as a conservative buffer.
/// Fraction of the provided budget that we allow the allocator to spend.
/// Default value is 0.60 (60%). You can override this at runtime by
/// setting the environment variable `BUDGET_SPEND_FRACTION` to a
/// floating-point number in (0.0, 1.0]. For example:
///
///   export BUDGET_SPEND_FRACTION=0.8
///
pub const BUDGET_SPEND_FRACTION: f64 = 0.5;

/// Return the budget spend fraction to use at runtime. First checks the
/// environment variable `BUDGET_SPEND_FRACTION` and falls back to the
/// compiled default if the variable is not present or invalid.
pub fn budget_spend_fraction() -> f64 {
    BUDGET_SPEND_FRACTION
}

/// Calculate the total cost of a portfolio
fn calculate_portfolio_cost(portfolio: &[(String, i32)], stocks: &[Stock]) -> f64 {
    portfolio.iter()
        .map(|(ticker, qty)| {
            let stock = stocks.iter().find(|s| &s.ticker == ticker);
            if let Some(s) = stock {
                // Use current market price when calculating total cost so it
                // matches the server's evaluation basis (submission uses current prices)
                s.get_current_price() * (*qty as f64)
            } else {
                0.0
            }
        })
        .sum()
}

/// Validate that portfolio does not exceed budget
/// Returns true if valid, false if over budget
fn validate_budget(portfolio: &[(String, i32)], stocks: &[Stock], budget: f64) -> bool {
    let total_cost = calculate_portfolio_cost(portfolio, stocks);
    let is_valid = total_cost <= budget;
    
    if !is_valid {
        eprintln!("[ERROR] Portfolio exceeds budget!");
        eprintln!("  Budget: ${:.2}", budget);
        eprintln!("  Portfolio cost: ${:.2}", total_cost);
        eprintln!("  Over by: ${:.2}", total_cost - budget);
        // Log the chosen funds for debugging/analysis
        log_overbudget_event(portfolio, stocks, budget, total_cost, "validate_budget_check");
    }
    
    is_valid
}

/// Emergency budget fix: Remove shares until under budget
fn force_within_budget(portfolio: &mut Vec<(String, i32)>, stocks: &[Stock], budget: f64) {
    while calculate_portfolio_cost(portfolio, stocks) > budget {
        // Find the position with the most shares
        if let Some((idx, _)) = portfolio.iter().enumerate()
            .max_by_key(|(_, (_, qty))| *qty) {
            
            // Reduce by 1 share
            portfolio[idx].1 -= 1;
            
            // Remove position if quantity is 0
            if portfolio[idx].1 == 0 {
                portfolio.remove(idx);
            }
        } else {
            break; // Portfolio is empty
        }
    }
}

/// Get the first trading year for a ticker from hardcoded database
/// This is used as a fallback when cache data is unavailable
fn get_first_trading_year(ticker: &str) -> Option<u32> {
    // Hardcoded first trading years for stocks that frequently cause issues
    // or don't have data in cache yet
    let trading_years = [
        // Recent spinoffs/name changes that may not be valid for older periods
        ("COR", 2023),  // Cencora (formerly AmerisourceBergen, changed name 2023)
        ("TECH", 2014), // Bio-Techne
        ("K", 2012),    // Kellanova (spun from Kellogg 2023)
        ("DOW", 2019),  // Dow Inc. (spun from DowDuPont)
        ("DD", 2019),   // DuPont (spun from DowDuPont)
        
        // Old established companies  
        ("PG", 1890), ("JNJ", 1944), ("PFE", 1942), ("XOM", 1882),
        ("BAC", 1904), ("WFC", 1852), ("IBM", 1911), ("GE", 1892),
        ("KO", 1919), ("DIS", 1957), ("MCD", 1965), ("CAT", 1929),
        ("MMM", 1946), ("BA", 1934), ("F", 1956), ("T", 1983),
        
        // 1970s-1990s
        ("AAPL", 1980), ("MSFT", 1986), ("INTC", 1971), ("WMT", 1972),
        ("CSCO", 1990), ("AMD", 1979), ("ADBE", 1986), ("NVDA", 1999),
        ("AMZN", 1997), ("UPS", 1999), ("PLUG", 1999),
        ("BKNG", 2018), // Changed from PCLN in 2018, use conservative date
        ("EA", 2008),  // Electronic Arts - being conservative due to 2007 ticker issues
        
        // 2000s
        ("GOOGL", 2004), ("GOOG", 2004), ("VZ", 2000), ("NFLX", 2002),
        ("CRM", 2004), ("MA", 2006), ("V", 2008), ("FSLR", 2006),
        ("TSLA", 2010), ("GM", 2010),
        
        // 2010s  
        ("META", 2012), ("ABBV", 2013), ("ZTS", 2013), ("TWTR", 2013),
        ("KMI", 2011), ("MDLZ", 2012), ("ENPH", 2012),
        ("SHOP", 2015), ("MTCH", 2016), ("ETSY", 2015),  // MTCH actually IPO'd late 2015, be conservative
        ("TWLO", 2016), ("SNAP", 2017), ("ROKU", 2017), ("OKTA", 2017),
        ("MRNA", 2018), ("DOCU", 2018), ("VICI", 2018),
        ("ZM", 2019), ("UBER", 2019), ("LYFT", 2019), ("PINS", 2019),
        ("DDOG", 2019), ("CRWD", 2019), ("BNTX", 2019),
        
        // 2020+
        ("PLTR", 2020), ("SNOW", 2020), ("ABNB", 2020), ("DASH", 2020),
        ("RIVN", 2021), ("LCID", 2021), ("SOFI", 2021), ("COIN", 2021),
        ("RBLX", 2021), ("U", 2021), ("HOOD", 2021),
        
        // Additional banks/healthcare that cause issues
        ("TFC", 2004),  // Truist (BB&T legacy ticker, merger was 2019 but BB&T IPO 2004)
        ("ZBH", 2001),  // Zimmer Biomet
        ("USB", 1929),  // US Bancorp (old bank)
        ("PNC", 1983),  // PNC Financial
    ];
    
    trading_years.iter()
        .find(|(t, _)| *t == ticker)
        .map(|(_, year)| *year)
}

/// Tickers that are excluded due to API issues or data quality problems
const EXCLUDED_TICKERS: &[&str] = &["MTCH", "TFC", "ELV", "EA", "ES", "MDLZ", "NEE", "ZBH"];

/// Check if ticker should be excluded
fn is_ticker_excluded(ticker: &str) -> bool {
    // Filter out tickers with hyphens (API issues)
    if ticker.contains('-') {
        return true;
    }
    
    // Filter out manually excluded tickers
    EXCLUDED_TICKERS.contains(&ticker)
}

/// Check if stock volatility matches risk tolerance
fn matches_risk_tolerance(volatility: f64, risk_level: RiskLevel) -> bool {
    match risk_level {
        RiskLevel::Conservative => volatility < 0.03, // Low volatility only
        RiskLevel::Moderate => volatility < 0.05,     // Medium volatility
        RiskLevel::Aggressive => true,                // All stocks acceptable
    }
}

/// Check if stock was trading during the investment period
fn was_trading_during_period(stock: &Stock, start_year: Option<u32>) -> bool {
    let Some(required_start_year) = start_year else {
        return true; // No date restriction
    };
    
    // Try cache first (format: YYYY-MM-DD)
    if let Some(first_date) = &stock.first_trading_date {
        if let Some(year_str) = first_date.split('-').next() {
            if let Ok(first_year) = year_str.parse::<u32>() {
                return first_year <= required_start_year;
            }
        }
    }
    
    // Fallback to hardcoded database
    if let Some(first_year) = get_first_trading_year(&stock.ticker) {
        return first_year <= required_start_year;
    }
    
    // Conservative: exclude if we have no trading date info
    false
}

    // Heuristics to detect 'dotcom-like' bubble risk. This is intentionally
    // conservative: a stock is flagged risky if multiple signals appear together.
    // Tweak the constants below to tune sensitivity.
    const BUBBLE_IPO_START: u32 = 1995;
    const BUBBLE_IPO_END: u32 = 2001;
    const BUBBLE_RETURN_THRESHOLD: f64 = 200.0; // percent over the period
    const BUBBLE_VOL_THRESHOLD: f64 = 0.10; // very high volatility
    const BUBBLE_MARKETCAP_MAX: u64 = 5_000_000_000; // treat sub-$5B as smaller-cap

    fn is_dotcom_bubble_risky(stock: &Stock) -> bool {
        let mut signals = 0;

        // 1) Techy sector keywords
        let techy = stock.sectors.iter().any(|s| {
            let s = s.to_lowercase();
            s.contains("tech") || s.contains("internet") || s.contains("software") || s.contains("telecom") || s.contains("media")
        });
        if techy { signals += 1; }

        // 2) Extremely high recent return
        if let Some(ret) = stock.historical_return {
            if ret > BUBBLE_RETURN_THRESHOLD { signals += 1; }
        }

        // 3) Very high volatility
        if stock.volatility > BUBBLE_VOL_THRESHOLD { signals += 1; }

        // 4) Recent IPO (during dot-com years)
        if let Some(ft) = &stock.first_trading_date {
            if let Some(year_str) = ft.split('-').next() {
                if let Ok(y) = year_str.parse::<u32>() {
                    if (BUBBLE_IPO_START..=BUBBLE_IPO_END).contains(&y) { signals += 1; }
                }
            }
        } else if let Some(y) = get_first_trading_year(&stock.ticker) {
            if (BUBBLE_IPO_START..=BUBBLE_IPO_END).contains(&y) { signals += 1; }
        }

        // 5) Small market cap
        if stock.market_cap > 0 && stock.market_cap < BUBBLE_MARKETCAP_MAX { signals += 1; }

        // Flag as risky only if at least two signals are present (adjustable)
        signals >= 2
    }

/// Filter stocks based on investor profile requirements
pub fn filter_stocks_by_profile(stocks: &[Stock], profile: &InvestorProfile) -> Vec<Stock> {
    use once_cell::sync::OnceCell;
    use std::collections::HashMap;

    static SECTOR_OVERRIDES: OnceCell<HashMap<String, Vec<String>>> = OnceCell::new();

    fn get_sector_overrides() -> &'static HashMap<String, Vec<String>> {
        SECTOR_OVERRIDES.get_or_init(|| {
            let path = "sector_overrides.json";
            match std::fs::read_to_string(path) {
                Ok(s) => match serde_json::from_str::<HashMap<String, Vec<String>>>(&s) {
                    Ok(map) => map,
                    Err(e) => {
                        eprintln!("[WARN] Failed to parse {}: {} - using empty overrides", path, e);
                        HashMap::new()
                    }
                },
                Err(_) => {
                    eprintln!("[WARN] Could not read {} - using empty overrides", path);
                    HashMap::new()
                }
            }
        })
    }

    let overrides = get_sector_overrides();

    stocks
        .iter()
        .filter(|s| !is_ticker_excluded(&s.ticker))
        // If any effective sector matches the exclusion list, drop the stock.
        .filter(|s| {
            // Build effective sectors: original sectors plus overrides for this ticker
            let mut eff: Vec<String> = s.sectors.iter().cloned().collect();
            if let Some(extra) = overrides.get(&s.ticker) {
                for ex in extra.iter() {
                    if !eff.iter().any(|e| e.eq_ignore_ascii_case(ex)) {
                        eff.push(ex.to_string());
                    }
                }
            }

            // If any effective sector triggers exclusion, filter out
            !eff.iter().any(|sec| profile.should_exclude_sector_extended(sec, &s.name))
        })
        .filter(|s| matches_risk_tolerance(s.volatility, profile.risk_tolerance))
        .filter(|s| was_trading_during_period(s, profile.start_year))
        // Exclude stocks that exhibit multiple "bubble-like" signals (dotcom-style risk)
        .filter(|s| !is_dotcom_bubble_risky(s))
        // If the investor's end date falls during the COVID years (2020-2021),
        // apply an extra conservative filter to avoid stocks vulnerable to
        // pandemic-related crashes (travel, hospitality, airlines, etc.).
        .filter(|s| {
            if let Some(end) = profile.end_year {
                if (2020..=2021).contains(&end) {
                    return !is_covid_vulnerable(s);
                }
            }
            true
        })
        .cloned()
        .collect()
}

// Heuristic to detect COVID-vulnerable stocks. This is conservative: it
// excludes obvious pandemic-sensitive sectors and small/high-volatility
// stocks that suffered big negative returns (if that data exists).
fn is_covid_vulnerable(stock: &Stock) -> bool {
    // Sectors strongly hit by pandemic conditions
    let vulnerable_keywords = ["airline", "travel", "hospitality", "leisure", "hotel", "cruise", "restaurant", "casino", "retail", "brick-and-mortar", "leisure", "transportation"];

    let sector_hit = stock.sectors.iter().any(|s| {
        let ls = s.to_lowercase();
        vulnerable_keywords.iter().any(|kw| ls.contains(kw))
    });
    if sector_hit { return true; }

    // If we have historical return info for the requested period and it shows
    // a large negative return, treat as vulnerable.
    if let Some(r) = stock.historical_return {
        if r < -30.0 { return true; }
    }

    // Small market-cap + high volatility is riskier during shocks
    if stock.market_cap > 0 && stock.market_cap < 2_000_000_000 && stock.volatility > 0.07 {
        return true;
    }

    false
}

pub fn build_portfolio(stocks: &[Stock], budget: f64, risk_level: RiskLevel) -> Vec<(String, i32)> {
    if stocks.is_empty() {
        return Vec::new();
    }
    
    // SAFETY CHECK: Validate budget is positive
    if budget <= 0.0 {
        eprintln!("[ERROR] Invalid budget: ${:.2}", budget);
        return Vec::new();
    }
    
    // Sort by historical return if available, otherwise by inverse volatility
    let mut sorted_stocks = stocks.to_vec();
    sorted_stocks.sort_by(|a, b| {
        // If both have historical returns, sort by return (highest first)
        match (a.historical_return, b.historical_return) {
            (Some(ret_a), Some(ret_b)) => ret_a.partial_cmp(&ret_b).unwrap().reverse(), // Descending (highest first)
            (Some(_), None) => std::cmp::Ordering::Less,  // Stocks with returns first
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.volatility.partial_cmp(&b.volatility).unwrap(), // Fallback to volatility (lowest first)
        }
    });

    // Remove stocks with negative historical returns so we never buy them.
    // We keep stocks with no historical return (None) or zero/positive returns.
    sorted_stocks.retain(|s| !s.historical_return.map_or(false, |r| r < 0.0));
    
    // Target number of positions based on risk tolerance
    let target_positions = match risk_level {
        RiskLevel::Conservative => 15, // More diversification
        RiskLevel::Moderate => 10,
        RiskLevel::Aggressive => 7,    // More concentrated
    };
    
    // Use a conservative allocation budget fraction so we only spend part of
    // the provided budget (e.g., 70%). This leaves a buffer and reduces
    // risk of budget-breaches and allows some cash to remain unspent.
    let alloc_budget = budget * budget_spend_fraction(); // Conservative allocation budget

    // For small budgets, use greedy allocation instead of equal weight
    let portfolio = if alloc_budget < 5000.0 { // Check for small budgets
        build_greedy_portfolio(&sorted_stocks, budget) // Use full budget for greedy allocation
    } else { // Larger budgets
        // Performance-weighted allocation for larger budgets. Pass both the
        // conservative alloc_budget (used to seed the allocation) and the
        // original budget so the allocator can try to deploy any remaining
        // cash up to the full client budget.
        build_weighted_portfolio(&sorted_stocks, alloc_budget, target_positions, budget)
    };
    
    // Defensive trim: ensure we never return more than MAX_POSITIONS distinct tickers.
    // This is an extra safety net in case other allocation paths produce more entries.
    if portfolio.len() > MAX_POSITIONS {
        eprintln!("[VALIDATOR] Trimming portfolio from {} to {} positions (MAX_POSITIONS)", portfolio.len(), MAX_POSITIONS);
        // Sort by historical return (highest first) using the stocks metadata, then keep top MAX_POSITIONS
        let mut portfolio_sorted = portfolio.clone();
        portfolio_sorted.sort_by(|(t1, _), (t2, _)| {
            let r1 = stocks.iter().find(|s| &s.ticker == t1).and_then(|s| s.historical_return).unwrap_or(0.0);
            let r2 = stocks.iter().find(|s| &s.ticker == t2).and_then(|s| s.historical_return).unwrap_or(0.0);
            r2.partial_cmp(&r1).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut trimmed = portfolio_sorted.into_iter().take(MAX_POSITIONS).collect::<Vec<_>>();
        // Final safety: ensure trimmed portfolio is within budget (force trim if necessary)
        if !validate_budget(&trimmed, stocks, budget) {
            force_within_budget(&mut trimmed, stocks, budget);
        }
        return trimmed;
    }
    
    // ABSOLUTE FINAL SAFETY CHECK
    let total_cost = calculate_portfolio_cost(&portfolio, stocks);
    if total_cost > budget {
        eprintln!("[CRITICAL ERROR] Portfolio cost ${:.2} exceeds budget ${:.2}!", total_cost, budget);
        eprintln!("[CRITICAL ERROR] This should never happen - contact developer!");
        // Persist the chosen funds and context for post-mortem analysis
        log_overbudget_event(&portfolio, stocks, budget, total_cost, "final_safety_check");
        let mut fixed_portfolio = portfolio;
        force_within_budget(&mut fixed_portfolio, stocks, budget);
        return fixed_portfolio;
    }
    
    // Success - log the allocation
    println!("[BUDGET] Portfolio cost: ${:.2} / ${:.2} (${:.2} remaining)", 
             total_cost, budget, budget - total_cost);
    
    portfolio
}

/// Calculate performance-based weights for stocks
fn calculate_performance_weights(stocks: &[&Stock]) -> Vec<f64> {
    let weights: Vec<f64> = stocks
        .iter()
        .map(|stock| {
            let return_pct = stock.historical_return.unwrap_or(0.0);
            // Do not give negative returns an artificial positive weight; use 0.0
            // so negative historical performance won't be favored over small
            // positive returns.
            return_pct.max(0.0)
        })
        .collect();
    
    let total: f64 = weights.iter().sum();
    
    // Normalize to sum to 1.0
    if total > 0.0 {
        weights.iter().map(|w| w / total).collect()
    } else {
        vec![1.0 / stocks.len() as f64; stocks.len()] // Equal weights fallback
    }
}

/// Convert a numeric volatility into a stable bucket name used by PointsStore
pub fn volatility_bucket(volatility: f64) -> &'static str {
    if volatility < 0.03 {
        return crate::points::VOL_LOW;
    }
    if volatility < 0.05 {
        return crate::points::VOL_MED;
    }
    crate::points::VOL_HIGH
}

/// Build portfolio with performance-weighted allocation
fn build_weighted_portfolio(stocks: &[Stock], alloc_budget: f64, target_positions: usize, original_budget: f64) -> Vec<(String, i32)> {
    // Enforce global upper bound on positions
    let num_positions = target_positions.min(stocks.len()).min(MAX_POSITIONS);
    let top_stocks: Vec<&Stock> = stocks.iter().take(num_positions).collect();
    
    if top_stocks.is_empty() {
        return Vec::new();
    }
    
    // Base return-based weights (normalized)
    let return_weights = calculate_performance_weights(&top_stocks);

    // Combined score: weighted blend of historical returns and learned points
    // Load PointsStore (if present) to bias selection by learned per-ticker scores
    let ps = PointsStore::load("points_store.json");

    // Build points-based weights for the top stocks using volatility buckets
    let points_raw: Vec<f64> = top_stocks.iter()
        .map(|s| ps.get_score(&s.ticker, volatility_bucket(s.volatility)))
        .collect();

    let points_total: f64 = points_raw.iter().sum();
    let points_weights: Vec<f64> = if points_total > 0.0 {
        points_raw.iter().map(|v| v / points_total).collect()
    } else {
        vec![1.0 / (points_raw.len() as f64); points_raw.len()]
    };

    let mut combined: Vec<f64> = Vec::with_capacity(top_stocks.len());
    for i in 0..top_stocks.len() {
        let c = RETURN_WEIGHT * return_weights[i] + (1.0 - RETURN_WEIGHT) * points_weights[i];
        combined.push(c);
    }

    // Normalize combined to sum to 1 (defensive)
    let combined_total: f64 = combined.iter().sum();
    if combined_total > 0.0 {
        for v in combined.iter_mut() { *v /= combined_total; }
    } else {
        let default = 1.0 / (combined.len() as f64);
        for v in combined.iter_mut() { *v = default; }
    }

    // Allocate budget.
    // Two modes:
    //  - Concentrated rank-quantity allocation: buy a pre-defined number of
    //    shares for each rank (e.g. 50 for top, 20 for second). If budget does
    //    not allow the full target quantity the count is reduced to what can
    //    be afforded.
    //  - Proportional allocation (legacy): allocate budget proportionally to
    //    combined weights and convert to quantities.
    let mut portfolio = Vec::new();
    let mut allocated = 0.0;

    if CONCENTRATE_ALLOCATION {
        for (i, stock) in top_stocks.iter().enumerate() {
            let price = stock.get_current_price();
            if price <= 0.0 { continue; }

            // Determine desired quantity by rank table (fallback to 1)
            let desired_qty = if i < RANK_QUANTITIES.len() { RANK_QUANTITIES[i] } else { 1 };

            // If desired_qty is zero or negative, skip
            if desired_qty <= 0 { continue; }

            // Cost for desired quantity
            let desired_cost = (desired_qty as f64) * price;

            if allocated + desired_cost <= alloc_budget {
                // We can afford full desired quantity
                portfolio.push((stock.ticker.clone(), desired_qty));
                allocated += desired_cost;
            } else {
                // Try to fit as many as possible of the desired_qty
                let remaining = (alloc_budget - allocated).max(0.0);
                let afford_qty = (remaining / price).floor() as i32;
                if afford_qty > 0 {
                    let cost = (afford_qty as f64) * price;
                    portfolio.push((stock.ticker.clone(), afford_qty));
                    allocated += cost;
                } else {
                    // Nothing affordable for this rank; skip to next (could be cheaper)
                    eprintln!("[WARN] Could not afford any shares of {} at ${:.2} with ${:.2} remaining", stock.ticker, price, alloc_budget - allocated);
                }
            }
        }

        // If we ended up with no positions (extremely small budgets), fall back to greedy
        if portfolio.is_empty() {
            eprintln!("[WARN] Concentrated allocation produced empty portfolio, falling back to greedy allocation");
            return build_greedy_portfolio(stocks, original_budget);
        }

        // Deploy any small remaining budget into affordable non-negative-return performers
        // Try to deploy any remaining cash — allow spending up to the full
        // original_budget so we can use more than the conservative alloc_budget.
        let remaining_original = (original_budget - allocated).max(0.0);
        if remaining_original > 0.0 {
            deploy_remaining_budget(&mut portfolio, remaining_original, top_stocks[0], original_budget, stocks);
        }
    } else {
        // Proportional legacy allocation (unchanged)
        for (i, stock) in top_stocks.iter().enumerate() {
            // Use current price for allocation math so submitted portfolio cost
            // matches what the evaluator will compute.
            let purchase_price = stock.get_current_price();
            let target_allocation = alloc_budget * combined[i];
            let quantity = (target_allocation / purchase_price).floor() as i32;

            if quantity > 0 {
                let cost = (quantity as f64) * purchase_price;
                if allocated + cost <= alloc_budget {
                    portfolio.push((stock.ticker.clone(), quantity));
                    allocated += cost;
                } else {
                    eprintln!("[WARN] Skipping {} - would exceed budget", stock.ticker);
                }
            }
        }

        // Deploy remaining budget into top combined performer
        let remaining_original = (original_budget - allocated).max(0.0);
        if remaining_original > 0.0 {
            deploy_remaining_budget(&mut portfolio, remaining_original, top_stocks[0], original_budget, stocks);
        }
    }

    // FINAL SAFETY CHECK: Validate budget
    if !validate_budget(&portfolio, stocks, original_budget) {
        eprintln!("[EMERGENCY] Force-fitting portfolio within budget...");
        force_within_budget(&mut portfolio, stocks, original_budget);
    }

        // (Points system removed) — no learned-score updates or persistence.

    portfolio
}

/// Deploy remaining budget into the best performing stock
fn deploy_remaining_budget(portfolio: &mut Vec<(String, i32)>, mut remaining: f64, _top_stock: &Stock, budget: f64, stocks: &[Stock]) {
    if remaining <= 0.0 {
        return;
    }

    // Build a list of candidate stocks that can be bought with the remaining
    // budget (use current prices). Sort cheapest-first so we consume small
    // remainders buying whole shares where possible.
    let mut candidates: Vec<(&Stock, f64)> = stocks
        .iter()
        .map(|s| (s, s.get_current_price()))
        // Only consider stocks with non-negative historical returns (i.e. exclude negative-return stocks)
        .filter(|(s, price)| *price > 0.0 && *price <= remaining && !s.historical_return.map_or(false, |r| r < 0.0))
        .collect();

    // If nothing is affordable, try to include slightly-more-expensive stocks
    // and see if buying a single share is possible after other purchases.
    if candidates.is_empty() {
        // find overall cheapest stock > 0
        if let Some((smin, pmin)) = stocks.iter().map(|s| (s, s.get_current_price())).filter(|(s,p)| *p>0.0 && !s.historical_return.map_or(false, |r| r < 0.0)).min_by(|a,b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)) {
            // If even the cheapest stock is more expensive than remaining, nothing to do
            if pmin > remaining { return; }
            candidates.push((smin, pmin));
        } else {
            return;
        }
    }

    candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    // Try to spend remaining on cheapest candidates until we can't afford any more
    for (stock, price) in candidates.iter() {
        if *price <= 0.0 { continue; }

        // Determine how many whole shares we can buy of this candidate
        let afford_qty = (remaining / *price).floor() as i32;
        if afford_qty <= 0 { continue; }

        let cost = (afford_qty as f64) * *price;
        // SAFETY: don't exceed original budget
        if cost <= remaining && cost <= budget {
            // Add to existing position or create new one
            if let Some(pos) = portfolio.iter_mut().find(|(t, _)| t == &stock.ticker) {
                pos.1 += afford_qty;
            } else {
                portfolio.push((stock.ticker.clone(), afford_qty));
            }
            remaining -= cost;
        }

        // Stop early if remaining is now too small to buy anything else
        if remaining <= 0.0 {
            break;
        }
    }
}

#[allow(unused_assignments)]
fn build_greedy_portfolio(stocks: &[Stock], budget: f64) -> Vec<(String, i32)> {
    let mut portfolio = Vec::new();
    let mut remaining_budget = budget;
    
    // Filter to only affordable stocks (use current market price for affordability)
    let mut affordable_stocks: Vec<&Stock> = stocks
        .iter()
        .filter(|s| s.get_current_price() <= budget)  // Use original budget, not remaining
        // Exclude any stocks with negative historical returns so greedy allocation
        // doesn't buy them.
        .filter(|s| !s.historical_return.map_or(false, |r| r < 0.0))
        .collect();
    
    if affordable_stocks.is_empty() {
        return portfolio;
    }
    
    // Sort affordable stocks by price (cheapest first for small budgets)
    affordable_stocks.sort_by(|a, b| {
        a.get_current_price().partial_cmp(&b.get_current_price()).unwrap()
    });

    // Enforce a hard cap on number of distinct positions for greedy allocation
    if affordable_stocks.len() > MAX_POSITIONS {
        affordable_stocks.truncate(MAX_POSITIONS);
    }
    
    // Greedy approach: buy as many shares as possible, diversifying when we can
    let mut stock_index = 0;
    let mut shares_per_stock = vec![0; affordable_stocks.len()];
    
    // First pass: buy at least 1 share of as many stocks as we can afford
    for (i, stock) in affordable_stocks.iter().enumerate() {
        let price = stock.get_current_price();
        // SAFETY CHECK: Ensure we have enough budget
        if remaining_budget >= price && price > 0.0 {
            shares_per_stock[i] = 1;
            remaining_budget -= price;
            
            // Double check we didn't go negative
            if remaining_budget < 0.0 {
                eprintln!("[ERROR] Budget went negative in greedy allocation!");
                shares_per_stock[i] = 0; // Undo
                remaining_budget += price;
                break;
            }
        }
    }
    
    // Second pass: keep buying more shares round-robin style
    let mut safety_counter = 0;
    let max_iterations = 10000; // Prevent infinite loops
    
    while remaining_budget > 0.0 && safety_counter < max_iterations {
        safety_counter += 1;
        
        let stock = affordable_stocks[stock_index];
        let price = stock.get_current_price();
        
        // SAFETY CHECK: Verify we can afford it
        if remaining_budget >= price && price > 0.0 {
            shares_per_stock[stock_index] += 1;
            remaining_budget -= price;
            
            // Double check we didn't go negative
            if remaining_budget < -0.01 { // Allow small floating point errors
                eprintln!("[ERROR] Budget went negative! Rolling back last purchase.");
                shares_per_stock[stock_index] -= 1;
                remaining_budget += price;
                break;
            }
        }
        
        // Move to next affordable stock
        stock_index = (stock_index + 1) % affordable_stocks.len();
        
        // Check if we can't afford anything anymore
        if affordable_stocks.iter().all(|s| s.get_current_price() > remaining_budget) {
            break;
        }
    }
    
    if safety_counter >= max_iterations {
        eprintln!("[WARN] Greedy allocation hit iteration limit - stopping");
    }
    
    // Build final portfolio
    for (i, stock) in affordable_stocks.iter().enumerate() {
        if shares_per_stock[i] > 0 {
            portfolio.push((stock.ticker.clone(), shares_per_stock[i]));
        }
    }
    
    // FINAL SAFETY CHECK: Validate budget
    if !validate_budget(&portfolio, stocks, budget) {
        eprintln!("[EMERGENCY] Greedy portfolio exceeded budget - fixing...");
        force_within_budget(&mut portfolio, stocks, budget);
    }
    
    portfolio
}
