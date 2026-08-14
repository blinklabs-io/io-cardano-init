// Bridge from the on-chain blueprint to the Tx3 `env` block.
//
// The Gift Card scripts are parameterized at runtime — the mint policy by
// (token_name, seed UTxO), the redeem spend by (token_name, policy_id) — so
// their final compiled bytes / hashes aren't known until a seed is chosen. This
// module reads the compiled validators from `../blueprint/plutus.json` (produced
// by whichever on-chain tool ran `just build`), applies the parameters with
// `@meshsdk/core`, and returns:
//
//   - `env`          the values for the tx3 `env {}` block, passed per-tx via the
//                    generated client's `.env(...)` before `.resolve()`;
//   - `redeemAddress` the bech32 address the gift is locked at (bound to the
//                    `Redeem` party);
//   - `policyId` / `unit` for display.
//
// Any on-chain tool composes here: it only has to emit the canonical
// `blueprint/plutus.json` with a `gift_card` mint validator and a `redeem` spend
// validator.

import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
    applyParamsToScript,
    resolveScriptHash,
    serializePlutusScript,
    stringToHex,
} from "@meshsdk/core";

import { unwrapCborBytes } from "../devnet/utils.js";

/** Plutus language version the on-chain validators are compiled to. */
const PLUTUS_VERSION = "V3" as const;

/** Testnet network id (devnet uses `addr_test...` enterprise/script addresses). */
const NETWORK_ID = 0;

/** Shape of the CIP-57 blueprint — only the fields this bridge reads are named. */
interface Blueprint {
    validators: Array<{ title: string; compiledCode: string }>;
}

/** A seed UTxO that parameterizes a single gift card. */
export interface Seed {
    txHash: string;
    outputIndex: number;
}

/** The `env {}` values the generated tx3 client needs to resolve a gift-card tx. */
export interface GiftCardEnv {
    gift_policy: string;
    gift_script: string;
    redeem_script: string;
    token_name: string;
}

/** Everything a caller needs to build + submit a gift-card transaction. */
export interface GiftCard {
    /** Values for the tx3 `env {}` block. */
    env: GiftCardEnv;
    /** Bech32 redeem script address the gift is locked at. */
    redeemAddress: string;
    /** Policy id of the gift-card token. */
    policyId: string;
    /** Asset unit (policy id + hex token name) of the gift-card token. */
    unit: string;
    /** Hex-encoded token name. */
    tokenNameHex: string;
}

/** Load and parse `../blueprint/plutus.json` (one level up from the component). */
export function loadBlueprint(): Blueprint {
    const here = dirname(fileURLToPath(import.meta.url));
    const path = resolve(here, "..", "..", "blueprint", "plutus.json");
    return JSON.parse(readFileSync(path, "utf-8")) as Blueprint;
}

// Aiken prefixes validator titles with their module, e.g. "giftcard.gift_card.mint".
// Match on the "<validator>.<purpose>" suffix so the lookup is module-independent.
function findValidator(blueprint: Blueprint, suffix: string): string {
    const v = blueprint.validators.find((val) => val.title.endsWith(suffix));
    if (!v) {
        throw new Error(
            `blueprint has no validator ending in "${suffix}" (found: ${blueprint.validators
                .map((x) => x.title)
                .join(", ")})`,
        );
    }
    return v.compiledCode;
}

/**
 * Parameterize the gift-card scripts for a specific seed + token name and derive
 * everything needed to build a transaction.
 *
 * The mint policy is applied with `(token_name, OutputReference)` and the redeem
 * spend with `(token_name, policy_id)`, matching the Aiken validators. The tx3
 * witness wants the *inner* single-CBOR script bytes, while Mesh's hash/address
 * helpers want the applied (double-CBOR) form — hence `unwrapCborBytes` for the
 * `env` scripts only.
 */
export function giftCardFor(
    blueprint: Blueprint,
    tokenName: string,
    seed: Seed,
): GiftCard {
    const tokenNameHex = stringToHex(tokenName);

    const giftCardCompiled = findValidator(blueprint, "gift_card.mint");
    const redeemCompiled = findValidator(blueprint, "redeem.spend");

    // gift_card(token_name: ByteArray, utxo_ref: OutputReference)
    // OutputReference = Constr 0 [transaction_id: Bytes, output_index: Int].
    const giftScriptCode = applyParamsToScript(
        giftCardCompiled,
        [
            { bytes: tokenNameHex },
            { constructor: 0, fields: [{ bytes: seed.txHash }, { int: seed.outputIndex }] },
        ],
        "JSON",
    );
    const policyId = resolveScriptHash(giftScriptCode, PLUTUS_VERSION);

    // redeem(token_name: ByteArray, policy_id: ByteArray)
    const redeemScriptCode = applyParamsToScript(
        redeemCompiled,
        [{ bytes: tokenNameHex }, { bytes: policyId }],
        "JSON",
    );
    const redeemAddress = serializePlutusScript(
        { code: redeemScriptCode, version: PLUTUS_VERSION },
        undefined,
        NETWORK_ID,
    ).address;

    return {
        env: {
            gift_policy: policyId,
            gift_script: unwrapCborBytes(giftScriptCode),
            redeem_script: unwrapCborBytes(redeemScriptCode),
            token_name: tokenNameHex,
        },
        redeemAddress,
        policyId,
        unit: policyId + tokenNameHex,
        tokenNameHex,
    };
}
