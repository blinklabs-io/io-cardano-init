import { existsSync } from "node:fs";

import { config as loadDotenv } from "dotenv";
import { TransactionHash } from "@evolution-sdk/evolution";
import { describe, expect, it } from "vitest";

import { GiftCardContract, hasBundledBlueprint } from "./contract.js";
import {
  createClient,
  topupOnDevnet,
  walletAddress,
  type GiftCardEnv,
} from "./node.js";

// End-to-end integration test: a full mint→lock→redeem round-trip against a
// real local devnet (Yaci DevKit). It is gated so it runs ONLY when a devnet is
// available and the on-chain blueprint has been built:
//
//   • INDEXER_URL — written to ../.env by `just -f devnet/Justfile dev`, or
//     exported by the devnet component's ephemeral `just test`.
//   • a bundled blueprint — produced by the on-chain `just build`.
//
// Otherwise it skips, so `just test` stays green with no devnet. This file ends
// in `.test.ts`, so it is excluded from the library build (tsconfig) and never
// reaches the importable package — only the unit-tested `contract.ts` does.

for (const path of ["../.env", ".env.local", ".env"]) {
  if (existsSync(path)) loadDotenv({ path, override: false });
}

const indexerUrl = (process.env.INDEXER_URL ?? "").trim();
const adminUrl = (process.env.YACI_ADMIN_URL ?? "http://localhost:10000").trim();
const canRun = indexerUrl !== "" && hasBundledBlueprint();

// When set (by the end-to-end CI smoke test), a skip is a failure — but only
// once a devnet is actually live (INDEXER_URL set). The top-level `just test`
// also runs this suite standalone with no devnet (blank INDEXER_URL) before the
// devnet phase; that skip is expected and must stay green. So strict mode fails
// only when the devnet is up yet the round-trip still can't run (e.g. the
// on-chain blueprint wasn't bundled) — a real, actionable breakage.
const requireDevnet = (process.env.CARDANO_INIT_REQUIRE_DEVNET ?? "").trim() !== "";

if (requireDevnet && indexerUrl !== "" && !hasBundledBlueprint()) {
  describe("GiftCard round-trip on a Yaci devnet", () => {
    it("must run under CARDANO_INIT_REQUIRE_DEVNET (devnet is live)", () => {
      throw new Error(
        "Devnet is live but no bundled blueprint — build the on-chain component first.",
      );
    });
  });
}

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

const env: GiftCardEnv = {
  network: "preprod",
  networkId: 0,
  provider: { kind: "yaci", baseUrl: indexerUrl, adminUrl },
  // A throwaway devnet mnemonic — funded from the faucet, never a real seed.
  mnemonic:
    "test test test test test test test test test test test test test test test test test test test test test test test sauce",
};

(canRun ? describe : describe.skip)("GiftCard round-trip on a Yaci devnet", () => {
  it(
    "mints + locks a gift card, then redeems it",
    async () => {
      const client = await createClient(env);
      const contract = new GiftCardContract({ client, networkId: 0 });

      // Fund from the faucet: a small UTxO usable as collateral + a large one to
      // spend. NOTE: Yaci's topup amount is in ADA, not lovelace. Wait until both
      // UTxOs are indexed before building a transaction.
      const address = await walletAddress(client);
      await topupOnDevnet(env.provider, address, 10); // collateral
      await topupOnDevnet(env.provider, address, 10_000); // funds
      for (let i = 0; i < 60; i++) {
        const utxos = await client.getWalletUtxos();
        if (utxos.length >= 2) break;
        await wait(1000);
      }

      // Create: mint a unique token and lock 5 ADA at the redeem script address.
      const { txHash: createTx, redeemAddress } = await contract.createGiftCard(
        "IntegrationGift",
        5_000_000n,
      );
      await client.awaitTx(TransactionHash.fromHex(createTx));

      let giftUtxo = await contract.getGiftCardUtxoAt(redeemAddress);
      for (let i = 0; giftUtxo === undefined && i < 60; i++) {
        await wait(1000);
        giftUtxo = await contract.getGiftCardUtxoAt(redeemAddress);
      }
      expect(giftUtxo, "the create tx should produce a gift-card UTxO").toBeDefined();

      // Redeem: burn the token and release the locked assets back to the wallet.
      const redeemTx = await contract.redeemGiftCard(giftUtxo!);
      await client.awaitTx(TransactionHash.fromHex(redeemTx));
      expect(redeemTx).toMatch(/^[0-9a-f]{64}$/);
    },
    180_000,
  );
});
