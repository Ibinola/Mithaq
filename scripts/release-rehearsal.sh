#!/usr/bin/env bash
# Release rehearsal on a test network (CONTRACT-40).
#
# Runs, in order: deployment, issuer registration (both done by deploy.sh), real proof
# submission (demo.sh), issuer key rotation, proof revocation, and client reads. It then
# writes release evidence: the exact command, the git commit, the contract ids and the
# on-chain `version()` of each contract.
#
# Usage:
#   ISSUER_PRIVATE_KEY=<hex> ./scripts/release-rehearsal.sh
#   NETWORK=testnet EVIDENCE_FILE=rehearsal-evidence.md ./scripts/release-rehearsal.sh
#
# Needs the `stellar` CLI, `node`, `openssl` and network access. The wallet created for the run is
# deleted on exit; deployed contract instances remain on the test network.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="${SOURCE:-mithaq-rehearsal-$$}"
NETWORK="${NETWORK:-testnet}"
EVIDENCE_FILE="${EVIDENCE_FILE:-$ROOT/release-rehearsal-evidence.md}"
TMP="$(mktemp -d)"
COMMAND_LINE="NETWORK=$NETWORK ./scripts/release-rehearsal.sh"

if [ "$NETWORK" = "mainnet" ]; then
  echo "Refusing to rehearse on mainnet." >&2
  exit 1
fi

cleanup() {
  status=$?
  set +e
  stellar keys delete --global "$SOURCE" >/dev/null 2>&1 || true
  rm -rf "$TMP"
  exit "$status"
}
trap cleanup EXIT

: "${ISSUER_PRIVATE_KEY:?ISSUER_PRIVATE_KEY is required}"
for tool in stellar node openssl; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done

invoke() { # invoke <contract-id> <source> <send yes|no> -- <fn> [args...]
  local id="$1" src="$2" send="$3"
  shift 3
  stellar contract invoke --id "$id" --source "$src" --network "$NETWORK" --send "$send" "$@"
}

step() { echo; echo "== $* =="; }

cd "$ROOT"

step "1. Deploy contracts and register the issuer"
stellar keys generate --global "$SOURCE" --network "$NETWORK" --fund >/dev/null
SOURCE="$SOURCE" NETWORK="$NETWORK" ./scripts/deploy.sh >"$TMP/deploy.log"
for key in ISSUER_ADDRESS ISSUER_REGISTRY_ID CREDENTIAL_VERIFIER_ID PROOF_REGISTRY_ID GATED_POOL_ID; do
  value="$(grep -E "^NEXT_PUBLIC_${key}=" "$TMP/deploy.log" | tail -1 | cut -d= -f2-)"
  [ -n "$value" ] || { echo "deploy.sh did not emit NEXT_PUBLIC_${key}" >&2; exit 1; }
  export "NEXT_PUBLIC_${key}=$value"
done
export STELLAR_NETWORK="$NETWORK"
export STELLAR_RPC_URL="${STELLAR_RPC_URL:-https://soroban-testnet.stellar.org}"

step "2. Submit real proofs (demo.sh issues credentials, proves and submits)"
./scripts/demo.sh >"$TMP/demo.log" 2>&1
HOLDER="$(grep -oE 'G[A-Z2-7]{55}' "$TMP/demo.log" | head -1)"
HOLDER_KEY="$(stellar keys ls 2>/dev/null | grep '^stcreddemo-' | tail -1 || true)"
[ -n "$HOLDER" ] && [ -n "$HOLDER_KEY" ] || { echo "Could not find the demo holder wallet" >&2; exit 1; }

step "3. Rotate the issuer key (the old key stays valid for one hour)"
NEW_PUBKEY="$(openssl rand -hex 64)"
OLD_VALID_UNTIL="$(( $(date +%s) + 3600 ))"
invoke "$NEXT_PUBLIC_ISSUER_REGISTRY_ID" "$SOURCE" yes -- rotate_issuer_key \
  --issuer_id "$NEXT_PUBLIC_ISSUER_ADDRESS" --new_pubkey "$NEW_PUBKEY" --old_key_valid_until "$OLD_VALID_UNTIL" >/dev/null
ROTATED="$(invoke "$NEXT_PUBLIC_ISSUER_REGISTRY_ID" "$SOURCE" no -- get_issuer_keys --issuer_id "$NEXT_PUBLIC_ISSUER_ADDRESS")"
echo "$ROTATED" | grep -qi "$NEW_PUBKEY" || { echo "Rotated key not visible in get_issuer_keys" >&2; exit 1; }

step "4. Revoke the holder's KYC proof and read it back"
invoke "$NEXT_PUBLIC_PROOF_REGISTRY_ID" "$HOLDER_KEY" yes -- revoke_proof --holder "$HOLDER" --credential_type kyc >/dev/null
VERIFIED="$(invoke "$NEXT_PUBLIC_PROOF_REGISTRY_ID" "$SOURCE" no -- check_claim --holder "$HOLDER" --credential_type kyc)"
[ "$VERIFIED" = "false" ] || { echo "Expected check_claim to be false after revocation, got: $VERIFIED" >&2; exit 1; }

step "5. Client reads: contract versions"
version_of() { invoke "$1" "$SOURCE" no -- version | tr -d '"'; }
V_ISSUER="$(version_of "$NEXT_PUBLIC_ISSUER_REGISTRY_ID")"
V_VERIFIER="$(version_of "$NEXT_PUBLIC_CREDENTIAL_VERIFIER_ID")"
V_PROOF="$(version_of "$NEXT_PUBLIC_PROOF_REGISTRY_ID")"
V_POOL="$(version_of "$NEXT_PUBLIC_GATED_POOL_ID")"

step "6. Write release evidence to $EVIDENCE_FILE"
cat >"$EVIDENCE_FILE" <<EVIDENCE
# Release rehearsal evidence

- Date (UTC): $(date -u +%Y-%m-%dT%H:%M:%SZ)
- Network: $NETWORK
- Source commit: $(git rev-parse HEAD 2>/dev/null || echo unknown)
- Command: \`$COMMAND_LINE\`

| Contract | Id | on-chain version() |
| --- | --- | --- |
| IssuerRegistry | $NEXT_PUBLIC_ISSUER_REGISTRY_ID | $V_ISSUER |
| CredentialVerifier | $NEXT_PUBLIC_CREDENTIAL_VERIFIER_ID | $V_VERIFIER |
| ProofRegistry | $NEXT_PUBLIC_PROOF_REGISTRY_ID | $V_PROOF |
| GatedPool | $NEXT_PUBLIC_GATED_POOL_ID | $V_POOL |

Steps passed: deploy, issuer registration, proof submission, key rotation, revocation, client reads.
EVIDENCE
echo "Rehearsal complete. Evidence: $EVIDENCE_FILE"
