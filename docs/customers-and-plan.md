# Who Cove is for, and the plan to get them in (2026-11-22)

The operator's notes of 2026-11-22, consolidated. They're spoken, in
`exec.ndexr.io/supporting_docs/go-note-2026-11-22-*.md`. The money figures
are measured in `fdrennan/billing.ndexr.io/MANAGED-COVE-BREAKEVEN.md`, and
live in billing.ndexr.io's admin accounting section. Prices and allowances
are the operator's call: this plan proposes, it changes no one's terms.

## The goal

"We have access to AWS, we have Stripe set up, we have Cove, and we want to get
as many people in and paying for it as possible with as little money out of
their pockets." Price managed Cove at **break-even + 7%** on what AWS charges,
the 7% kept after Stripe's fee. Beat what Posit offers (Posit Cloud, Connect)
on features; don't engage them, and build nothing that depends on them.

## Who we're building for

| who | how they get Cove | who pays | what they need from us |
|---|---|---|---|
| **People who aren't programmers** and want to build websites and do what ndexr does, themselves | Start on ndexr's servers: a cell, or a managed server | Them, through Stripe | Sign up, pay, start - no setup at all |
| **Regeneron**, internally | Run it themselves: fully open source, so they have full control | Nobody pays ndexr | No licence, no ndexr account, no payment. A self-hosted install that's complete on its own. Their own AI provider (Amazon Bedrock) |
| **Companies and developers** (LinkedIn, say) who want their own server, not the work of running it | A dedicated server ndexr launches and runs for them | Them, through Stripe | Pay, and the server is theirs: nothing to install |

## Three ways to run Cove

1. **On their own computer or server**:
   - No licence (Cove is MIT-licensed) and no ndexr account needed.
   - The stack comes over CernVM-FS, or offline as a local copy (airgap).
   - This is the route for Regeneron, and for anyone who'd rather run it themselves.
2. **On ndexr's shared servers** (cells today):
   - The cheapest: about $5-7 a person at break-even + 7%, if two numbers hold up when measured: memory per active session, and the share of people online at once.
3. **On a dedicated managed server**:
   - $14.67 a month if it stops when idle, $36.81 if always on (`t4g.medium`, Elastic IP, 50 GB disk).
   - Extra disk is $0.88 a month per 10 GB.
   - Domains go on the same invoice (.com $17.63 a year).

## AI providers: whose model the agent runs on

Cove's agent is Claude Code, run by `ndexr-claude-run`. It knows two credential sources today:
- **`client`**: the person's own Claude subscription or key.
- **`house`**: ndexr's key, metered in commands.

A third source is needed, and the house one changes from commands to dollars. Three in all:

| source | who it suits | how | status |
|---|---|---|---|
| **Their own** Claude subscription or API key | Anyone who has one; free accounts | `--claude-source client` | Built |
| **Through ndexr**: Cove credit | People who would rather not deal with Anthropic | Prepaid in dollars at billing.ndexr.io ($10, $25, $50, $100 or any amount). Credit = what was paid, less Stripe's fee, divided by 1.07. ndexr's gateway (the cell agent's house-key proxy) charges every model call at Anthropic's list price: a hold before the call, the exact token cost after, keyed on Anthropic's message id. A failed call costs nothing, and the next call is refused when the credit can't cover it. Hours are shown as an estimate, measured from Cove's own runs: Sonnet 5.5 about $8 an active hour, Opus 5.5 about $18 | **Built 2026-11-22, in Stripe test mode** (`pricing$CLAUDE_CREDIT$live`). Subscribers keep their weekly commands; credit is spent after them |
| **Their own Amazon Bedrock** | Regeneron, and any company with AWS terms already in place | Claude Code runs on Bedrock as it is: `CLAUDE_CODE_USE_BEDROCK=1`, `AWS_REGION`, their AWS credentials (or `AWS_BEARER_TOKEN_BEDROCK`), and models by their Bedrock IDs (`anthropic.`-prefixed). The installed CLI (2.1.330) knows these variables | **Not built**: add `--claude-source bedrock` to `ndexr-claude-run`, which passes those through and never stores them in ndexr. Add a Settings › AI provider row in Cove. Confirm with Regeneron which models their account has enabled |

## Every place money moves (managed servers)

1. The person pays ndexr's Stripe: subscription, server, extra disk, domains, AI funds, all on one invoice. Stripe takes 2.9% + $0.30 per charge (measured), so never charge small items on their own.
2. AWS bills ndexr afterwards, for the month used. **A server starts only after its first payment succeeds**, and stops (keeping its disk) when a payment fails, so the money is always ahead of the bill.
3. The admin accounting section of billing.ndexr.io shows it month by month: what people paid, every Stripe fee, Stripe's net, AWS's bill, and what's left. As of 2026-11-22, AWS has been more than Stripe's net every month since July.

## What's built, and what's next, in order

**Built:**
- **pay to start a managed server** (2026-11-22, Stripe test mode, admins only until the operator says sell it: `pricing$MANAGED$live`):
  - billing.ndexr.io `managed.r`: the checkout, a monthly subscription at break-even + 7% ($14.13 for a `t4g.medium` with 50 GB that stops when idle), and the owner's server with its address, token and Start;
  - `ndexr aws managed reconcile` (timer `ndexr-managed-reconcile`, every 2 minutes) follows the subscription: active launches it, a failed payment stops it with the disk kept, cancelled removes it;
  - the server is `https://cove.<name>.ndexr.io`, with the stack, and each project at `<project>.<name>.ndexr.io`;
  - proven end to end on 2026-11-22 against a real instance: launch, certificate, token, idle stop, Start, cancel and teardown. The stop-on-failed-payment branch is unit-tested only. Stripe's hosted page was not paid through in a browser; the subscription was made through the API;
- **the desktop app connects to any Cove server** by address and token: one ndexr manages, or one a company runs, with no ndexr account (the status-bar chip › Connect to a server…);
- the break-even analysis and the accounting section (billing.ndexr.io);
- the projects folder (`~/.cove/projects`);
- the stack;
- cells;
- the `/port/` app view;
- `client` and `house` credentials.

**Next:**
1. **Sell managed servers**: flip `pricing$MANAGED$live` once the operator has paid through a test checkout in a browser. Then cancel from billing, and email the owner when their server stops for a failed payment.
2. **Sell Cove credit**: flip `pricing$CLAUDE_CREDIT$live` once these four are done:
   - ndexr's Anthropic account is funded, with auto-reload on. On 2026-11-22 it was **out of credit**, and every house run failed;
   - Anthropic has confirmed in writing that metered Cove credit is not resale under Commercial Terms D.4(a);
   - tax and refund terms for prepaid credit are decided;
   - one browser payment has been made with Stripe's test card.

   Bedrock is set aside (operator, 2026-11-22: "we wont build on bedrock rn").
3. **Disk that grows**: resize the server's disk and bill the person. Should it grow automatically near full, or only when they press "Add 10 GB"? That's the operator's call.
4. **Domains bought through ndexr**: Stripe, then Route 53 registration, renewing a month ahead.
5. **A complete self-hosted install for Regeneron**:
   - `install.sh --server` on a fresh Ubuntu machine, with no ndexr account;
   - the projects-folder plan's steps 2 and 3;
   - their own nginx and TLS.
6. **Measure the shared model** (memory per active session, people online at once) before selling it.

**Decisions waiting on the operator:**
- Shared or dedicated first.
- A disk cap for paid accounts.
- The dollar allowance for house Claude.
- Whether to delete 145 GiB of superseded backup layers.
