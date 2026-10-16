# The agent loop: never block, always ask in writing

**This file is binding for every agent, alongside `SYSTEM_PROMPT.md`.**

The orchestrator is one process with one context. If agents stop to ask questions, everything stops.
So they do not stop. They write, and they keep going.

---

## 1. The question file — the only way to escalate

When a task is ambiguous, contradictory, under-specified, or blocked by another module, **do not
stop and do not guess silently**. Write a file into `docs/agents/questions/` and continue.

**Path:** `docs/agents/questions/Q-<area>-<short-slug>.md` — e.g. `Q-sketch-constraint-names.md`.
Use the area prefix of your task (`Q-ker-`, `Q-skt-`, `Q-mod-`, `Q-asm-`, `Q-drw-`, `Q-dat-`, …).
One question per file. Never edit or delete another agent's question.

**Template — copy it exactly, all fields required:**

```markdown
# Q-<area>-<slug>

- **Agent session:** <the subagent session id, or the task id if you do not know it>
- **Task:** <task id from docs/plan/, e.g. SKT-004>
- **Owned files:** <the files you were allowed to touch>
- **Status:** ASKED | ASSUMED-AND-CONTINUING | BLOCKED

## What is ambiguous

<two or three sentences. Name the exact symbol, file and line.>

## The options I can see

1. <option A, with its consequence>
2. <option B, with its consequence>

## What I am doing meanwhile

<REQUIRED. Which option you took, and why it is the least damaging if it is wrong.
An agent that asks a question and then does nothing is a stopped agent.>

## What would change if the answer is the other option

<the work that would have to be redone, stated concretely>
```

## 2. The orchestrator's side

The orchestrator reads `docs/agents/questions/` whenever it next wakes, answers every new question
into `docs/agents/answers/A-<same-slug>.md`, and — if the answer invalidates work already done —
launches a **fixer agent** for that specific correction. It does not fix it by hand while agents are
runnable.

**Answers are short and decisive:** the chosen option, one sentence of reasoning, and the files the
asker must revisit. No essay.

## 3. Never stop, never guess silently

| Situation | Wrong | Right |
|---|---|---|
| The interface you need is not public | Stop and wait | Ask, take the least-damaging option, continue, report it |
| Two documents contradict each other | Pick one silently | Ask, follow the one in your own index, continue |
| A test's expectation looks wrong | Change it to match the code | Ask, leave it failing, say why in your report |
| You cannot compile | Claim it compiles | Say `NOT EXECUTED` and continue with the next task |

**The single worst outcome is an agent that stops with work half-written and the reason in its
context**, because that context dies with the session. A written question survives.

## 4. Verification is a separate agent, not the author

When an agent reports residual problems or a doubt about its own work, the orchestrator does **not**
verify it by hand. It launches a **verifier** agent whose only job is to read the delivered files and
the claims made about them, and to say which claims are unsupported.

- The verifier **may not edit** the files it reviews.
- Its output is a verdict per claim: `SUPPORTED` / `UNSUPPORTED` / `UNCLEAR`, with the evidence.
- If anything is `UNSUPPORTED`, the orchestrator launches a **fixer** agent scoped to exactly those
  items. The fixer and the verifier are never the same model in the same round.
- A fixer that cannot resolve an item writes a question file and continues with the next item.

**Why separate agents:** the author of a mistake is the worst judge of it, and the orchestrator's
context is the scarcest resource in the system. Delegating the judgement does not remove it — it
makes it parallel, and it makes it written down.

## 5. What the orchestrator still does by hand, and why

Only these, and nothing that can be delegated:

1. **Read the CI verdict** and decide which red job matters next.
2. **Own the shared files** nobody else may touch: module declarations (`lib.rs`), crate
   registration (`Cargo.toml`, `xtask/src/layers.rs`), and `PROGRESS.md`.
3. **Commit and push.** Agents have no terminal, so their work is not durable until this happens.
4. **Answer questions** — deciding is the one thing that cannot be delegated to the agents being
   decided about.
5. **Refuse a delivery** when the verifier says it is unsupported and the fixer says it is done twice.

Everything else — writing, fixing, testing, reviewing — goes to an agent.

---

## 6. Model policy

**The default model for every agent is `opencode-go/muse-spark-1.3-contributor`** ($0.10 in / $0.20
out per million tokens — the cheapest non-free model available, chosen by the owner).

The free models (`cat-jam-free`, `longcat-2.5-preview-free`) are **rate-limited**, and the limit is
what caps the whole programme at roughly seven concurrent agents. Measured on 2026-10-16:
`longcat-2.5-preview-free` died on `Endpoint is unavailable` in **three of five** launches, always
without writing a single file, so its failures are silent and cost a whole context each time.

`muse-spark-1.3-contributor` has variants `minimal | low | medium | high | xhigh`. Use `low` or
`medium` for mechanical work with a well-specified contract, `high`/`xhigh` for geometry reasoning
(NURBS, topology, boolean robustness) where a wrong answer is expensive.

**Never switch a task to a different model without recording it here**, and never spend the owner's
credits on a model they did not choose. A model substitution is a cost decision and belongs to them.
