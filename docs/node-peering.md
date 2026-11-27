# Node peering

Nodes of one installation can talk to each other over authenticated peer
links. This document describes the supported topologies, the limits of a peer
path and the failure behaviour of a node that has no route yet.

The configuration is flat: there is one node command, one listener and one
configuration per node. A node either has a database or it has none, and the
difference is expressed by `PROMPT_FERRY__DATABASE_URL` alone. There is no
role flag, no upstream/downstream key and no separate process type.

## The nodes of a three-node installation

The common shape is a client behind an internal node, a workstation node and
a database node:

| Node | `PROMPT_FERRY__DATABASE_URL` | `PROMPT_FERRY__PEER_URLS` |
| --- | --- | --- |
| internal node (no database) | empty | `[]` |
| workstation node (no database) | empty | `["wss://<internal-entry>/_internal/node", "ws://<vpn-reachable-nas>:8787/_internal/node"]` |
| database node | `postgres://<user>:<password>@<host>:5432/<database>` | `[]` |

All three nodes share one `PROMPT_FERRY__MASTER_SECRET`. The secret is the
trust domain of the peer links: a node only accepts a peer that proves that it
holds the same secret, so the secret must be identical on every node of the
installation and must never leave the installation.

The workstation node dials both of its peers, one link per entry of
`PROMPT_FERRY__PEER_URLS`, and the two links are independent of each other:
either of them may be established first. The database node and the internal
node do not dial the workstation node. A node still accepts an inbound peer
link, so a node may also be reached by a peer that dials it; two nodes that
both own a database never link.

Only the database node owns a database, a registry, an executor and the
background collectors; the nodes without a database keep none of them. A node
without a database serves the browser and the AI API itself and reaches the
administration pages and the assets of the installation through its peer
link. The database node executes every request, with the endpoint and proxy
settings of its database.

`PROMPT_FERRY__PEER_URLS` accepts an absolute `ws://` or `wss://` URL with a
host. The path must be the node endpoint `/_internal/node`; an empty path or
`/` is normalized to it. Any other path, credentials, a query or a fragment
is refused, so a peer URL never points at an arbitrary prefix of a host.

## How a request finds the database node

The database node publishes its route and its configuration snapshot on every
peer link. A node without a database learns at most one route per peer and
uses a single one:

- Only a route that carries the current configuration of the database node is
  eligible. A node remembers the highest configuration version it ever
  validated for the pinned database node and keeps that mark for the life of
  the process, so a route that still carries an older configuration never
  serves the node and a revoked managed key does not come back through it.
- Among the eligible routes the one with the fewest nodes wins. Two routes of
  the same length are ordered by the identity of their peer, so the same set
  of routes always selects the same one. A current configuration outranks the
  distance of a route.
- A node uses exactly one route. A request is never broadcast to several
  peers and never raced between them.
- The first valid route pins the database node for the life of the process.
  A node never silently binds to another database node; a route of another
  database node is refused.
- The route of a node is advertised only to the peers that are not part of
  the path and only when the peer could still extend the path within the
  limit below. A route that is no longer usable is withdrawn.

A request that starts on a node without a database carries the full path from
its origin to the database node. Every node on the path checks its own
position, that the sender is the authenticated neighbour it arrived from,
that the end of the path is the pinned database node and that the rest of the
path is the route it selected itself. A request that fails one of these checks
is refused before it reaches the database node, so a stale or a foreign path
never loops back into the network.

## Limits

- A path holds at most three distinct nodes, so a request travels over at
  most two peer links.
- A path may not name the same node twice and may not name the node that
  checks it more than once.
- A node with no route serves no request: its local API answers with
  `503 no_worker` instead of waiting for a route.

### The outbound queue of a peer link

- A slot of the outbound queue of a link is one encoded frame, not one
  request: a request may travel over a link as several frames. A link holds
  256 pending frames, and the byte budget of the link stays at 16MiB + 64KiB
  of encoded envelopes. The bound of 1024 requests a node forwards at the
  same time is a separate bound and is unchanged.
- The forwarding actor never waits for a peer. It queues a frame without
  waiting and refuses it when the queue cannot take it. The queue is FIFO, so
  the frames of a link keep their order.
- A refusal of a full queue means that the link is saturated. The link loses
  its authority, it is disconnected and the requests that travel on it fail.
  A sustained overload still disconnects a link: the 256 frames are bounded
  headroom for a burst, not immunity against a peer that never drains its
  queue.
- The diagnostics of a refusal name the distinct reason: `queue_full` for a
  saturated link, `queue_closed` for a link that ended, `link_unavailable`
  for a link that is gone, replaced or dead, and the byte budget and the
  encoding failures for a frame that never fitted. A closed link is a
  teardown observation, not a saturation of the link.
- A cancelled request waits behind the data that is already queued on its
  link. There is no bypass for a cancellation and no bypass for a control
  frame.
- The larger queue mitigates the bursts that the regression tests cover. It
  does not prove the cause of every disconnect in the field, so an operator
  has to read the new diagnostics after a release to tell the remaining
  causes apart. Nothing here promises a lossless or downtime-free link, and
  the five-second bound of a session is a bounded refusal, not a network
  send timeout.

### The ingress of the forwarding actor

- The ingress of the forwarding actor holds 256 commands and a byte budget of
  16MiB + 64KiB. The worker pump never waits for that capacity: it queues a
  command without waiting and refuses it when the ingress cannot take it.
- A command that does not fit the ingress is admitted with one initial try and
  at most four further tries, and every further try is preceded by one
  cooperative yield. A refused command keeps its identity and its byte budget
  across the tries: the frame is never rebuilt and the budget is never
  reserved twice. The yield is a best effort of the scheduler, so the recovery
  is finite headroom for a burst and never a promise that the actor runs
  before the tries are over. The fixed number of tries is a policy of the
  ingress, not a wallclock latency or fairness guarantee. When the last try is
  still refused, the command takes the ordinary refusal path of its producer.
- The recovery never waits for the network, and the actor never waits for a
  producer or for the network. A producer of a command that does not fit the
  ingress may await up to four cooperative yields before its command is
  refused, and it holds its own command during that time.
- The guard of a command is checked before every try, so a command is never
  sent after its guard stopped it. The link of a peer is checked before every
  try, so a link that ended during the recovery publishes nothing. The failure
  of the worker of the route interrupts the recovery of the relay state, so a
  failure never waits for the ingress. The end of the failure watch of a
  worker is not a failure of that worker: it is an ordinary invalidation of
  the worker, so it never stops the link of the route.
- The priority path of the actor keeps its own capacity of 64 commands and its
  own byte budget, so a flood of traffic never starves the cleanup of a link
  or the retirement of a request.
- A refusal is scoped to the request of the refused frame. The worker of the
  route, the selected route, the configuration snapshot and the generation of
  the peer link stay, and the other requests of the node keep running. Only the
  request of the refused frame is retired, and a retired request is never
  replayed.
- A refused start of a local request fails its pending request, as it did
  before. A refused continuation or cancellation retires the request itself:
  the queued-start fence of the same request kind and id is cancelled first, so
  a start that waits in the ingress never creates an owner behind the
  retirement, and the pending request of the local client fails with
  `503 peer_overloaded` instead of waiting for an answer that cannot come.
- The queued-start fence is a private flag of the pump and the actor. It is
  never part of an owner and never part of the protocol, and it dies with the
  command that carries it. The fences of the queued starts are bounded by the
  256 slots of the ingress and the one start that waits for the actor, so at
  most 257 fences are alive at the same time.
- A refused frame of a peer schedules a scoped retirement with the live
  admission token of the link, and the caller waits for its acknowledgement, so
  a later frame of the link cannot overtake the retirement. The actor derives
  the direction of the request again from the link generation and the owner it
  holds, so an unknown, a stale or a foreign request id retires nothing and
  the link of the peer survives. Only a retirement that cannot be scheduled or
  acknowledged ends the link, as it did before.
- The retirement accepts a frame only in the direction the normal dispatch
  gives it: a frame of the executor of the route is a response of that
  request, and a frame of a peer never starts a request. A frame of the wrong
  direction is dropped by the normal dispatch, so a full ingress drops it as
  well and never ends a valid request.
- The failed flows are tracked in a bounded set of at most 1024 flows, so a
  request that is refused again and again needs no second retirement. The
  tracking of a flow ends with its terminal message and with the teardown of
  the worker. When that bounded set is exhausted, the worker of the route fails
  as it did before.
- The diagnostics of a refusal use the category `peer_ingress_diag` and name
  the distinct reason: `channel_full` for a full ingress, `byte_budget_exhausted`
  for an exhausted byte budget, `channel_closed` for an ingress that ended,
  `control_unavailable` for a priority path that cannot carry the retirement
  and `retirement_tracking_exhausted` for an exhausted bound. A saturation is
  a warning and a closed teardown is an observation. A closed ingress is
  never reported as a full ingress or as an exhausted byte budget. The
  diagnostics name the variant of the refused frame and the surface of its
  request, the counters of the ingress and the identifiers of the route, never
  a body, a message, a route key or a token.

## Failure behaviour

- The route disappears while the link to its next hop is down. The requests
  that were admitted on the old route fail, the work of the old route is
  cancelled before a new route takes over, and no request is replayed on the
  new route.
- The route comes back after a reconnect. The node waits for the route of the
  new link and serves requests again; the identity of the database node stays
  the pinned one.
- A route comes back with an older configuration than the node already
  served. The node stays without a route and answers `503 no_worker` until a
  peer advertises the current configuration; it never rolls its configuration
  back to an older one.
- The configuration snapshot of the database node is refreshed without a new
  route. The route and the running requests stay as they are; only the
  snapshot changes.
- The link of a peer ends. Only the requests of that link fail; the local
  requests of the node and the requests of the other links keep running.

## Transport and ingress

A peer link is a websocket to `/_internal/node`. The handshake starts with a
challenge on `/_internal/node/challenge` of the same origin, so a reverse
proxy in front of a peer has to publish both paths, and the peer URL has to
name the node endpoint exactly. The websocket carries the node frames; the
business protocol of the API and of the bridge is unchanged.

The peer link uses the TLS configuration of the node: a `wss://` target has to
be trusted by the trust bundle of the node, and the outbound client uses one
shared configuration for all of its peers. A `ws://` target is not encrypted
by the node itself and belongs inside an encrypted network that the operator
controls, such as a VPN between the nodes.

A peer link ignores the HTTP proxy of the host: the challenge request is
sent without a proxy and the websocket never passes through an HTTP proxy.
The proxy of an upstream endpoint stays with the database node. The heartbeat
of a link runs every 30 seconds, so a reverse proxy in front of a peer must
not cut an idle link earlier than that.

The peer endpoint belongs on a private path of the network: keep
`/_internal` off a public proxy and reach a peer over its private address.

## Compatibility and upgrades

The authenticated handshake requires matching peer protocol, peer envelope and
bridge wire versions (currently 3, 2 and 6). The application runtime version is
authenticated diagnostic metadata, not an admission requirement. Incompatible
changes to any of these formats must bump its corresponding format constant.
Protocol 3 refuses protocol 2, so all participating nodes must be upgraded once
for the initial rollout. After that, application releases may differ only while
all three format versions match. The separate database migration and admission
gate remains unchanged.

A database-less ingress can remain pinned to a compatible published release while
the database executor follows a newer release. Preserve the executor's data and
persistent node identity across upgrades, because routes are pinned to that
identity. With one executor, replacement still causes a route outage: requests
fail fast with `503 no_worker` while no route is available, and interrupted
requests are not replayed. A deployment may configure a 30-second executor drain
budget; long requests may be cancelled. This is not an application default, and
the database-less edge shutdown retains its existing 10-second cap. Routes,
links and forwarding state live in memory; this peer protocol upgrade itself
requires no database migration.
