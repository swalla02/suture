# Learned Interaction Machine

> **Status:** architecture and research notes, plus a **v0 kernel** (Rust, `src/`) that implements the typed IR, `INTERACT`, journaling and replay, and the two minimal experiments of §7 ([`docs/ir-spec.md`](docs/ir-spec.md), [`docs/experiments.md`](docs/experiments.md)). The thesis has since been clarified as **Decision-Native Computing** (§0.1); its operational model is [`docs/dnc.md`](docs/dnc.md) and its first experiment is [`docs/dnc-experiment-1.md`](docs/dnc-experiment-1.md). Still **not validated as a product**.  
> **Purpose:** capture the concrete design direction developed in our discussion, including its motivation, proposed primitives, service lifecycle, deployment model, learning hypothesis, and unresolved questions.  
> **Last updated:** 2026-10-14.

## Try the language prototype

Suture is becoming a language. You declare what exists, what each thing needs, the rules that must always hold and the goals to reach, and the runtime does the rest. To learn it, read **[the Suture guide](docs/guide/README.md)**: a tutorial, a chapter per construct, modelling patterns and a quick reference, all tested by the build. The design is in the [language spec](docs/superpowers/specs/2026-10-14-suture-language-design.md); its appendices B to H describe each prototype slice.

```
cargo run --release -p suture-cli -- build examples/tickets
cargo run --release -p suture-cli -- run examples/tickets --scenario examples/tickets/tickets.scenario
cargo run --release -p suture-cli -- build examples/shop
cargo run --release -p suture-cli -- run examples/shop --scenario examples/shop/orders.scenario
cargo run --release -p suture-cli -- explain examples/shop --scenario examples/shop/orders.scenario O1
```

`examples/shop/france.st` shows conditional depth. Only orders to France grow a customs form, and only alcohol grows an excise section with its own rules. Nothing in `shop.st` mentions France.

## 0. The thesis

Build a computer whose **running program is a network of interacting computational entities**. Execution is the *local, atomic replacement of connected structures*. The long-term research bet is that some of those replacement laws can be **learned**, allowing the machine to reorganize executable structure rather than merely pick an action or ask a model a question.

The desired departure from conventional AI software is important:

| Approach | What runs |
| --- | --- |
| Conventional program | An explicit sequence/control flow of operations |
| LLM agent | A model chooses among tools and operations |
| Semantic API wrapper | Conventional code calls `judge()` / `score()` / `gpt.ask()` |
| **Proposed machine** | A typed port graph evolves through **local graph-rewrite interactions**; a learned rule may eventually construct a *replacement executable subgraph* |

**Do not mistake this for:** a chatbot, semantic conditional library, generic task orchestrator, YAML that merely states goals, or an AI-powered exception handler. Those alternatives were explored and explicitly rejected as the core novelty.

**Success is not merely showing that graph rewriting works.** The kernel should support multiple different classes of computation, and learned local transformations should generalize to new graph configurations without violating hard safety and resource constraints.

### 0.1 Clarified thesis: Decision-Native Computing

The inspiration is the arrival of very fast, inexpensive **decision models** such as Jev. These models make bounded judgments: choosing among alternatives, scoring possibilities, deciding a condition. They don't need to generate programs or act as agents. The research question is what happens when such decisions become part of how the machine executes, rather than services that conventional programs call.

> **A program is a persistent network of interacting computational entities. Its execution is the continuous reorganization of that network through local transformations, with decision models resolving certain computational possibilities.**

How this differs from §3's learned-law framing:

| | §3 learned laws (earlier framing) | Decision-Native Computing |
| --- | --- | --- |
| What the model produces | a replacement subnetwork (patch synthesis) | a **selection** among admissible alternatives that deterministic rules enumerated |
| Where it acts | inside an interaction rule | at an explicit, typed **decision site** in the graph |
| How it takes effect | the patch is installed after validation | a typed **decision token** is consumed by an ordinary deterministic rule |
| Hard constraints | validate the proposed patch | enforced before (admissible set) and after (enactment validation) the decision |

The model is not a central planner, not a tool router, and not the application. It resolves **local** choice points using bounded local observations. One shared decision law serves every site of its kind, and decisions change the topology, which changes the next decisions. The hypothesis under test:

> A shared, bounded decision mechanism, embedded into local graph-reduction semantics, can enable reusable computational behavior that emerges from the composition of decisions rather than explicitly programmed global procedures.

This is a hypothesis, not a result. Generative patch synthesis (§3) stays a later option, for cases where bounded selection proves insufficient. Two claims are kept apart:
- the **research claim**: learned local decisions produce useful emergent computation;
- the **product claim**: this execution model improves production software.

Proving one does not prove the other.

## 1. Research lineage and what is actually established

These are separate, established ideas motivating the proposed combination—not proof the proposed system already works.

- **Yves Lafont, _Interaction Nets_ (1990):** computational agents have ports; two connected principal ports form an **active pair**; a rule replaces the active pair with a new network preserving its external interface. This supplies a precise computational reduction model. [Paper](https://doi.org/10.1145/96709.96718)
- **Interaction combinators (Lafont, 1997):** a particularly small universal interaction system—three symbols and six rules—illustrates how much can emerge from local interactions.
- **HVM / Bend:** implementations and higher-level approaches worth studying for graph reduction, parallel evaluation, compilation, and runtime design. [HVM2](https://github.com/HigherOrderCO/HVM2) · [Bend](https://github.com/bendlang/bend)
- **Chemical Abstract Machine and higher-order chemical languages:** computation by transformations of interacting objects; some variants represent reaction rules as manipulable entities. [Chemical Abstract Machine](https://doi.org/10.1016/0304-3975(92)90185-I)
- **Neural cellular automata:** repeated learned local updates can produce global self-organization and regeneration. [Growing Neural Cellular Automata](https://distill.pub/2020/growing-ca/)
- **Neural cellular automata for reasoning:** earlier in the discussion we identified work on learned local dynamics for puzzles and reasoning; the applicability to *general service software* remains unproven. [Previously cited preprint](https://arxiv.org/abs/2609.36126) · [Associated repository cited in discussion](https://github.com/paradigms-of-intelligence/reasoning-nca)
- **Constraint Handling Rules (CHR):** useful neighboring model for guarded rewriting, but its execution semantics should not be conflated with strict interaction nets. [Overview](https://dtai.cs.kuleuven.be/projects/CHR/about.shtml)

**Distinctions:** a true interaction net has stricter topology and rewrite semantics than a generic mutable property graph. Typed guards, external state, matchmaking, or arbitrary learned rewrites are *extensions* and may sacrifice properties such as confluence.

## 2. Core computational model

### 2.1 Machine state

The **compute state** is a typed **port graph**:

- **Agents:** nodes with a type, a distinguished **principal port**, zero or more auxiliary ports, and optionally typed payloads.
- **Wires:** connections between ports, respecting the graph representation's linearity and port constraints.
- **Active pair:** two agents connected through their principal ports.
- **Interaction law:** a rule that replaces an active pair with a new subgraph having a compatible external boundary.
- **Scheduler:** finds active pairs and reduces independent pairs concurrently where safe.

The pure core has **one computational primitive**:

```text
INTERACT(A, B) -> N
```

Where `A` and `B` form an active pair, and `N` replaces that pair **without changing the boundary wires visible to the rest of the graph**. The replacement can erase, create, and reconnect internal agents. The whole graph evolves as new active pairs emerge.

Symbolically:

```text
A >< B  =>  N
```

This is **not** an API that asks a model which operation to call. The local graph replacement **is the computation**.

### 2.2 Example reduction

Two principal ports meet:

```text
NeedDB(continuation) >< DBReady(resource)
    =>
wire(continuation, resource)
```

After the interaction, the two agents disappear and the external continuation connects to an authorized database capability. The example is a *sketch* of a type-preserving interaction; production rules would require formal port arities, linearity constraints, and capability semantics.

### 2.3 Machine instructions vs. host operations

Keep a strict distinction:

| Level | Operation | Meaning |
| --- | --- | --- |
| **Pure compute** | `INTERACT(A, B)` | Atomically replace a local active pair with a compatible network |
| Host / graph admission | `INJECT(net)` | Turn an external command or event into graph state |
| Host / graph construction | `CONNECT(portA, portB)` | Wire compatible interfaces during authorized graph construction |
| Persistence | `COMMIT(reduction)` | Make a reduction durable, update indexes, journal it |
| Side effects | `DISPATCH(effect)` | Invoke an approved external adapter; never pretend network I/O is a pure rewrite |
| Observation | `OBSERVE(graph)` | Produce status, projections, inspection, and debugging views |

**Open architectural fork — `BIND`:** Real systems must *discover* which previously unconnected requirement and capability should meet. Two possible directions:

1. **Strict net:** `INTERACT` is the sole computational primitive; discovery, routing, and matching must be expressed as explicit net structures. `CONNECT` is only graph construction / admission.
2. **Dynamic interaction machine:** promote an authorized `BIND` operation alongside `INTERACT`; `BIND` establishes a typed, resource-safe connection between eligible interfaces. More practical for dynamic services, but farther from classical interaction nets.

This is a **central unsolved issue**. A large conventional resource planner hidden inside `BIND` would undermine the new paradigm.

### 2.4 What ports and compatibility actually mean

A **port** is an **attachment point on an agent**, not intrinsically a queue, socket, data channel, or messaging medium. A **wire** relates two ports. In the pure interaction-net model, wires are graph topology: they determine which agents may become an active pair and how a rewrite reconnects its surroundings. Data movement or communication is a *possible interpretation implemented by a network*, not the definition of a wire.

A type-enriched host language might describe a port contract as `P = (T, D, O, C)`:

- `T` — value/capability type (e.g., `DBRef`);
- `D` — optional producer/consumer direction or interaction role;
- `O` — ownership discipline (linear, shared, borrowed, etc.);
- `C` — static constraints and/or separately checked runtime obligations.

**This tuple is a proposed language-level design, not a requirement of Lafont's original nets.** In particular, `In<T>` / `Out<T>` may be useful for service ergonomics, but **direction is not automatically built into the kernel's ports**.

There are two separate predicates to design:

1. **Connection legality:** can these two interfaces be wired under the type, arity, ownership, and static protocol rules?
2. **Rewrite legality:** if connected agents reduce, does the replacement preserve the *external boundary* and satisfy the applicable type, ownership, and hard-policy invariants?

Dynamic eligibility is a third, independent check: a US database offer and an EU-only requirement might have structurally compatible `DBRef` ports yet be **forbidden** to bind by residency policy. Being type-compatible does not establish authorization.

**Preserve boundary, not information.** A valid interaction rule reconnects the same exposed external interface (up to the formally defined boundary correspondence), but it may create, erase, or rearrange internal agents and values. The kernel does **not** promise information conservation, confidentiality, or semantic correctness merely because a rewrite is structurally well formed. Duplication and erasure are meaningful computational patterns, though linear capability policies may prohibit them for specific resources.

The compact design mantra is:

> **Ports establish connectivity; types constrain legality; interactions compute by rewriting topology; protocols are composed from those interactions.**

### 2.5 Type and resource semantics

Candidate *high-level* agent families (not extra CPU instructions):

| Family | Examples | Why it exists |
| --- | --- | --- |
| Values | `String`, `DBRef`, `TenantId`, `Result` | Carry data |
| Demands | `NeedDB`, `AwaitResult`, `RequireIdentity` | Express pending obligations |
| Capabilities | `DatabaseLease`, `StorageQuota`, `Permission` | Express possession of authority or scarce resources |
| Effects | `ProvisionIntent`, `SendEvent`, `CommitRecord` | Represent approved work outside the pure kernel |

Desirable property: *linear/nonduplicable* capability tokens for scarce resources, explicit shareability for read handles. A token **does not replace** the authoritative external allocation ledger; the adapter must independently enforce actual access and capacity.

### 2.6 Protocols as programs, not kernel instructions

**Current preference:** keep the kernel's ports minimal; express resource lifecycles, sessions, acknowledgments, and transaction protocols as *networks of interacting structures*. The type checker may verify protocol states and allowed transitions, but the reducer should not need hardcoded knowledge of a particular business protocol.

For example, a database lease can be represented through a sequence of different graph structures:

```text
AvailableLease  --(valid interaction)-->  ReservedLease
ReservedLease   --(provider result)-----> ActiveLease
ReservedLease   --(timeout/failure)-----> ReleasedLease
```

This diagram is **a conceptual protocol state machine, not literal graph syntax**. Each transition would have to compile into lawful active-pair reductions; provider results and timeouts arrive as external events through `INJECT`, not hidden reads of the world from inside a pure rule.

Three layers should remain distinct:

| Layer | Owns | Must not secretly do |
| --- | --- | --- |
| **Kernel / physics** | Agents, ports, wires, active-pair reduction, atomic local replacement | Know about SAML, leases, HTTP, or business transaction semantics |
| **Types / static guarantees** | Port compatibility, boundary checks, linear ownership, analyzable protocol states | Pretend that type checking grants runtime authority |
| **Protocol networks** | Resource lifecycles, request/response, retries, coordinated interactions | Introduce a hidden central workflow engine outside the graph |

**Why this preference?** A developer should be able to invent a new service protocol by composing existing computational mechanisms rather than updating the kernel's instruction set. But minimality is a hypothesis, not a religion: if even a small typed session becomes awkward or inefficient, we should identify the missing abstraction instead of forcing it.

### 2.7 Memory model — intentionally unresolved

We have not chosen how the machine represents **values and persistent shared state**. This is more foundational than surface syntax and directly affects local reduction, replay, concurrency, performance, and learning.

Candidate representations to test:

- **Payload-on-agent:** agents carry small typed values; rewrites replace agents and their payloads. Simple local reasoning, but large mutable data becomes awkward.
- **Value-as-network:** complex data is itself a subgraph, reducible and possibly structurally shared. More native to the paradigm, but can increase graph size and make ordinary data operations expensive.
- **External data by capability/reference:** graph nodes hold restricted handles to conventional databases or immutable blobs; adapters own durable data effects. Production-friendly, but easy to smuggle arbitrary global mutable state behind references.
- **Hybrid:** local immutable payloads and explicit linear capability handles for external persistence, with carefully defined sharing. This is the *initial engineering hypothesis*, not a decision.

Design questions include whether a wire is exclusively a linear structural relationship or whether sharing needs explicit duplication/reference nodes; how to represent state read by many consumers without breaking isolation; whether persistent state lives in the graph, outside it, or both; and how crash replay avoids rerunning nondeterministic model inference or external effects.

***Do not treat `Port<T>` as an ordinary mutable pointer.*** That would risk importing unrestricted aliasing and global mutation into an execution model whose point is local, analyzable interactions.

### 2.8 A plausible runtime kernel

```rust
struct Agent {
    kind: AgentKind,
    payload: Payload,
    principal: PortId,
    auxiliary: Vec<PortId>,
}

struct Patch {
    new_agents: Vec<Agent>,
    new_wires: Vec<Wire>,
    boundary: Vec<PortId>,
}

trait InteractionLaw {
    fn reduce(
        &self,
        left: &Agent,
        right: &Agent,
    ) -> Result<Patch, InvalidReduction>;
}
```

Illustrative reducer:

```text
pair = active_pairs.take()
rule = rule_table.lookup(pair.types)
patch = rule.reduce(pair.local_data)
validate_ports_types_boundary_and_invariants(patch)
atomically_replace_pair_and_journal(pair, patch)
enqueue_newly_created_active_pairs()
```

Implementation must additionally handle **stale work items, concurrent reductions touching shared graph state, scheduling fairness, deterministic replay, transaction isolation, and bounded work**.

### 2.9 Programming surface: IR first, syntax later

**No standalone language required for v1.** Specify a canonical, versioned, typed **port-graph intermediate representation (IR)** and its legal rewrite semantics. Implement the kernel in Rust; use Rust macros or a small embedded rule DSL to construct valid IR. Keep APIs, databases, authorization integrations, deployment, and adapters in ordinary Rust/TypeScript/Go.

A later dedicated language may be worthwhile *only if* it makes topology, linear ownership, boundary preservation, and compositional rewrites significantly easier to express, inspect, and verify than the host-language API. It should compile to the **same IR**, so the machine is not tied to source syntax. A live graph inspector/debugger may prove more valuable early on than a text language.

For example, possible future source notation:

```text
agent NeedDB(continuation: DBRef)
agent DBReady(resource: DBRef)

rule connect_database:
    NeedDB(k) >< DBReady(db)
    =>
    k ~ db
```

This is **invented pseudocode**. The compiler still must establish proper arities, connected principal ports, linear use of exposed wires, and type-preserving boundary correspondence. Ordinary imperative code inside a reduction body must not bypass those guarantees.

## 3. The learned-law hypothesis

The first implementation should have **only deterministic, explicit laws**. It should demonstrate a useful graph computer before adding model inference.

Eventually, some interaction laws may be trainable:

```text
Handwritten: R(A, B, local_state) -> replacement_net
Learned:     R_theta(A, B, local_state) -> proposed_replacement_net
```

The model would **construct a replacement executable local graph**, not return prose, a boolean, a score, or a tool name. The runtime then checks the proposal before installation:

1. Same permitted external boundary, valid typed ports and graph structure.
2. No unauthorized capabilities, resource duplication, or new arbitrary side-effect adapters.
3. Hard business and security policies still hold.
4. A bounded transformation cost; cycle/resource limits and safe failure behavior.
5. A fixed, versioned rule/model artifact for recording and replay.

Initial learning strategy: **train offline → test in simulation/replay → deploy frozen versions → observe**. No unrestricted self-modifying rules in critical business infrastructure.

**Major theoretical limit:** stochastic or context-sensitive learned rewrites may destroy confluence and reproducibility. Type checking is not a proof of global correctness. For consequential actions, rule validation, authorization, and external effect verification remain mandatory.

### Relationship to Jev

Jev motivated the exploration because low-cost model judgments make repeated semantic decisions more feasible. But **Jev is not assumed to be the execution engine**: a hosted decision API is not a local trainable graph-rewrite kernel. It might serve as a research evaluator, weak supervisor, or data-labeling tool; the fundamental computation should not be `gpt.ask()` hidden behind a different name.

## 4. First real use case: multi-tenant workspace provisioning

**Product hypothesis:** a durable interaction runtime for continuously changing service topology, initially sold to platform engineering teams responsible for complex multi-tenant SaaS infrastructure.

**Customer objective:** create and operate an enterprise workspace without specifying an exact provisioning workflow.

**Business/system requirements:**

| Domain | Concrete requirement |
| --- | --- |
| API/UX | Create workspace through REST and admin console; inspect progress |
| Entitlements | Apply enterprise plan, quotas, billing limits |
| Data | Dedicated or isolated Postgres/database resource |
| Residency | Enforce allowed regions |
| Identity | SAML/SSO and tenant administrators |
| Security | Tenant isolation, least privilege, KMS, auditable authorization |
| Availability | Survive partial failures, retries, and infrastructure changes |
| Lifecycle | Create, resize, migrate, suspend, restore, delete |
| Operations | Visible blockers, pending effects, topology, replay, alerting |
| Target SLO | Standard workspace provisioning under 10 minutes (proposed target; measure it) |

### Customer-facing request

```http
POST /v1/workspaces
Authorization: Bearer <token>
Idempotency-Key: req-8472
Content-Type: application/json
```

```json
{
  "organization_id": "acme",
  "plan": "enterprise",
  "region": "eu-west-1",
  "database_isolation": "dedicated",
  "identity_provider": "saml",
  "encryption": "customer_managed"
}
```

```http
HTTP/1.1 202 Accepted
Location: /v1/operations/op_8472
```

```json
{
  "operation_id": "op_8472",
  "workspace_id": "ws_1024",
  "status": "provisioning"
}
```

The **user sees normal software**: requests, operation status, webhooks, dashboard, stable service API. The novel execution model should remain largely internal.

### Resulting graph (conceptual)

```text
WorkspaceRequested(acme, enterprise, EU)
    |
    +-- NeedDatabase ---------- DatabaseOffer(EU, capacity)
    +-- NeedEncryption -------- KMSCapability(EU)
    +-- NeedIdentity ---------- IdentityAdapter(SAML)
    +-- NeedCompute ----------- ComputeCapacity(EU)
```

These edges are a domain sketch, **not literal Lafont interaction-net wiring**. A compiler needs to translate them into a valid port-based network, including how competing offers are discovered and selected.

### Full request lifecycle

1. **Admission:** validate token, tenant membership, entitlement, scope, input schema, rate limits, and idempotency key.
2. **Durable command:** write accepted command, operation identifier, initial graph state, and journal record transactionally; return `202`.
3. **Graph initialization:** construct typed demand, authorization, and capability interfaces according to the versioned service definition.
4. **Reductions:** schedule local active pairs; apply typed deterministic rules; transact graph rewrites; run disjoint pairs concurrently.
5. **Effects:** a successful rewrite emits durable intents such as `ProvisionDatabaseIntent`. Separate approved workers execute them.
6. **Result ingestion:** successes, failures, provider callbacks, and external observations become new graph events, activating further reductions.
7. **Completion:** graph reaches a verified state with all requirements satisfied; SQL read models show `ready`, or a concrete `blocked`/`failed` reason.
8. **Ongoing reconciliation:** quota changes, infrastructure failures, migration requests, and policy changes enter as new events and alter the running graph.

**Crash consistency:** effect intents are committed to an **outbox** with the state transition; external workers claim and retry them. If the provider succeeds but the reply is lost, reconciliation/idempotency tokens must detect the existing resource. Exactly-once effects **cannot** be promised across arbitrary third-party APIs. Compensations must respect data safety; failed SSO configuration is not permission to destroy a customer's database.

## 5. Deployable architecture

```text
Customer REST API / SDK / Admin UI
                  |
        Authn, authz, admission
                  |
      Versioned domain + compiler
                  |
        Interaction Kernel (Rust)
       /            |             \
Active-pair      Graph +       Effect outbox
scheduler        journal            |
       \            |          Effect workers
        \---- PostgreSQL ----/      |
                           Cloud / DB / KMS / IdP
                  |
          SQL read projections
                  |
          Status API / webhooks
```

| Component | Default choice | Responsibilities |
| --- | --- | --- |
| External API | Go or TypeScript | User contract, authentication, validation |
| Runtime | Rust | Typed ports, active-pair index, rewrite evaluation, atomic replacement |
| State | PostgreSQL | Graph persistence, concurrency state, durable event journal, outbox |
| Worker processes | Isolated adapters | Execute scoped external operations, report results, reconcile providers |
| Messaging | Outbox plus queue | Deliver events without assuming exactly-once external effects |
| Authorization | OIDC/IAM + explicit policy | Admission and *independent* effect authorization |
| Observability | OpenTelemetry + graph inspector | Traces, reduction history, blocked demands, topology, replay |
| Deployment | OCI containers, Docker Compose, Helm | Local development, self-hosting, production scaling |

**Scale deliberately:** begin with a single PostgreSQL-backed graph partition and multiple effect workers. Prove correctness and measure contention before adding distributed graph partitions or specialized storage. Local graph parallelism **does not imply** cheap distributed transactions or guaranteed linear scaling.

### Security and authority boundaries

- Hard policy is **not learned**: residency, tenant isolation, permission scopes, resource limits, and allowed adapters must be enforced independently.
- A graph rewrite may *propose* a change; it cannot invent authority, credentials, quotas, or arbitrary network I/O.
- Graph state should store restricted references to protected business records, not unneeded sensitive payloads; encryption and retention requirements also apply to graph journals.
- Effect adapters independently authorize resource scope, tenancy, and operations.
- Shared-capacity reservations require an authoritative ledger, atomic leasing, and reconciliation.
- Rule and model versions must be recorded so earlier decisions can be reconstructed without silently re-running changed learned code.

### Proposed developer repository

```text
workspace-service/
  api/             # OpenAPI and handlers
  domain/          # Business types and capability interfaces
  interactions/    # Typed local rewrite rules
  policies/        # Authn, authz, residency and effect constraints
  effects/         # Cloud, database, KMS, IdP adapters
  db/              # Migrations, journal, read projections
  tests/           # Invariants, concurrency, failures, replay
  deploy/          # Docker, Compose, Helm
```

**Development:** define versioned types/rules, compile to net IR, test and visualize reductions locally.  
**CI:** property tests, port/boundary checking, invariant checks, randomized event scheduling, fault injection, migration validation.  
**Deployment:** publish signed runtime/rules/adapters, roll out conservatively, preserve graph version compatibility.  
**Operations:** explain *which unsatisfied demand* blocks progress and *which rewrite/effect result* caused it.

## 6. Why would someone adopt it?

A useful challenge is comparing it against established alternatives:

| Alternative | Already good at | Our hypothesis must prove |
| --- | --- | --- |
| Temporal | Durable workflows, retries, activities | Better behavior for dynamically reconfiguring dependency networks |
| Kubernetes controllers | Desired-state reconciliation of infrastructure | Useful generality across complex business and infrastructure relationships |
| CHR / rule engines | Constraint/rule rewriting | Clearer execution semantics, visibility, composability, deployment |
| Interaction nets / HVM | Fine-grained local reduction and parallelism | Durability, policy, I/O, running services, possibly learning |
| Conventional code | Mature tooling, predictable behavior | Less custom coordination complexity on changing topologies |

**Do not** claim the new system is better by default. If a Temporal workflow and Kubernetes controller solve the job simply, use them.

**Potential wedge:** companies dealing with evolving service dependencies, resources, entitlements, and failure recovery, where hand-built orchestration produces sprawling custom state machines.

**Distribution proposal:**

1. **Open-source, self-hostable core:** one binary + PostgreSQL + adapter workers; SDKs, examples, local graph debugger.
2. **Concrete reference product:** SaaS tenant/workspace provisioning using Kubernetes, Postgres, one cloud provider, and one identity provider.
3. **Design partners:** a few real platform teams; compare maintenance burden, incident investigation, topology changes, and failure recovery to their incumbent system.
4. **Commercial extension:** managed runtime, policy governance, enterprise operations, audit retention, support. Charge for managed infrastructure/production deployments—not Jev tokens.

## 7. Implementation program and falsification criteria

### Two minimal programs to settle port/protocol design

Before expanding the language or training a rewrite model, implement **both** with the same kernel and IR:

**Experiment A — scarce linear resources.** Ten concurrent workspace requests compete for three database leases. Bind eligible demands, grant at most three active reservations, then release/reclaim them after injected completions and failures. Assert no duplication, double consumption, tenant-boundary violation, or leaked lease across randomized schedules and crashes. This also tests whether `BIND` is genuinely necessary.

**Experiment B — typed communication protocol.** Two logical services establish a session, exchange several typed messages, react to a timeout injected by the host, and close or recover. Represent the protocol in graph structures rather than a hidden workflow executor. Verify legal sequencing, no invalid message acceptance, inspectable progress, and explainable quiescence.

**Decision rule:** if both examples are natural, concise, and efficient, keep ports structurally simple and protocols compositional. If not, identify the *smallest precise deficiency* (typing, IR, binding, scheduling, or kernel), measure it, and only then consider a new primitive.

### Suggested first 12 weeks (hypothesis, not a promise)

| Phase | Deliverable |
| --- | --- |
| Weeks 1–3 | Typed port-graph reducer in Rust; active pairs, rule table, local invariants, deterministic traces |
| Weeks 4–6 | PostgreSQL durability, optimistic transactions, outbox, idempotency, worker retries |
| Weeks 7–9 | Real workspace API and adapters, policy enforcement, failure tests, observability |
| Weeks 10–12 | Visual graph debugger, packaging, comparative benchmark vs. conventional implementation |

**Crucially:** no learned rule in the production critical path in this phase. First demonstrate that the computational substrate has value by itself.

### What must be measured

- **Correctness:** no known permission/residency/tenant-isolation violations under adversarial tests.
- **Durability:** no lost committed commands or effect intents across crashes; retries converge safely.
- **Concurrency:** safe and useful parallel reduction of independent work; explicit contention metrics.
- **Operability:** reliable replay and explanation of pending demands, deadlocks/quiescence, and external failures.
- **Developer value:** time and amount of coordination logic needed for a new business requirement vs. Temporal/controller baseline.
- **Generality:** demonstrate the *same kernel* for a second, significantly different domain—for example incremental computation, distributed dataflow, or executable constraint networks.
- **Learned rules (later):** out-of-distribution graph transformations that remain valid, improve a measurable outcome, and beat fixed rewrite baselines rather than merely memorizing templates.

### Kill criteria

- Most useful logic lives in a giant hidden binding planner while the graph is cosmetic.
- Rewrites reproduce ordinary workflows with worse tooling and no measurable benefit.
- External side effects cannot be reconciled reliably.
- Graph transformations are not debuggable or replayable.
- Every new problem requires a completely new set of global handwritten algorithms or retraining with no transferable computational substrate.
- Learned transformations cannot respect hard constraints or deliver meaningful gains over deterministic baselines.

## 8. Unresolved design decisions (working register)

This is the concise register of unresolved architectural choices. **The final section provides prioritized, directional questions, proposed experiments, and decision criteria.**

1. **`BIND` vs. pure interaction nets:** is connection discovery computationally internal, or does the host expose an explicit authorized binding primitive?
2. **Core calculus:** strict Lafont interaction nets vs. a guarded typed interaction system closer to CHR. What properties can we actually retain?
3. **Confluence vs. dynamic policy:** how will we handle competing offers, nondeterministic matches, and competing learned replacements?
4. **IR and compiler:** how do practical domain rules compile to well-formed linear port graphs? What remains normal TypeScript/Go code?
5. **Persistence:** how to store and atomically update small graph neighborhoods without excessive global database contention?
6. **I/O semantics:** exact outbox format, effect ownership, idempotency, reconciliation, compensation, and observability.
7. **Capability accounting:** linear resource semantics in the graph vs. real-world shared-resource ledgers.
8. **Training:** what dataset, supervision signal, and graph representation can yield *generalizing structural rewrites* rather than action selection?
9. **Adoption:** what benchmark task demonstrates a clear advantage over Temporal/Kubernetes/controller code?
10. **Second application:** what very different problem would prove this is a reusable computational paradigm instead of an infrastructure orchestrator?
11. **Port semantics:** are structural ports plus static types and linear ownership enough, or does a practical runtime require richer primitive port behavior?
12. **Protocol composition:** can typed sessions, timeouts, leases, and failure recovery be expressed as nets without a conventional orchestration engine hiding outside the graph?
13. **Memory and aliasing:** where do values, sharing, and persistent state live while preserving locality, auditability, and safe concurrent reduction?
14. **Language ergonomics:** does a canonical graph IR plus Rust macros suffice, or is a standalone interaction syntax a measurable improvement?

## 9. The guiding principles

1. **The running graph is the program.** Computation is local structural rewriting.
2. **Minimal kernel.** One pure `INTERACT` primitive unless experiments justify another.
3. **Make connections explicit.** Do not hide the whole algorithm in `BIND`, a global scheduler, or the compiler.
4. **Separate pure reductions from real effects.** The external world is not atomically rewritable.
5. **Hard constraints are enforced, never merely scored.** Learned behavior is subordinate to authority and invariants.
6. **Learned laws produce structure.** Not booleans, prose, or chosen tool calls.
7. **Ship ordinary software interfaces.** API, auth, database, adapters, dashboards, CI, observability, and on-call operability are essential.
8. **Prove advantages in practice.** Benchmark against conventional systems and abandon the direction if it cannot win on a meaningful dimension.
9. **Keep the port primitive honest.** A port is an attachment interface, not inherently a message channel, pointer, data store, or business contract.
10. **Prefer compositional protocols.** Put the meaning of a resource lifecycle in the net and its static checks—not in hidden kernel instructions.
11. **Make memory semantics explicit before optimizing.** Do not accidentally import unrestricted mutable aliasing into a local-rewrite machine.
12. **Design IR before a new syntax.** Preserve one executable semantics across handwritten rules, compiled DSLs, and validated learned graph patches.

---

### One-sentence summary

> **A durable, typed interaction-net-inspired computer whose programs are evolving port graphs, whose basic computation is atomic local graph replacement, and whose longer-term research frontier is safe learned generation of replacement executable structure.**

### Provenance note

This README captures an exploratory conversation. Its code and system interfaces are **design sketches**, not existing APIs. Research references listed above come from works discussed during that exploration; their applicability and details should be independently checked before using them as formal technical claims.

---

## 10. Directional open questions — ordered research agenda

These questions are deliberately phrased to **drive a choice**, not just prolong speculation. Each has a default hypothesis, a discriminating experiment, and a concrete reason to change direction. This is the next work queue, in priority order.

### P0 — What is the exact graph and memory model?

**Direction:** begin with *linear structural ports*, fixed agent arities, immutable small payloads, explicitly represented duplication/sharing, and typed capabilities to external persistent storage. Do **not** add implicit mutable references.

**Questions to answer:**
- What precisely is a valid graph, a free interface, a port type, an active pair, and an atomic patch? Which invariants can the type checker prove?
- Can multiple consumers inspect persistent information without silently converting wires into shared mutable pointers?
- Does a `DBRef` live as an agent payload, a subnetwork, or a capability to SQL state? How are ownership, snapshots, and replay defined?

**Test / exit condition:** write a machine-checkable IR specification, prove or systematically check port-boundary preservation, and demonstrate immutable values, one-shot capabilities, and explicitly shared read-only data. **Change the memory approach** if a representative data structure needs global mutation or pathological copying just to run.

### P1 — Is `INTERACT` sufficient, or must `BIND` be fundamental?

**Direction:** treat `INTERACT` as the only pure computation initially. Represent matchmaking using explicit nets, with only authenticated event admission/construction available to the host.

**Questions to answer:**
- How do disconnected demand and offer structures find one another without a global planner doing all the meaningful computation?
- Can eligibility, fairness, capacity, and race-free acquisition be expressed locally?
- Where is the precise line between graph admission (`CONNECT`) and computational discovery (`BIND`)?

**Test / exit condition:** build **Experiment A** (10 demands, 3 leases), instrument matching complexity, concurrency conflicts, and graph-size overhead. **Promote a narrowly specified `BIND`** only if explicit-net discovery is demonstrably unacceptable, and document exactly what additional expressive power or operational efficiency it gives.

### P2 — Can real protocols be composed from interactions?

**Direction:** keep ports structurally minimal; express communication and lifecycles as networks. Let the compiler check protocol-state constraints where possible.

**Questions to answer:**
- Can we represent a typed session, acknowledgments, retries, cancellation, timeout, and orderly closure with no hidden imperative workflow?
- What is compile-time protocol legality versus runtime permission and provider-specific truth?
- Is quiescence a valid completion state, a deadlock, or an external wait—and can the runtime distinguish them?

**Test / exit condition:** build **Experiment B** end-to-end with deterministic replay and an inspectable trace. **Enrich the type system before enlarging the kernel**; add a primitive only if a specific limitation survives that attempt.

### P3 — What guarantees survive practical service extensions?

**Direction:** precisely separate the strict interaction-net core from typed guards, matching, nondeterministic selection, durable effects, and learned patch proposals. Never claim confluence or determinism for the whole platform without proof.

**Questions to answer:**
- Which reductions commute? When are scheduling order and offer selection observably consequential?
- Can we specify a deterministic conflict resolution policy without a hidden global application controller?
- What can be statically verified, runtime-checked, or only tested empirically?

**Test / exit condition:** property-based tests and schedule exploration on critical invariants; document every lost formal property and its replacement guarantee.

### P4 — Where do durability, I/O, and security cross the boundary?

**Direction:** keep external effects out of pure rewrites. Commit graph transformations and outbox intents atomically; execute effects with independently authorized, idempotent adapters.

**Questions to answer:**
- What is the atomic unit of persistence: active pair, graph partition, or multiple interacting leases?
- How do we handle a successful external side effect whose reply is lost?
- Can replay reconstruct *why* a graph changed without re-running nondeterministic inference, timeouts, or unauthorized effects?

**Test / exit condition:** crash/fault-injection suite over provisioning, quota exhaustion, recovery, duplicate delivery, and tenant-residency policy enforcement. Do not expand into learned production rules before these pass.

### P5 — Does a dedicated language actually help?

**Direction:** publish a canonical graph IR and prototype Rust macros/embedded definitions first. Compare them with a small standalone interaction-rule DSL targeting the same IR.

**Questions to answer:**
- Does explicit source syntax reveal topology, ownership, and boundaries more clearly than Rust?
- Can IDEs, graph visualizers, errors, and debuggers target the shared IR rather than each front end?
- Can proposed learned patches and human-written rules be checked with the same validator?

**Test / exit condition:** have developers implement the lease and session examples both ways; measure correctness, amount of boilerplate, debuggability, and comprehension. Only commit to a language if it wins on those measures.

### P6 — Do learned interaction laws introduce genuinely new capability?

**Direction:** first prove the deterministic substrate. Then train small *local* models to propose **bounded replacement subgraphs** subject to exact validation; do not replace the kernel with an LLM tool router.

**Questions to answer:**
- Can one learned law transfer to unseen graph sizes and topologies without per-workflow retraining?
- What should the model observe locally, and how do we avoid hidden access to global state?
- Does a generated network accomplish tasks that compact handwritten rule libraries and ordinary planners cannot accomplish as effectively?

**Test / exit condition:** holdout graph families, frozen model versions, policy/invariant rejection statistics, replay, and side-by-side comparisons with deterministic baselines. If generated patches only encode preselected workflow templates, the learned-law hypothesis fails.

### P7 — Is there a reusable computer and a viable product?

**Direction:** make the workspace-provisioning service the **engineering proving ground**, not the definition of the machine. Keep a second unrelated use case on the roadmap from the start.

**Questions to answer:**
- Can the same kernel and IR serve an incremental-computation engine, distributed dataflow, or executable constraint network without redesign?
- Which observable metric beats Temporal, Kubernetes controllers, CHR, or simpler code: change complexity, recovery, throughput, explanation, or adaptability?
- Who integrates, operates, and pays for it, and what can they deploy without trusting experimental learned rules?

**Test / exit condition:** two distinct applications on the same kernel; published reproducible baseline benchmarks; design-partner evidence of an operational advantage. If the graph is merely an elaborate orchestration visualization, narrow the project or stop.

### P8 — Does decision-native execution add capability beyond its substrate?

**Direction:** keep the model of `docs/dnc.md`:
- decisions are typed sites whose admissible alternatives are enumerated by deterministic rules from local data;
- one shared law per kind of site resolves them;
- a typed token, consumed by an ordinary rule, enacts the choice;
- every decision is journaled.

Experiment DNC-1 (`docs/dnc-experiment-1.md`) found:
- decision quality matters under contention (+8–15 points over fixed or random choices on most contended suites);
- the shared law transfers to unseen sizes and shapes, and reorganizes after disturbances without a coordinator;
- a *learned* law only matched a handwritten one;
- a centralized planner was better and cheaper.

**Questions to answer:**
- **Merges as decisions:** should "resolve one of several pending interactions" become a decision site, that is, a journaled nondeterministic merge, instead of boundary mailboxes? The merge is P1's precise deficiency, and the resolver is already the machine's journaled source of choice.
- **Information at the site:** which interaction-borne signals (pushed scarcity pressure, observed reliability) must reach a site before a learned law can beat a handwritten one? Is bounded-neighbourhood perception worth giving up causal independence?
- **Credit during execution:** can outcome information shape decisions at run time, not only through offline training, without becoming an implicit global controller?
- **Price of the global view:** under what staleness, churn and re-planning cost do local decision dynamics beat centralized re-planning? DNC-1 gave the planner a free, instant global view.
- **General decision models:** can a zero-shot, Jev-like model reach handwritten quality at these sites without task-specific training, and at what latency once calls are batched per round?
- **Order sensitivity:** outcomes varied by 5–9 points across execution orders. Is that acceptable, or does the machine need decision-order control, such as deterministic batching or priorities?

**Test / exit condition:** Experiment DNC-2 has four parts:
1. continuous churn instead of a single disturbance;
2. a planner that re-plans from *k*-round-stale snapshots;
3. propagated pressure signals;
4. a zero-shot decision model alongside trained and handwritten laws.

**Narrow the learning claim** if trained laws still stay within noise of handwritten ones when given richer local information. **Narrow the paradigm claim** if centralized re-planning still wins once its global view has a realistic cost.

### Current decisions to carry forward

| Decision | Working position | Revisit when |
| --- | --- | --- |
| Core operation | **`INTERACT`** is the only pure primitive | Experiments demonstrate unavoidable extra computational semantics |
| Binding | **Not** a fundamental instruction yet | Resource discovery cannot be composed or scaled adequately |
| Ports | Structural attachment points, **not** inherently messages | Practical protocols require an unrepresentable primitive |
| Types | Static connection/boundary/ownership rules; runtime authority checked separately | Formal checks cannot express critical resource constraints |
| Protocols | Construct from nets; type-check where useful | Session experiment forces unreasonable contortions |
| Memory | **Open**, favor immutable local payload + explicit sharing/capabilities | Data structures or persistence tests falsify that hybrid |
| Syntax | **IR first**, Rust-based definitions for v1 | A dedicated DSL measurably improves developer experience |
| Learned laws | Offline-trained, bounded, validated graph replacements **later** | Deterministic runtime and baseline tests are established |
| Product | Ordinary service APIs and deployable infrastructure | Real platform teams show no meaningful advantage |
| Decisions | Typed decision sites resolved by a shared law; tokens consumed by deterministic rules; journaled; no `BIND` primitive | A decision needs information or authority a token cannot carry |
| Learned decisions | Bounded selection among admissible alternatives first; generative patch synthesis (§3) later | Selection proves insufficient for a task family |

**Immediate next technical artifact:** a short **formal IR + memory-semantics specification** (including three worked reductions) followed by the lease and typed-session example programs. That is the smallest next step that can resolve several foundational questions without introducing another grand abstraction.

---

## Code (v0)

```text
src/            kernel: values, signatures, lineage ids, graph, nets + splice resolution,
                static checks, rules, policies, machine (INTERACT/INJECT/effects), journal,
                explanation, DOT export
src/dnc.rs      decision layer: laws, model-independent backends, batched resolver
src/nets/       programs on the kernel: arith (Peano), combinators, lease (Experiment A),
                session (Experiment B), session_typed (Experiment B0),
                depnet (Experiment DNC-1: self-organizing dependency network)
tests/          kernel, journal/replay, explanation, and per-program suites
docs/           ir-spec.md (formal IR), experiments.md (findings), dnc.md (decision model),
                dnc-experiment-1.md (DNC findings), superpowers/ (design + plan)
```

```bash
cargo test                                      # the full suite
cargo run --release --example lease_pool        # Experiment A measurements
cargo run --release --example session           # Experiment B measurements
cargo run --release --example arith             # Peano demo + throughput
cargo run --release --example dnc_experiment    # Experiment DNC-1 (training, baselines, disturbances)
```
