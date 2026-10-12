[English](endpoint-key-persistence.md) | [简体中文](endpoint-key-persistence.zh-CN.md)

# Endpoint key persistence

An endpoint save writes its own row and its API key set as one unit. This guide
covers what that guarantees, what a save actually writes to the key table, and
what the key-deletion indexes mean for operations.

## Atomicity

`create_endpoint`, `create_endpoint_with_mcp` and `update_endpoint` run on one
transaction. An update takes the endpoint row's `FOR UPDATE` lock before the key
set is read, so:

- Two overlapping saves of one endpoint serialize. The second waits for the
  first rather than planning its writes against rows the first has not finished
  moving.
- The read that builds the returned endpoint is taken on the same transaction,
  so the response describes the rows this save wrote and not a later state.
- A request that cannot be stored leaves nothing behind. The endpoint row, its
  new name and its key set all roll back together, and a create that is refused
  inserts no endpoint.

A create does not need a separate lock: its own insert already holds the new
parent row for the rest of the transaction.

The handler's work after the store returns — OAuth and managed-MCP side effects —
is outside this transaction and is not part of the guarantee.

## What a save writes

A save compares the stored key set against the requested one and issues only the
statements the difference implies:

| Change | Statement | `updated_at` |
| --- | --- | --- |
| none | nothing | preserved |
| label, secret or enabled flag | update | advances |
| position only | renumber, then restore | preserved |
| new key | insert | new row |
| dropped key | delete | row removed |

Two consequences are worth stating plainly. An unchanged key is not written at
all, so saving a form without touching it does not age it. Reordering a key set
is not an edit to any key, so a reorder moves positions without advancing a
single `updated_at`.

A key is identified by its `key_id` when the request carries one, and by its
`key_label` otherwise, so a client that omits `key_id` keeps the identity and
creation time of the key it names. An `key_id` that is no longer stored — a
concurrent save may have replaced it — falls back to the label, and to a fresh
insert when the label is new too. That is what keeps a stale request from either
failing or losing a key.

Reordering moves every moving key out of the way first and then puts it back at
its requested position, so a cycle never sends two keys to the same slot. A
request whose positions or key claims collide is refused before any statement
runs.

## Key deletion and the foreign key indexes

`usage_charges.endpoint_key_id` and `conversation_endpoint_overrides.endpoint_key_id`
are both foreign keys onto `endpoint_api_keys` with `ON DELETE SET NULL`. Deleting
a key has to find every row that references it, and both columns carry indexes
built for exactly that lookup:

- `idx_usage_charges_endpoint_key_id`
- `idx_conversation_endpoint_overrides_endpoint_key_id`

Deleting a key still keeps its rows. The references are set to NULL, so a charge
and its history remain, attributed to the endpoint rather than to a key.

`request_records` carries no foreign key onto the key table and is deliberately
not indexed for one.

### Index operations

Both indexes are built with `CREATE INDEX CONCURRENTLY` through sqlx's
`-- no-transaction` directive. PostgreSQL refuses `CONCURRENTLY` inside a
transaction block, and sqlx sends a migration file as one batch, so each
migration holds exactly one statement — there are two migration pairs, one per
table, for that reason.

The consequence for operators is that a concurrent build that fails leaves an
index that exists but is not valid. PostgreSQL will not use it, and it has to be
inspected and repaired or dropped by hand. The tests check validity rather than
presence, because a name in the catalog proves nothing about whether the index is
usable. Nothing in the application drops an index it did not create.

Applying either migration to production needs operator approval and a window.
The index build is online by construction, but the rollout itself is not part of
this codebase's remit.

## Verification

`tests/endpoint_key_fk_baseline.rs` measures what a key deletion costs on a given
database and prints the plans it chose, so the same measurement can be taken
before and after an index exists. It reports, and does not gate on, elapsed time:
a timing assertion would turn a machine's mood into a test failure. The state it
measured — including which indexes were present — is printed alongside, so a
later run cannot be read as an earlier one.
