use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
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
    if front.token_in != victim.token_out || victim.token_in != back.token_out {
        return false;
    }

    // Victim and back should be different token direction (attacker sells back to victim)
    if victim.token_in == back.token_out {
        return false;
    }

    return true;
}

/// Takes 3 swap transactions which have already been validated to have
/// a sandwich pattern and calculates the confidence that the attacker
/// is a MEV bot using gas prices, if contract checks, and profit analysis.
///
/// The base confidence is 0.5, a coin flip. The max confidence is 1.0.
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
