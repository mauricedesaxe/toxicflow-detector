use crate::sandwich::transactions::SwapTransaction;
use crate::sandwich::utils::is_sandwich_pattern;

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

/// Simulates different transaction orderings to detect sandwich attacks
pub fn detect_sandwich_attacks_by_simulation(
    initial_pool: &Pool,
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
                    match simulate_sandwich_attack(initial_pool, front, victim, back, &transactions)
                    {
                        Ok(attack) => detected_attacks.push(attack),
                        Err(error) => println!("Sandwich simulation error: {}", error),
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

    fn load_sample_transactions() -> Vec<SwapTransaction> {
        let csv_content = include_str!("../../data/sandwiches.csv");
        let mut transactions = Vec::new();

        for (i, line) in csv_content.lines().enumerate() {
            if i == 0 {
                continue;
            } // Skip header

            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() >= 16 {
                transactions.push(SwapTransaction {
                    tx_hash: fields[0].to_string(),
                    block_number: fields[1].parse().unwrap_or(0),
                    timestamp: fields[2].parse().unwrap_or(0),
                    tx_position_in_block: fields[3].parse().unwrap_or(0),
                    from_address: fields[4].to_string(),
                    token_in: fields[5].to_string(),
                    token_out: fields[6].to_string(),
                    amount_in: fields[7].parse().unwrap_or(0.0),
                    amount_out: fields[8].parse().unwrap_or(0.0),
                    gas_price: fields[9].parse().unwrap_or(0),
                    pool_address: fields[10].to_string(),
                    token_launch_block: fields[11].parse().unwrap_or(0),
                    is_contract_caller: fields[12] == "true",
                    usd_value_in: fields[13].parse().unwrap_or(0.0),
                    usd_value_out: fields[14].parse().unwrap_or(0.0),
                    gas_cost_usd: fields[15].parse().unwrap_or(0.0),
                });
            }
        }

        transactions
    }

    fn create_test_pools() -> HashMap<String, Pool> {
        let mut pools = HashMap::new();

        // Pool1 (USDC/SHIB) - established token pair with realistic reserves
        pools.insert(
            "0xpool1".to_string(),
            Pool::new(
                500000.0,      // USDC reserve
                25000000000.0, // SHIB reserve (price around 0.00002)
                "USDC".to_string(),
                "SHIB".to_string(),
            ),
        );

        // Pool4 (USDC/NEWTOKEN) - new token launch with smaller initial liquidity
        pools.insert(
            "0xpool4".to_string(),
            Pool::new(
                50000.0,   // USDC reserve
                5000000.0, // NEWTOKEN reserve (price around 0.01)
                "USDC".to_string(),
                "NEWTOKEN".to_string(),
            ),
        );

        // Pool2 (ETH/USDC) - major pair
        pools.insert(
            "0xpool2".to_string(),
            Pool::new(
                1000.0,    // ETH reserve
                3200000.0, // USDC reserve
                "ETH".to_string(),
                "USDC".to_string(),
            ),
        );

        pools
    }

    #[test]
    fn test_pool_simulation_basic() {
        let pool = Pool::new(
            1000.0, // Token A reserve
            2000.0, // Token B reserve
            "TOKENA".to_string(),
            "TOKENB".to_string(),
        );

        let swap = SwapTransaction {
            tx_hash: "test".to_string(),
            block_number: 12345,
            timestamp: 1640995200,
            tx_position_in_block: 1,
            from_address: "0xtest".to_string(),
            token_in: "TOKENA".to_string(),
            token_out: "TOKENB".to_string(),
            amount_in: 100.0,
            amount_out: 181.8, // Expected from constant product
            gas_price: 100,
            pool_address: "0xpool_test".to_string(),
            token_launch_block: 12000,
            is_contract_caller: false,
            usd_value_in: 100.0,
            usd_value_out: 181.8,
            gas_cost_usd: 5.0,
        };

        let result = pool.simulate_swap(&swap);

        // Should receive approximately 181.8 tokens based on constant product formula
        assert!((result.tokens_received - 181.81).abs() < 0.1);
        assert!(result.slippage > 0.0);

        // Pool state should update correctly
        assert!((result.new_pool_state.token_a_reserve - 1100.0).abs() < 0.1);
        assert!((result.new_pool_state.token_b_reserve - 1818.18).abs() < 0.1);
    }

    #[test]
    fn test_detect_sandwich_attacks_with_sample_data() {
        let transactions = load_sample_transactions();
        let test_pools = create_test_pools();

        // Group transactions by block
        let mut blocks: HashMap<u64, Vec<SwapTransaction>> = HashMap::new();
        for tx in transactions {
            blocks
                .entry(tx.block_number)
                .or_insert_with(Vec::new)
                .push(tx);
        }

        let mut total_attacks_found = 0;
        let mut attack_details = Vec::new();

        for (block_num, block_txs) in blocks.iter() {
            if let Some(pool) = test_pools.get(&block_txs[0].pool_address) {
                let attacks = detect_sandwich_attacks_by_simulation(pool, block_txs);
                total_attacks_found += attacks.len();

                for attack in attacks {
                    attack_details.push(format!(
                        "Block {}: Front: {} -> Victim: {} -> Back: {}, Loss: {:.2}%",
                        block_num,
                        attack.front_run_tx.tx_hash,
                        attack.victim_tx.tx_hash,
                        attack.back_run_tx.tx_hash,
                        attack.victim_loss_percentage
                    ));
                }
            }
        }

        // Should find some sandwich attacks from our test data
        assert!(
            total_attacks_found > 0,
            "Should detect some sandwich attacks in test data"
        );

        // Print results for debugging
        println!("Found {} sandwich attacks:", total_attacks_found);
        for detail in attack_details {
            println!("  {}", detail);
        }
    }

    #[test]
    fn test_known_sandwich_pattern() {
        let pool1 = Pool::new(
            1000000.0,     // USDC reserve
            50000000000.0, // SHIB reserve
            "USDC".to_string(),
            "SHIB".to_string(),
        );

        // Create known sandwich from CSV data - block 12360
        let front_tx = SwapTransaction {
            tx_hash: "0xsandwich1".to_string(),
            block_number: 12360,
            timestamp: 1640995400,
            tx_position_in_block: 1,
            from_address: "0xattacker1".to_string(),
            token_in: "USDC".to_string(),
            token_out: "SHIB".to_string(),
            amount_in: 1000.0,
            amount_out: 49950050.0,
            gas_price: 140,
            pool_address: "0xpool1".to_string(),
            token_launch_block: 12340,
            is_contract_caller: false,
            usd_value_in: 1000.0,
            usd_value_out: 999.0,
            gas_cost_usd: 48.0,
        };

        let victim_tx = SwapTransaction {
            tx_hash: "0xvictim001".to_string(),
            block_number: 12360,
            timestamp: 1640995400,
            tx_position_in_block: 2,
            from_address: "0xvictim1".to_string(),
            token_in: "USDC".to_string(),
            token_out: "SHIB".to_string(),
            amount_in: 5000.0,
            amount_out: 248260656.0,
            gas_price: 120,
            pool_address: "0xpool1".to_string(),
            token_launch_block: 12340,
            is_contract_caller: false,
            usd_value_in: 5000.0,
            usd_value_out: 4963.0,
            gas_cost_usd: 57.6,
        };

        let back_tx = SwapTransaction {
            tx_hash: "0xsandwich2".to_string(),
            block_number: 12360,
            timestamp: 1640995400,
            tx_position_in_block: 3,
            from_address: "0xattacker1".to_string(),
            token_in: "SHIB".to_string(),
            token_out: "USDC".to_string(),
            amount_in: 49950050.0,
            amount_out: 950.0,
            gas_price: 80,
            pool_address: "0xpool1".to_string(),
            token_launch_block: 12340,
            is_contract_caller: false,
            usd_value_in: 999.0,
            usd_value_out: 950.0,
            gas_cost_usd: 72.0,
        };

        let block_transactions = vec![front_tx, victim_tx, back_tx];
        let attacks = detect_sandwich_attacks_by_simulation(&pool1, &block_transactions);

        assert!(attacks.len() > 0, "Should detect the known sandwich attack");

        let attack = &attacks[0];
        assert_eq!(attack.front_run_tx.tx_hash, "0xsandwich1");
        assert_eq!(attack.victim_tx.tx_hash, "0xvictim001");
        assert_eq!(attack.back_run_tx.tx_hash, "0xsandwich2");

        // Victim should have some measurable loss
        assert!(attack.victim_loss_percentage > 0.0);

        println!("Detected sandwich attack:");
        println!("  Victim loss: {:.2}%", attack.victim_loss_percentage);
    }

    #[test]
    fn test_constant_product_formula() {
        let pool = Pool::new(1000.0, 2000.0, "A".to_string(), "B".to_string());

        // Test constant product: (x + dx) * (y - dy) = x * y
        let dx = 100.0;
        let dy = pool.constant_product_formula(1000.0, 2000.0, dx);

        // Should be approximately 181.8 based on (1000 * 2000) = (1100 * (2000 - dy))
        assert!((dy - 181.81).abs() < 0.1);

        // Verify constant product is maintained
        let original_k = 1000.0 * 2000.0;
        let new_k = (1000.0 + dx) * (2000.0 - dy);
        assert!((original_k - new_k).abs() < 0.1);
    }
}
