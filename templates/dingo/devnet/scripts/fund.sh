#!/bin/sh
# Fund a testnet address from Dingo's bundled devnet faucet key.
set -eu

devnet_dir=$(CDPATH='' cd "$(dirname "$0")/.." && pwd)
cd "$devnet_dir"

address=${1:-}
amount=${2:-100000000}
faucet_key=/data/db/devnet-config/keys/faucet.skey
faucet_vkey=/data/db/devnet-config/keys/faucet.vkey
socket=/ipc/dingo.socket
tx_body="/tmp/cardano-init-fund-$$.tx"
tx_signed="/tmp/cardano-init-fund-$$.signed"

if [ -z "$address" ]; then
  echo "usage: sh scripts/fund.sh ADDRESS [LOVELACE]" >&2
  exit 2
fi
case "$amount" in
  ''|*[!0-9]*)
    echo "LOVELACE must be a positive integer." >&2
    exit 2
    ;;
esac
if [ "$amount" -lt 1000000 ]; then
  echo "LOVELACE must be at least 1000000." >&2
  exit 2
fi

faucet_address=$(sh scripts/compose.sh exec -T dingo \
  cardano-cli address build \
  --payment-verification-key-file "$faucet_vkey" \
  --testnet-magic 42)

utxos=$(sh scripts/compose.sh exec -T dingo \
  cardano-cli query utxo \
  --address "$faucet_address" \
  --socket-path "$socket" \
  --testnet-magic 42)
tx_in=$(printf '%s\n' "$utxos" | awk -F'"' '/#[0-9]+":/ { print $2; exit }')
if [ -z "$tx_in" ]; then
  echo "Dingo faucet has no spendable UTxO." >&2
  exit 1
fi

sh scripts/compose.sh exec -T dingo \
  cardano-cli conway transaction build \
  --testnet-magic 42 \
  --socket-path "$socket" \
  --tx-in "$tx_in" \
  --tx-out "$address+$amount" \
  --change-address "$faucet_address" \
  --out-file "$tx_body"

sh scripts/compose.sh exec -T dingo \
  cardano-cli conway transaction sign \
  --tx-body-file "$tx_body" \
  --signing-key-file "$faucet_key" \
  --testnet-magic 42 \
  --out-file "$tx_signed"

sh scripts/compose.sh exec -T dingo \
  cardano-cli conway transaction submit \
  --socket-path "$socket" \
  --testnet-magic 42 \
  --tx-file "$tx_signed"
