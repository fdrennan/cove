# Cove as a remote MCP server

Claude.ai, Claude Desktop and Claude Code connect to Cove as a custom
connector and work in the person's Cove: its projects, files, git changes,
environments, commands and deployments, as that person (operator,
2026-11-26: "make Cove a workspace that Claude can operate within, rather
than simply a website Claude can read").

## Endpoint

    https://cove.ndexr.io/mcp

MCP over Streamable HTTP: one JSON-RPC message (or a batch) per POST, the
answer in the response as JSON. No server push, so GET /mcp is 405. Protocol
2025-06-18; a client asking 2025-03-26 or 2024-11-05 is answered in that.

## Sign-in: OAuth 2.1

| Path | What |
|---|---|
| `/.well-known/oauth-protected-resource` | RFC 9728: the resource and its authorization server |
| `/.well-known/oauth-authorization-server` | RFC 8414: the endpoints below |
| `POST /mcp/register` | RFC 7591 dynamic registration (Claude registers itself); https redirects, or http to loopback |
| `GET /mcp/authorize` | the consent page, behind the ndexr sign-in, as Cove itself |
| `POST /mcp/token` | `authorization_code` (PKCE S256 required) and `refresh_token` |
| `POST /mcp` | the MCP endpoint; `Authorization: Bearer <token>` only |

/mcp without a valid token is 401 with `WWW-Authenticate: Bearer
resource_metadata=".../.well-known/oauth-protected-resource"`, which is how a
client finds the rest.

Tokens are random (256 bits), stored root-only in
`$COVE_STATE_DIR/mcp.json` (`/var/lib/cove/mcp.json`): access tokens live 30
days, refresh tokens 180 and rotate on every use. Codes live 10 minutes and
are spent on their first try, right or wrong.

## Permissions

Chosen by the person on the consent page; reading is always granted, the
rest off until ticked:

| Scope | Tools |
|---|---|
| `read` | list_projects, get_project, list_files, read_file, search_code, git_diff, get_environment |
| `write` | apply_patch |
| `run` | run_command |
| `deploy` | deploy_project |

A connector without a scope does not see its tools (tools/list), and a call
to one is refused with what to do. Every tool beyond `read` carries
`destructiveHint`, so Claude asks the person before each call. To change a
connector's scopes, remove it in Claude and connect again.

## Isolation

Every tool runs as the person who allowed it, through `cove multiuser`
(server/multiuser.rs `McpHost`): their projects only (`workable_domains`:
their own and those shared with them), each path checked to stay inside its
project, and each query answered as Cove's page is - by the multi-person
server where only it can (publishing, git as the platform account), else by
the person's own Cove session, which runs as their Unix account. MCP opens
no other way into the machine.

## Audit

The multi-person server's audit log (`/var/lib/cove/audit.log`) records each
grant (`mcp-authorize` with its scopes), each tool call (`mcp-tool <name>
done|refused`), each patch, command (its first 200 characters) and deploy,
and each revoke.

## Tools

| Tool | Cove query | |
|---|---|---|
| `list_projects` | the person's domains | |
| `get_project` | AppStatus, ListDir, ReadFile of CONTEXT.md / CLAUDE.md / README.md | |
| `list_files` | ListDir | |
| `read_file` | ReadFile, by line ranges | |
| `search_code` | Grep (Find in files) | |
| `git_diff` | GitStatus, or GitDiff for one file (the Git window) | |
| `get_environment` | Record (.cove/record.json) and AppStatus | |
| `apply_patch` | GitPatch check, then apply | write |
| `run_command` | RunCommand: a login shell in the project folder, as the person, at most 600 s | run |
| `deploy_project` | AppAction up, then PublishDomain | deploy |

## In Cove

Cove menu › Use Cove from Claude… shows the address, the steps for
Claude.ai / Desktop (Customize › Connectors › Add custom connector) and
Claude Code (`claude mcp add --transport http cove https://cove.ndexr.io/mcp`),
and the person's connectors with what each may do and Revoke
(Query::McpGrants / McpRevoke).

## Code

- `app/src/server/mcp.rs`: protocol, OAuth, store and tools (tested in full:
  register → consent → PKCE → token → initialize → tools → refresh → revoke).
- `app/src/server/multiuser.rs`: routing, `McpHost`, `answer_on_behalf`.
- `app/src/host/runner.rs` `run_command`; `app/src/ui/connect.rs`.
- `nginx.conf`: the open paths, outside the sign-in.

## Verification

2026-11-26:

- `cargo test`: 363 passed. `mcp::tests::a_connector_registers_is_allowed_and_calls_tools`
  runs the whole lifecycle - 401 with resource metadata, discovery,
  registration (plain http elsewhere refused), consent, a wrong PKCE verifier
  refused and the code spent, the right one exchanged, initialize,
  notifications 202, tools/list limited to the grant, tools/call, refresh
  rotation (old refresh refused), revoke (only by its owner) then 401.
  Path escapes and other people's projects are refused; run_command reports
  exit code, both streams and its timeout.
- The live server (release 20261126T090036Z, `cove multiuser` restarted
  09:00:16 UTC), asked on 127.0.0.1:8796: both discovery documents, 401 with
  `WWW-Authenticate` on /mcp, registration 201, the token endpoint's
  `invalid_grant`, and /mcp/authorize unsigned → 302 to the ndexr sign-in.
- Through nginx (https://cove.ndexr.io/...): 302 to the sign-in until the
  vhost's new open locations are synced and nginx reloaded (referral #831 to
  exec.ndexr.io). Not yet added to Claude as a custom connector: that is the
  next check, once #831 is done.
