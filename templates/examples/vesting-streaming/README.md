# Vesting and Streaming Payments

A Soroban template for time-based token vesting and payment streaming.

## Features

- Cliff vesting followed by linear vesting.
- Zero-duration schedules.
- Schedules whose start time is already in the past.
- Optional admin revocation with vested amounts preserved.
- Beneficiary changes before revocation.
- Events for claims, beneficiary changes, and revocation.
- Query functions for vested, claimed, and claimable amounts.

## Usage

Initialize a schedule with:

- `admin`: account allowed to fund and revoke the schedule.
- `beneficiary`: account receiving vested tokens.
- `token`: token contract used for payments.
- `amount`: total amount to vest.
- `start`: Unix timestamp when the schedule starts.
- `cliff`: timestamp before which nothing is vested.
- `end`: timestamp when the full amount is vested.
- `revocable`: whether the admin may revoke the schedule.

After initialization, the admin calls `fund` to transfer the full schedule
amount to the contract. The beneficiary calls `claim` as tokens become vested.

`change_beneficiary` allows the current beneficiary to transfer the remaining
vesting rights to another account.

If the schedule is revocable, `revoke` returns the unvested balance to the
admin and freezes the vested amount so the beneficiary can still claim it.

## Vesting rules

- Before the cliff: `0` is vested.
- At or after the end: the full amount is vested.
- Between cliff and end: vesting is linear.
- If `start == end`, the full amount becomes vested at the schedule timestamp.
- A start timestamp in the past is valid; vesting is calculated from the
  current ledger timestamp.
- After revocation, vesting is frozen at the amount vested when revocation
  occurred.

## Events

The contract emits events for:

- `claim`
- `beneficiary`
- `revoke`

These events provide indexers with the key state transitions.

## Tests

The template includes tests covering:

- Linear vesting after the cliff.
- Zero-duration schedules.
- Past start timestamps.
- Beneficiary changes.
- Revocation and recovery of unvested tokens.
