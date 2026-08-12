{-# LANGUAGE NoImplicitPrelude #-}

module GiftCardMintingPolicy where

import           GHC.Generics             (Generic)

import           PlutusLedgerApi.V1.Value (Value, flattenValue)
import           PlutusLedgerApi.V3       (CurrencySymbol, ScriptContext (..),
                                           ScriptInfo (..), TokenName (..),
                                           TxInInfo (..), TxInfo (..), TxOutRef,
                                           getRedeemer, mintValueBurned,
                                           mintValueMinted)
import           PlutusTx
-- Hide the blueprint 'Mint' *purpose* so it doesn't clash with our redeemer's constructor
import           PlutusTx.Blueprint       hiding (Mint)
import qualified PlutusTx.List            as List
import           PlutusTx.Prelude

{- | Redeemer for the gift-card minting policy: mint the one-shot token, or burn
it. Encoded as @Constr 0 []@ / @Constr 1 []@, matching what the off-chain side
supplies as the mint redeemer.
-}
data GiftCardAction = Mint | Burn
  deriving stock (Generic)
  deriving anyclass (HasBlueprintDefinition)

makeIsDataSchemaIndexed ''GiftCardAction [('Mint, 0), ('Burn, 1)]

{-# INLINEABLE giftCardTypedMintingPolicy #-}

{- | One-shot minting policy parameterised by the gift-card token name and a seed
UTxO. Minting requires that seed UTxO to be spent (making the token unique) and
mints exactly one token of the given name; burning requires exactly one to be
burned. The policy id (own currency symbol) comes from the V3 script info.
-}
giftCardTypedMintingPolicy ::
  BuiltinByteString ->
  TxOutRef ->
  ScriptContext ->
  Bool
giftCardTypedMintingPolicy tokenNameBytes seed (ScriptContext txInfo scriptRedeemer scriptInfo) =
  case action of
    Mint -> seedSpent && exactlyOne (mintValueMinted (txInfoMint txInfo))
    Burn -> exactlyOne (mintValueBurned (txInfoMint txInfo))
  where
    action :: GiftCardAction
    action = case fromBuiltinData (getRedeemer scriptRedeemer) of
      Just a  -> a
      Nothing -> traceError "gift_card: invalid redeemer"

    ownCurrencySymbol :: CurrencySymbol
    ownCurrencySymbol = case scriptInfo of
      MintingScript cs -> cs
      _                -> traceError "gift_card: not a minting script"

    -- The (token name, quantity) entries of this policy in the given value.
    ownEntries :: Value -> [(TokenName, Integer)]
    ownEntries value =
      List.map (\(_, tn, amt) -> (tn, amt))
        (List.filter (\(cs, _, _) -> cs == ownCurrencySymbol) (flattenValue value))

    -- Exactly one token of this policy, named `token_name`, quantity one. For a
    -- burn, `mintValueBurned` reports the burned amount as a positive quantity.
    exactlyOne :: Value -> Bool
    exactlyOne value = case ownEntries value of
      [(tn, amt)] -> tn == TokenName tokenNameBytes && amt == 1
      _           -> False

    -- The parameterised seed UTxO is among this transaction's inputs. Marked
    -- lazy (`~`) so the strict-by-default 'Strict' extension doesn't force it
    -- on the 'Burn' path, which never inspects the inputs.
    seedSpent :: Bool
    ~seedSpent =
      List.any (\txIn -> txInInfoOutRef txIn == seed) (txInfoInputs txInfo)

{-# INLINEABLE giftCardUntypedMintingPolicy #-}
giftCardUntypedMintingPolicy ::
  BuiltinData -> BuiltinData -> BuiltinData -> BuiltinUnit
giftCardUntypedMintingPolicy tokenName seed ctx =
  check
    ( giftCardTypedMintingPolicy
        (unsafeFromBuiltinData tokenName)
        (unsafeFromBuiltinData seed)
        (unsafeFromBuiltinData ctx)
    )

{- | The parameterised minting policy. The two compile-time parameters — token
name and seed UTxO — are left free, so the off-chain side applies them with
`applyParamsToScript` and every gift card gets a fresh policy id.
-}
giftCardMintingPolicyScript ::
  CompiledCode (BuiltinData -> BuiltinData -> BuiltinData -> BuiltinUnit)
giftCardMintingPolicyScript = $$(compile [||giftCardUntypedMintingPolicy||])
