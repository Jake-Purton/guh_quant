mod investor;
mod stocks;
mod portfolio;

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use std::error::Error;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

use investor::InvestorProfile;
use stocks::{prefetch_all_stocks, update_stock_prices, fetch_historical_returns};
use portfolio::{filter_stocks_by_profile, build_portfolio};

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
                    eprintln!("⚠️  Network error (attempt {}): {}. Retrying...", attempt, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err("Failed after 3 attempts".into())
}

async fn get_my_current_information() -> Result<String, Box<dyn Error>> {
    send_get_request("/info").await
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
    // Load initial stock data from cache
    println!("🚀 Loading initial stock data...");
    let initial_stocks = prefetch_all_stocks().await?;
    
    // Create shared state with Arc + RwLock for thread-safe access
    let stocks_cache = Arc::new(RwLock::new(initial_stocks));
    
    // Spawn background task to update prices every 60 seconds
    let stocks_cache_clone = Arc::clone(&stocks_cache);
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(60)).await;
            
            let mut stocks = stocks_cache_clone.write().await;
            println!("⏰ Background: Updating stock prices...");
            if let Err(e) = update_stock_prices(&mut stocks).await {
                eprintln!("⚠️  Background price update failed: {}", e);
            }
        }
    });
    
    println!("✅ Background price updater started (updates every 60s)\n");

    loop {
        // Get team info
        // match get_my_current_information().await {
        //     Ok(info) => println!("Team information: {}", info),
        //     Err(e) => println!("Error: {}", e),
        // }
    
        // Get and parse context
        let context = get_context().await?;
        println!("Context provided: {}", context);
        
        if let Ok(profile) = InvestorProfile::from_context(&context) {

            println!("\n📊 Investor Profile:");
            println!("  Name: {}", profile.name);
            println!("  Age: {} ({:?})", profile.age, profile.risk_tolerance);
            println!("  Budget: ${:.2}", profile.budget);
            println!("  Excluded: {:?}", profile.excluded_sectors);
        
            // Get current stocks from shared cache (instant read)
            let mut all_stocks = stocks_cache.read().await.clone();
            
            // Fetch historical returns for the investment period if we have dates
            if let (Some(start_year), Some(end_year)) = (profile.start_year, profile.end_year) {
                // Construct date strings from the profile
                // Use approximate dates if exact dates aren't available
                let start_date = format!("{}-01-01", start_year);
                let end_date = format!("{}-12-31", end_year);
                
                println!("📊 Fetching historical returns ({} to {})...", start_date, end_date);
                if let Err(e) = fetch_historical_returns(&mut all_stocks, &start_date, &end_date).await {
                    eprintln!("⚠️  Could not fetch historical returns: {}", e);
                }
            }
            
            // Filter by investor profile
            let eligible_stocks = filter_stocks_by_profile(&all_stocks, &profile);
            // println!("📋 Eligible stocks after filtering: {}", eligible_stocks.len());
            
            if eligible_stocks.is_empty() {
                return Err("No eligible stocks found!".into());
            }
        
            // Build portfolio - use FULL budget, not just stock allocation
            let portfolio = build_portfolio(
                &eligible_stocks,
                profile.budget,  // Use full budget
                profile.risk_tolerance
            );
            
            // println!("\n💼 Proposed Portfolio:");
            let mut total_cost = 0.0;
            for (ticker, qty) in &portfolio {
                let stock = eligible_stocks.iter().find(|s| s.ticker == *ticker).unwrap();
                let cost = stock.price * (*qty as f64);
                total_cost += cost;
                println!(
                    "  {} x{} @ ${:.2} = ${:.2} (vol: {:.2}%)",
                    ticker, qty, stock.price, cost, stock.volatility * 100.0
                );
            }
            println!("  Total: ${:.2} / ${:.2}", total_cost, profile.budget);
        
            // Convert to required format
            let portfolio_refs: Vec<(&str, i32)> = portfolio
                .iter()
                .map(|(t, q)| (t.as_str(), *q))
                .collect();
        
            // Submit portfolio
            match send_portfolio(portfolio_refs).await {
                Ok(response) => println!("\n✅ Evaluation: {}", response),
                Err(e) => println!("❌ Error: {}", e),
            }
        } else {
            println!("error in profile skipping")
        }
    }

    Ok(())
}