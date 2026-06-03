An intervention: `React` just turned 13, and how we work has changed. To bundle, or not to bundle?

## Why we bundle

We bundle with `webpack` and `rollup` — but also with inlining, data URLs, and sprites. The *why* is historical: `TCP` was slow, and `HTTP/1.1` told us we had to. Caching hated bundling, so **code splitting** was invented to balance bundling against caching.

## The protocols moved on

Then `HTTP/2` (`SPDY`) fixed the `HTTP/1.1` problem — but `TCP` is still the problem. `HTTP/3` over `QUIC` fixed a lot of hard problems. But did we actually *fix* this?

Look at request granularity. Under `HTTP/1.1`, bundling is genuinely faster — the more you split into smaller pieces, the worse it looks, because the overhead of splitting goes up. `HTTP/3` is the opposite: the more you split, the **better** it looks, and delivery stays consistent. **`HTTP/3` plus caching is a love story.**

## Takeaways, by audience

- **Average developers** — optimize for *caching* and stop stressing about the network.
- **Framework and bundler maintainers** — is the default still the right answer?
- **Cloudflare** — can you get us more data? Follow up on the 2020 article on `HTTP/3` performance.
- **Everyone else** — anyone have cool data?
