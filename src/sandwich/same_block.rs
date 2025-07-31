use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct SwapTransaction {
    pub tx_hash: String,
    pub block_number: u64,
    pub timestamp: u64,
    pub tx_position_in_block: u32,
    pub from_address: String,
    pub token_in: String,
    pub token_out: String,
    pub amount_in: f64,
    pub amount_out: f64,
    pub gas_price: u64,
    pub pool_address: String,
    pub token_launch_block: u64,
    pub is_contract_caller: bool,
    pub usd_value_in: f64,
    pub usd_value_out: f64,
    pub gas_cost_usd: f64,
}

#[derive(Debug, PartialEq)]
pub struct SandwichAttack {
    pub front_run_tx: SwapTransaction,
    pub victim_tx: SwapTransaction,
    pub back_run_tx: SwapTransaction,
    pub confidence_score: f32,
}

/// Find same block sandwich attacks in a list of swap transactions.
///
/// First we group transactions by their block number, sorting them by position within the block.
/// Then we find sandwiches within each block.
pub fn find_same_block_sandwiches(transactions: &[SwapTransaction]) -> Vec<SandwichAttack> {
    let mut attacks = Vec::new();
    let transactions_by_block = group_transactions_by_block(transactions);

    for (_block_number, block_transactions) in transactions_by_block {
        let block_attacks = find_sandwiches_in_block(&block_transactions);
        match block_attacks {
            Ok(block_attacks) => attacks.extend(block_attacks),
            Err(err) => println!("Error finding sandwiches: {}", err),
        }
    }

    return attacks;
}

/// Groups transactions by their block number, sorting them by position within the block.
fn group_transactions_by_block(
    transactions: &[SwapTransaction],
) -> HashMap<u64, Vec<SwapTransaction>> {
    let mut grouped = HashMap::new();

    for tx in transactions {
        grouped
            .entry(tx.block_number)
            .or_insert_with(Vec::new)
            .push(tx.clone());
    }

    for txs in grouped.values_mut() {
        txs.sort_by_key(|tx| tx.tx_position_in_block);
    }

    return grouped;
}

/// Go through the given swap transactions (assumed to be in the same block)
/// and find any sandwich attacks.
fn find_sandwiches_in_block(
    transactions: &[SwapTransaction],
) -> Result<Vec<SandwichAttack>, String> {
    let mut attacks = Vec::new();

    if transactions.len() < 3 {
        return Err("not enough transactions to have a sandwich".to_string());
    }

    for front_pos in 0..transactions.len() - 2 {
        let front_tx = &transactions[front_pos];

        for back_pos in front_pos + 2..transactions.len() {
            let back_tx = &transactions[back_pos];

            if front_tx.from_address != back_tx.from_address {
                continue;
            }

            if !are_tokens_reversed(front_tx, back_tx) {
                continue;
            }

            for victim_pos in front_pos + 1..back_pos {
                let victim_tx = &transactions[victim_pos];

                if is_sandwich_pattern(front_tx, victim_tx, back_tx) {
                    attacks.push(SandwichAttack {
                        front_run_tx: front_tx.clone(),
                        victim_tx: victim_tx.clone(),
                        back_run_tx: back_tx.clone(),
                        confidence_score: calculate_sandwich_confidence(
                            front_tx, victim_tx, back_tx,
                        ),
                    });
                }
            }
        }
    }

    Ok(attacks)
}

/// Checks if the tokens in the swap transactions are reversed,
/// for example buying first and selling second.
/// It supports economically equivalent tokens (e.g., USDC/USDT, ETH/WETH).
fn are_tokens_reversed(a: &SwapTransaction, b: &SwapTransaction) -> bool {
    return are_tokens_equivalent(&a.token_in, &b.token_out)
        && are_tokens_equivalent(&a.token_out, &b.token_in);
}

/// A rudimentary sandwich pattern detection function.
/// It assumes the transactions are in the correct order (front, victim, back).
///
/// Returning `true` doesn't mean it was a (profitable) sandwich attack,
/// but it means the swap directions are there.
fn is_sandwich_pattern(
    front: &SwapTransaction,
    victim: &SwapTransaction,
    back: &SwapTransaction,
) -> bool {
    // Should be same attacker
    if front.from_address != back.from_address {
        return false;
    }

    // Attacker should not be victim
    if front.from_address == victim.from_address {
        return false;
    }

    // Attacker should have gotten equivalent token back
    if !are_tokens_equivalent(&front.token_in, &back.token_out) {
        return false;
    }

    // Front and victim should be same token direction (attacker buys before victim)
    if !are_tokens_equivalent(&front.token_in, &victim.token_in)
        || !are_tokens_equivalent(&front.token_out, &victim.token_out)
    {
        return false;
    }

    // Victim and back should be different token direction (attacker sells back to victim)
    if are_tokens_equivalent(&victim.token_in, &back.token_in)
        && are_tokens_equivalent(&victim.token_out, &back.token_out)
    {
        return false;
    }

    return true;
}

/// Token equivalence groups for cross-token sandwich detection
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

/// Check if two tokens are economically equivalent
fn are_tokens_equivalent(token_a: &str, token_b: &str) -> bool {
    get_token_equivalence_group(token_a) == get_token_equivalence_group(token_b)
}

/// Takes 3 swap transactions which have already been validated to have
/// a sandwich pattern and calculates the confidence that the attacker
/// is a MEV sandwich bot.
///
/// The base confidence is 0.5, a coin flip. The max confidence is 1.0.
///
/// TODO: This detection "algorithm" is very rudimentary to say the least.
/// We can add things like a swap size factor, profit validation in USD,
/// flashloan detection, known MEV bot addresses,
/// priority fee analysis, figure out private mempools,
/// and more sophisticated confidence scoring weights.
fn calculate_sandwich_confidence(
    front: &SwapTransaction,
    victim: &SwapTransaction,
    back: &SwapTransaction,
) -> f32 {
    let mut confidence = 0.5;

    if front.gas_price > victim.gas_price {
        confidence += 0.2;
    }

    if back.gas_price < victim.gas_price {
        confidence += 0.1;
    }

    if front.is_contract_caller {
        confidence += 0.1;
    }

    if back.is_contract_caller {
        confidence += 0.1;
    }

    let total_profit =
        back.usd_value_out - front.usd_value_in - front.gas_cost_usd - back.gas_cost_usd;
    if total_profit > 0.0 {
        confidence += 0.25;
    }

    if is_proportional_sandwich(front, victim, back) {
        confidence += 0.15;
    }

    if confidence > 1.0 {
        return 1.0;
    }

    return confidence;
}

/// Check if sandwich trades are proportionally sized to the victim trade.
/// Professional MEV bots typically size their trades as 10-30% of victim trade.
fn is_proportional_sandwich(
    front: &SwapTransaction,
    victim: &SwapTransaction,
    back: &SwapTransaction,
) -> bool {
    let front_ratio = front.usd_value_in / victim.usd_value_in;
    let back_ratio = back.usd_value_in / victim.usd_value_in;

    // Front-run should be 5-50% of victim trade
    let front_proportional = front_ratio >= 0.05 && front_ratio <= 0.5;

    // Back-run should be similar size to front-run (within 2x range)
    let back_proportional = back_ratio >= front_ratio * 0.5 && back_ratio <= front_ratio * 2.0;

    front_proportional && back_proportional
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn load_sample_transactions() -> Vec<SwapTransaction> {
        let csv_content =
            fs::read_to_string("data/sample_swaps.csv").expect("Failed to read sample CSV file");

        let mut reader = csv::Reader::from_reader(csv_content.as_bytes());
        let mut transactions = Vec::new();

        for result in reader.deserialize() {
            let transaction: SwapTransaction = result.expect("Failed to parse CSV row");
            transactions.push(transaction);
        }

        transactions
    }

    #[test]
    fn test_sandwich_detection_with_sample_data() {
        let transactions = load_sample_transactions();

        assert!(
            !transactions.is_empty(),
            "Should load some transactions from CSV"
        );

        let attacks = find_same_block_sandwiches(&transactions);

        assert_eq!(attacks.len(), 6, "Should detect exactly 6 sandwich attacks");

        // Print detected attacks for debugging
        for attack in &attacks {
            println!(
                "Attack: {} -> {} -> {} (confidence: {:.2})",
                attack.front_run_tx.tx_hash,
                attack.victim_tx.tx_hash,
                attack.back_run_tx.tx_hash,
                attack.confidence_score
            );
        }

        let attack_hashes: Vec<(&str, &str, &str)> = attacks
            .iter()
            .map(|a| {
                (
                    a.front_run_tx.tx_hash.as_str(),
                    a.victim_tx.tx_hash.as_str(),
                    a.back_run_tx.tx_hash.as_str(),
                )
            })
            .collect();

        assert!(
            attack_hashes.contains(&("0xsandwich1", "0xvictim001", "0xsandwich2")),
            "Should detect USDC/SHIB sandwich attack by 0xattacker1"
        );
        assert!(
            attack_hashes.contains(&("0xsandwich3", "0xvictim002", "0xsandwich4")),
            "Should detect ETH/NEWTOKEN sandwich attack by 0xbot123"
        );
        assert!(
            attack_hashes.contains(&("0xfront_run", "0xvictim_nc", "0xback_run")),
            "Should detect non-consecutive USDC/SHIB sandwich attack by 0xsandwich_bot"
        );
        assert!(
            attack_hashes.contains(&("0xcross_dex1", "0xcross_victim", "0xcross_dex2")),
            "Should detect cross-DEX USDC/ETH sandwich attack by 0xcross_bot"
        );
        assert!(
            attack_hashes.contains(&("0xequiv_front", "0xequiv_victim", "0xequiv_back")),
            "Should detect equivalent token USDC->USDT sandwich attack by 0xstable_bot"
        );
        assert!(
            attack_hashes.contains(&("0xweth_front", "0xweth_victim", "0xweth_back")),
            "Should detect WETH/ETH equivalent sandwich attack by 0xweth_mev"
        );

        let unprofitable_attack = attacks
            .iter()
            .find(|a| {
                a.front_run_tx.tx_hash == "0xsandwich1" && a.back_run_tx.tx_hash == "0xsandwich2"
            })
            .expect("Should find unprofitable sandwich attack");

        let profitable_attack1 = attacks
            .iter()
            .find(|a| {
                a.front_run_tx.tx_hash == "0xsandwich3" && a.back_run_tx.tx_hash == "0xsandwich4"
            })
            .expect("Should find profitable sandwich attack");

        let profitable_attack2 = attacks
            .iter()
            .find(|a| {
                a.front_run_tx.tx_hash == "0xfront_run" && a.back_run_tx.tx_hash == "0xback_run"
            })
            .expect("Should find profitable sandwich attack");

        let cross_dex_attack = attacks
            .iter()
            .find(|a| {
                a.front_run_tx.tx_hash == "0xcross_dex1" && a.back_run_tx.tx_hash == "0xcross_dex2"
            })
            .expect("Should find cross-DEX sandwich attack");

        let equiv_token_attack = attacks
            .iter()
            .find(|a| {
                a.front_run_tx.tx_hash == "0xequiv_front" && a.back_run_tx.tx_hash == "0xequiv_back"
            })
            .expect("Should find equivalent token sandwich attack");

        let weth_attack = attacks
            .iter()
            .find(|a| {
                a.front_run_tx.tx_hash == "0xweth_front" && a.back_run_tx.tx_hash == "0xweth_back"
            })
            .expect("Should find WETH/ETH sandwich attack");

        let unprofitable_profit = unprofitable_attack.back_run_tx.usd_value_out
            - unprofitable_attack.front_run_tx.usd_value_in
            - unprofitable_attack.front_run_tx.gas_cost_usd
            - unprofitable_attack.back_run_tx.gas_cost_usd;

        let profitable_profit1 = profitable_attack1.back_run_tx.usd_value_out
            - profitable_attack1.front_run_tx.usd_value_in
            - profitable_attack1.front_run_tx.gas_cost_usd
            - profitable_attack1.back_run_tx.gas_cost_usd;

        let profitable_profit2 = profitable_attack2.back_run_tx.usd_value_out
            - profitable_attack2.front_run_tx.usd_value_in
            - profitable_attack2.front_run_tx.gas_cost_usd
            - profitable_attack2.back_run_tx.gas_cost_usd;

        assert!(
            unprofitable_profit < 0.0,
            "First sandwich should be unprofitable after gas costs: profit = {}",
            unprofitable_profit
        );

        assert!(
            profitable_profit1 > 0.0,
            "Second sandwich should be profitable after gas costs: profit = {}",
            profitable_profit1
        );
        assert!(
            profitable_profit2 > 0.0,
            "Third sandwich should be profitable after gas costs: profit = {}",
            profitable_profit2
        );

        assert!(
            profitable_attack1.confidence_score > unprofitable_attack.confidence_score,
            "Profitable sandwich should have higher confidence than unprofitable one"
        );
        assert!(
            profitable_attack2.confidence_score > unprofitable_attack.confidence_score,
            "Profitable sandwich should have higher confidence than unprofitable one"
        );

        // Test cross-DEX attack (should be profitable)
        let cross_dex_profit = cross_dex_attack.back_run_tx.usd_value_out
            - cross_dex_attack.front_run_tx.usd_value_in
            - cross_dex_attack.front_run_tx.gas_cost_usd
            - cross_dex_attack.back_run_tx.gas_cost_usd;
        assert!(
            cross_dex_profit > 0.0,
            "Cross-DEX sandwich should be profitable: profit = {}",
            cross_dex_profit
        );

        // Test equivalent token attacks (should be profitable)
        let equiv_profit = equiv_token_attack.back_run_tx.usd_value_out
            - equiv_token_attack.front_run_tx.usd_value_in
            - equiv_token_attack.front_run_tx.gas_cost_usd
            - equiv_token_attack.back_run_tx.gas_cost_usd;
        assert!(
            equiv_profit > 0.0,
            "Equivalent token sandwich should be profitable: profit = {}",
            equiv_profit
        );

        let weth_profit = weth_attack.back_run_tx.usd_value_out
            - weth_attack.front_run_tx.usd_value_in
            - weth_attack.front_run_tx.gas_cost_usd
            - weth_attack.back_run_tx.gas_cost_usd;
        assert!(
            weth_profit > 0.0,
            "WETH/ETH sandwich should be profitable: profit = {}",
            weth_profit
        );

        // Verify cross-DEX detection (different pools)
        assert_ne!(
            cross_dex_attack.front_run_tx.pool_address, cross_dex_attack.victim_tx.pool_address,
            "Cross-DEX attack should involve different pools"
        );

        // Verify equivalent token detection
        assert_ne!(
            equiv_token_attack.front_run_tx.token_in, equiv_token_attack.back_run_tx.token_out,
            "Equivalent token attack should use different but equivalent tokens"
        );
        assert_ne!(
            weth_attack.front_run_tx.token_in, weth_attack.victim_tx.token_in,
            "WETH/ETH attack should use different but equivalent tokens"
        );

        // Test proportional sizing detection
        // Check if any attacks have proportional sizing bonus
        let has_proportional_bonus = attacks.iter().any(|attack| {
            let base_confidence = 0.5 + 0.25; // base + profitability bonus
            attack.confidence_score > base_confidence + 0.1 // additional bonuses beyond basic
        });

        assert!(
            has_proportional_bonus,
            "At least one attack should have proportional sizing or other bonus"
        );
    }
}
