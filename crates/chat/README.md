# Local XMPP chat

With `xmpp_listen` configured, the platform advertises an XMPP address and issues an opaque chat password at session start. The username is the player's UUID. A client opens a TCP stream to the advertised host and port, negotiates STARTTLS, authenticates with SASL PLAIN, restarts the XML stream and binds a resource. The client must trust the listener's TLS certificate for that host. The server accepts a bare UUID or a UUID at its configured domain during authentication.

Clients can request a roster, announce available or unavailable presence, send direct messages and use XMPP ping. Both sender and recipient need a current platform session and authorization through the directory. The server supplies the sender JID on delivery. Direct messages to an unavailable bare JID receive a service-unavailable response. Messages stay in memory, with a bounded queue per resource. The listener limits each stanza to 16 KiB and accepts up to 256 concurrent connections and eight bound resources per player.

## Evidence and tests

The archived Android HTTP trace contains 139 occurrences of both advertised endpoint fields, including repeated responses. Every chat URL uses XMPP on port 5222. The separate notification URL uses WebSocket on port 2112. The archive includes both true and false values of the in-game chat setting. The trace does not establish a native SASL mechanism or document a completed XML stream.

Run `cargo test -p halcyon-chat` to exercise two independent TLS client connections against a real loopback listener. The test checks authentication failure, binding, rosters, presence, escaped message text, server-supplied sender identity, unauthorized delivery and disconnect. Negotiation and stanza behavior follow the implemented subset of [RFC 6120](https://www.rfc-editor.org/rfc/rfc6120.html) and [RFC 6121](https://www.rfc-editor.org/rfc/rfc6121.html).
