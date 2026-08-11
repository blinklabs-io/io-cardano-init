{-# LANGUAGE NoImplicitPrelude #-}

module RedeemValidator where

import           PlutusLedgerApi.V1.Value (CurrencySymbol (..), TokenName (..),
                                           flattenValue)
import           PlutusLedgerApi.V3       (ScriptContext (..), TxInfo (..),
                                           mintValueBurned)
import           PlutusTx
import qualified PlutusTx.List            as List
import           PlutusTx.Prelude

{-# INLINEABLE redeemTypedValidator #-}

{- | Spending validator guarding the assets locked alongside a gift card. It
only allows the UTxO to be spent when the matching gift-card token — identified
by the parameterised policy id and token name — is burned in the same
transaction. The datum and redeemer are unused: the burn is the authorisation.
-}
redeemTypedValidator ::
  BuiltinByteString ->
  BuiltinByteString ->
  ScriptContext ->
  Bool
redeemTypedValidator tokenNameBytes policyIdBytes (ScriptContext txInfo _ _) =
  case ownBurned of
    [(tn, amt)] -> tn == TokenName tokenNameBytes && amt == 1
    _           -> False
  where
    policy :: CurrencySymbol
    policy = CurrencySymbol policyIdBytes

    -- Tokens of the gift-card policy burned in this transaction, as
    -- (token name, positive quantity) pairs.
    ownBurned :: [(TokenName, Integer)]
    ownBurned =
      List.map (\(_, tn, amt) -> (tn, amt))
        ( List.filter
            (\(cs, _, _) -> cs == policy)
            (flattenValue (mintValueBurned (txInfoMint txInfo)))
        )

{-# INLINEABLE redeemUntypedValidator #-}
redeemUntypedValidator ::
  BuiltinData -> BuiltinData -> BuiltinData -> BuiltinUnit
redeemUntypedValidator tokenName policyId ctx =
  check
    ( redeemTypedValidator
        (unsafeFromBuiltinData tokenName)
        (unsafeFromBuiltinData policyId)
        (unsafeFromBuiltinData ctx)
    )

{- | The parameterised redeem validator. Its two compile-time parameters — token
name and gift-card policy id — are left free so the off-chain side applies them.
-}
redeemValidatorScript ::
  CompiledCode (BuiltinData -> BuiltinData -> BuiltinData -> BuiltinUnit)
redeemValidatorScript = $$(compile [||redeemUntypedValidator||])
