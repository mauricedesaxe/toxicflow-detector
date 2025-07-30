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
}

#[derive(Debug, PartialEq)]
pub struct SandwichAttack {
    pub front_run_tx: SwapTransaction,
    pub victim_tx: SwapTransaction,
    pub back_run_tx: SwapTransaction,
    pub confidence_score: f32,
}

/// Find sandwich attacks in a list of swap transactions.
pub fn find_sandwiches(transactions: &[SwapTransaction]) -> Vec<SandwichAttack> {
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

fn find_sandwiches_in_block(
    transactions: &[SwapTransaction],
) -> Result<Vec<SandwichAttack>, String> {
    let mut attacks = Vec::new();

    if transactions.len() < 3 {
        return Err("not enough transactions to have a sandwich".to_string());
    }

    // TODO: Improve scanning logic - currently only checks consecutive transactions (i, i+1, i+2)
    // Real sandwich attacks can have multiple victims or other unrelated transactions between
    // front-run and back-run. Need more sophisticated pattern matching.
    for i in 0..transactions.len() - 2 {
        let potential_front = &transactions[i];
        let potential_victim = &transactions[i + 1];
        let potential_back = &transactions[i + 2];

        if is_sandwich_pattern(potential_front, potential_victim, potential_back) {
            attacks.push(SandwichAttack {
                front_run_tx: potential_front.clone(),
                victim_tx: potential_victim.clone(),
                back_run_tx: potential_back.clone(),
                confidence_score: calculate_sandwich_confidence(
                    potential_front,
                    potential_victim,
                    potential_back,
                ),
            });
        }
    }

    return Ok(attacks);
}

/// A rudimentary sandwich pattern detection function.
/// It assumes the transactions are in the correct order (front, victim, back).
///
/// Returning `true` doesn't mean it was a (profitable) sandwich attack,
/// but it means the swap directions are there.
///
/// Note: some of these, like forcing the same token for the attacker in/out are
/// not always true for real sandwich attacks, but good enough for us for now.
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

    // Should be same pool
    if front.pool_address != victim.pool_address || victim.pool_address != back.pool_address {
        return false;
    }

    // Attacker should have gotten same type of token back.
    if front.token_in != back.token_out {
        return false;
    }

    // Front and victim should be same token direction (attacker buys before victim)
    if front.token_in != victim.token_in || front.token_out != victim.token_out {
        return false;
    }

    // Victim and back should be different token direction (attacker sells back to victim)
    if victim.token_in == back.token_in && victim.token_out == back.token_out {
        return false;
    }

    return true;
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

    if back.amount_out > front.amount_in {
        confidence += 0.25;
    }

    if confidence > 1.0 {
        return 1.0;
    }

    return confidence;
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

        let attacks = find_sandwiches(&transactions);

        assert_eq!(attacks.len(), 2, "Should detect exactly 2 sandwich attacks");

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

        for attack in &attacks {
            assert!(
                attack.confidence_score > 0.5,
                "Attack confidence should be > 0.5, got {}",
                attack.confidence_score
            );
        }
    }
}
