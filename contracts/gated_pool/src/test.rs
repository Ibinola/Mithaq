use super::*;
use proptest::prelude::*;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _},
    token, vec, Address, Env, IntoVal,
};
use test_support::{Contracts, GatedPoolClient, FUNDS, KYC};

// Deploy-and-wire, register-issuer, set-vk, and build-valid-submission all
// live in `test_support`. The artifacts are still the real checked-in
// UltraHonk fixtures, so the KYC gate exercises genuine verification.

/// A deployed stack plus a GatedPool gated on `required_type`, and the issuer
/// trusted for the demo KYC/funds fixtures (both signed by the same key).
struct Pool {
    c: Contracts,
    pool: GatedPoolClient<'static>,
    issuer: Address,
}

fn deploy_with_gate(env: &Env, required_type: &str, min_threshold: Option<u64>) -> Pool {
    let c = Contracts::deploy(env);
    let issuer = c.register_issuer_for(env, &KYC, &["kyc", "funds"]);
    c.enable_vks(env, &[&KYC, &FUNDS]);
    let pool = c.deploy_pool(env, required_type, min_threshold);
    Pool { c, pool, issuer }
}

fn deploy(env: &Env) -> Pool {
    deploy_with_gate(env, "kyc", None)
}

#[test]
fn deposit_blocked_without_kyc() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    let res = h.pool.try_deposit(&user, &100);
    assert!(res.is_err());
    assert_eq!(h.pool.get_balance(&user), 0);
}

#[test]
fn deposit_allowed_after_kyc() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
    h.pool.deposit(&user, &100);
    assert_eq!(h.pool.get_balance(&user), 100);
}

#[test]
fn gate_config_is_stored_and_threshold_gated_deposit_is_enforced() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy_with_gate(&env, "funds", Some(50_000));
    let user = Address::generate(&env);

    assert_eq!(h.pool.gate(), (symbol_short!("funds"), Some(50_000)));

    h.c.submit(&env, &user, &h.issuer, &FUNDS, 9999);
    h.pool.deposit(&user, &100);
    assert_eq!(h.pool.get_balance(&user), 100);

    let strict_h = deploy_with_gate(&env, "funds", Some(250_000));
    let strict_user = Address::generate(&env);
    strict_h.c.submit(&env, &strict_user, &strict_h.issuer, &FUNDS, 9999);
    let res = strict_h.pool.try_deposit(&strict_user, &100);
    assert!(res.is_err());
    assert_eq!(strict_h.pool.get_balance(&strict_user), 0);
}

#[test]
fn withdraw_is_open() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
    h.pool.deposit(&user, &100);
    h.pool.withdraw(&user, &40);
    assert_eq!(h.pool.get_balance(&user), 60);
}

#[test]
fn withdraw_exact_balance_leaves_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
    h.pool.deposit(&user, &100);
    h.pool.withdraw(&user, &100);

    assert_eq!(h.pool.get_balance(&user), 0);
}

#[test]
fn withdraw_rejects_zero_and_negative_amounts() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
    h.pool.deposit(&user, &100);

    assert!(h.pool.try_withdraw(&user, &0).is_err());
    assert!(h.pool.try_withdraw(&user, &-1).is_err());
    assert_eq!(h.pool.get_balance(&user), 100);
}

#[test]
fn withdraw_remains_available_after_kyc_expires() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    h.c.submit(&env, &user, &h.issuer, &KYC, 2);
    h.pool.deposit(&user, &100);

    env.ledger().with_mut(|li| li.timestamp = 3);
    assert!(!h.c.verify(&env, &user, &KYC));

    h.pool.withdraw(&user, &100);
    assert_eq!(h.pool.get_balance(&user), 0);
}

#[test]
fn withdraw_remains_available_after_kyc_is_revoked() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
    h.pool.deposit(&user, &100);
    h.c.registry.revoke(&h.issuer, &user, &symbol_short!("kyc"), &None);
    assert!(!h.c.verify(&env, &user, &KYC));

    h.pool.withdraw(&user, &100);
    assert_eq!(h.pool.get_balance(&user), 0);
}

// ── Property-based tests ──────────────────────────────────

/// Property: Deposits are gated behind a valid KYC proof.
/// For any holder, if no valid KYC proof exists, deposit must fail.
/// After submitting a valid proof, deposit must succeed.
#[test]
fn prop_deposit_gated_by_kyc() {
    let config = proptest::test_runner::Config {
        cases: 10,
        ..proptest::test_runner::Config::default()
    };
    let mut runner = proptest::test_runner::TestRunner::new(config);
    runner
        .run(&(0u64..u64::MAX), |_seed| {
            let env = Env::default();
            env.mock_all_auths();
            let h = deploy(&env);
            let user = Address::generate(&env);

            // Without a KYC proof, deposit must be rejected.
            let res = h.pool.try_deposit(&user, &100);
            prop_assert!(res.is_err(), "Deposit without KYC must fail");
            prop_assert_eq!(h.pool.get_balance(&user), 0);

            // After getting KYC, deposit must succeed.
            h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
            h.pool.deposit(&user, &100);
            prop_assert_eq!(h.pool.get_balance(&user), 100);
            Ok(())
        })
        .unwrap();
}

#[test]
fn withdraw_rejects_amount_exceeding_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
    h.pool.deposit(&user, &100);

    let res = h.pool.try_withdraw(&user, &101);
    assert!(res.is_err());
    // Balance is unaffected by the rejected withdrawal.
    assert_eq!(h.pool.get_balance(&user), 100);
}

#[test]
fn registry_address_matches_constructor_provided_registry() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    assert_eq!(h.pool.registry_address(), h.c.registry.address);
}

#[test]
fn deposit_uses_constructor_provided_registry_not_an_unrelated_one() {
    // Deploys a SECOND, independent ProofRegistry (with its own issuer/verifier)
    // and proves KYC there for `user` — while the pool remains wired to the
    // FIRST registry from `deploy()`, where `user` has no proof. This proves the
    // gate actually consults the constructor-provided `registry` address, not
    // some other reachable/default registry.
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let other = deploy(&env);
    let user = Address::generate(&env);

    other.c.submit(&env, &user, &other.issuer, &KYC, 1_000_000);

    let res = h.pool.try_deposit(&user, &100);
    assert!(res.is_err());
    assert_eq!(h.pool.get_balance(&user), 0);

    // Sanity: the same proof against the pool's OWN registry succeeds.
    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
    h.pool.deposit(&user, &100);
    assert_eq!(h.pool.get_balance(&user), 100);
}

#[test]
fn deposit_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
    h.pool.deposit(&user, &250);

    assert_eq!(
        env.events().all().filter_by_contract(&h.pool.address),
        vec![
            &env,
            (
                h.pool.address.clone(),
                (symbol_short!("gate_pool"), symbol_short!("deposit")).into_val(&env),
                EventDeposit {
                    caller: user.clone(),
                    amount: 250,
                    new_balance: 250,
                }
                .into_val(&env),
            ),
        ],
    );

    // Verify balance was updated
    assert_eq!(h.pool.get_balance(&user), 250);
}

#[test]
fn withdraw_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);

    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);
    h.pool.deposit(&user, &500);

    // Drain deposit events
    let _ = env.events().all();

    h.pool.withdraw(&user, &200);

    assert_eq!(
        env.events().all().filter_by_contract(&h.pool.address),
        vec![
            &env,
            (
                h.pool.address.clone(),
                (symbol_short!("gate_pool"), symbol_short!("withdraw")).into_val(&env),
                EventWithdraw {
                    caller: user.clone(),
                    amount: 200,
                    new_balance: 300,
                }
                .into_val(&env),
            ),
        ],
    );

    // Verify balance was updated
    assert_eq!(h.pool.get_balance(&user), 300);
}

struct TokenHarness {
    c: Contracts,
    pool: GatedPoolClient<'static>,
    token: token::Client<'static>,
    token_admin: token::StellarAssetClient<'static>,
    issuer: Address,
}

fn deploy_with_token(env: &Env) -> TokenHarness {
    let c = Contracts::deploy(env);
    let issuer = c.register_issuer_for(env, &KYC, &["kyc", "funds"]);
    c.enable_vks(env, &[&KYC, &FUNDS]);

    let token_admin = Address::generate(env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_addr = token_contract.address();

    let pool_id = env.register(
        GatedPool,
        (
            c.registry.address.clone(),
            symbol_short!("kyc"),
            None::<Option<u64>>,
            Some(token_addr.clone()),
        ),
    );

    TokenHarness {
        c,
        pool: GatedPoolClient::new(env, &pool_id),
        token: token::Client::new(env, &token_addr),
        token_admin: token::StellarAssetClient::new(env, &token_addr),
        issuer,
    }
}

#[test]
fn test_token_address_query() {
    let env = Env::default();
    env.mock_all_auths();
    let th = deploy_with_token(&env);
    assert_eq!(th.pool.token_address(), Some(th.token.address));

    let h = deploy(&env);
    assert_eq!(h.pool.token_address(), None);
}

#[test]
fn test_real_token_deposit_and_withdraw_success() {
    let env = Env::default();
    env.mock_all_auths();
    let th = deploy_with_token(&env);
    let user = Address::generate(&env);

    // Mint 1000 tokens to user
    th.token_admin.mint(&user, &1000);
    assert_eq!(th.token.balance(&user), 1000);
    assert_eq!(th.token.balance(&th.pool.address), 0);

    // Submit KYC proof to registry
    th.c.submit(&env, &user, &th.issuer, &KYC, 1_000_000);

    // User deposits 400 into pool
    th.pool.deposit(&user, &400);

    // Token balances transferred
    assert_eq!(th.token.balance(&user), 600);
    assert_eq!(th.token.balance(&th.pool.address), 400);
    assert_eq!(th.pool.get_balance(&user), 400);

    // User withdraws 150 from pool
    th.pool.withdraw(&user, &150);

    // Token balances updated
    assert_eq!(th.token.balance(&user), 750);
    assert_eq!(th.token.balance(&th.pool.address), 250);
    assert_eq!(th.pool.get_balance(&user), 250);
}

#[test]
#[should_panic]
fn test_real_token_deposit_fails_when_user_has_insufficient_tokens() {
    let env = Env::default();
    env.mock_all_auths();
    let th = deploy_with_token(&env);
    let user = Address::generate(&env);

    // Mint only 50 tokens
    th.token_admin.mint(&user, &50);

    th.c.submit(&env, &user, &th.issuer, &KYC, 1_000_000);

    // Attempting to deposit 100 should panic / fail due to token balance
    th.pool.deposit(&user, &100);
}

#[test]
#[should_panic]
fn test_real_token_withdraw_fails_when_pool_has_insufficient_tokens() {
    let env = Env::default();
    env.mock_all_auths();
    let th = deploy_with_token(&env);
    let user = Address::generate(&env);

    th.token_admin.mint(&user, &200);

    th.c.submit(&env, &user, &th.issuer, &KYC, 1_000_000);

    th.pool.deposit(&user, &200);
    assert_eq!(th.pool.get_balance(&user), 200);

    // Forcibly drain the pool's token balance to simulate token shortage
    let thief = Address::generate(&env);
    th.token.transfer(&th.pool.address, &thief, &200);
    assert_eq!(th.token.balance(&th.pool.address), 0);

    // User attempts to withdraw: should fail because pool doesn't have the tokens
    th.pool.withdraw(&user, &100);
}



// ── CONTRACT-29: token transfer invariants ───────────────────────────────────

/// Sum of the pool's per-user balances for `users`.
fn tracked_total(th: &TokenHarness, users: &[Address]) -> i128 {
    users.iter().map(|u| th.pool.get_balance(u)).sum()
}

/// Property: after any sequence of deposits and withdrawals (including ones that fail), the
/// pool's token holdings equal the sum of per-user balances, and no tokens are created or lost.
#[test]
fn prop_pool_token_holdings_match_user_balances() {
    let config = proptest::test_runner::Config {
        cases: 8,
        ..proptest::test_runner::Config::default()
    };
    let mut runner = proptest::test_runner::TestRunner::new(config);
    let ops = proptest::collection::vec((0usize..3, any::<bool>(), 1i128..700), 1..10);
    runner
        .run(&ops, |ops| {
            let env = Env::default();
            env.mock_all_auths();
            let th = deploy_with_token(&env);
            let users = [
                Address::generate(&env),
                Address::generate(&env),
                Address::generate(&env),
            ];
            for user in &users {
                th.token_admin.mint(user, &1000);
                th.c.submit(&env, user, &th.issuer, &KYC, 1_000_000);
            }
            let supply = 3000i128;

            for (who, is_deposit, amount) in ops {
                let user = &users[who];
                // Failed calls (insufficient tokens or balance) must leave no trace.
                if is_deposit {
                    let _ = th.pool.try_deposit(user, &amount);
                } else {
                    let _ = th.pool.try_withdraw(user, &amount);
                }

                let pool_tokens = th.token.balance(&th.pool.address);
                prop_assert_eq!(pool_tokens, tracked_total(&th, &users));
                let held: i128 = users.iter().map(|u| th.token.balance(u)).sum();
                prop_assert_eq!(held + pool_tokens, supply);
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn failed_token_transfer_rolls_back_the_internal_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let th = deploy_with_token(&env);
    let user = Address::generate(&env);

    th.token_admin.mint(&user, &50);
    th.c.submit(&env, &user, &th.issuer, &KYC, 1_000_000);

    // The user holds 50 tokens, so transferring 100 fails inside `deposit`.
    assert!(th.pool.try_deposit(&user, &100).is_err());
    assert_eq!(th.pool.get_balance(&user), 0);
    assert_eq!(th.token.balance(&user), 50);
    assert_eq!(th.token.balance(&th.pool.address), 0);
}

#[test]
fn deposit_and_withdraw_require_the_callers_authorization() {
    let env = Env::default();
    env.mock_all_auths();
    let th = deploy_with_token(&env);
    let user = Address::generate(&env);
    th.token_admin.mint(&user, &500);
    th.c.submit(&env, &user, &th.issuer, &KYC, 1_000_000);
    th.pool.deposit(&user, &200);

    // Drop the blanket authorization: now the caller's own signature is required.
    env.set_auths(&[]);
    assert!(th.pool.try_deposit(&user, &100).is_err());
    assert!(th.pool.try_withdraw(&user, &100).is_err());

    env.mock_all_auths();
    assert_eq!(th.pool.get_balance(&user), 200);
    assert_eq!(th.token.balance(&user), 300);
    assert_eq!(th.token.balance(&th.pool.address), 200);
}

// ── CONTRACT-31: threshold edge cases in check_claim ─────────────────────────

#[test]
fn check_claim_threshold_boundaries_are_inclusive() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy_with_gate(&env, "funds", None);
    let user = Address::generate(&env);
    h.c.submit(&env, &user, &h.issuer, &FUNDS, 9999);

    let funds = symbol_short!("funds");
    let recorded = h
        .c
        .registry
        .get_record(&user, &funds)
        .unwrap()
        .threshold
        .unwrap_or(0);
    let check = |min: Option<u64>| h.c.registry.check_claim(&user, &funds, &min, &None);

    assert!(check(None), "no threshold requested: any live claim passes");
    assert!(check(Some(0)), "a zero minimum always passes");
    assert!(check(Some(recorded)), "equal to the recorded threshold passes (inclusive)");
    assert!(!check(Some(recorded + 1)), "one above the recorded threshold fails");
    assert!(!check(Some(u64::MAX)), "the maximum minimum fails");
}

#[test]
fn check_claim_treats_a_missing_recorded_threshold_as_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);
    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);

    let kyc = symbol_short!("kyc");
    let recorded = h.c.registry.get_record(&user, &kyc).unwrap().threshold.unwrap_or(0);
    let check = |min: Option<u64>| h.c.registry.check_claim(&user, &kyc, &min, &None);

    assert!(check(None));
    assert!(check(Some(recorded)));
    assert!(!check(Some(recorded + 1)));
    // No record at all never passes, whatever the threshold.
    let stranger = Address::generate(&env);
    assert!(!h.c.registry.check_claim(&stranger, &kyc, &None, &None));
    assert!(!h.c.registry.check_claim(&stranger, &kyc, &Some(0), &None));
}

// ── CONTRACT-32: expiry boundaries ───────────────────────────────────────────

#[test]
fn claim_expiry_boundary_equal_to_now_is_expired() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let user = Address::generate(&env);
    h.c.submit(&env, &user, &h.issuer, &KYC, 1_000_000);

    let kyc = symbol_short!("kyc");
    env.ledger().set_timestamp(999_999);
    assert!(h.c.registry.check_claim(&user, &kyc, &None, &None));
    env.ledger().set_timestamp(1_000_000);
    assert!(!h.c.registry.check_claim(&user, &kyc, &None, &None));
}

#[test]
fn delegated_verification_boundary_equal_to_now_is_expired_and_renewal_extends_it() {
    let env = Env::default();
    env.mock_all_auths();
    let h = deploy(&env);
    let holder = Address::generate(&env);
    let verifier = Address::generate(&env);
    h.c.submit(&env, &holder, &h.issuer, &KYC, 1_000_000);
    let kyc = symbol_short!("kyc");

    env.ledger().set_timestamp(100);
    h.c.registry.grant_verification(&holder, &verifier, &kyc, &200);

    env.ledger().set_timestamp(199);
    assert!(h.c.registry.check_delegated_verification(&holder, &verifier, &kyc).0);
    env.ledger().set_timestamp(200);
    assert_eq!(
        h.c.registry.check_delegated_verification(&holder, &verifier, &kyc),
        (false, 0, 0)
    );

    // Renewing with a later expiry makes the delegation live again.
    h.c.registry.grant_verification(&holder, &verifier, &kyc, &300);
    env.ledger().set_timestamp(250);
    assert!(h.c.registry.check_delegated_verification(&holder, &verifier, &kyc).0);
}
