package app

import org.scalatest.funsuite.AnyFunSuite

/** Transaction-building assertions shared by any environment that can supply an [[AppCtx]] (the
  * emulator here; a real devnet in an integration test elsewhere).
  *
  * The validators are parameterised from the interface-contract blueprint, so these tests run only
  * once an on-chain component has produced `../blueprint/plutus.json`; standalone (no on-chain in the
  * project) they are skipped.
  */
trait TransactionsTestBase { self: AnyFunSuite =>
    def createAppCtx(tokenName: String): AppCtx

    /** Load the blueprint's validators, or cancel the test if no on-chain component has produced the
      * blueprint yet.
      */
    protected def loadScripts(): GiftCardScripts = {
        assume(
          GiftCardScripts.blueprintExists(),
          s"${GiftCardScripts.DefaultBlueprintPath} not found — build an on-chain component to run " +
              "the gift-card flow tests"
        )
        GiftCardScripts.fromFile()
    }

    test("createGiftCard mints one token and locks the gift at the redeem address") {
        val appCtx = createAppCtx("Gift Card")
        val txBuilder = Transactions(appCtx, loadScripts())

        txBuilder.createGiftCard(5_000_000L) match
            case Right(card) =>
                println(s"created gift card: ${card.txHash}")
                // The card's unit is its policy id (28 bytes = 56 hex chars) followed by the token
                // name in hex — non-empty, so a real token was parameterised and minted.
                assert(card.unit.length > 56)
                assert(card.redeemAddress.nonEmpty)
            case Left(err) => fail(err)
    }
}
