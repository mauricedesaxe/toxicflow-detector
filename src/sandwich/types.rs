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
