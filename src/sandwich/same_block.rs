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
) -> (f32, ConfidenceFlags) {
    let mut confidence = 0.5;

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

        // === Test Proportional Sizing Detection ===
        let cross_dex_front_ratio =
            cross_dex_attack.front_run_tx.usd_value_in / cross_dex_attack.victim_tx.usd_value_in;
        let cross_dex_back_ratio =
            cross_dex_attack.back_run_tx.usd_value_in / cross_dex_attack.victim_tx.usd_value_in;

        assert!(
            cross_dex_front_ratio >= 0.05 && cross_dex_front_ratio <= 0.5,
            "Cross-DEX front-run should be proportionally sized (5-50% of victim): ratio = {}",
            cross_dex_front_ratio
        );

        assert!(
            cross_dex_back_ratio >= cross_dex_front_ratio * 0.5 && cross_dex_back_ratio <= cross_dex_front_ratio * 2.0,
            "Cross-DEX back-run should be similar size to front-run: back_ratio = {}, front_ratio = {}",
            cross_dex_back_ratio, cross_dex_front_ratio
        );

        // === Test Price Impact Detection ===
        let front_rate = equiv_token_attack.front_run_tx.usd_value_out
            / equiv_token_attack.front_run_tx.usd_value_in;
        let victim_rate =
            equiv_token_attack.victim_tx.usd_value_out / equiv_token_attack.victim_tx.usd_value_in;

        assert!(
            victim_rate < front_rate,
            "Victim should have gotten worse exchange rate than front-runner: victim_rate = {}, front_rate = {}",
            victim_rate, front_rate
        );

        let price_impact = (front_rate - victim_rate) / front_rate;
        assert!(
            price_impact > 0.05,
            "Price impact should be significant (>5%): impact = {}",
            price_impact
        );

        // === Test Gas Price Analysis ===
        assert!(
            profitable_attack1.front_run_tx.gas_price > profitable_attack1.victim_tx.gas_price,
            "Front-runner should pay higher gas than victim for priority"
        );

        assert!(
            profitable_attack1.back_run_tx.gas_price < profitable_attack1.victim_tx.gas_price,
            "Back-runner can pay lower gas since they go after victim"
        );

        // === Test Contract Caller Detection ===
        let contract_attacks = attacks
            .iter()
            .filter(|a| a.front_run_tx.is_contract_caller && a.back_run_tx.is_contract_caller)
            .count();

        assert!(
            contract_attacks >= 2,
            "Should detect at least 2 attacks using smart contracts: found {}",
            contract_attacks
        );

        // === Test Confidence Score Ranges ===
        assert!(
            unprofitable_attack.confidence_score < 0.85,
            "Unprofitable attacks should have lower confidence: {}",
            unprofitable_attack.confidence_score
        );

        let max_confidence = attacks
            .iter()
            .map(|a| a.confidence_score)
            .fold(0.0, f32::max);
        assert!(
            max_confidence > 0.8,
            "Best attacks should have high confidence (>0.8): max = {}",
            max_confidence
        );

        // === Test Profitable vs Unprofitable Confidence Comparison ===
        let profitable_attacks_avg = attacks
            .iter()
            .filter(|a| {
                let profit = a.back_run_tx.usd_value_out
                    - a.front_run_tx.usd_value_in
                    - a.front_run_tx.gas_cost_usd
                    - a.back_run_tx.gas_cost_usd;
                profit > 0.0
            })
            .map(|a| a.confidence_score)
            .sum::<f32>()
            / attacks.len() as f32;

        assert!(
            profitable_attacks_avg > unprofitable_attack.confidence_score,
            "Average profitable attack confidence ({:.3}) should be higher than unprofitable attack confidence ({:.3})",
            profitable_attacks_avg, unprofitable_attack.confidence_score
        );
    }

    #[test]
    fn test_token_equivalence_groups() {
        assert_eq!(get_token_equivalence_group("USDC"), "STABLECOINS");
        assert_eq!(get_token_equivalence_group("USDT"), "STABLECOINS");
        assert_eq!(get_token_equivalence_group("DAI"), "STABLECOINS");
        assert_eq!(get_token_equivalence_group("ETH"), "ETH_GROUP");
        assert_eq!(get_token_equivalence_group("WETH"), "ETH_GROUP");
        assert_eq!(get_token_equivalence_group("WBTC"), "BTC_GROUP");
        assert_eq!(get_token_equivalence_group("SHIB"), "SHIB");
    }

    #[test]
    fn test_are_tokens_equivalent() {
        assert!(are_tokens_equivalent("USDC", "USDT"));
        assert!(are_tokens_equivalent("ETH", "WETH"));
        assert!(are_tokens_equivalent("WBTC", "renBTC"));
        assert!(!are_tokens_equivalent("USDC", "ETH"));
        assert!(!are_tokens_equivalent("SHIB", "USDC"));
    }

    #[test]
    fn test_are_tokens_reversed() {
        let tx_a = SwapTransaction {
            tx_hash: "0x1".to_string(),
            block_number: 1,
            timestamp: 1,
            tx_position_in_block: 1,
            from_address: "0x1".to_string(),
            token_in: "USDC".to_string(),
            token_out: "ETH".to_string(),
            amount_in: 1000.0,
            amount_out: 1.0,
            gas_price: 100,
            pool_address: "0xpool".to_string(),
            token_launch_block: 1,
            is_contract_caller: false,
            usd_value_in: 1000.0,
            usd_value_out: 3200.0,
            gas_cost_usd: 50.0,
        };

        let tx_b_reversed = SwapTransaction {
            token_in: "WETH".to_string(),
            token_out: "USDT".to_string(),
            ..tx_a.clone()
        };

        let tx_b_not_reversed = SwapTransaction {
            token_in: "USDC".to_string(),
            token_out: "ETH".to_string(),
            ..tx_a.clone()
        };

        assert!(are_tokens_reversed(&tx_a, &tx_b_reversed));
        assert!(!are_tokens_reversed(&tx_a, &tx_b_not_reversed));
    }

    #[test]
    fn test_is_proportional_sandwich() {
        let front = SwapTransaction {
            tx_hash: "0x1".to_string(),
            block_number: 1,
            timestamp: 1,
            tx_position_in_block: 1,
            from_address: "0x1".to_string(),
            token_in: "USDC".to_string(),
            token_out: "ETH".to_string(),
            amount_in: 1000.0,
            amount_out: 1.0,
            gas_price: 100,
            pool_address: "0xpool".to_string(),
            token_launch_block: 1,
            is_contract_caller: false,
            usd_value_in: 1000.0, // 20% of victim
            usd_value_out: 3200.0,
            gas_cost_usd: 50.0,
        };

        let victim = SwapTransaction {
            usd_value_in: 5000.0,
            ..front.clone()
        };

        let back_proportional = SwapTransaction {
            usd_value_in: 1200.0, // Similar to front (24% of victim)
            ..front.clone()
        };

        let back_too_large = SwapTransaction {
            usd_value_in: 10000.0, // Too large (200% of victim)
            ..front.clone()
        };

        assert!(is_proportional_sandwich(
            &front,
            &victim,
            &back_proportional
        ));
        assert!(!is_proportional_sandwich(&front, &victim, &back_too_large));
    }

    #[test]
    fn test_calculate_victim_price_impact() {
        let front = SwapTransaction {
            tx_hash: "0x1".to_string(),
            block_number: 1,
            timestamp: 1,
            tx_position_in_block: 1,
            from_address: "0x1".to_string(),
            token_in: "USDC".to_string(),
            token_out: "ETH".to_string(),
            amount_in: 1000.0,
            amount_out: 1.0,
            gas_price: 100,
            pool_address: "0xpool".to_string(),
            token_launch_block: 1,
            is_contract_caller: false,
            usd_value_in: 1000.0,
            usd_value_out: 1000.0, // 1.0 exchange rate
            gas_cost_usd: 50.0,
        };

        let victim_worse_rate = SwapTransaction {
            usd_value_in: 5000.0,
            usd_value_out: 4500.0, // 0.9 exchange rate (10% worse)
            ..front.clone()
        };

        let victim_better_rate = SwapTransaction {
            usd_value_in: 5000.0,
            usd_value_out: 5500.0, // 1.1 exchange rate (better)
            ..front.clone()
        };

        let impact = calculate_victim_price_impact(&front, &victim_worse_rate);
        assert!(impact > 0.0, "Should detect price impact");
        assert!(impact < 0.15, "Impact should be reasonable");

        let no_impact = calculate_victim_price_impact(&front, &victim_better_rate);
        assert_eq!(
            no_impact, 0.0,
            "Should detect no price impact when victim gets better rate"
        );
    }
}
