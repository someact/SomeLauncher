# somelauncherd protocol

The UI and the CLI talk to `somelauncherd` over a unix socket, `$XDG_RUNTIME_DIR/somelauncher.sock`. `SOMELAUNCHER_SOCKET` overrides the path, which tests use. The socket is mode 0600.
Each message is one JSON object on one line. The types live in `core/crates/proto`.

## Messages

Client → daemon, a request:
```json
{"id": 1, "method": "settings.get", "params": {"key": "gameDirs"}}
```
`params` may be left out. `id` is chosen by the client and comes back in the reply.

Daemon → client, a reply (exactly one per request, in request order):
```json
{"id": 1, "result": ["/games"]}
{"id": 1, "error": {"code": "bad_params", "message": "missing string `key`"}}
```
A line that is not a request gets an error reply with `id` 0.

Daemon → client, an event (no `id`):
```json
{"event": "settings.changed", "data": {"key": "gameDirs", "value": ["/games"]}}
```
Every client gets every event. Replies and events share one ordered stream, so a client tells them apart by the `id` field.

On connect the daemon first sends `hello`:
```json
{"event": "hello", "data": {"protocol": 1, "version": "0.1.0"}}
```
`protocol` is bumped on any incompatible change. A client that does not know the number must not send anything else.

## Error codes
| code | meaning |
| :--- | :--- |
| `bad_request` | the line is not a request |
| `unknown_method` | no such method |
| `bad_params` | a param is missing or has the wrong type |
| `internal` | the daemon failed (the message says why; the log has more) |

## Methods
| method | params | result |
| :--- | :--- | :--- |
| `ping` | — | `{"pong": true}` |
| `status` | — | `{version, protocol, pid, db}` |
| `settings.get` | `{key?}` | the value (`null` when unset), or every setting as an object without `key` |
| `settings.set` | `{key, value}` | `null`; a `null` value removes the key. Emits `settings.changed` |
| `shutdown` | — | `null`, then the daemon exits |

## Events
| event | data |
| :--- | :--- |
| `hello` | `{protocol, version}`, sent only on connect |
| `settings.changed` | `{key, value}` |

## Settings keys
Daemon settings live in `library.db` and are changed only through `settings.set`. UI preferences (looks, motion) are not stored here: they live in `~/.config/somelauncher/config.json`, which the UI owns.
