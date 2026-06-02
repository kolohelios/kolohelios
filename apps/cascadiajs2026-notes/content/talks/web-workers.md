UX responsiveness is a workload problem. **Jank is a workload problem.** When
the main thread is busy, the UI freezes — so move the heavy work off it.

## The mental model

The main thread is the **front desk**: clicks, paints, UI updates — and any
heavy JS blocks the line. A worker is **not** a second UI; the main thread
still owns the user experience. A worker thread is for plain data calculation:
the main thread requests work, the worker returns a result.

## THREAD — the method

1. **Trigger** the stutter — reproduce the freeze.
2. **Hunt** for the blocker — dev tools, code review, performance recording.
3. **Relocate** the blocker — move it out of the UI thread.
4. **Establish** the protocol for handing work to/from the worker.
5. **Add** guardrails — progress, debounce, cancellation.
6. **Decide** where the work belongs.

## Trigger / hunt

The demo: a heartbeat UI that freezes when a click kicks off an expensive task
(sin/cos math over state data). Do a performance recording and you can see the
JS work blocking the main thread.

## Relocate

Offload the CPU-heavy work to a worker and the main thread is no longer
UI-blocked.

## Establish / guardrails

- **Make worker messages boring.** Boring is good when two threads talk — e.g.
  `{ type, requestId, payload }`.
- **Make requests cancellable.** A request can become stale; preempt when a new
  message arrives by treating the latest message's `requestId` as the live one.
- Add the usual usability guardrails: **debounce** (wait), **progress**
  (inform), **cancel** (stop). For fast filters, ask whether the worker *can*
  do the work — and whether it can do *only enough* of it.

## Decide

Workers add complexity, so they're worth it for genuinely heavy computation —
but **too heavy-handed for ordinary UI work.** Use the pattern where it earns
its keep.
