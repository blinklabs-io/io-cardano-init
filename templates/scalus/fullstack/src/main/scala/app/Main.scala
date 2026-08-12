package app

import scalus.cardano.blueprint.Blueprint
import scalus.crypto.ed25519.given
import scalus.utils.Hex.toHex

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Paths}

/** The gift-card token this example mints/burns. Change it and watch the policy ID change. */
private val ExampleTokenName = "Gift Card"

/** Write the CIP-57 blueprint for the gift-card validators to `path`.
  *
  * This is the on-chain interface-contract seam: the blueprint is the canonical, tool-agnostic
  * description of the compiled validators that every other component reads from
  * `../blueprint/plutus.json`. Called by `just build`.
  */
@main def genBlueprint(path: String): Unit = {
    val json = GiftCardContract.blueprint.toJson()
    val out = Paths.get(path)
    Option(out.getParent).foreach(Files.createDirectories(_))
    Files.write(out, json.getBytes(StandardCharsets.UTF_8))
    println(s"Wrote CIP-57 blueprint for the gift-card validators to $path")
}

/** Cross-check the on-chain blueprint at `path` against this component's bundled gift-card
  * validators.
  *
  * Used by the off-chain `just build`: it consumes the interface-contract blueprint and reports
  * whether it matches the validators this off-chain example builds transactions for. Missing or
  * mismatched blueprints only warn — the off-chain code still compiles and runs against its own
  * bundled validators.
  */
@main def verifyBlueprint(path: String): Unit = {
    val out = Paths.get(path)
    if (!Files.exists(out))
        println(
          s"[blueprint] $path not found yet — build the on-chain component to produce it. " +
              "Off-chain still compiles and runs against its bundled gift-card validators."
        )
    else {
        val onDisk = Blueprint.fromJson(Files.readString(out)).validators.flatMap(_.compiledCode).toSet
        val ours = Set(
          GiftCardContract.giftCardCompiled.program.cborEncoded.toHex,
          GiftCardContract.redeemCompiled.program.cborEncoded.toHex
        )
        if (ours.subsetOf(onDisk))
            println("[blueprint] OK — the on-chain blueprint matches the bundled gift-card validators.")
        else if (onDisk.isEmpty)
            println(s"[blueprint] WARNING: no compiledCode found in $path.")
        else
            println(
              s"[blueprint] NOTE: $path describes a different contract than this off-chain " +
                  "example's bundled gift-card validators. That is expected when Scalus off-chain is " +
                  "paired with a non-Scalus on-chain tool; adapt Transactions.scala to it."
            )
    }
}

/** Start the gift-card REST API, choosing the network from the shared `../.env` (see
  * AppCtx.fromEnv): a local devnet if one is running, otherwise Blockfrost.
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
