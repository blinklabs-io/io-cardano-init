import {
  Address,
  Assets,
  Bytes,
  CBOR,
  Client,
  Credential,
  Data,
  EnterpriseAddress,
  InlineDatum,
  PlutusV3,
  ScriptHash,
  TransactionHash,
  UPLC,
  type UTxO,
} from "@evolution-sdk/evolution";

import { bundledBlueprint } from "./blueprint.generated.js";

// Framework-agnostic bindings for the GiftCard contract, built on Evolution
// SDK. No Node-only dependencies (no `fs`), so this module is safe to import in
// a browser frontend as well as a backend: the on-chain blueprint is bundled in
// at build time from ../blueprint/plutus.json (see scripts/bundle-blueprint.mjs),
// so the contract always knows its validators — consumers never supply them.

/** Plutus language version the on-chain validators are compiled to. */
export const LANGUAGE_VERSION = "PlutusV3" as const;

/**
 * Shape of the CIP-57 blueprint produced by the on-chain `aiken build`. Only
 * the fields this library reads are named; the index signatures tolerate the
 * many other fields a real plutus.json carries (preamble, redeemer, datum, …).
 */
export type Blueprint = {
  validators: { title: string; compiledCode: string; [key: string]: unknown }[];
  [key: string]: unknown;
};

/** The two compiled validators the GiftCard flow needs (double-CBOR hex). */
export type GiftCardValidators = {
  /** Compiled code of the `gift_card` minting policy. */
  giftCard: string;
  /** Compiled code of the `redeem` spending validator. */
  redeem: string;
};

/** A reference to the seed UTxO that parameterises a gift card. */
export type SeedUtxo = {
  txHash: string;
  outputIndex: number;
};

/** The result of a successful `createGiftCard`. */
export type GiftCardCreation = {
  /** Hash of the create transaction. */
  txHash: string;
  /** Bech32 redeem script address the gift landed at — pass to `getGiftCardUtxoAt`. */
  redeemAddress: string;
  /** The gift-card token's asset unit (policy id + hex token name). */
  unit: string;
};

/**
 * The parameterised scripts and derived identifiers for a single gift card.
 * Useful on a frontend to display the policy id / script address without
 * building a transaction.
 */
export type GiftCardScripts = {
  giftCardScript: PlutusV3.PlutusV3;
  policyId: string;
  redeemScript: PlutusV3.PlutusV3;
  redeemAddress: Address.Address;
  /** The full asset unit (policy id + hex token name) of the gift-card token. */
  unit: string;
};

// Aiken prefixes validator titles with their module name, e.g.
// "giftcard.gift_card.mint". We match on the "<validator>.<purpose>" suffix so
// the lookup is independent of the module (file) name.
export function findValidator(blueprint: Blueprint, suffix: string): string {
  const validator = blueprint.validators.find(
    (v) => v.title === suffix || v.title.endsWith(`.${suffix}`),
  );
  if (!validator) {
    const known = blueprint.validators.map((v) => v.title).join(", ");
    throw new Error(
      `Validator "${suffix}" not found in blueprint. Available: ${known}`,
    );
  }
  return validator.compiledCode;
}

/** Pull the gift_card (mint) and redeem (spend) validators out of a blueprint. */
export function getGiftCardValidators(blueprint: Blueprint): GiftCardValidators {
  return {
    giftCard: findValidator(blueprint, "gift_card.mint"),
    redeem: findValidator(blueprint, "redeem.spend"),
  };
}

/** The blueprint inlined at build time, or null if none was bundled. */
export { bundledBlueprint };

/** Whether a blueprint was bundled into this build. */
export function hasBundledBlueprint(): boolean {
  return bundledBlueprint !== null;
}

/**
 * The GiftCard validators bundled at build time. Throws if none was bundled —
 * build the on-chain component and run the off-chain `just build`, or pass
 * `validators` to the contract explicitly.
 */
export function getBundledValidators(): GiftCardValidators {
  if (bundledBlueprint === null) {
    throw new Error(
      "No blueprint was bundled into this build. Build the on-chain component " +
        "(its `just build` writes ../blueprint/plutus.json), then run the " +
        "off-chain `just build`; or pass `validators` to GiftCardContract.",
    );
  }
  return getGiftCardValidators(bundledBlueprint);
}

const encoder = new TextEncoder();

/** UTF-8 text → hex string (token names are supplied as plain text). */
export function textToHex(text: string): string {
  return Bytes.toHex(encoder.encode(text));
}

// Evolution's `applyParamsToScript` returns the *double*-CBOR-encoded script
// (the blueprint "compiledCode" convention). A PlutusV3 script wraps the inner
// *single*-CBOR bytes — the ledger form whose blake2b-224 hash is the script
// hash — so we peel exactly one CBOR bytestring layer before constructing it.
function plutusV3FromApplied(appliedDoubleCbor: string): PlutusV3.PlutusV3 {
  const inner = CBOR.decodeItemWithOffset(
    Bytes.fromHex(appliedDoubleCbor),
    0,
  ).item as Uint8Array;
  return new PlutusV3.PlutusV3({ bytes: inner });
}

/**
 * Minimal core: derive the parameterised scripts, policy id, and redeem address
 * for a gift card, and build the two transactions. Pure derivation
 * (`getScripts`) touches no wallet or network, so it is safe to call from a
 * frontend to preview an address.
 */
export class GiftCardContract {
  private readonly client: Client.SigningClient;
  private readonly networkId: number;
  private readonly giftCardCompiledCode: string;
  private readonly redeemCompiledCode: string;

  constructor(input: {
    /** A signing client (provider + wallet), e.g. from `createGiftCardContractFromEnv`. */
    client: Client.SigningClient;
    /** 0 for testnets (preview/preprod), 1 for mainnet. */
    networkId: number;
    /** Override the bundled blueprint (defaults to the build-time bundle). */
    validators?: GiftCardValidators;
  }) {
    this.client = input.client;
    this.networkId = input.networkId;
    const validators = input.validators ?? getBundledValidators();
    this.giftCardCompiledCode = validators.giftCard;
    this.redeemCompiledCode = validators.redeem;
  }

  /** Apply the (token name, seed UTxO) parameters to the one-shot mint policy. */
  private giftCardScriptFor(
    tokenNameHex: string,
    seedTxHash: string,
    seedIndex: number,
  ): PlutusV3.PlutusV3 {
    // gift_card params: (token_name: ByteArray, utxo_ref: OutputReference).
    // OutputReference is Constr 0 [transaction_id (bare bytes), output_index].
    const applied = UPLC.applyParamsToScript(this.giftCardCompiledCode, [
      Data.bytearray(tokenNameHex),
      Data.constr(0n, [Data.bytearray(seedTxHash), Data.int(BigInt(seedIndex))]),
    ]);
    return plutusV3FromApplied(applied);
  }

  /** Apply the (token name, policy id) parameters to the redeem spend script. */
  private redeemScriptFor(
    tokenNameHex: string,
    policyId: string,
  ): PlutusV3.PlutusV3 {
    const applied = UPLC.applyParamsToScript(this.redeemCompiledCode, [
      Data.bytearray(tokenNameHex),
      Data.bytearray(policyId),
    ]);
    return plutusV3FromApplied(applied);
  }

  private scriptAddress(script: PlutusV3.PlutusV3): Address.Address {
    const hash = ScriptHash.fromScript(script);
    const credential = Credential.makeScriptHash(ScriptHash.toBytes(hash));
    const enterprise = new EnterpriseAddress.EnterpriseAddress({
      networkId: this.networkId,
      paymentCredential: credential,
    });
    return Address.fromBytes(EnterpriseAddress.toBytes(enterprise));
  }

  /**
   * Derive the parameterised scripts, policy id, and redeem address for a gift
   * card seeded by `seedUtxo`. Pure: no wallet or network access.
   */
  getScripts(tokenName: string, seedUtxo: SeedUtxo): GiftCardScripts {
    const tokenNameHex = textToHex(tokenName);
    const giftCardScript = this.giftCardScriptFor(
      tokenNameHex,
      seedUtxo.txHash,
      seedUtxo.outputIndex,
    );
    const policyId = ScriptHash.toHex(ScriptHash.fromScript(giftCardScript));
    const redeemScript = this.redeemScriptFor(tokenNameHex, policyId);
    return {
      giftCardScript,
      policyId,
      redeemScript,
      redeemAddress: this.scriptAddress(redeemScript),
      unit: policyId + tokenNameHex,
    };
  }

  /**
   * Build, sign, and submit a transaction that mints a unique gift-card token
   * and locks `giftLovelace` at the redeem script address.
   *
   * Returns the tx hash plus the (unique) redeem script address the gift landed
   * at and its asset unit — pass the address to `getGiftCardUtxoAt` to redeem.
   * Each gift card has its own address (the redeem script is parameterised by
   * the one-shot policy id), so the address alone identifies this gift card.
   */
  createGiftCard = async (
    tokenName: string,
    giftLovelace: bigint,
  ): Promise<GiftCardCreation> => {
    const utxos = await this.client.getWalletUtxos();
    const seed = utxos[0];
    if (seed === undefined) throw new Error("No UTxOs found in wallet");

    const tokenNameHex = textToHex(tokenName);
    const seedTxHash = TransactionHash.toHex(seed.transactionId);
    const seedIndex = Number(seed.index);
    const { giftCardScript, policyId, redeemAddress, unit } = this.getScripts(
      tokenName,
      { txHash: seedTxHash, outputIndex: seedIndex },
    );

    const mint = Assets.addByHex(Assets.fromLovelace(0n), policyId, tokenNameHex, 1n);
    const locked = Assets.addByHex(
      Assets.fromLovelace(giftLovelace),
      policyId,
      tokenNameHex,
      1n,
    );

    // Remember the seed reference + token name in the inline datum so `redeem`
    // can rebuild the parameterised scripts from just the on-chain UTxO. The
    // validator itself ignores the datum — burning the token is the auth.
    const datum = Data.constr(0n, [
      Data.bytearray(seedTxHash),
      Data.int(BigInt(seedIndex)),
      Data.bytearray(tokenNameHex),
    ]);

    const built = await this.client
      .newTx()
      .collectFrom({ inputs: [seed] })
      .mintAssets({ assets: mint, redeemer: Data.constr(0n, []) }) // Mint
      .attachScript({ script: giftCardScript })
      .payToAddress({
        address: redeemAddress,
        assets: locked,
        datum: new InlineDatum.InlineDatum({ data: datum }),
      })
      .build();

    const signed = await built.sign();
    const txHash = TransactionHash.toHex(await signed.submit());
    return { txHash, redeemAddress: Address.toBech32(redeemAddress), unit };
  };

  /**
   * Find the gift-card UTxO locked at a redeem script address: the output
   * carrying an inline datum. `redeemAddress` is the bech32 address returned by
   * `createGiftCard`. Queried by address (not by tx hash) so it relies only on
   * the address-UTxO endpoint every provider supports.
   */
  getGiftCardUtxoAt = async (
    redeemAddress: string,
  ): Promise<UTxO.UTxO | undefined> => {
    const utxos = await this.client.getUtxos(Address.fromBech32(redeemAddress));
    return utxos.find(
      (u) => u.datumOption !== undefined && InlineDatum.isInlineDatum(u.datumOption),
    );
  };

  /**
   * Build, sign, and submit a transaction that burns a gift-card token and
   * releases the locked assets. `giftCardUtxo` sits at the redeem script address
   * (see `getGiftCardUtxoAt`). Returns the tx hash.
   */
  redeemGiftCard = async (giftCardUtxo: UTxO.UTxO): Promise<string> => {
    const datumOption = giftCardUtxo.datumOption;
    if (datumOption === undefined || !InlineDatum.isInlineDatum(datumOption)) {
      throw new Error("Gift-card UTxO has no inline datum.");
    }
    const data = datumOption.data;
    if (!Data.isConstr(data)) throw new Error("Unexpected datum shape.");
    const [seedTxHashB, seedIndexB, tokenNameB] = data.fields;
    const seedTxHash = Bytes.toHex(seedTxHashB as Uint8Array);
    const seedIndex = Number(seedIndexB as bigint);
    const tokenNameHex = Bytes.toHex(tokenNameB as Uint8Array);

    const giftCardScript = this.giftCardScriptFor(
      tokenNameHex,
      seedTxHash,
      seedIndex,
    );
    const policyId = ScriptHash.toHex(ScriptHash.fromScript(giftCardScript));
    const redeemScript = this.redeemScriptFor(tokenNameHex, policyId);
    const burn = Assets.addByHex(Assets.fromLovelace(0n), policyId, tokenNameHex, -1n);

    const built = await this.client
      .newTx()
      .collectFrom({ inputs: [giftCardUtxo], redeemer: Data.constr(0n, []) })
      .attachScript({ script: redeemScript })
      .mintAssets({ assets: burn, redeemer: Data.constr(1n, []) }) // Burn
      .attachScript({ script: giftCardScript })
      .build();

    const signed = await built.sign();
    return TransactionHash.toHex(await signed.submit());
  };
}
