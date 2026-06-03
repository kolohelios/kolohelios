How do we get safety from agents? Put a human in the loop — because agents aren't just answering questions anymore, they're taking actions. Three patterns, ordered by stakes.

## Pattern 1: Interrupt

A mid-task pause — the agent asks, the human answers, the agent continues. Low stakes, synchronous, **one function call**.

## Pattern 2: Token Vault

The agent hits a wall, you authorize it, and it succeeds on retry. The demo runs both patterns in one flow: hand out a **short-lived token scoped ONLY to what's needed**.

## Pattern 3: CIBA

`CIBA` — Client-Initiated Backchannel Authentication. The industry is **already doing this**.

## Match the pattern to the stakes

That's the rule of thumb. **The higher the stakes, the more security you want.**
