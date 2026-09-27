# Security policy

## Supported versions

Security fixes land on the latest release. Older releases are not patched;
upgrade to the latest tag instead.

| Version | Supported |
| ------- | --------- |
| 0.2.x   | Yes       |
| < 0.2   | No        |

## Reporting a vulnerability

Report vulnerabilities privately through GitHub's
[private vulnerability reporting](https://github.com/Jenish-Shobhit/paneMorph/security/advisories/new)
for this repository. Do not open a public issue, pull request or discussion
for a suspected vulnerability.

A useful report includes:

- the paneMorph version (`./bin/panemorph version`), the herdr version
  (`herdr --version`) and your operating system;
- what an attacker controls and what they gain;
- the smallest set of steps that reproduces it.

Leave out private terminal output, socket paths, home directory paths and
anything else you would not publish. If a log is needed, trim it to the
relevant lines first.

You can expect an acknowledgement within 7 days. Once the issue is confirmed,
a fix is prepared in a private fork and released, and the advisory is
published with credit to the reporter unless you ask otherwise.

## Scope

paneMorph runs as your user, inside herdr, and talks only to herdr's local
Unix socket. It opens no network connections and collects no telemetry. It
reads herdr's `config.toml` and writes only to its plugin state directory:
the undo journal, a queue lock, short-lived worker result files and
`panemorph.log`. `panemorph preview` uses a temporary directory instead.

In scope:

- the code in this repository and the release archives built from it;
- anything that lets pane, tab or space names, running commands or other
  herdr state make paneMorph run a command, write outside its state
  directory, or close or restart a terminal.

Out of scope:

- vulnerabilities in herdr itself; report those to
  [herdr](https://github.com/herdrdev/herdr/security);
- attacks that already require running code as your user.
