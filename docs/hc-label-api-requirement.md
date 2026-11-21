## What fails today

Setting a pane label is impossible without the legacy control plane. Measured on `kooshapari-desk`, 2026-11-21T10:15Z:

```
herdr pane rename wJ:p5G "srvc map (chicken)"
  -> {"id":"cli:pane:rename","error":{"code":"server_not_running",
      "message":"no herdr server is running at C:\\Users\\koosh\\AppData\\Roaming\\herdr\\herdr.sock;
                 run `herdr` to start or attach it"}}

herdr pane current
  -> same server_not_running
herdr pane list
  -> same server_not_running
dir /s /b C:\Users\koosh\AppData\Roaming\herdr\*.sock
  -> nothing (no socket file exists at all)

herdr-core pane current
  -> {"error":{"kind":"usage","message":"unrecognized subcommand 'pane' tip: a similar
      subcommand exists: 'update'"}}
herdr-core status
  -> {"error":{"kind":"usage","message":"unrecognized subcommand 'status'"}}
```

So the legacy `herdr` CLI has the pane verbs but no server, and `herdr-core` (hc) has no pane or status surface at all. The label capability therefore has no native equivalent on hc core.

## Why it matters

The HC app shows no labels, so the operator cannot tell chats apart. Every pane's visible name is a slice of the session's first prompt. For the session that filed this, the title reads:

```
Please Map all Deployed Services on Cloudflare, Github (pages, o…
```

which is a 60-character truncation of a request made 12 days earlier. Two chats in the same lane are indistinguishable on screen.

## Requirement

A native label API on hc core, usable without the legacy socket:

1. **Set** a display label for a pane, keyed by pane id: `hc pane label <pane-id> "<text>"`.
2. **Set** a display label for a session, keyed by session id or short name, so the label survives a resume: `hc session label <session> "<text>"`.
3. **Read** the current labels, machine-readable: `hc pane labels` returning `{pane_id, label, session_id}` rows as JSON.
4. **Optional key/value metadata**, matching what the legacy CLI accepted (`label <t> k=v...`), so a label can carry lane and host separately rather than one opaque string.
5. **Idempotent and non-destructive**: setting a label must not restart, refocus, or resize the pane, and must not touch the session store.

## Acceptance

```
hc pane label wJ:p5G "srvc map (chicken)"
hc pane labels            # includes {"pane_id":"wJ:p5G","label":"srvc map (chicken)"}
```

and the HC app shows `srvc map (chicken)` for that pane instead of the truncated first prompt. Re-running the set is a no-op. A resumed session keeps its label.

## Constraint

The setter must not require the old daemon at `%APPDATA%\herdr\herdr.sock`. If hc core already has an IPC surface, this belongs on it; if the only working path is the legacy socket, that reproduces the deprecation problem instead of fixing it.

Measured and filed from session `session_chicken_1794285185286_89eb7c5f908001ad` (`chicken`), running `jcode v1.0.1` on `kooshapari-desk`. Pane id `wJ:p5G`, held by `herdr.exe 0.9.3`.
