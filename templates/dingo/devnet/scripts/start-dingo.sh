#!/bin/sh
# Create a fresh timestamped genesis from Dingo's bundled devnet configuration.
set -eu

config_dir=/data/db/devnet-config

if [ ! -f "$config_dir/config.json" ]; then
  mkdir -p "$config_dir"
  cp -R /opt/cardano/config/devnet/. "$config_dir/"

  now=$(date -u +%s)
  now_iso=$(date -u -d "@$now" +%Y-%m-%dT%H:%M:%SZ)
  sed -i -E "s/(\"startTime\": )[0-9]+/\1$now/" "$config_dir/byron-genesis.json"
  sed -i -E "s/(\"systemStart\": \")[^\"]+/\1$now_iso/" "$config_dir/shelley-genesis.json"
fi

exec /bin/entrypoint.sh serve
