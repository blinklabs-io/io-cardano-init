package app

import org.scalatest.funsuite.AnyFunSuite
import scalus.utils.await

import scala.concurrent.ExecutionContext.Implicits.global
import scala.concurrent.duration.*

/** End-to-end mint + burn against the in-memory emulator — the full off-chain flow with no
  * blockchain: build, submit, confirm, then verify the tokens were burned.
  */
class TransactionsEmulatorTest extends AnyFunSuite with TransactionsTestBase with EmulatorTest {

    test("submitMintingTx burns tokens when the amount is negative") {
        val appCtx = createAppCtx("CO2 Tonne")
        val txBuilder = Transactions(appCtx)

        assert(txBuilder.submitMintingTx(1000).isRight)
        assert(txBuilder.submitMintingTx(-1000).isRight)

        val utxos = appCtx.provider.findUtxos(appCtx.address).await(10.seconds).toOption.get
        val tokenUtxos = utxos.filter { case (_, utxo) =>
            utxo.value.assets.assets.contains(appCtx.mintingScript.script.scriptHash)
        }
        assert(tokenUtxos.isEmpty, s"Expected all tokens burned but found: $tokenUtxos")
    }

    test("submitMintingTx rejects a zero amount") {
        val appCtx = createAppCtx("CO2 Tonne")
        assert(Transactions(appCtx).submitMintingTx(0).isLeft)
    }
}
