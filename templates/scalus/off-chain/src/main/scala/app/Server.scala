package app

import scalus.uplc.builtin.ByteString
import scalus.cardano.address.{Address, Network}
import scalus.cardano.ledger.CardanoInfo
import scalus.utils.await

import scala.concurrent.duration.*
import scalus.cardano.node.{BlockchainProvider, BlockfrostProvider}
import scalus.cardano.txbuilder.TransactionSigner
import scalus.cardano.wallet.hd.HdAccount
import scalus.crypto.ed25519.Ed25519Signer
import sttp.client4.DefaultFutureBackend
import sttp.tapir.*
import sttp.tapir.server.netty.sync.NettySyncServer
import sttp.tapir.swagger.bundle.SwaggerInterpreter

import scala.concurrent.ExecutionContext.Implicits.global

// STTP backend required for BlockfrostProvider's HTTP calls
given sttp.client4.Backend[scala.concurrent.Future] = DefaultFutureBackend()

/** Application context holding all configuration and dependencies.
  *
  * Wires together the network connection (via provider), the wallet/signing capabilities, and the
  * gift-card token name. The gift-card scripts themselves are *not* here: each card is parameterised
  * by its own one-shot seed UTxO, so its policy ID and redeem address are computed per card in
  * [[Transactions]].
  *
  * @param cardanoInfo
  *   protocol parameters and network configuration
  * @param provider
  *   blockchain data provider (Blockfrost or a local devnet)
  * @param account
  *   HD wallet account for address derivation
  * @param signer
  *   transaction signer with private keys
  * @param tokenName
  *   the name of the gift-card token minted/burned
  */
case class AppCtx(
    cardanoInfo: CardanoInfo,
    provider: BlockchainProvider,
    account: HdAccount,
    signer: TransactionSigner,
    tokenName: String
) {
    lazy val tokenNameByteString: ByteString = ByteString.fromString(tokenName)
    lazy val address: Address = account.baseAddress(cardanoInfo.network)
}

/** Factory methods for creating AppCtx for different environments. */
object AppCtx {

    /** Creates an AppCtx from the shared `../.env` (the cardano-init connection seam).
      *
      * The choice of network is driven entirely by `../.env` and `.env.local`; this code never
      * names the tool that wrote the connection details:
      *   - `INDEXER_URL` set (a local devnet is up, e.g. Yaci DevKit's `just dev`) -> connect to
      *     that devnet with its built-in test wallet; no Blockfrost key needed.
      *   - `INDEXER_URL` absent -> Blockfrost, using `BLOCKFROST_API_KEY` + `MNEMONIC` and
      *     `CARDANO_NETWORK` (preview | preprod | mainnet; default preview).
      *
      * @param tokenName
      *   name for the gift-card token
      */
    def fromEnv(tokenName: String)(using Ed25519Signer): AppCtx = {
        val env = DotEnv.load()
        def get(key: String): Option[String] = env.get(key).map(_.trim).filter(_.nonEmpty)

        get("INDEXER_URL") match
            case Some(_) => yaciDevKit(tokenName)
            case None =>
                val apiKey = get("BLOCKFROST_API_KEY").getOrElse(
                  sys.error(
                    "No local devnet is running (INDEXER_URL is unset in ../.env) and " +
                        "BLOCKFROST_API_KEY is not set. Either start a devnet " +
                        "(e.g. `just -f devnet/Justfile dev`) or set BLOCKFROST_API_KEY + " +
                        "MNEMONIC in .env.local (see .env.example)."
                  )
                )
                val mnemonic = get("MNEMONIC").getOrElse(
                  sys.error("MNEMONIC is not set. See .env.example.")
                )
                val network = get("CARDANO_NETWORK").map(_.toLowerCase).getOrElse("preview")
                apply(network, mnemonic, apiKey, tokenName)
    }

    /** Creates an AppCtx for a public network (mainnet / preprod / preview) using Blockfrost. */
    def apply(
        network: String,
        mnemonic: String,
        blockfrostApiKey: String,
        tokenName: String
    )(using Ed25519Signer): AppCtx = {
        val provider = network match
            case "mainnet" => BlockfrostProvider.mainnet(blockfrostApiKey).await(30.seconds)
            case "preprod" => BlockfrostProvider.preprod(blockfrostApiKey).await(30.seconds)
            case _         => BlockfrostProvider.preview(blockfrostApiKey).await(30.seconds)

        val account = HdAccount.fromMnemonic(mnemonic)

        new AppCtx(provider.cardanoInfo, provider, account, account.signerForUtxos, tokenName)
    }

    /** Creates an AppCtx for local development with Yaci DevKit.
      *
      * Uses a hardcoded test mnemonic and connects to a local Yaci DevKit node. No API keys
      * required. Prerequisites: a local Yaci DevKit devnet running (the devnet component's
      * `just dev`, or see https://devkit.yaci.xyz).
      */
    def yaciDevKit(tokenName: String)(using Ed25519Signer): AppCtx = {
        // Standard test mnemonic - DO NOT use in production!
        val mnemonic =
            "test test test test test test test test test test test test test test test test test test test test test test test sauce"

        val provider = BlockfrostProvider.localYaci().await(30.seconds)
        val account = HdAccount.fromMnemonic(mnemonic)

        new AppCtx(provider.cardanoInfo, provider, account, account.signerForUtxos, tokenName)
    }
}

/** REST API server for the gift-card service.
  *
  * Built with Tapir for type-safe endpoint definitions and automatic OpenAPI generation. Swagger UI
  * is available at /docs for interactive API exploration.
  *
  *   - `POST /gift-card?lovelace=N` — create a gift card locking N lovelace; returns the tx hash.
  *   - `POST /redeem` — redeem the gift card most recently created by this server.
  *
  * @param ctx
  *   application context with blockchain connection and signing capabilities
  */
class Server(ctx: AppCtx):
    // Load the validators from the interface-contract blueprint. Fails fast with a clear message if
    // no on-chain component has produced ../blueprint/plutus.json yet.
    private val txBuilder = Transactions(ctx, GiftCardScripts.fromFile())

    // The last gift card this server created, so `/redeem` knows which card to redeem without the
    // caller having to pass its seed back. A real frontend would track cards per user instead.
    private var lastCard: Option[scalus.cardano.onchain.plutus.v3.TxOutRef] = None

    private val create = endpoint.post
        .in("gift-card")
        .in(query[Long]("lovelace"))
        .out(stringBody)
        .errorOut(stringBody)
        .handle(createGiftCard)

    private val redeem = endpoint.post
        .in("redeem")
        .out(stringBody)
        .errorOut(stringBody)
        .handle(_ => redeemGiftCard())

    private val apiEndpoints = List(create, redeem)

    private val swaggerEndpoints = SwaggerInterpreter()
        .fromEndpoints[[X] =>> X](apiEndpoints.map(_.endpoint), "Gift Card", "0.1")

    private def createGiftCard(lovelace: Long): Either[String, String] =
        txBuilder.createGiftCard(lovelace) match
            case Right(card) =>
                synchronized { lastCard = Some(card.seed) }
                println(s"Created gift card ${card.unit} at ${card.redeemAddress}: ${card.txHash}")
                Right(s"created ${card.unit} in tx ${card.txHash}; redeem with POST /redeem")
            case Left(err) =>
                println(s"Error creating gift card: $err")
                Left(err)

    private def redeemGiftCard(): Either[String, String] =
        synchronized(lastCard) match
            case None => Left("no gift card to redeem — create one first with POST /gift-card")
            case Some(seed) =>
                txBuilder.redeemGiftCard(seed) match
                    case Right(hash) =>
                        synchronized { lastCard = None }
                        println(s"Redeemed gift card in tx $hash")
                        Right(s"redeemed in tx $hash")
                    case Left(err) =>
                        println(s"Error redeeming gift card: $err")
                        Left(err)

    /** Starts the HTTP server on port 8088 (Swagger UI at /docs). */
    def start(): Unit =
        NettySyncServer()
            .port(8088)
            .addEndpoints(apiEndpoints ++ swaggerEndpoints)
            .startAndWait()
