-- | Placeholder parameters used to instantiate the example validators for
-- blueprint generation and the smoke test. Replace these with your own values
-- (seller key hash, auctioned asset, minimum bid, deadline) for a real auction.
module Demo (demoSellerPkh, demoAuctionParams) where

import AuctionValidator (AuctionParams (..))
import PlutusLedgerApi.V1.Crypto qualified as Crypto
import PlutusLedgerApi.V1.Time qualified as Time
import PlutusLedgerApi.V1.Value qualified as Value
import PlutusTx.Builtins.HasOpaque (stringToBuiltinByteStringHex)

-- | Hex-encoded public key hash of the seller. Replace with a real one.
demoSellerPkh :: Crypto.PubKeyHash
demoSellerPkh =
  Crypto.PubKeyHash
    ( stringToBuiltinByteStringHex
        "0000000000000000000000000000000000000000\
        \0000000000000000000000000000000000000000"
    )

-- | Fully-applied auction parameters for the example.
demoAuctionParams :: AuctionParams
demoAuctionParams =
  AuctionParams
    { apSeller = demoSellerPkh
    , apCurrencySymbol =
        -- Replace with your desired currency symbol (minting policy hash):
        Value.CurrencySymbol
          ( stringToBuiltinByteStringHex
              "00000000000000000000000000000000000000000000000000000000"
          )
    , apTokenName =
        -- Replace with your desired token name:
        Value.tokenName "MY_TOKEN"
    , apMinBid =
        -- Minimal bid in lovelace:
        100
    , apEndTime =
        -- Replace with your desired end time in milliseconds:
        Time.fromMilliSeconds 1_725_227_091_000
    }
