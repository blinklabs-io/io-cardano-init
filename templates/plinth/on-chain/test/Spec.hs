module Main where

import Control.Monad (when)
import Data.ByteString.Short qualified as Short
import GiftCardMintingPolicy (giftCardMintingPolicyScript)
import PlutusLedgerApi.Common (serialiseCompiledCode)
import RedeemValidator (redeemValidatorScript)
import System.Exit (exitFailure)

-- | A minimal smoke test. Serialising the compiled code forces the Plinth
-- plugin to compile each validator all the way to Plutus Core, so a compile
-- error surfaces here; we then assert the resulting scripts are non-empty.
main :: IO ()
main = do
  let mintBytes = Short.length (serialiseCompiledCode giftCardMintingPolicyScript)
      redeemBytes = Short.length (serialiseCompiledCode redeemValidatorScript)
  putStrLn ("gift-card minting policy: " <> show mintBytes <> " bytes of Plutus Core")
  putStrLn ("redeem validator:         " <> show redeemBytes <> " bytes of Plutus Core")
  when (mintBytes == 0 || redeemBytes == 0) $ do
    putStrLn "FAIL: a validator compiled to an empty script"
    exitFailure
  putStrLn "ok: both validators compile to Plutus Core and serialise"
