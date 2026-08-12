package app

import org.scalatest.Suite
import scalus.cardano.address.Network
import scalus.cardano.node.Emulator
import scalus.cardano.wallet.hd.HdAccount
import scalus.crypto.ed25519.given

/** Mixes an in-memory Cardano [[Emulator]] into a test: transactions are built, submitted, and
  * confirmed instantly, with no blockchain and no Docker.
  */
trait EmulatorTest { self: Suite =>
    def createAppCtx(tokenName: String): AppCtx = {
        val mnemonic =
            "test test test test test test test test test test test test test test test test test test test test test test test sauce"
        val account = HdAccount.fromMnemonic(mnemonic)
        val address = account.baseAddress(Network.Mainnet)
        // Fund the wallet with several UTxOs (like a real wallet): the gift-card flow spends one as
        // the one-shot seed and still needs others to cover the fee and the Plutus collateral.
        val emulator = Emulator.withAddresses(Seq.fill(5)(address))
        new AppCtx(
          emulator.cardanoInfo,
          emulator,
          account,
          account.signerForUtxos,
          tokenName
        )
    }
}
