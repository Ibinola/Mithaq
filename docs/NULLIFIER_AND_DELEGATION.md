# Delegation events and reads, and nullifier contexts (CONTRACT-33 to CONTRACT-36)

All behaviour below is read from `contracts/proof_registry/src/lib.rs`.

## 1. Delegated-grant lifecycle events (CONTRACT-33)

| Action | Topics | Payload |
| --- | --- | --- |
| `grant_verification` | `("proof_reg", "dlg_grant", credential_type)` | `EventVerificationGranted { holder, verifier, expiry }` |
| `revoke_verification` | `("proof_reg", "dlg_revok", credential_type)` | `EventVerificationRevoked { holder, verifier }` |

- The **credential-type scope is the third topic** in both events, not a payload field. A decoder
  must read it from the topics; the payload alone is not enough to tell which credential type a
  grant or revocation applies to.
- Revoking a grant that does not exist is a no-op that still emits `dlg_revok`.
- Granting again overwrites the previous expiry and emits another `dlg_grant`.
- Tests: `delegation_grant_and_revoke_events_are_fully_specified` and
  `delegation_events_carry_the_credential_type_scope_in_their_topics`.

Not done: `docs/EVENTS.md` and the downstream decoder fixtures were not updated or checked.

## 2. Delegation expiry read (CONTRACT-34)

`delegation_expiry(holder, verifier, credential_type) -> Option<u64>` returns `Some(expiry)` while
the grant is live and `None` when there is no grant or it has expired. It is a single-key read.
It was added next to `check_delegated_verification`, which still returns `(false, 0, 0)` for both
"no grant" and "no claim".

Privacy: grants are public persistent storage entries, so anyone can already read them; the new
method makes that read convenient and adds no new exposure. It is not a confidentiality boundary.

**Boundary:** a grant is live while `expiry > now`. At `expiry` itself it is expired, in both
`delegation_expiry` and `check_delegated_verification`.

## 3. Nullifier uniqueness (CONTRACT-35)

- A nullifier is unique **per `(app_context, nullifier)`**. The same nullifier in two contexts is
  independent.
- `submit_proof_with_nullifier` records the nullifier and then submits in one transaction. If the
  submission fails, the transaction reverts and the nullifier is **not** consumed.
- `submit_proofs` (batch) and `submit_aggregate_proof` take **no** nullifier or context. The
  contract has no batch or aggregate nullifier path, so uniqueness across "single, batch and
  aggregate" flows can only be tested for the single path; the batch test asserts that batches do
  not touch nullifiers.

## 4. Application context derivation (CONTRACT-36)

`app_context` is the 32-byte value passed to `record_nullifier`, `is_nullifier_used` and
`submit_proof_with_nullifier`. Integrators derive it as:

```text
app_context = SHA-256( "mithaq:app-context:v1|" || network || "|" || app_id )
```

- `network` is the lowercase network name (`testnet`, `mainnet`), so a testnet nullifier cannot
  collide with a mainnet one.
- `app_id` is a stable, unique identifier for the application or protocol (UTF-8), so two
  protocols never share a context by accident.
- Include a new version suffix in the prefix (`v2`) if the construction ever changes.

This construction is a **convention this change proposes**; the contract itself accepts any 32
bytes and does not enforce it.

Test vectors (also asserted in `app_context_domain_vectors_match_the_documented_construction`):

| network | app_id | app_context (hex) |
| --- | --- | --- |
| `testnet` | `example-dao` | `353bdd7dfc20272bd11f3d9a3cb558ad930418398bcca6a54ba8f39eb67311cf` |
| `mainnet` | `example-dao` | `36d4c0b3dc783bc05cd5751c6aa593d51d7d3cd85a74ceee5bb14e2a72b64885` |
| `testnet` | `other-app` | `79c0bbd9036cdab9708522e05d7a20f4a9f523d4d1471841c02ea608be9ca2a2` |

SDK verification of these vectors was not added.
