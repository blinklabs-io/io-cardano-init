import { existsSync } from "node:fs";

import { config as loadDotenv } from "dotenv";
import {
  Address,
  type Chain,
  Client,
  mainnet,
  preprod,
  preview,
} from "@evolution-sdk/evolution";

import { GiftCardContract } from "./contract.js";

// Node-only helpers: reading configuration from the environment and wiring up a
// provider + wallet into an Evolution SDK signing client. Importing this in a
// browser bundle would pull in `node:fs` (via dotenv); frontends import the
// package root ("." → ./contract), which already carries the bundled blueprint,
// and build their own client with `.withCip30(walletApi)`.

const CHAINS = { preview, preprod, mainnet } as const;
const NETWORK_IDS = { preview: 0, preprod: 0, mainnet: 1 } as const;
export type Network = keyof typeof CHAINS;

const BLOCKFROST_BASE_URLS: Record<Network, string> = {
  preview: "https://cardano-preview.blockfrost.io/api/v0",
  preprod: "https://cardano-preprod.blockfrost.io/api/v0",
  mainnet: "https://cardano-mainnet.blockfrost.io/api/v0",
};

// How this project reaches the chain. The choice is driven entirely by the
// shared ../.env, which is the cardano-init interface contract's connection
// seam — this code never names the tool that wrote it:
//
//   • INDEXER_URL set → a local devnet is up (e.g. Yaci DevKit). Yaci serves a
//     Blockfrost-compatible API, so we point Evolution's Blockfrost provider at
//     it; no Blockfrost project id is needed, and the devnet's faucet
//     (YACI_ADMIN_URL) can fund wallets.
//   • INDEXER_URL absent → talk to Blockfrost, which needs BLOCKFROST_PROJECT_ID.
export type ProviderConfig =
  | { kind: "yaci"; baseUrl: string; adminUrl?: string }
  | { kind: "blockfrost"; baseUrl: string; projectId: string };

export type GiftCardEnv = {
  network: Network;
  networkId: 0 | 1;
  provider: ProviderConfig;
  mnemonic: string;
};

export type EnvResult =
  | { ok: true; env: GiftCardEnv }
  | { ok: false; missing: string[] };

/**
 * Load configuration from the environment. Reads the given dotenv files (the
 * shared `../.env` for CARDANO_NETWORK + the infrastructure connection details,
 * and the gitignored `.env.local` for secrets) without overriding anything
 * already set in `process.env`.
 *
 * Connecting to a local devnet (INDEXER_URL set): requires MNEMONIC.
 * Connecting to Blockfrost (INDEXER_URL absent): requires BLOCKFROST_PROJECT_ID
 * and MNEMONIC. CARDANO_NETWORK is optional (preview | preprod | mainnet;
 * default preview).
 */
export function loadEnv(
  envFiles = ["../.env", ".env.local", ".env"],
): EnvResult {
  for (const path of envFiles) {
    if (existsSync(path)) loadDotenv({ path, override: false });
  }

  const network = (process.env.CARDANO_NETWORK ?? "preview") as Network;
  const mnemonic = (process.env.MNEMONIC ?? "").trim();
  const indexerUrl = (process.env.INDEXER_URL ?? "").trim();

  const missing: string[] = [];
  if (!(network in CHAINS))
    missing.push("CARDANO_NETWORK (one of preview|preprod|mainnet)");

  // The connection seam: a local devnet (INDEXER_URL) wins over Blockfrost.
  let provider: ProviderConfig;
  if (indexerUrl) {
    const adminUrl = (process.env.YACI_ADMIN_URL ?? "").trim();
    // Strip any trailing slash: the Blockfrost provider joins paths as
    // `${baseUrl}/addresses/…`, so a trailing slash (Yaci publishes
    // INDEXER_URL as `…/api/v1/`) would produce a `//` that the indexer 404s —
    // and the provider silently maps 404 to an empty UTxO set, which surfaces
    // much later as a confusing "No UTxOs found in wallet".
    provider = {
      kind: "yaci",
      baseUrl: indexerUrl.replace(/\/+$/, ""),
      adminUrl: adminUrl || undefined,
    };
  } else {
    const projectId = process.env.BLOCKFROST_PROJECT_ID ?? "";
    if (!projectId)
      missing.push("BLOCKFROST_PROJECT_ID (or start a local devnet: INDEXER_URL)");
    provider = {
      kind: "blockfrost",
      baseUrl: BLOCKFROST_BASE_URLS[network in CHAINS ? network : "preview"],
      projectId,
    };
  }

  // A signing wallet is needed in both modes.
  if (!mnemonic) missing.push("MNEMONIC");
  if (missing.length > 0) return { ok: false, missing };

  return {
    ok: true,
    env: {
      network,
      networkId: NETWORK_IDS[network],
      provider,
      mnemonic,
    },
  };
}

// The default Yaci DevKit admin API, used to fetch the devnet genesis when
// YACI_ADMIN_URL wasn't published to ../.env.
const DEFAULT_YACI_ADMIN_URL = "http://localhost:10000";

/**
 * Build an Evolution SDK `Chain` descriptor from Yaci DevKit's live shelley
 * genesis, so slot timing / network magic match the running devnet (which
 * resets whenever the cluster restarts). Mirrors the official Yaci DevKit +
 * Evolution SDK example.
 */
export async function fetchYaciChain(adminUrl: string): Promise<Chain> {
  const base = adminUrl.replace(/\/+$/, "");
  const shelley = await fetch(
    `${base}/local-cluster/api/admin/devnet/genesis/shelley`,
  ).then((r) => {
    if (!r.ok) throw new Error(`Fetching devnet genesis failed: HTTP ${r.status}`);
    return r.json() as Promise<{
      systemStart: string;
      slotLength: number;
      networkMagic: number;
      epochLength: number;
    }>;
  });

  return {
    id: 0, // Yaci DevKit is a testnet
    name: "Yaci DevKit",
    networkMagic: shelley.networkMagic,
    epochLength: shelley.epochLength,
    slotConfig: {
      zeroTime: BigInt(new Date(shelley.systemStart).getTime()),
      zeroSlot: 0n,
      slotLength: shelley.slotLength * 1000, // seconds → milliseconds
    },
  };
}

/**
 * Install a global `fetch` shim that normalizes a few Yaci Store responses so
 * they parse against Evolution SDK's stricter Blockfrost schemas. Idempotent,
 * and only touches the two diverging endpoints — every other request passes
 * through untouched. Mirrors the official Yaci DevKit + Evolution SDK example;
 * once Yaci Store aligns with upstream Blockfrost this can be removed.
 *
 *   - `/epochs/latest/parameters`: `drep_deposit` / `gov_action_deposit` come
 *     back as numbers; the SDK expects strings.
 *   - `/addresses/{addr}/utxos`: the SDK requires `tx_index` and `block` on
 *     every row; Yaci Store emits `output_index` / `block_number` instead.
 *
 * Without these patches, `client.getUtxos()` and `.build()` fail with schema
 * parse errors (e.g. "Blockfrost getUtxos failed").
 */
export function installYaciStoreFetchShim(): void {
  const originalFetch = globalThis.fetch;
  if ((originalFetch as { __yaciShimInstalled?: boolean }).__yaciShimInstalled) return;

  const shimmed: typeof fetch = async (input, init) => {
    const url =
      typeof input === "string"
        ? input
        : input instanceof URL
          ? input.href
          : input.url;
    const response = await originalFetch(input, init);
    if (!response.ok) return response;

    let body: unknown;
    if (url.includes("/epochs/latest/parameters")) {
      body = await response.json();
      const params = body as Record<string, unknown>;
      for (const k of ["drep_deposit", "gov_action_deposit"] as const) {
        if (typeof params[k] === "number") params[k] = String(params[k]);
      }
    } else if (/\/addresses\/[^/]+\/utxos/.test(url)) {
      body = await response.json();
      if (Array.isArray(body)) {
        for (const u of body as Array<Record<string, unknown>>) {
          if (u.tx_index === undefined && typeof u.output_index === "number") {
            u.tx_index = u.output_index;
          }
          if (typeof u.block !== "string") {
            u.block =
              typeof u.block_hash === "string"
                ? u.block_hash
                : typeof u.block_number === "number"
                  ? String(u.block_number)
                  : "";
          }
        }
      }
    } else {
      return response;
    }

    return new Response(JSON.stringify(body), {
      status: response.status,
      statusText: response.statusText,
      headers: response.headers,
    });
  };

  (shimmed as { __yaciShimInstalled?: boolean }).__yaciShimInstalled = true;
  globalThis.fetch = shimmed;
}

/**
 * Build an Evolution SDK signing client from the resolved environment.
 *
 * Async because the Yaci path fetches the devnet's live genesis to build a
 * matching `Chain` (and installs the Yaci Store response shim). The public
 * Blockfrost path uses the static network chain and resolves immediately.
 */
export async function createClient(env: GiftCardEnv): Promise<Client.SigningClient> {
  // Defensive: the provider joins request paths as `${baseUrl}/…`, so a
  // trailing slash would create a `//` the indexer 404s on.
  const baseUrl = env.provider.baseUrl.replace(/\/+$/, "");

  let read;
  if (env.provider.kind === "yaci") {
    // Yaci Store's Blockfrost API diverges slightly from upstream — normalize
    // its responses and match the running devnet's slot config.
    installYaciStoreFetchShim();
    const chain = await fetchYaciChain(env.provider.adminUrl ?? DEFAULT_YACI_ADMIN_URL);
    read = Client.make(chain).withBlockfrost({
      baseUrl,
      // Yaci's Blockfrost-compatible API ignores the project id.
      projectId: "yaci-devnet",
    });
  } else {
    read = Client.make(CHAINS[env.network]).withBlockfrost({
      baseUrl,
      projectId: env.provider.projectId,
    });
  }
  return read.withSeed({ mnemonic: env.mnemonic, accountIndex: 0 });
}

/**
 * Wire up a provider (Yaci devnet or Blockfrost, per the environment), a
 * mnemonic-backed wallet, and a GiftCardContract. The contract uses the
 * blueprint bundled at build time. Backend convenience; a frontend builds its
 * own client with `.withCip30(...)` and constructs GiftCardContract directly.
 */
export async function createGiftCardContractFromEnv(options: {
  env: GiftCardEnv;
}): Promise<{
  contract: GiftCardContract;
  client: Client.SigningClient;
  provider: ProviderConfig;
}> {
  const { env } = options;
  const client = await createClient(env);
  const contract = new GiftCardContract({ client, networkId: env.networkId });
  return { contract, client, provider: env.provider };
}

/** The wallet's own bech32 change address. */
export async function walletAddress(client: Client.SigningClient): Promise<string> {
  return Address.toBech32(await client.address());
}

/**
 * On a local Yaci devnet, fund an address from the built-in faucet so a fresh
 * wallet has something to spend. No-op on Blockfrost (use a pre-funded wallet
 * there). Requires YACI_ADMIN_URL to have been published to ../.env by the
 * devnet's `just dev`. `ada` is the amount in **ADA** (Yaci's topup unit).
 */
export async function topupOnDevnet(
  provider: ProviderConfig,
  address: string,
  ada: number,
): Promise<boolean> {
  if (provider.kind !== "yaci" || !provider.adminUrl) return false;
  const base = provider.adminUrl.replace(/\/$/, "");
  const res = await fetch(`${base}/local-cluster/api/addresses/topup`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ address, adaAmount: ada }),
  });
  if (!res.ok) {
    throw new Error(`Devnet faucet topup failed: HTTP ${res.status}`);
  }
  return true;
}
