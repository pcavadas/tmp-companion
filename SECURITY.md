# Security Policy

## Reporting a vulnerability

Please report security vulnerabilities **privately** — do not open a public issue.

Use GitHub's [private vulnerability reporting](https://github.com/pcavadas/tmp-companion/security/advisories/new) (the repo's **Security → Report a vulnerability**) to open a confidential advisory that only the maintainer can see.

## Scope

TMP Companion is an owner-side interoperability tool: a desktop app (macOS, with alpha Linux and unsigned Windows builds) that talks to a Fender Tone Master Pro **you own** over USB.
It has **no server, network service, or multi-user surface** — it runs locally and acts only on your own device and your own preset files.
It makes two kinds of outbound request: on macOS it checks this project's GitHub Releases for an update, and **Report a bug**, when you choose to send, uploads your description and a support bundle (recent logs, captured device settings, app info, and a preset only if you pick one) to the maintainer's report endpoint.

The areas most relevant to a security report are where untrusted input is parsed: the USB/HID handling (`src-tauri/src/hid.rs`), the wire codec (`proto.rs`), and the `.preset` file parsing (`backup.rs`, `library.rs`) — anywhere a malformed device response or a crafted `.preset` file could cause unsafe behavior.

## What to expect

This is a solo-maintained project, so responses are best-effort and there is no bug-bounty program.
Valid reports will be addressed and, if you wish, credited in the advisory / release notes.
