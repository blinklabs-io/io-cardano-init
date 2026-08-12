package app

import scalus.uplc.builtin.{FromData, ToData}

/** Off-chain view of the gift-card contract's data. This is the only contract-specific type the
  * off-chain side needs to define: the seed UTxO parameter and datum are just Scalus's own
  * `TxOutRef` (which serialises to the same `Constr 0 [transaction_id, index]` shape Aiken and MeshJS
  * use). It builds redeemers, parameters, and datums, then applies them to the compiled validators
  * taken from the blueprint (see [[GiftCardScripts]]); the validator *logic* lives in the on-chain
  * component, not here.
  */

/** Mint redeemer: create the gift card (`Constr 0 []`) or redeem it (`Constr 1 []`). Matches the
  * `Action` type of the Aiken/Plinth/Scalus gift-card validators.
  */
enum Action derives FromData, ToData:
    case Mint
    case Burn
