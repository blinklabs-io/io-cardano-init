package app

import org.scalatest.funsuite.AnyFunSuite
import scalus.*
import scalus.uplc.builtin.ByteString
import scalus.uplc.builtin.ByteString.utf8
import scalus.uplc.builtin.Data.toData
import scalus.cardano.ledger.{AssetName, CardanoInfo}
import scalus.cardano.onchain.plutus.prelude.*
import scalus.cardano.onchain.plutus.v3.{TxId, TxOutRef}
import scalus.cardano.txbuilder.{RedeemerPurpose, txBuilder}
import scalus.testing.kit.ScalusTest
import scalus.testing.kit.TestUtil.getScriptContextV3
import scalus.uplc.*
import scalus.uplc.eval.*

import scala.language.implicitConversions

enum Expected {
    case Success
    case Failure(reason: String)
}

/** Unit tests for the two gift-card validators, run BOTH as plain Scala functions (debuggable,
  * breakpoints work) and as compiled Plutus scripts on the built-in evaluator — no blockchain,
  * milliseconds per test.
  *
  * The full create → redeem happy path (which spends the one-shot seed and burns the token across a
  * multi-transaction flow) is exercised end-to-end against the in-memory emulator in the off-chain
  * and fullstack variants; here we drive each validator's logic directly.
  */
class GiftCardTest extends AnyFunSuite with ScalusTest {
    import Expected.*

    private given env: CardanoInfo = CardanoInfo.mainnet

    private val tokenName = utf8"Gift Card"
    // A dummy seed for parameterisation. The one-shot check (seed must be an input) is covered
    // end-to-end by the emulator tests; here we exercise the token-name/amount branches.
    private val seed = TxOutRef(TxId(ByteString.fromHex("00" * 32)), BigInt(0))

    private val giftCardScript = GiftCardContract.makeGiftCardScript(tokenName, seed)
    private val policyId = giftCardScript.script.scriptHash
    private val redeemScript = GiftCardContract.makeRedeemScript(tokenName, policyId)

    // --- Minting policy: creating (Mint) and destroying (Burn) the gift-card token ---

    test("mint fails when the one-shot seed UTxO is not spent") {
        val ctx = mintContext(Map(AssetName(tokenName) -> 1L), Action.Mint)
        val ex = intercept[Exception](
          GiftCard.giftCardPolicy(tokenName, seed, Action.Mint, policyId, ctx.txInfo)
        )
        assert(ex.getMessage == "Seed UTxO not spent")
        // The blueprint scripts are compiled for release (no error traces), so on-chain a failed
        // `require` surfaces only as a generic evaluation error, not the Scala-side message above.
        assertEval(giftCardScript.program $ ctx.toData, Failure("Error evaluated"))
    }

    test("mint fails when the token name is wrong") {
        val ctx = mintContext(Map(AssetName(tokenName ++ utf8"!") -> 1L), Action.Mint)
        val ex = intercept[Exception](
          GiftCard.giftCardPolicy(tokenName, seed, Action.Mint, policyId, ctx.txInfo)
        )
        assert(ex.getMessage == "Wrong token name")
    }

    test("mint fails when more than one token is minted") {
        val ctx = mintContext(Map(AssetName(tokenName) -> 2L), Action.Mint)
        val ex = intercept[Exception](
          GiftCard.giftCardPolicy(tokenName, seed, Action.Mint, policyId, ctx.txInfo)
        )
        assert(ex.getMessage == "Mint must create exactly one token")
    }

    test("mint fails when a second token name rides along") {
        val ctx = mintContext(
          Map(AssetName(tokenName) -> 1L, AssetName(utf8"Extra") -> 1L),
          Action.Mint
        )
        val ex = intercept[Exception](
          GiftCard.giftCardPolicy(tokenName, seed, Action.Mint, policyId, ctx.txInfo)
        )
        assert(ex.getMessage == "Exactly one gift-card token must be minted or burned")
    }

    test("burn succeeds when exactly one token is destroyed") {
        val ctx = mintContext(Map(AssetName(tokenName) -> -1L), Action.Burn)
        GiftCard.giftCardPolicy(tokenName, seed, Action.Burn, policyId, ctx.txInfo) // Scala function
        assertEval(giftCardScript.program $ ctx.toData, Success) // Plutus script
    }

    test("burn fails when more than one token is destroyed") {
        val ctx = mintContext(Map(AssetName(tokenName) -> -2L), Action.Burn)
        val ex = intercept[Exception](
          GiftCard.giftCardPolicy(tokenName, seed, Action.Burn, policyId, ctx.txInfo)
        )
        assert(ex.getMessage == "Burn must destroy exactly one token")
    }

    // --- Redeem validator: releases the locked gift only when the token is burned ---

    test("redeem succeeds when the gift-card token is burned in the same transaction") {
        val ctx = mintContext(Map(AssetName(tokenName) -> -1L), Action.Burn)
        // The redeem validator only inspects the mint field, so we drive it directly with the
        // burn transaction's info.
        Redeem.redeemValidator(tokenName, policyId, ctx.txInfo)
    }

    test("redeem fails when nothing of the gift-card policy is burned") {
        val otherPolicy = redeemScript.script.scriptHash // a different policy id
        val ctx = mintContext(Map(AssetName(tokenName) -> -1L), Action.Burn)
        val ex = intercept[Exception](Redeem.redeemValidator(tokenName, otherPolicy, ctx.txInfo))
        assert(ex.getMessage == "Gift-card token not burned")
    }

    test("both validators compile to non-empty Plutus V3 scripts") {
        assert(giftCardScript.program.cborEncoded.length > 0)
        assert(redeemScript.program.cborEncoded.length > 0)
    }

    /** A minting/burning ScriptContext for this policy: builds a draft transaction that mints the
      * given assets under `policyId`, tagged with `action` as the mint redeemer.
      */
    private def mintContext(mint: Map[AssetName, Long], action: Action) = {
        val tx = txBuilder
            .mint(script = giftCardScript.script, assets = mint, redeemer = action.toData)
            .draft
        tx.getScriptContextV3(Map.empty, RedeemerPurpose.ForMint(policyId))
    }

    private def assertEval(p: Program, expected: Expected): Unit = {
        val result = p.evaluateDebug
        (result, expected) match
            case (_: Result.Success, Expected.Success) => ()
            case (result: Result.Failure, Expected.Failure(expected)) =>
                assert(
                  result.exception.getMessage.startsWith(expected),
                  s"Expected message starting with '$expected', got '${result.exception.getMessage}'"
                )
            case _ => fail(s"Unexpected result: $result, expected: $expected")
    }
}
