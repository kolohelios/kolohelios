**JavaScript winning the web is not controversial** — it's the default choice for front-ends, has a huge ecosystem, fast iteration, and most web teams already know it.

Web only as product work, performance, latency, memory pressure, runtime predictability => **this is the critical path.**

## Is Rust Ready for Web Development?

This question is useless; it's a dumb question.

**What does web development even mean?**

- Landing pages, proxies, `WASM`, and so on.

**Ready for what?**

- Product-heavy UI work.
- When constraints change? Yes.

The web stack is much more than the browser — frontend, edge, lots of other stuff.

Different layers optimize for different things.

## Where Rust Wins

**Rust wins when the web becomes system programming** -> operating critical systems.

- Predictable, low latency, memory efficiency, safe concurrency, fewer runtime surprises.

**Rust is not JUST fast** — explicit errors, strong types, safe concurrency, `WASM`-ready, ownership and allocation control, memory safety without garbage collection.

**Rust is NOT for everything.**

- From the outside, looks like web development.
- From the inside, behaves as a system language.

## Real-World Patterns

`Cloudflare` built `Pingora` — HTTP proxy.

- 1T+ requests/day with 1/3 CPU and memory.
- When the web becomes infrastructure, the choice becomes easy.

`Discord` — Read states rewritten in Rust.

- **GARBAGE COLLECTION** caused latency spikes; P99 latency mattered more than simplicity.
- Critical path, GC spikes, hot path, targeted replacement.
- Not an ideological rewrite, a very practical choice.

`Shopify` Functions -> `WASM`.

- Rust for portable business logic, fast.

All three examples — recognizable pattern.

**Rust is entering the parts that need stronger guarantees.**

## Rust Web Stack

- Web frameworks: `Axum` + `Actix`
- Foundations: `Hyper` + `Tower`
- Database layer: `SQLx`, `Diesel`, `SeaORM`
- Async runtime: `Tokio`
- Observability: `OpenTelemetry`

## Tokio and Futures — Mental Model

SIMILAR to `NodeJS` — `async fn` creates a future — lazy work.

`Tokio` runtime — executor / scheduler / reactor / timers / async I/O / task spawning.

- When ready, it runs.
- Machinery that lets many futures make progress efficiently.
- It is **NOT a thread.**

`Axum` flow: production engineering starts when many handlers share the same runtime.

## Database Access

`SQLx` keeps SQL **EXPLICIT** and moves more failures earlier.

- Compile time — verifies the Rust types against the schema.
- NOT an ORM, but has compile-time guarantees.

## Where Does Rust Win?

- Backend, auth, billing, realtime, proxies, `WASM`/tooling, queues.
- **Realtime is the biggest win.**

## Where Does Rust NOT Win?

- If speed of change is most important.
- Landing pages, blogs, classic CMS websites.
- Three-day MVPs, frontend-heavy apps.

## Rust Has a Cost!

Adoption costs:

- Learning curve
- Compile time
- Async complexity

**Pay the Rust cost when the leverage is worth it.**

## Different Tools, Different Wins

**Rust wins WHEN correctness, latency, safety, and predictability matter most.**

- `Go` is a more simple choice, but not the correctness win.
- `Java`/`Kotlin` for tooling / maturity / big teams.
- `JS`/`TS` for the fastest speed of development.

Keep `TypeScript` for the frontend — UI, fast iteration; use Rust for constraints.

**Hybrid wins.**

Reusable business logic — `WASM`, back-end, CLI.

**Rust does not need to win the web, but it should be in the critical path.**
