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

#[derive(Debug, PartialEq, Clone)]
pub struct ConfidenceFlags {
    pub higher_front_gas_price: bool,
    pub lower_back_gas_price: bool,
    pub front_is_contract: bool,
    pub back_is_contract: bool,
    pub is_profitable: bool,
    pub is_proportional: bool,
    pub price_impact_score: f32,
    pub total_profit_usd: f64,
}

#[derive(Debug)]
pub struct SandwichAttack {
    pub front_run_tx: SwapTransaction,
    pub victim_tx: SwapTransaction,
    pub back_run_tx: SwapTransaction,
    pub confidence_score: f32,
    pub confidence_flags: ConfidenceFlags,
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
                    let (confidence_score, confidence_flags) =
                        calculate_sandwich_confidence(front_tx, victim_tx, back_tx);
                    attacks.push(SandwichAttack {
                        front_run_tx: front_tx.clone(),
                        victim_tx: victim_tx.clone(),
                        back_run_tx: back_tx.clone(),
                        confidence_score,
                        confidence_flags,
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
    // Front-run and victim should be same pool
    if front.pool_address != victim.pool_address {
        return false;
    }

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
/// The base confidence is 0.3. The max confidence is 1.0.
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
) -> (f32, ConfidenceFlags) {
    let mut confidence = 0.3;

    let higher_front_gas_price = front.gas_price > victim.gas_price;
    if higher_front_gas_price {
        confidence += 0.2;
    }

    let lower_back_gas_price = back.gas_price < victim.gas_price;
    if lower_back_gas_price {
        confidence += 0.1;
    }

    let front_is_contract = front.is_contract_caller;
    if front_is_contract {
        confidence += 0.1;
    }

    let back_is_contract = back.is_contract_caller;
    if back_is_contract {
        confidence += 0.1;
    }

    let total_profit_usd =
        back.usd_value_out - front.usd_value_in - front.gas_cost_usd - back.gas_cost_usd;
    let is_profitable = total_profit_usd > 0.0;
    if is_profitable {
        confidence += 0.25;
    }

    let is_proportional = is_proportional_sandwich(front, victim, back);
    if is_proportional {
        confidence += 0.15;
    }

    let price_impact_score = calculate_victim_price_impact(front, victim);
    if price_impact_score > 0.0 {
        confidence += match price_impact_score {
            p if p < 0.25 => p,
            _ => 0.25,
        };
    }

    let final_confidence = if confidence > 1.0 { 1.0 } else { confidence };

    let flags = ConfidenceFlags {
        higher_front_gas_price,
        lower_back_gas_price,
        front_is_contract,
        back_is_contract,
        is_profitable,
        is_proportional,
        price_impact_score,
        total_profit_usd,
    };

    (final_confidence, flags)
}

/// Check if sandwich trades are proportionally sized to the victim trade.
/// Professional MEV bots typically size their trades as 10-30% of victim trade.
///
/// TODO: This is rudimentary, we could improve it by calculating real
/// price impact of various sizes and expecting the attacker to try and
/// maximize profit.
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

/// Calculate price impact suffered by victim due to front-running.
/// Returns the percentage worse rate the victim got (e.g., 0.05 = 5% worse).
/// If the victim got a better rate than the front-runner, returns 0.0.
fn calculate_victim_price_impact(front: &SwapTransaction, victim: &SwapTransaction) -> f32 {
    // Only calculate if they're trading in the same direction (same tokens)
    if !are_tokens_equivalent(&front.token_in, &victim.token_in)
        || !are_tokens_equivalent(&front.token_out, &victim.token_out)
    {
        return 0.0;
    }

    let front_rate = (front.usd_value_out / front.usd_value_in) as f32;
    let victim_rate = (victim.usd_value_out / victim.usd_value_in) as f32;

    if victim_rate < front_rate {
        (front_rate - victim_rate) / front_rate
    } else {
        0.0
    }
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
    fn test_find_same_block_sandwiches_with_sample_data() {
        let transactions = load_sample_transactions();
        let attacks = find_same_block_sandwiches(&transactions);

        // Should find exactly 6 sandwich attacks from the sample data
        assert_eq!(attacks.len(), 6, "Expected 6 sandwich attacks");

        // Block 12360: Basic USDC/SHIB sandwich
        let block_12360_attack = attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12360)
            .expect("Should find attack in block 12360");

        assert_eq!(block_12360_attack.front_run_tx.from_address, "0xattacker1");
        assert_eq!(block_12360_attack.victim_tx.from_address, "0xvictim1");
        assert_eq!(block_12360_attack.back_run_tx.from_address, "0xattacker1");
        assert_eq!(block_12360_attack.front_run_tx.token_in, "USDC");
        assert_eq!(block_12360_attack.front_run_tx.token_out, "SHIB");
        assert_eq!(block_12360_attack.back_run_tx.token_in, "SHIB");
        assert_eq!(block_12360_attack.back_run_tx.token_out, "USDC");

        // Should be profitable (950 out - 1000 in - gas costs = negative, but let's check the flags)
        assert!(
            block_12360_attack.confidence_flags.total_profit_usd < 0.0,
            "This attack is actually not profitable"
        );
        assert!(!block_12360_attack.confidence_flags.is_profitable);
        assert!(
            block_12360_attack.confidence_score > 0.5,
            "Should have some confidence indicators"
        );

        // Block 12361: ETH/NEWTOKEN sandwich with contract callers
        let block_12361_attack = attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12361)
            .expect("Should find attack in block 12361");

        assert_eq!(block_12361_attack.front_run_tx.from_address, "0xbot123");
        assert_eq!(block_12361_attack.victim_tx.from_address, "0xinnocent");
        assert_eq!(block_12361_attack.back_run_tx.from_address, "0xbot123");
        assert!(block_12361_attack.confidence_flags.front_is_contract);
        assert!(block_12361_attack.confidence_flags.back_is_contract);
        assert!(
            block_12361_attack.confidence_flags.higher_front_gas_price,
            "Front gas price (300) > victim gas price (150)"
        );
        assert!(
            block_12361_attack.confidence_flags.lower_back_gas_price,
            "Back gas price (80) < victim gas price (150)"
        );

        // This should be profitable: 2000 out - 1600 in - gas costs
        let expected_profit = 2000.0 - 1600.0 - 240.0 - 64.0; // around 96 USD
        assert!(block_12361_attack.confidence_flags.is_profitable);
        assert!(
            (block_12361_attack.confidence_flags.total_profit_usd - expected_profit).abs() < 1.0
        );

        // Block 12362: USDC/SHIB sandwich with unrelated transactions in between
        let block_12362_attack = attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12362)
            .expect("Should find attack in block 12362");

        assert_eq!(
            block_12362_attack.front_run_tx.from_address,
            "0xsandwich_bot"
        );
        assert_eq!(
            block_12362_attack.victim_tx.from_address,
            "0xinnocent_trader"
        );
        assert_eq!(
            block_12362_attack.back_run_tx.from_address,
            "0xsandwich_bot"
        );
        assert!(block_12362_attack.confidence_flags.front_is_contract);
        assert!(block_12362_attack.confidence_flags.back_is_contract);
        assert!(
            block_12362_attack.confidence_flags.higher_front_gas_price,
            "Front gas price (280) > victim gas price (180)"
        );
        assert!(
            block_12362_attack.confidence_flags.lower_back_gas_price,
            "Back gas price (120) < victim gas price (180)"
        );

        // Block 12363: Cross-DEX case is correctly filtered out as false positive
        // (front-run and victim used different pools, so no price manipulation occurred)

        // Block 12366: Legitimate cross-DEX sandwich (front-run and victim same pool, back-run different pool)
        let block_12366_attack = attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12366)
            .expect("Should find legitimate cross-DEX attack in block 12366");

        assert_eq!(block_12366_attack.front_run_tx.from_address, "0xlegit_mev");
        assert_eq!(block_12366_attack.victim_tx.from_address, "0xinnocent_dex");
        assert_eq!(block_12366_attack.back_run_tx.from_address, "0xlegit_mev");

        // Front-run and victim should use same pool (this is what makes it a valid sandwich)
        assert_eq!(
            block_12366_attack.front_run_tx.pool_address,
            "0xpool_uniswap"
        );
        assert_eq!(block_12366_attack.victim_tx.pool_address, "0xpool_uniswap");

        // Back-run can use different pool (attacker optimizing exit)
        assert_eq!(
            block_12366_attack.back_run_tx.pool_address,
            "0xpool_sushiswap"
        );

        assert!(block_12366_attack.confidence_flags.front_is_contract);
        assert!(block_12366_attack.confidence_flags.back_is_contract);
        assert!(block_12366_attack.confidence_flags.higher_front_gas_price);
        assert!(block_12366_attack.confidence_flags.lower_back_gas_price);
        assert!(block_12366_attack.confidence_flags.is_profitable);

        // Block 12364: Token equivalence test (USDC/USDT are equivalent stablecoins)
        let block_12364_attack = attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12364)
            .expect("Should find attack in block 12364 testing stablecoin equivalence");

        assert_eq!(block_12364_attack.front_run_tx.from_address, "0xstable_bot");
        assert_eq!(block_12364_attack.victim_tx.from_address, "0xlegit_user");
        assert_eq!(block_12364_attack.back_run_tx.from_address, "0xstable_bot");
        // Front: USDC->SHIB, Victim: USDT->SHIB (equivalent), Back: SHIB->USDT (equivalent)
        assert_eq!(block_12364_attack.front_run_tx.token_in, "USDC");
        assert_eq!(block_12364_attack.victim_tx.token_in, "USDT");
        assert_eq!(block_12364_attack.back_run_tx.token_out, "USDT");

        // Block 12365: Token equivalence test (ETH/WETH are equivalent)
        let block_12365_attack = attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12365)
            .expect("Should find attack in block 12365 testing ETH/WETH equivalence");

        assert_eq!(block_12365_attack.front_run_tx.from_address, "0xweth_mev");
        assert_eq!(block_12365_attack.victim_tx.from_address, "0xeth_holder");
        assert_eq!(block_12365_attack.back_run_tx.from_address, "0xweth_mev");
        // Front: WETH->NEWTOKEN, Victim: ETH->NEWTOKEN (equivalent), Back: NEWTOKEN->WETH
        assert_eq!(block_12365_attack.front_run_tx.token_in, "WETH");
        assert_eq!(block_12365_attack.victim_tx.token_in, "ETH");
        assert_eq!(block_12365_attack.back_run_tx.token_out, "WETH");

        // Check that all attacks have reasonable confidence scores
        for attack in &attacks {
            assert!(
                attack.confidence_score >= 0.3,
                "All attacks should have at least base confidence"
            );
            assert!(
                attack.confidence_score <= 1.0,
                "Confidence should not exceed 1.0"
            );

            // Verify attack structure makes sense
            assert_ne!(
                attack.front_run_tx.from_address, attack.victim_tx.from_address,
                "Attacker should not be victim"
            );
            assert_eq!(
                attack.front_run_tx.from_address, attack.back_run_tx.from_address,
                "Front and back should be same attacker"
            );
            assert!(
                attack.front_run_tx.tx_position_in_block < attack.victim_tx.tx_position_in_block,
                "Front should come before victim"
            );
            assert!(
                attack.victim_tx.tx_position_in_block < attack.back_run_tx.tx_position_in_block,
                "Victim should come before back"
            );
        }

        println!("Found {} sandwich attacks:", attacks.len());
        for (i, attack) in attacks.iter().enumerate() {
            println!(
                "Attack {}: Block {} - Confidence {:.3}",
                i + 1,
                attack.front_run_tx.block_number,
                attack.confidence_score
            );
            println!(
                "  Profit: ${:.2} USD",
                attack.confidence_flags.total_profit_usd
            );
            println!(
                "  Flags: profitable={}, contracts={}/{}, gas_priority={}/{}",
                attack.confidence_flags.is_profitable,
                attack.confidence_flags.front_is_contract,
                attack.confidence_flags.back_is_contract,
                attack.confidence_flags.higher_front_gas_price,
                attack.confidence_flags.lower_back_gas_price
            );
        }
    }
}
