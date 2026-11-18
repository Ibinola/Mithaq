# Verification-key version retention policy (CONTRACT-37, CONTRACT-38)

Read from `contracts/credential_verifier/src/lib.rs`. Version numbers are per credential type.

## Lifecycle

| Step | Entrypoint | Effect | Event |
| --- | --- | --- | --- |
| Register | `set_vk(type, version, vk)` | Stores the VK. A version is **immutable**: re-registering it fails with `VkAlreadySet`. Version `0` is reserved. The latest-version pointer moves to the highest version registered, never backwards. | `("cred_ver", "vk_set", type)` with `EventVkSet` |
| Deprecate | `deprecate_version(type, version)` | New submissions against that version are rejected with `VersionDeprecated`. The VK bytes are **kept**. The deprecation time is recorded once. | **none** |
| Refresh | `refresh_latest_version_ttl(type)` | Extends the TTL of the latest pointer and the VK it points to. | **none** |
| Prune | `prune_version(type, version)` | Removes the VK bytes of a **deprecated** version, after the window below. The deprecation marker stays. | `("cred_ver", "vk_pruned", type)` with `EventVkPruned` |

## How long an old version is available

- A version stays verifiable until it is deprecated; after deprecation it stays **stored** for at
  least `MAX_PROOF_VALIDITY_SECONDS` (currently 90 days, the on-chain form of the support window
  in `SUPPORT_POLICY.md`).
- `prune_version` fails with `VkStillReferenceable` before `deprecated_at + MAX_PROOF_VALIDITY_SECONDS`
  and succeeds from exactly that timestamp (the boundary is inclusive).
- Only a deprecated version can be pruned, so the latest, active version cannot be pruned by
  accident.
- Storage TTL is separate from this policy: a VK that is neither the latest version nor refreshed
  can still lapse from storage. Only the latest pointer and its VK are covered by
  `refresh_latest_version_ttl`.

## What happens to credentials that reference a pruned version

- A proof **already verified and cached** in ProofRegistry stays readable: reads
  (`is_verified`, `check_claim`, `get_record`) do not re-verify, so they do not need the VK.
- New verification against the pruned version cannot succeed (the VK is gone), and the version
  can never be registered again (`VersionDeprecated`). Holders must re-submit a proof against a
  current version.
- Issuers should plan re-issuance before the end of the 90-day window.

## Event ordering and indexers (CONTRACT-38)

A sequence of `set_vk`, `deprecate_version`, `refresh_latest_version_ttl` and `prune_version`
calls emits events only for `set_vk` (in call order) and `prune_version`.

- **Active version:** an indexer reconstructs it as the **highest version seen in `vk_set`
  events** for the credential type, since the pointer never moves backwards. Pruning an older
  version does not change it.
- **Gap:** deprecation emits no event, so an indexer cannot observe that a version was deprecated
  until it is pruned, and cannot tell from events alone that the latest version has been
  deprecated. Adding a `vk_deprecated` event would close this; it was **not** added here because
  it changes the event schema and the existing test `deprecate_version_emits_no_events` asserts
  the current behaviour.

Tests (`contracts/credential_verifier/src/test.rs`):
`vk_lifecycle_events_are_emitted_in_order_and_deprecation_is_silent`,
`active_version_is_reconstructable_from_vk_set_events_after_a_sequence_of_changes`,
`prune_boundary_is_inclusive_and_a_pruned_version_cannot_be_reused_or_verified`.
