mod investor;
mod stocks;
mod portfolio;

use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use std::error::Error;

use investor::InvestorProfile;
use stocks::prefetch_all_stocks;
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
    send_get_request("/request").await
}

async fn get_my_current_information() -> Result<String, Box<dyn Error>> {
    send_get_request("/info").await
}

async fn send_portfolio(weighted_stocks: Vec<(&str, i32)>) -> Result<String, Box<dyn Error>> {
    let data: Vec<Value> = weighted_stocks
        .into_iter()
        .map(|(ticker, quantity)| json!({ "ticker": ticker, "quantity": quantity }))
        .collect();
    send_post_request("/submit", &json!(data)).await
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {

    loop {

        // Get team info
        match get_my_current_information().await {
            Ok(info) => println!("Team information: {}", info),
            Err(e) => println!("Error: {}", e),
        }
    
        // Get and parse context
        let context = get_context().await?;
        // println!("Context provided: {}", context);
        
        let profile = InvestorProfile::from_context(&context)?;
        // println!("\n📊 Investor Profile:");
        // println!("  Name: {}", profile.name);
        // println!("  Age: {} ({:?})", profile.age, profile.risk_tolerance);
        // println!("  Budget: ${:.2}", profile.budget);
        // println!("  Stock allocation: {:.0}%", profile.stock_allocation_pct() * 100.0);
        // println!("  Excluded: {:?}", profile.excluded_sectors);
        // println!("  Stock budget: ${:.2}", profile.stock_budget());
    
        // Pre-fetch all stock data
        let all_stocks = prefetch_all_stocks().await?;
        
        // Filter by investor profile
        let eligible_stocks = filter_stocks_by_profile(&all_stocks, &profile);
        // println!("📋 Eligible stocks after filtering: {}", eligible_stocks.len());
        
        if eligible_stocks.is_empty() {
            return Err("No eligible stocks found!".into());
        }
    
        // Build portfolio
        let portfolio = build_portfolio(
            &eligible_stocks,
            profile.stock_budget(),
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
        println!("  Total: ${:.2} / ${:.2}", total_cost, profile.stock_budget());
    
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
    }

    Ok(())
}