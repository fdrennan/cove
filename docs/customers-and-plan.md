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
| **Through ndexr**: they pay us, we pay Anthropic | People who'd rather not deal with Anthropic | ndexr's API key. Prepaid in **dollars**, charged at what the model cost + 7%. A customer command averaged $0.245 (measured), and "100 commands a week" can reach about $106 a month, so the allowance must be in dollars, not commands | Metered in commands today; **move to dollars** |
| **Their own Amazon Bedrock** | Regeneron, and any company with AWS terms already in place | Claude Code runs on Bedrock as it is: `CLAUDE_CODE_USE_BEDROCK=1`, `AWS_REGION`, their AWS credentials (or `AWS_BEARER_TOKEN_BEDROCK`), and models by their Bedrock IDs (`anthropic.`-prefixed). The installed CLI (2.1.330) knows these variables | **Not built**: add `--claude-source bedrock` to `ndexr-claude-run`, which passes those through and never stores them in ndexr. Add a Settings › AI provider row in Cove. Confirm with Regeneron which models their account has enabled |

## Every place money moves (managed servers)

1. The person pays ndexr's Stripe: subscription, server, extra disk, domains, AI funds, all on one invoice. Stripe takes 2.9% + $0.30 per charge (measured), so never charge small items on their own.
2. AWS bills ndexr afterwards, for the month used. **A server starts only after its first payment succeeds**, and stops (keeping its disk) when a payment fails, so the money is always ahead of the bill.
3. The admin accounting section of billing.ndexr.io shows it month by month: what people paid, every Stripe fee, Stripe's net, AWS's bill, and what's left. As of 2026-11-22, AWS has been more than Stripe's net every month since July.

## What's built, and what's next, in order

**Built:**
- the break-even analysis and the accounting section (billing.ndexr.io);
- the projects folder (`~/.cove/projects`);
- the stack;
- cells;
- the `/port/` app view;
- `client` and `house` credentials.

**Next:**
1. **Start by paying**: Stripe checkout first, then the server launches. Stop it on a failed payment. Serves the first and third customers.
2. **AI provider choice**: `--claude-source bedrock`, a Settings row in Cove, and the house allowance in dollars. Serves all three customers.
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
