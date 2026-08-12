package app

import org.scalatest.funsuite.AnyFunSuite
import scalus.cardano.ledger.AssetName

/** Transaction-building assertions shared by any environment that can supply an [[AppCtx]] (the
  * emulator here; a real devnet in an integration test elsewhere).
  */
trait TransactionsTestBase { self: AnyFunSuite =>
    def createAppCtx(tokenName: String): AppCtx

    test("createGiftCard mints one token and locks the gift at the redeem address") {
        val appCtx = createAppCtx("Gift Card")
        val txBuilder = Transactions(appCtx)

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
