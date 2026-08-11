module Main where

import AuctionValidator (auctionValidatorScript)
import AuctionMintingPolicy (auctionMintingPolicyScript)
import Demo (demoAuctionParams, demoSellerPkh)
import Control.Monad (when)
import Data.ByteString.Short qualified as Short
import PlutusLedgerApi.Common (serialiseCompiledCode)
import System.Exit (exitFailure)

-- | A minimal smoke test. Serialising the compiled code forces the Plinth
-- plugin to compile each validator all the way to Plutus Core, so a compile
-- error surfaces here; we then assert the resulting scripts are non-empty.
main :: IO ()
main = do
  let auctionBytes = Short.length (serialiseCompiledCode (auctionValidatorScript demoAuctionParams))
      mintBytes = Short.length (serialiseCompiledCode (auctionMintingPolicyScript demoSellerPkh))
  putStrLn ("auction validator: " <> show auctionBytes <> " bytes of Plutus Core")
  putStrLn ("minting policy:    " <> show mintBytes <> " bytes of Plutus Core")
  when (auctionBytes == 0 || mintBytes == 0) $ do
    putStrLn "FAIL: a validator compiled to an empty script"
    exitFailure
  putStrLn "ok: both validators compile to Plutus Core and serialise"
