package app

import java.nio.charset.StandardCharsets
import java.nio.file.{Files, Paths}

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
