JavaScript winning the web isn't controversial — default for front-ends, huge ecosystem, fast iteration, the thing most web teams already know. The interesting frontier is the **critical path**: product work where performance, latency, memory pressure, and runtime predictability dominate.

## "Is Rust ready for web development?" is a useless, dumb question

Ready for *what*? "Web development" could mean landing pages, proxies, or `WASM` — each optimizing for something different. For product-heavy UI work, no. When the constraints change, yes. The web stack is far more than the browser — frontend, edge, and a lot of layers underneath.

Rust wins when the web becomes **systems programming**: predictable, low-latency, memory-efficient, safe concurrency, fewer runtime surprises. It's not *just* fast — explicit errors, strong types, `WASM`-ready, ownership and allocation control, memory safety without a garbage collector. From the outside it looks like web dev; from the inside it behaves like a systems language. Not for everything.

## One pattern, real examples

Rust entering the parts that need stronger guarantees:

- **Cloudflare** built `Pingora`, an HTTP proxy — 1T+ requests/day on a third of the CPU and memory.
- **Discord** rewrote Read States in Rust: GC caused latency spikes, and P99 mattered more than simplicity. A targeted hot-path swap, not an ideological rewrite.
- **Shopify Functions** compile to `WASM` — Rust for portable business logic, fast.

## The stack, and where to reach for it

Frameworks `Axum` + `Actix`; foundations `Hyper` + `Tower`; data `SQLx`, `Diesel`, `SeaORM`; runtime `Tokio`; observability `OpenTelemetry`.

The `Tokio` mental model is close to Node: an `async fn` creates a lazy future, and the runtime — executor, scheduler, reactor, timers, async I/O, task spawning — runs it when ready. It's **not a thread**; it's machinery to let many futures make progress. Production engineering starts when many handlers share one runtime. `SQLx` keeps SQL explicit and moves failures earlier — verifying Rust types against the schema at compile time. Not an ORM, but with compile-time guarantees.

Rust wins at backend, auth, billing, realtime, proxies, `WASM`/tooling, queues — **realtime is the biggest win**. It loses when speed of change matters most: landing pages, blogs, classic CMS, three-day MVPs, frontend-heavy apps. It has a cost — learning curve, compile time, async complexity — so pay it when the leverage is worth it. Go is simpler but not the correctness win; Java/Kotlin for tooling and big teams; JS/TS for speed. Keep TypeScript for the frontend, reach for Rust at the constraints. **Hybrid wins.** Rust doesn't need to win the web — it should be on the critical path.
