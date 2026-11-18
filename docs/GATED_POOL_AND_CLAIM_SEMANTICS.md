# GatedPool balance enumeration and claim boundary semantics (CONTRACT-30 to CONTRACT-32)

Read from `contracts/gated_pool/src/lib.rs` and `contracts/proof_registry/src/lib.rs`.

## 1. Enumerating GatedPool balances (CONTRACT-30)

**Today:** balances are one persistent entry per address, read with `get_balance(account)`. The
contract keeps no list of depositors, so there is nothing on-chain to iterate.

**Recommendation: do not add an on-chain enumerator.** Enumeration needs an index that grows with
every new depositor (unbounded storage, rent, and a hot entry that every first deposit would
write), and any loop over it is unbounded work in a metered transaction.

**How integrators enumerate instead:**

1. Build the depositor set off-chain from the `("gate_pool", "deposit")` and
   `("gate_pool", "withdraw")` events. Each carries `caller`, `amount` and `new_balance`, so the
   latest event per address already gives its balance.
2. To confirm a balance at a ledger, call `get_balance(account)` for each address, batching the
   simulations client-side.

**If a query surface is added later:** keep `get_balance` unchanged and add a caller-supplied-keys
read, so the contract still never iterates storage:

```text
get_balances(accounts: Vec<Address>) -> Vec<i128>      // accounts.len() <= MAX_BALANCE_BATCH (suggest 50)
```

It would return balances in input order, panic with a typed error above the cap, and need no new
storage. Not implemented here: the contract ABI and its API reference are unchanged.

## 2. `check_claim` threshold semantics (CONTRACT-31)

| Input | Result |
| --- | --- |
| no record, revoked, or expired | `false`, whatever the threshold |
| `min_threshold = None` | `true` for a live record (the recorded threshold is ignored) |
| `min_threshold = Some(0)` | `true` for a live record |
| `Some(min)` with recorded threshold `t` | `t >= min`, so **inclusive**: `t == min` passes, `t == min - 1` fails |
| record has no threshold (for example KYC) | treated as `0`: passes only `Some(0)` or `None` |
| `Some(u64::MAX)` | passes only if the recorded threshold is `u64::MAX` |

SDKs must use the same inclusive comparison. Different credential types differ only in what
threshold the circuit encodes (a lower bound, such as funds); the contract applies one comparison
(`>=`) to all of them and has no per-type comparison mode. A credential whose real meaning is an
upper bound would need its own convention or a contract change.

## 3. Expiry boundaries (CONTRACT-32)

| Entrypoint | Live while | At `expiry == now` |
| --- | --- | --- |
| `check_claim` | `expiry > now` | **expired** (`r.expiry <= now` returns `false`) |
| `check_delegated_verification` | delegation `expiry > now` (and the claim itself is live) | **expired**, returns `(false, 0, 0)` |
| `delegation_expiry` (if present) | `expiry > now` | `None` |
| `grant_verification` (creating or renewing) | requires `expiry > now` and within the maximum credential lifetime | `InvalidExpiry` |

Renewal is a re-grant: `grant_verification` with the same `(holder, verifier, credential_type)`
overwrites the stored expiry. A shorter new expiry shortens the grant.

Tests: `check_claim_threshold_boundaries_are_inclusive`,
`check_claim_treats_a_missing_recorded_threshold_as_zero`,
`claim_expiry_boundary_equal_to_now_is_expired`,
`delegated_verification_boundary_equal_to_now_is_expired_and_renewal_extends_it`,
`prop_pool_token_holdings_match_user_balances`, `failed_token_transfer_rolls_back_the_internal_balance`
and `deposit_and_withdraw_require_the_callers_authorization` (all in `contracts/gated_pool/src/test.rs`).
