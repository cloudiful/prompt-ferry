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

## Coordinated upgrade

The peer protocol of the node links is version 2. A node that speaks the
previous peer protocol is refused during the authenticated handshake, so all
nodes of an installation are upgraded together. The upgrade needs no
database migration: the routes, the links and the forwarding state live in
memory only. The protocol of the database gate and the wire format of the
business API are unchanged.
