import { Address } from "@evolution-sdk/evolution";
import { describe, expect, it } from "vitest";

import {
  getBundledValidators,
  GiftCardContract,
  hasBundledBlueprint,
} from "./contract.js";

// The blueprint is bundled before tests run (the npm "pretest" hook calls
// scripts/bundle-blueprint.mjs), so these run only once the on-chain component
// has been built. They need no wallet or network — script parameterisation and
// address derivation are pure, so the client is never touched.
const bundled = hasBundledBlueprint();

// A throwaway seed UTxO — script derivation is pure, so any well-formed
// reference works for an offline test.
const SEED_UTXO = {
  txHash: "0000000000000000000000000000000000000000000000000000000000000000",
  outputIndex: 0,
};

describe("GiftCard off-chain (Evolution SDK)", () => {
  it.runIf(bundled)("bundles both validators from the blueprint", () => {
    const { giftCard, redeem } = getBundledValidators();
    expect(giftCard.length).toBeGreaterThan(0);
    expect(redeem.length).toBeGreaterThan(0);
  });

  it.runIf(bundled)(
    "derives a policy id and redeem address from the bundled blueprint",
    () => {
      // No client needed — getScripts is pure; cast for the offline test.
      const contract = new GiftCardContract({
        client: undefined as never,
        networkId: 0,
      });

      const scripts = contract.getScripts("hello world", SEED_UTXO);

      // Policy id is a 28-byte blake2b-224 hash → 56 hex chars.
      expect(scripts.policyId).toMatch(/^[0-9a-f]{56}$/);
      // Preview/preprod script addresses are bech32 with the `addr_test` HRP.
      const bech32 = Address.toBech32(scripts.redeemAddress);
      expect(bech32.startsWith("addr_test")).toBe(true);
      // The asset unit is policy id + hex token name ("hello world").
      expect(scripts.unit).toBe(scripts.policyId + "68656c6c6f20776f726c64");
    },
  );
});
