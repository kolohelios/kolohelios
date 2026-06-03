How do we get safety from agents? **Put a human in the loop.**

Agents aren't just answering questions anymore.

## Pattern 1: Interrupt

- Mid-task pause — agent asks, human answers, agent continues.
- Low stakes, sync, one function call.

## Pattern 2: Token Vault

- Agent hits a wall, you auth it, agent succeeds on retry.

**DEMO:** both patterns in one flow.

Give out a short-lived token that is scoped only to what is needed.

## Pattern 3: CIBA

- Client Initiated Backchannel Auth.
- Industry is already doing this.

**Match the pattern to the stakes** — the higher the stakes, the more security we need.
