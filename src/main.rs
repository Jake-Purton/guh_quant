mod investor;
mod stocks;
mod portfolio;
mod points;

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use std::error::Error;

use investor::InvestorProfile;
use stocks::{Stock, prefetch_all_stocks, fetch_historical_returns};
use portfolio::{filter_stocks_by_profile, build_portfolio};
use std::collections::HashMap;

const URL: &str = "http://www.prism-challenge.com";
const PORT: u16 = 8082;
const TEAM_API_CODE: &str = "f7f47b3680640b753e6cccfd14bbca89";

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
                    eprintln!("[WARN] Network error (attempt {}): {}. Retrying...", attempt, e);
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
                if let Err(e) = fetch_historical_returns(&mut all_stocks, &start, &end).await {
                    eprintln!("[WARN] Could not fetch historical returns: {}", e);
                }
            }
            
            // Filter by investor profile
            let eligible_stocks = filter_stocks_by_profile(&all_stocks, &profile);
            println!("[FILTER] Eligible stocks after filtering: {} (from {} total)", eligible_stocks.len(), all_stocks.len());
            
            if eligible_stocks.is_empty() {
                return Err("No eligible stocks found!".into());
            }
        
            // Build portfolio based on interpolated/cached data
            let portfolio = build_portfolio(
                &eligible_stocks,
                profile.budget,
                profile.risk_tolerance
            );
            
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
                print_portfolio_and_submit(&cleaned, &eligible_stocks, &profile).await?;
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
    profile: &InvestorProfile
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

    // Submit portfolio
    match send_portfolio(portfolio_refs).await {
        Ok(response) => println!("\n[SUCCESS] Evaluation: {}", response),
        Err(e) => println!("[ERROR] {}", e),
    }
    
    Ok(())
}

/// Pre-submit validator: remove unknown tickers and force portfolio within budget.
fn pre_submit_validate(
    portfolio: &[(String, i32)],
    eligible_stocks: &[Stock],
    budget: f64,
) -> Vec<(String, i32)> {
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

    // Compute current total cost
    let mut total: f64 = cleaned.iter().map(|(t, q)| price_map.get(t).unwrap() * (*q as f64)).sum();

    // If already within budget, return as-is
    if total <= budget { return cleaned; }

    eprintln!("[VALIDATOR] Portfolio exceeds budget before submit: ${:.2} > ${:.2}. Reducing...", total, budget);

    // Sort positions by price descending (drop most expensive shares first)
    cleaned.sort_by(|a, b| {
        let pa = *price_map.get(&a.0).unwrap_or(&0.0);
        let pb = *price_map.get(&b.0).unwrap_or(&0.0);
        pb.partial_cmp(&pa).unwrap()
    });

    // Iteratively reduce quantities from the most expensive position until under budget
    let mut idx = 0;
    while total > budget && !cleaned.is_empty() {
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
            cleaned.remove(idx);
        }
    }

    eprintln!("[VALIDATOR] Reduced portfolio cost to ${:.2}", total);
    cleaned
}