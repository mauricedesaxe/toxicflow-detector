use crate::sandwich::transactions::SwapTransaction;
use crate::sandwich::utils::is_sandwich_pattern;
use std::collections::HashMap;

/// Represents the state of an AMM liquidity pool at a specific point
#[derive(Debug, Clone)]
pub struct Pool {
    pub token_a_reserve: f64,
    pub token_b_reserve: f64,
    pub token_a_address: String,
    pub token_b_address: String,
}

/// Result of simulating a single swap transaction
#[derive(Debug, Clone)]
pub struct SwapSimulationResult {
    pub tokens_received: f64,
    pub price_per_token: f64,
    pub slippage: f64,
    pub new_pool_state: Pool,
}

/// Represents a confirmed sandwich attack found through simulation
#[derive(Debug)]
pub struct SandwichAttackBySimulation {
    pub front_run_tx: SwapTransaction,
    pub victim_tx: SwapTransaction,
    pub back_run_tx: SwapTransaction,
    pub victim_loss_percentage: f64,
}

impl Pool {
    pub fn new(
        token_a_reserve: f64,
        token_b_reserve: f64,
        token_a_address: String,
        token_b_address: String,
    ) -> Self {
        Self {
            token_a_reserve,
            token_b_reserve,
            token_a_address,
            token_b_address,
        }
    }

    pub fn get_token_a_price(&self) -> f64 {
        self.token_b_reserve / self.token_a_reserve
    }

    pub fn get_token_b_price(&self) -> f64 {
        self.token_a_reserve / self.token_b_reserve
    }

    pub fn constant_product_formula(&self, x: f64, y: f64, dx: f64) -> f64 {
        (y * dx) / (x + dx)
    }

    pub fn calculate_slippage(&self, initial_price: f64, execution_price: f64) -> f64 {
        ((execution_price - initial_price) / initial_price * 100.0).abs()
    }

    pub fn simulate_swap(&self, swap: &SwapTransaction) -> SwapSimulationResult {
        let is_buying_token_a = &swap.token_out == &self.token_a_address;

        let initial_price = if is_buying_token_a {
            self.get_token_a_price()
        } else {
            self.get_token_b_price()
        };

        let (output_reserve, input_reserve) = if is_buying_token_a {
            (self.token_a_reserve, self.token_b_reserve)
        } else {
            (self.token_b_reserve, self.token_a_reserve)
        };

        let tokens_received =
            self.constant_product_formula(input_reserve, output_reserve, swap.amount_in);

        let execution_price = swap.amount_in / tokens_received;
        let slippage = self.calculate_slippage(initial_price, execution_price);

        let (new_token_a_reserve, new_token_b_reserve) = if is_buying_token_a {
            (
                self.token_a_reserve - tokens_received,
                self.token_b_reserve + swap.amount_in,
            )
        } else {
            (
                self.token_a_reserve + swap.amount_in,
                self.token_b_reserve - tokens_received,
            )
        };

        return SwapSimulationResult {
            tokens_received,
            price_per_token: execution_price,
            slippage,
            new_pool_state: Pool {
                token_a_reserve: new_token_a_reserve,
                token_b_reserve: new_token_b_reserve,
                token_a_address: self.token_a_address.clone(),
                token_b_address: self.token_b_address.clone(),
            },
        };
    }
}

/// Find sandwich attacks across all blocks using simulation
pub fn find_sandwich_attacks_by_simulation(
    pool_map: &HashMap<String, Pool>,
    transactions: &[SwapTransaction],
) -> Vec<SandwichAttackBySimulation> {
    // Group transactions by block number
    let mut blocks: std::collections::HashMap<u64, Vec<SwapTransaction>> =
        std::collections::HashMap::new();
    for tx in transactions {
        blocks
            .entry(tx.block_number)
            .or_insert_with(Vec::new)
            .push(tx.clone());
    }

    let mut all_attacks = Vec::new();

    // Process each block separately
    for (_block_number, block_txs) in blocks {
        let block_attacks = find_sandwiches_in_block_by_simulation(&pool_map, &block_txs);
        all_attacks.extend(block_attacks);
    }

    all_attacks
}

/// Find sandwich attacks within a single block using simulation
fn find_sandwiches_in_block_by_simulation(
    pool_map: &HashMap<String, Pool>,
    transactions: &[SwapTransaction],
) -> Vec<SandwichAttackBySimulation> {
    let mut detected_attacks = Vec::new();

    for i in 0..transactions.len() {
        for j in i + 1..transactions.len() {
            for k in j + 1..transactions.len() {
                let front = &transactions[i];
                let victim = &transactions[j];
                let back = &transactions[k];

                if is_sandwich_pattern(front, victim, back) {
                    if let Some(pool) = pool_map.get(&front.pool_address) {
                        match simulate_sandwich_attack(pool, front, victim, back, &transactions) {
                            Ok(attack) => detected_attacks.push(attack),
                            Err(error) => println!("Sandwich simulation error: {}", error),
                        }
                    }
                }
            }
        }
    }

    detected_attacks
}

/// Simulates a specific sandwich attack to measure victim impact
fn simulate_sandwich_attack(
    initial_pool: &Pool,
    front: &SwapTransaction,
    victim: &SwapTransaction,
    back: &SwapTransaction,
    all_transactions: &[SwapTransaction],
) -> Result<SandwichAttackBySimulation, String> {
    let pool_transactions: Vec<&SwapTransaction> = all_transactions
        .iter()
        .filter(|tx| tx.pool_address == victim.pool_address)
        .collect();
    if pool_transactions.is_empty() {
        return Err("No transaction's found in the victim pool.".to_string());
    }

    if !check_simulation_is_like_reality(initial_pool, &pool_transactions, victim) {
        return Err("Initial simulation is not like reality.".to_string());
    }

    let difference_pct = simulate_without_attacker(initial_pool, &pool_transactions, front, victim);

    Ok(SandwichAttackBySimulation {
        front_run_tx: front.clone(),
        victim_tx: victim.clone(),
        back_run_tx: back.clone(),
        victim_loss_percentage: difference_pct,
    })
}

/// Try simulate what actually happened during the real block
/// to see if we'd get the same amount_out for the would-be victim.
/// This acts as a sanity check to ensure the simulation is accurate.
fn check_simulation_is_like_reality(
    initial_pool: &Pool,
    pool_transactions: &[&SwapTransaction],
    victim: &SwapTransaction,
) -> bool {
    let before_victim_transactions: Vec<&&SwapTransaction> = pool_transactions
        .iter()
        .filter(|tx| tx.tx_position_in_block < victim.tx_position_in_block)
        .collect();

    let mut current_pool = initial_pool.clone();
    for tx in before_victim_transactions {
        let simulation = current_pool.simulate_swap(tx);
        current_pool = simulation.new_pool_state;
    }

    let victim_simulation = current_pool.simulate_swap(victim);

    let actual_amount_out = victim.amount_out;
    let simulated_amount_out = victim_simulation.tokens_received;
    let difference_percentage =
        ((actual_amount_out - simulated_amount_out) / actual_amount_out * 100.0).abs();

    return difference_percentage < 1.0;
}

/// Try and simulate what actually happens during the block
/// if we'd remove the would-be front and back transactions.
/// This will allow us to later test if there was any price impact
/// to the victim's transaction.
fn simulate_without_attacker(
    initial_pool: &Pool,
    pool_transactions: &[&SwapTransaction],
    front: &SwapTransaction,
    victim: &SwapTransaction,
) -> f64 {
    let no_attacker_before_victim_txns: Vec<&&SwapTransaction> = pool_transactions
        .iter()
        .filter(|tx| tx.tx_position_in_block < victim.tx_position_in_block)
        .filter(|tx| tx.tx_position_in_block != front.tx_position_in_block)
        .collect();

    let mut current_pool = initial_pool.clone();
    for tx in no_attacker_before_victim_txns {
        let simulation = current_pool.simulate_swap(tx);
        current_pool = simulation.new_pool_state;
    }

    let victim_simulation = current_pool.simulate_swap(victim);

    let actual_amount_out = victim.amount_out;
    let simulated_amount_out = victim_simulation.tokens_received;
    return ((actual_amount_out - simulated_amount_out) / actual_amount_out * 100.0).abs();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandwich::transactions::SwapTransaction;
    use std::collections::HashMap;
    use std::fs;

    fn load_sample_transactions() -> Vec<SwapTransaction> {
        let csv_content =
            fs::read_to_string("data/sandwiches.csv").expect("Failed to read sample CSV file");

        let mut reader = csv::Reader::from_reader(csv_content.as_bytes());
        let mut transactions = Vec::new();

        for result in reader.deserialize() {
            let transaction: SwapTransaction = result.expect("Failed to parse CSV row");
            transactions.push(transaction);
        }

        transactions
    }

    #[test]
    fn test_detect_sandwich_attacks_with_sample_data() {
        let mut pool_map = HashMap::new();
        pool_map.insert(
            "0xpool1".to_string(),
            Pool::new(
                1000000.0,
                50000000000.0,
                "USDC".to_string(),
                "SHIB".to_string(),
            ),
        );
        pool_map.insert(
            "0xpool_uniswap".to_string(),
            Pool::new(800.0, 800000.0, "ETH".to_string(), "NEWTOKEN".to_string()),
        );
        pool_map.insert(
            "0xpool_sushiswap".to_string(),
            Pool::new(850.0, 850000.0, "ETH".to_string(), "NEWTOKEN".to_string()),
        );
        pool_map.insert(
            "0xpool_usdt".to_string(),
            Pool::new(
                1000000.0,
                50000000000.0,
                "USDT".to_string(),
                "SHIB".to_string(),
            ),
        );

        let transactions = load_sample_transactions();
        let all_attacks = find_sandwich_attacks_by_simulation(&pool_map, &transactions);

        // Should find exactly the same attacks as heuristics method
        assert_eq!(
            all_attacks.len(),
            6,
            "Expected 6 sandwich attacks from simulation"
        );

        // Block 12360: Basic USDC/SHIB sandwich - should detect victim loss
        let block_12360_attack = all_attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12360)
            .expect("Should find attack in block 12360");

        assert_eq!(block_12360_attack.front_run_tx.from_address, "0xattacker1");
        assert_eq!(block_12360_attack.victim_tx.from_address, "0xvictim1");
        assert_eq!(block_12360_attack.back_run_tx.from_address, "0xattacker1");

        // Victim should experience measurable slippage loss from front-running
        assert!(
            block_12360_attack.victim_loss_percentage > 0.5,
            "Victim should lose at least 0.5% due to front-running"
        );

        // Block 12361: ETH/NEWTOKEN sandwich with high-value victim
        let block_12361_attack = all_attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12361)
            .expect("Should find attack in block 12361");

        assert_eq!(block_12361_attack.front_run_tx.from_address, "0xbot123");
        assert_eq!(block_12361_attack.victim_tx.from_address, "0xinnocent");
        assert_eq!(block_12361_attack.back_run_tx.from_address, "0xbot123");

        // Large victim transaction should show significant impact
        assert!(
            block_12361_attack.victim_loss_percentage > 1.0,
            "Large victim transaction should show >1% loss"
        );

        // Block 12362: USDC/SHIB sandwich with unrelated transactions
        let block_12362_attack = all_attacks
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

        // Medium-sized attack should show moderate loss
        assert!(
            block_12362_attack.victim_loss_percentage > 0.3,
            "Medium victim should lose >0.3%"
        );
        assert!(
            block_12362_attack.victim_loss_percentage < 2.0,
            "But not excessive loss"
        );

        // Block 12364: Token equivalence test (USDC/USDT)
        let block_12364_attack = all_attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12364)
            .expect("Should find stablecoin equivalence attack in block 12364");

        assert_eq!(block_12364_attack.front_run_tx.from_address, "0xstable_bot");
        assert_eq!(block_12364_attack.victim_tx.from_address, "0xlegit_user");
        assert_eq!(block_12364_attack.back_run_tx.from_address, "0xstable_bot");

        // Even with stablecoin equivalence, victim should experience loss
        assert!(
            block_12364_attack.victim_loss_percentage > 0.2,
            "Stablecoin sandwich should still cause victim loss"
        );

        // Block 12365: ETH/WETH equivalence test
        let block_12365_attack = all_attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12365)
            .expect("Should find ETH/WETH equivalence attack in block 12365");

        assert_eq!(block_12365_attack.front_run_tx.from_address, "0xweth_mev");
        assert_eq!(block_12365_attack.victim_tx.from_address, "0xeth_holder");
        assert_eq!(block_12365_attack.back_run_tx.from_address, "0xweth_mev");

        // Block 12366: Cross-DEX sandwich (legitimate)
        let block_12366_attack = all_attacks
            .iter()
            .find(|a| a.front_run_tx.block_number == 12366)
            .expect("Should find cross-DEX attack in block 12366");

        assert_eq!(block_12366_attack.front_run_tx.from_address, "0xlegit_mev");
        assert_eq!(block_12366_attack.victim_tx.from_address, "0xinnocent_dex");
        assert_eq!(block_12366_attack.back_run_tx.from_address, "0xlegit_mev");

        // Cross-DEX attack should show measurable victim impact
        assert!(
            block_12366_attack.victim_loss_percentage > 0.4,
            "Cross-DEX victim should experience significant loss"
        );

        // Verify all attacks have reasonable victim loss percentages
        for attack in &all_attacks {
            assert!(
                attack.victim_loss_percentage >= 0.0,
                "Victim loss percentage should not be negative"
            );
            assert!(
                attack.victim_loss_percentage <= 10.0,
                "Victim loss percentage should be reasonable (<10%)"
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

        println!(
            "Simulation detected {} sandwich attacks:",
            all_attacks.len()
        );
        for (i, attack) in all_attacks.iter().enumerate() {
            println!(
                "Attack {}: Block {} - Victim Loss: {:.3}%",
                i + 1,
                attack.front_run_tx.block_number,
                attack.victim_loss_percentage
            );
            println!(
                "  Front: {} ({} -> {})",
                attack.front_run_tx.tx_hash,
                attack.front_run_tx.token_in,
                attack.front_run_tx.token_out
            );
            println!(
                "  Victim: {} ({} -> {})",
                attack.victim_tx.tx_hash, attack.victim_tx.token_in, attack.victim_tx.token_out
            );
            println!(
                "  Back: {} ({} -> {})",
                attack.back_run_tx.tx_hash,
                attack.back_run_tx.token_in,
                attack.back_run_tx.token_out
            );
        }
    }
}
