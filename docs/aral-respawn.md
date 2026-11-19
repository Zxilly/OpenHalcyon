# ARAL respawn timer

Seven played ARAL captures contain 103 server-to-client 0x0433 records. The
actor is always a hero declared in the match opening. Fourteen records
identify the recording seat, 48 an ally, and 41 an opponent. The body
starts with the big-endian actor ID and a big-endian float duration in
seconds. The six remaining bytes in every captured body are zero.

Seventy-four timers arrived within 300 ms of a recorded death for the same
actor. The other 29 followed later. The samples range from about 0.57 to
50 seconds. In all 63 cases where the next lifecycle marker for the actor
was 0x0432, its arrival was within 0.18 seconds of the timer's prediction.
This supports a remaining-duration value and establishes that it can be
refreshed during the same death. The captures do not establish a regular
refresh cadence or a fresh-connection sequence for recovering a dead actor.

For a new ARAL death, the server sends the actual number of simulated
ticks until that hero respawns, divided by the tick rate. An opposing team
receives a timer only when the death was eligible for its combat stream.
An ally already known to the receiving side can receive a timer even
when the killer is hidden. The timer carries no source or coordinates.
An authorized live observer receives the same countdown through the
observer view. The server sends no periodic refreshes and does not
synthesize dead-actor registration on reconnect.
