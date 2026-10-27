# Isolated local development identities

Start an explicit private development world:

```sh
arkline-server --local-development ./local-world --bind 127.0.0.1:7777
```

This mode keeps the existing `Connect` registration and resume-token authentication. A new pilot receives a random token; reconnecting that pilot requires its saved token. Names alone cannot reclaim an existing identity. Ownership, corporation permissions, boarding, remote-control grants, mining and economic validation are unchanged. Development identities are not official accounts and do not grant access to an official server.

The server stores the world in `DIR/world.sqlite` and marks its SQLite identity realm. Ordinary server startup refuses that development database; development startup refuses an existing unmarked or ordinary database. Older unmarked worlds remain accepted through the ordinary `--db` path. Do not delete realm markers or copy credentials between worlds. Back up the complete database consistently; the marker is part of it.

`--local-development` cannot be combined with `--db`. Every requested TCP/WebSocket listener must use a numeric loopback address; wildcard, public, private-network and DNS bind addresses fail before creating a world. The default TCP address is loopback. A WebSocket listener additionally requires an explicit local browser origin, for example:

```sh
arkline-server --local-development ./local-world --bind 127.0.0.1:7777 \
  --ws-bind 127.0.0.1:7778 --ws-origin http://localhost:8080
```

Only loopback/localhost HTTP(S) origins are accepted in this mode; the ordinary public-site origin list is not inherited. Native clients without an Origin header retain their existing transport behavior. Loopback binding does not make a user-created reverse proxy safe: do not publish or tunnel this mode to other machines.

On Unix, the data directory is private (0700). Direct directory/file symlinks and hardlinked database sidecars are rejected to avoid aliasing another world. This is isolation from accidental reuse, not protection against a hostile process already controlling the same OS account. Local tokens remain credentials and should be stored in the client's protected credential storage, never printed in logs or committed.

For editor projects, use a separate `.arkline-editor` directory and a stable credential scope tied to the canonical project path. An ephemeral listener port may change on restart; the world and credential scope must remain stable.

The server library exposes `open_local_development` and `is_local_development` for transport/configuration integration. Its normal `open` behavior remains legacy authentication. Official identity configuration must reject the development realm; local mode is not a global disable-auth setting.

Run the real local gameplay acceptance journey:

```sh
cargo build -p arkline-server -p arkline-verifier
python3 tools/mining-loop-acceptance.py --local-development --receipt /tmp/local-mining-receipt.json
```

That journey creates an isolated temporary world, exercises manual mining/storage plus an autonomous multi-trip job, reconnects and restarts with the same credentials, and verifies the economic export. It does not access a deployed server or print resume tokens.
