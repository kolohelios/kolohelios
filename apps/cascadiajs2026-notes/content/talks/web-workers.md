Keep the Main Thread Free with Web Workers

UX responsiveness

- `github.com/cyatteau`
- `cascadiajs-2026-web-workers`

**Feel the freeze, prove the blocker, move the work, add guardrails, use the pattern.**

**Jank is a workload problem.**

## Thread

- Trigger the stutter
- Hunt for the blocker (dev tools, code review)
- Relocate the blocker to move it out of the UI
- Establish the protocol for handling the blocker
- Add progress / debounce / cancellation
- Decide where the process belongs

## Trigger / Hunt

Heartbeat freezes — click causes expensive task (sin/cos maths on state data).

CPU-heavy process in any case.

Synthetic UI.

Do a performance recording — see what's going on.

JS work blocking main thread.

**Mental model: the main thread is the front desk experience** — clicks, paints, UI updates, and **HEAVY JS** blocks the line at the front desk.

A worker is **NOT a second UI** — main thread still owns the user experience.

Worker thread is for plain data calculation.

```
main thread -> requests -> worker -> returns result
```

## Relocate

Move the work to a different thread.

Main thread blocked — offload to worker, and **no longer UI-blocked!**

## Establish / Add Guardrails

Make worker messages boring — **boring is good when two threads talk.**

- e.g. `type`, `requestId`, `payload`

Make cancellable — request becomes stale.

Preemptive if there's a new message — take the latest message as the `requestId`.

**Guardrails** — make this even more usable:

- debouncing — wait
- progress — inform
- cancel — stop state

Fast filters — worker filter flow. Can the worker do the work? Yes — and can it only do *enough* work.

## Decide

Workers are more expensive in complexity.

Too heavy for UI work.
