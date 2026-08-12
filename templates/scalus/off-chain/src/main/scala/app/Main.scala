package app

import scalus.crypto.ed25519.given

/** The gift-card token this example mints/burns. Change it and watch the policy ID change. */
private val ExampleTokenName = "Gift Card"

/** Start the gift-card REST API, choosing the network from the shared `../.env` (see
  * AppCtx.fromEnv): a local devnet if one is running, otherwise Blockfrost.
  *
  * The validators are loaded from `../blueprint/plutus.json` (produced by whichever on-chain tool
  * fills the on-chain role), so build an on-chain component first.
  */
@main def start(): Unit = {
    Logging.configure()
    val ctx = AppCtx.fromEnv(ExampleTokenName)
    println("Starting the gift-card server on http://localhost:8088 (Swagger UI at /docs) ...")
    Server(ctx).start()
}

/** Start the gift-card REST API against a local Yaci DevKit devnet (built-in test wallet). */
@main def yaciDevKit(): Unit = {
    Logging.configure()
    val ctx = AppCtx.yaciDevKit(ExampleTokenName)
    println("Starting the gift-card server against a local Yaci DevKit devnet on http://localhost:8088 ...")
    Server(ctx).start()
}
