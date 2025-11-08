use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde_json::{json, Value};
use std::error::Error;

const URL: &str = "http://www.prism-challenge.com";
const PORT: u16 = 8082;
const TEAM_API_CODE: &str = "input your api code here";
// const TEAM_API_CODE: &str = "d3e63892502a2bf7839f3dc7b0f26801";


async fn send_get_request(path: &str) -> Result<String, Box<dyn Error>> {
    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert("X-API-Code", HeaderValue::from_static(TEAM_API_CODE));

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
    headers.insert("X-API-Code", HeaderValue::from_static(TEAM_API_CODE));
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
    // Get team info
    match get_my_current_information().await {
        Ok(info) => println!("Team information: {}", info),
        Err(e) => println!("Error: {}", e),
    }

    // Get context
    match get_context().await {
        Ok(context) => println!("Context provided: {}", context),
        Err(e) => println!("Error: {}", e),
    }

    // Example portfolio submission
    let portfolio = vec![("AAPL", 10000), ("MSFT", 1)];
    match send_portfolio(portfolio).await {
        Ok(response) => println!("Evaluation response: {}", response),
        Err(e) => println!("Error: {}", e),
    }

    Ok(())
}
