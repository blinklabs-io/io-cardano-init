package app

import org.scalatest.funsuite.AnyFunSuite
import scalus.cardano.ledger.AssetName
import scalus.utils.await

import scala.concurrent.ExecutionContext.Implicits.global
import scala.concurrent.duration.*

/** End-to-end create + redeem against the in-memory emulator — the full gift-card flow with no
  * blockchain: mint and lock the card, then burn it and release the gift, verifying the token is
  * gone at each stage.
  */
class TransactionsEmulatorTest extends AnyFunSuite with TransactionsTestBase with EmulatorTest {

    test("a gift card can be created and then redeemed, burning its token") {
        val appCtx = createAppCtx("Gift Card")
        val txBuilder = Transactions(appCtx)

        val card = txBuilder.createGiftCard(5_000_000L) match
            case Right(c)  => c
            case Left(err) => fail(s"create failed: $err")

        // After creation the token is locked at the redeem address, not in the wallet.
        assert(walletHoldsGiftToken(appCtx, card).isEmpty, "token should be locked, not in the wallet")

        assert(txBuilder.redeemGiftCard(card.seed).isRight, "redeem should succeed")

        // After redeeming, the token is burned: it exists nowhere the wallet can see.
        assert(walletHoldsGiftToken(appCtx, card).isEmpty, "token should be burned after redeem")
    }

    test("redeem fails when there is no matching gift card at the redeem address") {
        val appCtx = createAppCtx("Gift Card")
        val txBuilder = Transactions(appCtx)

        // Create then redeem once; a second redeem of the same card must fail (nothing left to spend).
        val card = txBuilder.createGiftCard(5_000_000L).toOption.get
        assert(txBuilder.redeemGiftCard(card.seed).isRight)
        assert(txBuilder.redeemGiftCard(card.seed).isLeft)
    }

    /** UTxOs in the wallet that carry this card's gift-card token, if any. */
    private def walletHoldsGiftToken(appCtx: AppCtx, card: CreatedGiftCard) = {
        val policyId = GiftCardContract
            .makeGiftCardScript(appCtx.tokenNameByteString, card.seed)
            .script
            .scriptHash
        val utxos = appCtx.provider.findUtxos(appCtx.address).await(10.seconds).toOption.get
        utxos.filter { case (_, out) => out.value.assets.assets.contains(policyId) }
    }
}
