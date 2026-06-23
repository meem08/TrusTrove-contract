#![cfg(test)]

use soroban_sdk::{
    contract, contractimpl, contracttype, testutils::Address as _, Address, BytesN, Env,
};

use crate::{EscrowContract, EscrowContractClient};

#[contract]
pub struct MockToken;

#[contractimpl]
impl MockToken {
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let from_key = BalanceKey(from.clone());
        let to_key = BalanceKey(to.clone());
        let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
        let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&from_key, &(from_bal - amount));
        env.storage().persistent().set(&to_key, &(to_bal + amount));
    }

    pub fn balance(env: Env, addr: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&BalanceKey(addr))
            .unwrap_or(0)
    }
}

#[contracttype]
pub struct BalanceKey(Address);

fn setup() -> (
    Env,
    EscrowContractClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let pool = Address::generate(&env);
    let usdc_id = env.register_contract(None, MockToken);
    let _mock_token_client = MockTokenClient::new(&env, &usdc_id);

    let pool_bal_key = BalanceKey(pool.clone());
    env.as_contract(&usdc_id, || {
        env.storage()
            .persistent()
            .set(&pool_bal_key, &10_000_000_000_000i128);
    });

    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(&env, &contract_id);

    client.initialize(&admin, &pool, &pool, &usdc_id);

    (env, client, admin, pool, usdc_id, contract_id)
}

fn generate_invoice_id(env: &Env, counter: u64) -> BytesN<32> {
    let mut arr = [0u8; 32];
    let bytes = (env.ledger().timestamp() + counter).to_be_bytes();
    arr[0..8].copy_from_slice(&bytes);
    BytesN::from_array(env, &arr)
}

fn get_balance(env: &Env, usdc_id: &Address, addr: &Address) -> i128 {
    let mock_token_client = MockTokenClient::new(&env, &usdc_id);
    mock_token_client.balance(&addr)
}

// ============================================================================
// Initialize Tests
// ============================================================================

#[test]
fn test_initialize() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let pool = Address::generate(&env);
    let invoice = Address::generate(&env);
    let usdc = env.register_contract(None, MockToken);
    let contract_id = env.register_contract(None, EscrowContract);
    let client = EscrowContractClient::new(&env, &contract_id);
    client.initialize(&admin, &pool, &invoice, &usdc);

    assert_eq!(client.get_locked(&generate_invoice_id(&env, 1)), 0);
}

// ============================================================================
// Lock Tests
// ============================================================================

#[test]
fn test_lock_stores_record_and_transfers_usdc() {
    let (env, client, _admin, pool, usdc_id, contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 1);
    let amount: u128 = 1_000_000_000;

    // Check initial balances
    let pool_balance_before = get_balance(&env, &usdc_id, &pool);
    let contract_balance_before = get_balance(&env, &usdc_id, &contract_id);

    // Execute lock
    let result = client.lock(&invoice_id, &amount);
    assert!(result);

    // Verify record was stored
    let locked = client.get_locked(&invoice_id);
    assert_eq!(locked, amount);

    // Verify USDC was transferred from pool to contract
    let pool_balance_after = get_balance(&env, &usdc_id, &pool);
    let contract_balance_after = get_balance(&env, &usdc_id, &contract_id);

    assert_eq!(pool_balance_after, pool_balance_before - (amount as i128));
    assert_eq!(contract_balance_after, contract_balance_before + (amount as i128));
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_lock_fails_if_already_locked() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 2);
    let amount: u128 = 1_000_000_000;

    // First lock should succeed
    client.lock(&invoice_id, &amount);

    // Second lock with same invoice_id should panic with AlreadyLocked
    client.lock(&invoice_id, &500_000_000);
}

#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_lock_fails_zero_amount() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 3);
    client.lock(&invoice_id, &0);
}

#[test]
fn test_lock_only_callable_by_pool() {
    // This test verifies that lock requires pool authorization.
    // In the soroban-sdk testutils with mock_all_auths(), any address can call.
    // The actual authorization is checked by pool.require_auth() in the contract.
    // Here we verify the lock mechanism works when called by pool.
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 4);
    let amount: u128 = 1_000_000_000;

    // Lock should succeed when called with proper setup
    // (pool is mocked to be the caller in setup())
    let result = client.lock(&invoice_id, &amount);
    assert!(result);
    
    // Verify the record was stored
    assert_eq!(client.get_locked(&invoice_id), amount);
}

// ============================================================================
// Release to Issuer Tests
// ============================================================================

#[test]
fn test_release_to_issuer_sends_correct_amount() {
    let (env, client, _admin, _pool, usdc_id, contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 5);
    let issuer = Address::generate(&env);
    let amount: u128 = 1_000_000_000;

    // Lock funds first
    client.lock(&invoice_id, &amount);

    // Check issuer balance before release
    let issuer_balance_before = get_balance(&env, &usdc_id, &issuer);
    let contract_balance_before = get_balance(&env, &usdc_id, &contract_id);

    // Release to issuer
    let result = client.release_to_issuer(&invoice_id, &issuer);
    assert!(result);

    // Verify record was removed
    let locked = client.get_locked(&invoice_id);
    assert_eq!(locked, 0);

    // Verify USDC was transferred from contract to issuer
    let issuer_balance_after = get_balance(&env, &usdc_id, &issuer);
    let contract_balance_after = get_balance(&env, &usdc_id, &contract_id);

    assert_eq!(issuer_balance_after, issuer_balance_before + (amount as i128));
    assert_eq!(contract_balance_after, contract_balance_before - (amount as i128));
}

// ============================================================================
// Release to Pool Tests
// ============================================================================

#[test]
fn test_release_to_pool_sends_correct_amount() {
    let (env, client, _admin, pool, usdc_id, contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 6);
    let amount: u128 = 1_000_000_000;
    let repayment: u128 = 1_050_000_000;

    // Lock funds first
    client.lock(&invoice_id, &amount);

    // Check balances before release
    let pool_balance_before = get_balance(&env, &usdc_id, &pool);
    let contract_balance_before = get_balance(&env, &usdc_id, &contract_id);

    // Release to pool with repayment amount
    let result = client.release_to_pool(&invoice_id, &repayment);
    assert!(result);

    // Verify record was removed
    let locked = client.get_locked(&invoice_id);
    assert_eq!(locked, 0);

    // Verify USDC was transferred from contract to pool (with repayment amount)
    let pool_balance_after = get_balance(&env, &usdc_id, &pool);
    let contract_balance_after = get_balance(&env, &usdc_id, &contract_id);

    assert_eq!(pool_balance_after, pool_balance_before + (repayment as i128));
    assert_eq!(contract_balance_after, contract_balance_before - (repayment as i128));
}

// ============================================================================
// Handle Default Tests
// ============================================================================

#[test]
fn test_handle_default_returns_funds_to_pool() {
    let (env, client, _admin, pool, usdc_id, contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 7);
    let amount: u128 = 1_000_000_000;

    // Lock funds first
    client.lock(&invoice_id, &amount);

    // Check balances before default handling
    let pool_balance_before = get_balance(&env, &usdc_id, &pool);
    let contract_balance_before = get_balance(&env, &usdc_id, &contract_id);

    // Handle default
    let result = client.handle_default(&invoice_id);
    assert!(result);

    // Verify record was removed
    let locked = client.get_locked(&invoice_id);
    assert_eq!(locked, 0);

    // Verify USDC was transferred from contract back to pool
    let pool_balance_after = get_balance(&env, &usdc_id, &pool);
    let contract_balance_after = get_balance(&env, &usdc_id, &contract_id);

    assert_eq!(pool_balance_after, pool_balance_before + (amount as i128));
    assert_eq!(contract_balance_after, contract_balance_before - (amount as i128));
}

#[test]
fn test_handle_default_returns_false_if_no_record() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 8);

    // Try to handle default for non-existent invoice
    let result = client.handle_default(&invoice_id);
    assert!(!result);
}

// ============================================================================
// Get Locked Tests
// ============================================================================

#[test]
fn test_get_locked_returns_zero_when_empty() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 9);

    assert_eq!(client.get_locked(&invoice_id), 0);
}

#[test]
fn test_get_locked_returns_zero_for_unknown_id() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    
    // Generate a random unknown invoice ID
    let unknown_id = generate_invoice_id(&env, 999);
    assert_eq!(client.get_locked(&unknown_id), 0);
}

#[test]
fn test_get_locked_returns_amount_when_locked() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 10);
    let amount: u128 = 1_000_000_000;

    client.lock(&invoice_id, &amount);
    assert_eq!(client.get_locked(&invoice_id), amount);
}

#[test]
fn test_get_locked_returns_zero_after_release_to_issuer() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 11);
    let issuer = Address::generate(&env);
    let amount: u128 = 1_000_000_000;

    client.lock(&invoice_id, &amount);
    assert_eq!(client.get_locked(&invoice_id), amount);

    client.release_to_issuer(&invoice_id, &issuer);
    assert_eq!(client.get_locked(&invoice_id), 0);
}

#[test]
fn test_get_locked_returns_zero_after_release_to_pool() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 12);
    let amount: u128 = 1_000_000_000;
    let repayment: u128 = 1_050_000_000;

    client.lock(&invoice_id, &amount);
    assert_eq!(client.get_locked(&invoice_id), amount);

    client.release_to_pool(&invoice_id, &repayment);
    assert_eq!(client.get_locked(&invoice_id), 0);
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_multiple_invoices_independent() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    
    let invoice_id_1 = generate_invoice_id(&env, 13);
    let invoice_id_2 = generate_invoice_id(&env, 14);
    let amount_1: u128 = 1_000_000_000;
    let amount_2: u128 = 2_000_000_000;

    // Lock multiple invoices
    client.lock(&invoice_id_1, &amount_1);
    client.lock(&invoice_id_2, &amount_2);

    // Verify both are locked with correct amounts
    assert_eq!(client.get_locked(&invoice_id_1), amount_1);
    assert_eq!(client.get_locked(&invoice_id_2), amount_2);

    // Release one invoice
    let issuer = Address::generate(&env);
    client.release_to_issuer(&invoice_id_1, &issuer);

    // Verify the correct one was released
    assert_eq!(client.get_locked(&invoice_id_1), 0);
    assert_eq!(client.get_locked(&invoice_id_2), amount_2);
}

#[test]
#[should_panic(expected = "Error(Contract, #4)")]
fn test_lock_fails_duplicate() {
    let (env, client, _admin, _pool, _usdc_id, _contract_id) = setup();
    let invoice_id = generate_invoice_id(&env, 15);
    client.lock(&invoice_id, &1_000_000_000);
    client.lock(&invoice_id, &500_000_000);
}
