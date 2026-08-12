package app

import scalus.uplc.builtin.Data
import scalus.uplc.builtin.Data.toData
import scalus.cardano.ledger.{AssetName, Coin, Transaction, Utxo, Value}
import scalus.cardano.onchain.plutus.v3.{TxId, TxOutRef}
import scalus.cardano.txbuilder.TxBuilder
import scalus.utils.*
import scalus.utils.await

import scala.concurrent.ExecutionContext.Implicits.global
import scala.concurrent.duration.*
import scala.util.Try

/** A gift card the wallet just created — everything needed to locate and later redeem it.
  *
  * The token name (fixed for this app) plus `seed` (the one-shot UTxO) fully determine the gift
  * card's policy ID and redeem address, so `seed` is all a redeemer needs.
  */
case class CreatedGiftCard(
    txHash: String,
    seed: TxOutRef,
    redeemAddress: String,
    unit: String
)

/** Transaction building for the gift-card flow, using Scalus's [[TxBuilder]] (UTxO selection,
  * collateral, and balancing are automatic).
  *
  * The validators come from the interface-contract blueprint (`../blueprint/plutus.json`) via
  * [[GiftCardScripts]] — this component builds transactions for whatever contract the on-chain tool
  * produced, and never carries its own copy.
  *
  * A gift card is created in one transaction and redeemed in another:
  *   - **create** mints a unique one-shot token and locks it, together with the gift, at the redeem
  *     script address (recording the seed as an inline datum).
  *   - **redeem** spends that locked UTxO and burns the token in the same transaction, releasing the
  *     gift to the wallet.
  *
  * @param ctx
  *   application context: provider, signer, wallet address, and the gift-card token name.
  * @param scripts
  *   the gift-card validators loaded and parameterised from the blueprint.
  */
class Transactions(ctx: AppCtx, scripts: GiftCardScripts) {

    private val tokenName = ctx.tokenNameByteString
    private val assetName = AssetName(tokenName)

    /** Create a gift card holding `giftLovelace` lovelace.
      *
      * Picks a wallet UTxO as the one-shot seed (spending it is what makes the gift-card token
      * unique), parameterises the minting policy and redeem validator with it, mints one token, and
      * locks the gift plus the token at the redeem address.
      *
      * @param giftLovelace
      *   lovelace to lock in the card (must cover the min-UTxO for an output carrying a token)
      * @return
      *   either an error message or details of the created card
      */
    def createGiftCard(giftLovelace: Long): Either[String, CreatedGiftCard] = {
        Try {
            val walletUtxos = ctx.provider
                .findUtxos(ctx.address)
                .await(30.seconds)
                .getOrElse(sys.error("could not query wallet UTxOs"))
            val (seedInput, seedOutput) =
                walletUtxos.headOption.getOrElse(
                  sys.error("wallet has no UTxOs to seed a gift card")
                )
            val seed = TxOutRef(TxId(seedInput.transactionId), BigInt(seedInput.index))

            val giftCardScript = scripts.makeGiftCardScript(tokenName, seed)
            val policyId = giftCardScript.scriptHash
            val redeemScript = scripts.makeRedeemScript(tokenName, policyId)
            val redeemAddress = scripts.scriptAddress(redeemScript, ctx.cardanoInfo.network)
            val lockedValue = Value.asset(policyId, assetName, 1L, Coin(giftLovelace))

            val tx = TxBuilder(ctx.cardanoInfo)
                .spend(Utxo(seedInput, seedOutput))
                .mint(giftCardScript, Map(assetName -> 1L), Action.Mint)
                .payTo(redeemAddress, lockedValue, seed.toData)
                .complete(ctx.provider, sponsor = ctx.address)
                .await(30.seconds)
                .sign(ctx.signer)
                .transaction

            println(tx.showDetailed)
            val hash = submit(tx)
            CreatedGiftCard(
              txHash = hash,
              seed = seed,
              redeemAddress = redeemAddress.encode.getOrElse(redeemAddress.toString),
              unit = (policyId ++ tokenName).toHex
            )
        }.toEither.left.map(_.getMessage)
    }

    /** Redeem a previously created gift card: burn its token and release the locked gift back to the
      * wallet.
      *
      * @param seed
      *   the one-shot seed the card was created with (from [[CreatedGiftCard]]); with the app's
      *   token name it pins down the policy ID and redeem address.
      * @return
      *   either an error message or the redeeming transaction's hash (hex)
      */
    def redeemGiftCard(seed: TxOutRef): Either[String, String] = {
        Try {
            val giftCardScript = scripts.makeGiftCardScript(tokenName, seed)
            val policyId = giftCardScript.scriptHash
            val redeemScript = scripts.makeRedeemScript(tokenName, policyId)
            val redeemAddress = scripts.scriptAddress(redeemScript, ctx.cardanoInfo.network)

            val utxos = ctx.provider
                .findUtxos(redeemAddress)
                .await(30.seconds)
                .getOrElse(sys.error("could not query the redeem address"))
            val (giftInput, giftOutput) = utxos
                .find { case (_, out) =>
                    out.value.assets.assets.get(policyId).exists(_.contains(assetName))
                }
                .getOrElse(sys.error("no gift-card UTxO found at the redeem address"))

            val tx = TxBuilder(ctx.cardanoInfo)
                // The redeem validator ignores its redeemer — burning the token authorises the spend.
                .spend(Utxo(giftInput, giftOutput), (_: Transaction) => Data.unit, redeemScript)
                .mint(giftCardScript, Map(assetName -> -1L), Action.Burn)
                .complete(ctx.provider, sponsor = ctx.address)
                .await(30.seconds)
                .sign(ctx.signer)
                .transaction

            println(tx.showDetailed)
            submit(tx)
        }.toEither.left.map(_.getMessage)
    }

    /** Submit a signed transaction, returning its hash (hex) or throwing on a submit error. */
    private def submit(tx: Transaction): String =
        ctx.provider.submit(tx).await(30.seconds) match
            case Right(hash) => hash.toHex
            case Left(err)   => sys.error(err.toString)
}
