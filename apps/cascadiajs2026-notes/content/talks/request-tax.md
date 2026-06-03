An intervention.

React turned 13 years old — and how we've worked has changed.

## To Bundle or Not to Bundle?

**How** we bundle:

- `webpack`, `rollup`
- BUT ALSO: inlining, data URLs, sprites

**Why** do we bundle?

TCP was slow; HTTP/1.1 told us we had to bundle.

Caching hated it.

Code splitting invented to find a balance between bundling and caching.

## HTTP Evolution

`HTTP/2` — `SPDY` fixed the HTTP/1.1 problem.

But TCP is still the problem.

`HTTP/3` over `QUIC` — fixed a lot of hard problems.

But did we actually **FIX** this?

## Request Granularity

HTTP/1.1 is actually faster when bundling.

The more we split and make smaller, the better HTTP/3 looks.

- **HTTP/1.1** — overhead of splitting goes up
- **HTTP/3** — more consistent at delivery

```
HTTP/3 + caching = ❤️
```

## Prompts

- **Average developers:** OPTIMIZE for CACHING, stop stressing about the network.
- **Framework / bundler maintainers:** is the default still the answer?
- **Cloudflare:** can you get us more data? Can you follow up on the 2020 article on HTTP/3 performance?
- Anyone else have cool data?
