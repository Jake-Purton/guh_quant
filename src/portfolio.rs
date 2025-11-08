use crate::investor::{InvestorProfile, RiskLevel};
use crate::stocks::Stock;

pub fn filter_stocks_by_profile(stocks: &[Stock], profile: &InvestorProfile) -> Vec<Stock> {
    stocks
        .iter()
        .filter(|s| !profile.should_exclude_sector(&s.sector))
        .filter(|s| {
            // Filter by risk tolerance
            match profile.risk_tolerance {
                RiskLevel::Conservative => s.volatility < 0.03, // Low volatility only
                RiskLevel::Moderate => s.volatility < 0.05,     // Medium volatility
                RiskLevel::Aggressive => true,                   // All stocks ok
            }
        })
        .filter(|s| {
            // Filter by IPO date - stock must have been trading during investment period
            if let (Some(start_year), Some(ipo_year)) = (profile.start_year, s.ipo_year) {
                ipo_year <= start_year
            } else {
                true // If no date info, include it
            }
        })
        .cloned()
        .collect()
}

pub fn build_portfolio(stocks: &[Stock], budget: f64, risk_level: RiskLevel) -> Vec<(String, i32)> {
    if stocks.is_empty() {
        return Vec::new();
    }
    
    // Sort by inverse volatility (prefer stable stocks first)
    let mut sorted_stocks = stocks.to_vec();
    sorted_stocks.sort_by(|a, b| {
        a.volatility.partial_cmp(&b.volatility).unwrap()
    });
    
    // Target number of positions based on risk tolerance
    let target_positions = match risk_level {
        RiskLevel::Conservative => 15, // More diversification
        RiskLevel::Moderate => 10,
        RiskLevel::Aggressive => 7,    // More concentrated
    };
    
    // For small budgets, use greedy allocation instead of equal weight
    if budget < 5000.0 {
        return build_greedy_portfolio(&sorted_stocks, budget);
    }
    
    // Equal weight allocation for larger budgets
    let num_positions = target_positions.min(sorted_stocks.len());
    let allocation_per_stock = budget / num_positions as f64;
    let mut portfolio = Vec::new();
    
    for stock in sorted_stocks.iter().take(num_positions) {
        let quantity = (allocation_per_stock / stock.price).floor() as i32;
        if quantity > 0 {
            portfolio.push((stock.ticker.clone(), quantity));
        }
    }
    
    portfolio
}

fn build_greedy_portfolio(stocks: &[Stock], budget: f64) -> Vec<(String, i32)> {
    let mut portfolio = Vec::new();
    let mut remaining_budget = budget;
    
    // Filter to only affordable stocks (price <= budget)
    let mut affordable_stocks: Vec<&Stock> = stocks
        .iter()
        .filter(|s| s.price <= budget)  // Use original budget, not remaining
        .collect();
    
    if affordable_stocks.is_empty() {
        return portfolio;
    }
    
    // Sort affordable stocks by price (cheapest first for small budgets)
    affordable_stocks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
    
    // Greedy approach: buy as many shares as possible, diversifying when we can
    let mut stock_index = 0;
    let mut shares_per_stock = vec![0; affordable_stocks.len()];
    
    // First pass: buy at least 1 share of as many stocks as we can afford
    for (i, stock) in affordable_stocks.iter().enumerate() {
        if remaining_budget >= stock.price {
            shares_per_stock[i] = 1;
            remaining_budget -= stock.price;
        }
    }
    
    // Second pass: keep buying more shares round-robin style
    while remaining_budget > 0.0 {
        let stock = affordable_stocks[stock_index];
        if remaining_budget >= stock.price {
            shares_per_stock[stock_index] += 1;
            remaining_budget -= stock.price;
        }
        
        // Move to next affordable stock
        stock_index = (stock_index + 1) % affordable_stocks.len();
        
        // Check if we can't afford anything anymore
        if affordable_stocks.iter().all(|s| s.price > remaining_budget) {
            break;
        }
    }
    
    // Build final portfolio
    for (i, stock) in affordable_stocks.iter().enumerate() {
        if shares_per_stock[i] > 0 {
            portfolio.push((stock.ticker.clone(), shares_per_stock[i]));
        }
    }
    
    portfolio
}
