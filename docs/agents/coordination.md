# Coordination protocol

Referenced by `docs/master-plan.md`. Read this before launching any agent.

## The one problem this solves

Six agents editing one geometry kernel will produce a merge conflict at best and a subtly wrong
solid at worst. Parallelism is safe only when **file ownership is exclusive** and **contracts are
written down before the work starts**, not after.

## 1. Rules

| Rule | How it is enforced |
|---|---|
| One owner per file, per wave | The ownership table in `master-plan.md` §6. A file listed for agent X is `BLOCKED` to everyone else. |
| Contracts before concurrency | A task that consumes a not-yet-written interface is `BLOCKED`, not "assume and proceed". |
| Quality is not optional | A lot is not done because an agent wrote code. See the state machine in `master-plan.md` §2. |
| Integrate in a controlled order | The orchestrator merges and runs the suite. Agents never merge. |
| Report state honestly | Files changed, tests added, limits, unlocks — with execution status stated. |

## 2. What the orchestrator does and what agents do

**Orchestrator (me):** maintains the master plan, assigns lots, owns the contracts, decides wave
gates, compiles and tests everything, integrates in order, verifies claims. I do not hand out work I
have not verified.

**Agents:** write code and tests inside their owned files, document their limits, report.

## 3. Why agents cannot be trusted with the terminal here

They cannot. `subagent` exposes no tool-permission parameter, so the prohibition in
`SYSTEM_PROMPT.md` is a prompt-level control only, not a sandbox. Consequences:

1. **I compile and run every test myself**, which serialises the bottleneck and caps throughput at
   roughly what one 2-core VM can build.
2. **An agent's code arrives unexecuted.** It is `CODE WRITTEN` until I build it. If it does not
   compile, I either fix it or send it back with the compiler output.
3. Therefore: **maximise agent parallelism on writing, minimise it on verification.** Two to three
   agents concurrently is the realistic ceiling on this machine, not six. Six agents on two cores
   would each spend most of their time waiting on a cargo lock.

This is an environment constraint, not a plan preference, and it is the main reason the wave table
opens slowly.

## 4. Launching an agent

Every agent instruction must contain, in this order:

1. The path to `docs/agents/SYSTEM_PROMPT.md` and the instruction to follow it verbatim.
2. Its task IDs from the master plan, quoted exactly.
3. The list of files it owns exclusively, and the list it must not touch.
4. The interfaces it consumes, with the exact signatures as they exist in the repository today
   (never a remembered signature — the agent must read it).
5. The acceptance criteria, including "tests must exist but you may not run them".
6. The report format.

Never give an agent a task described only in prose. A task ID in the plan is the contract.

## 5. When agents disagree with the plan

They will, and that is the point. An agent that finds the plan is wrong should say so with a
concrete counter-example rather than working around it silently. The master plan is a document, not
a scripture.
