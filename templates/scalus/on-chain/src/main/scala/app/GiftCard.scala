package app

import scalus.*
import scalus.compiler.*
import scalus.cardano.onchain.plutus.prelude.List.{Cons, Nil}
import scalus.cardano.onchain.plutus.prelude.{*, given}
import scalus.cardano.onchain.plutus.v1.{PolicyId, TokenName}
import scalus.cardano.onchain.plutus.v3.{ScriptContext, ScriptInfo, TxInfo, TxOutRef}
import scalus.uplc.builtin.{Data, FromData, ToData}

import scala.language.implicitConversions

/** What the holder of a gift-card token is doing: creating the card (mint the token) or redeeming it
  * (burn the token). Encoded as `Constr 0 []` / `Constr 1 []`, matching what off-chain tooling —
  * including the MeshJS bindings and the Aiken/Plinth variants — supplies as the mint redeemer.
  */
enum Action derives FromData, ToData:
    case Mint
    case Burn

/** Gift-card one-shot minting policy.
  *
  * Creating a gift card mints exactly one token of `tokenName` under this policy; redeeming it burns
  * exactly one. Parameterised by two *separate* curried arguments — the token name and the seed UTxO
  * — so the compiled code is ABI-identical to the Aiken/Plinth gift card and can be driven by the
  * same off-chain bindings.
  */
@Compile
object GiftCard {

    /** Core validation logic, runnable as a plain Scala function (debuggable) and on-chain. */
    def giftCardPolicy(
        tokenName: TokenName,
        seed: TxOutRef,
        action: Action,
        ownPolicyId: PolicyId,
        tx: TxInfo
    ): Unit = {
        val ownTokens = tx.mint.toSortedMap.getOrFail(ownPolicyId, "No tokens for this policy")

        // Exactly one token entry, of the parameterised name, is minted or burned. Requiring a
        // single entry stops a caller sneaking a second token name under the same policy.
        ownTokens.toList match
            case Cons((tokName, amount), Nil) =>
                require(tokName == tokenName, "Wrong token name")
                action match
                    case Action.Mint =>
                        require(amount == BigInt(1), "Mint must create exactly one token")
                        // One-shot: the parameterised seed UTxO must be among the inputs.
                        require(
                          tx.inputs.exists(_.outRef === seed),
                          "Seed UTxO not spent"
                        )
                    case Action.Burn =>
                        require(amount == BigInt(-1), "Burn must destroy exactly one token")
            case _ =>
                fail("Exactly one gift-card token must be minted or burned")
    }

    /** On-chain entry point: `\tokenName -> \seed -> \scriptContext -> Unit` (Plutus V3).
      *
      * The two leading parameters are left free so the off-chain side applies them with
      * `applyParamsToScript`; the ledger then applies the final script context.
      */
    def mint(tokenName: Data)(seed: Data)(ctx: Data): Unit = {
        val scriptContext = ctx.to[ScriptContext]
        val ownPolicyId = scriptContext.scriptInfo match
            case ScriptInfo.MintingScript(currencySymbol) => currencySymbol
            case _                                         => fail("Not a minting script")
        giftCardPolicy(
          tokenName.to[TokenName],
          seed.to[TxOutRef],
          scriptContext.redeemer.to[Action],
          ownPolicyId,
          scriptContext.txInfo
        )
    }
}

/** Redeem spending validator.
  *
  * Guards the assets locked alongside a gift card. It only allows its UTxO to be spent when the
  * matching gift-card token — identified by the parameterised policy ID and token name — is burned
  * in the same transaction. The datum and redeemer are unused: the burn is the authorisation.
  *
  * Parameterised by two separate curried arguments (token name, then policy ID), ABI-identical to
  * the Aiken/Plinth redeem validator.
  */
@Compile
object Redeem {

    def redeemValidator(
        tokenName: TokenName,
        policyId: PolicyId,
        tx: TxInfo
    ): Unit = {
        val burnedTokens = tx.mint.toSortedMap.getOrFail(policyId, "Gift-card token not burned")
        burnedTokens.toList match
            case Cons((tokName, amount), Nil) =>
                require(tokName == tokenName, "Wrong token name")
                require(amount == BigInt(-1), "Gift-card token must be burned to redeem")
            case _ =>
                fail("Exactly one gift-card token must be burned")
    }

    /** On-chain entry point: `\tokenName -> \policyId -> \scriptContext -> Unit` (Plutus V3). */
    def spend(tokenName: Data)(policyId: Data)(ctx: Data): Unit = {
        val scriptContext = ctx.to[ScriptContext]
        redeemValidator(tokenName.to[TokenName], policyId.to[PolicyId], scriptContext.txInfo)
    }
}
