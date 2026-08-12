package app

import java.nio.file.{Files, Paths}

import scalus.cardano.address.{Address, Network, ShelleyAddress, ShelleyDelegationPart, ShelleyPaymentPart}
import scalus.cardano.blueprint.Blueprint
import scalus.cardano.ledger.Script
import scalus.cardano.onchain.plutus.v1.{PolicyId, TokenName}
import scalus.cardano.onchain.plutus.v3.TxOutRef
import scalus.uplc.Program
import scalus.uplc.builtin.Data.toData

// The gift-card validators, loaded from the interface-contract blueprint (`../blueprint/plutus.json`).
object GiftCardScripts {

    /** Canonical location of the blueprint, relative to this component's directory. */
    val DefaultBlueprintPath = "../blueprint/plutus.json"

    /** Whether the blueprint has been produced yet (i.e. an on-chain component has been built). */
    def blueprintExists(path: String = DefaultBlueprintPath): Boolean = Files.exists(Paths.get(path))

    /** Load and parse the blueprint, failing with a clear message if it is not there yet. */
    def fromFile(path: String = DefaultBlueprintPath): GiftCardScripts = {
        val p = Paths.get(path)
        if (!Files.exists(p))
            sys.error(
              s"$path not found — build an on-chain component first so it writes the gift-card " +
                  "blueprint the off-chain side parameterises."
            )
        new GiftCardScripts(Blueprint.fromJson(Files.readString(p)))
    }
}

class GiftCardScripts(blueprint: Blueprint) {

    // The unapplied compiled code for each validator, located by the conventional
    // `<validator>.<purpose>` title suffix so the lookup is independent of the module name.
    private val giftCardProgram: Program = programFor("gift_card.mint")
    private val redeemProgram: Program = programFor("redeem.spend")

    private def programFor(titleSuffix: String): Program = {
        val validator = blueprint.validators
            .find(v => v.title == titleSuffix || v.title.endsWith("." + titleSuffix))
            .getOrElse {
                val known = blueprint.validators.map(_.title).mkString(", ")
                sys.error(s"validator '$titleSuffix' not found in blueprint. Available: $known")
            }
        val code = validator.compiledCode.getOrElse(
          sys.error(s"validator '$titleSuffix' has no compiledCode in the blueprint")
        )
        parseCompiledCode(code)
    }

    // Blueprints store compiledCode as single- or double-CBOR depending on the tool; accept both.
    private def parseCompiledCode(hex: String): Program =
        scala.util.Try(Program.fromDoubleCborHex(hex)).getOrElse(Program.fromCborHex(hex))

    /** Parameterise the one-shot minting policy with a token name and seed UTxO, applied as two
      * separate arguments (as `applyParamsToScript` would). Every distinct seed yields a fresh
      * policy id.
      */
    def makeGiftCardScript(tokenName: TokenName, seed: TxOutRef): Script.PlutusV3 =
        Script.PlutusV3(giftCardProgram $ tokenName.toData $ seed.toData)

    /** Parameterise the redeem spending validator with the token name and the gift-card policy id. */
    def makeRedeemScript(tokenName: TokenName, policyId: PolicyId): Script.PlutusV3 =
        Script.PlutusV3(redeemProgram $ tokenName.toData $ policyId.toData)

    /** The enterprise (no-staking) address of a Plutus script on the given network. */
    def scriptAddress(script: Script.PlutusV3, network: Network): Address =
        ShelleyAddress(
          network,
          ShelleyPaymentPart.Script(script.scriptHash),
          ShelleyDelegationPart.Null
        )
}
