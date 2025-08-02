use super::transactions::SwapTransaction;

/// Checks if the tokens in the swap transactions are reversed,
/// for example buying first and selling second.
/// It supports economically equivalent tokens (e.g., USDC/USDT, ETH/WETH).
pub fn are_tokens_reversed(a: &SwapTransaction, b: &SwapTransaction) -> bool {
    return are_tokens_equivalent(&a.token_in, &b.token_out)
        && are_tokens_equivalent(&a.token_out, &b.token_in);
}

/// Check if two tokens are economically equivalent
pub fn are_tokens_equivalent(token_a: &str, token_b: &str) -> bool {
    get_token_equivalence_group(token_a) == get_token_equivalence_group(token_b)
}

/// Token equivalence groups for cross-token sandwich detection
///
/// TODO: Certainly there could be more equivalent tokens out there.
fn get_token_equivalence_group(token: &str) -> &str {
    match token {
        // Stablecoins - all ~$1 USD
        "USDC" | "USDT" | "DAI" | "FRAX" | "BUSD" => "STABLECOINS",
        // ETH variants
        "ETH" | "WETH" | "stETH" => "ETH_GROUP",
        // Bitcoin variants
        "WBTC" | "renBTC" | "sBTC" => "BTC_GROUP",
        // Everything else is its own group
        _ => token,
    }
}
