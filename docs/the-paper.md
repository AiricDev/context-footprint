## Abstract

Coupling metrics are widely used as proxies for software maintainability, yet the dominant approach in research and practice quantifies coupling by counting dependencies—implicitly assuming all dependencies impose equal reasoning cost. This assumption ignores a fundamental distinction: dependencies mediated by narrow interfaces expose far less information than dependencies on concrete implementations, even when the edge count is identical.

We argue that a more fundamental measure of coupling is the *total context volume* required to reason soundly about a code unit—not the number of its dependencies, but the extent of information they expose. Building on this perspective, we introduce Context-Footprint (CF), a static metric that estimates this reasoning scope by modeling abstraction boundaries—interfaces, immutable types, and explicit specifications—as cut points that limit dependency traversal. The metric is grounded in principles of *local reasoning* and *information hiding* from programming language research, which emphasize confining analysis to a well-defined footprint of state and specification.

We evaluate CF through an empirical study on 500 real-world software maintenance tasks from SWE-bench Verified, examining whether CF predicts LLM-based repair success beyond what traditional metrics capture. **[Placeholder: Key findings—e.g., "Results show that CF explains X% additional variance in task success beyond traditional metrics such as CBO."]**

## **1. Introduction**

Coupling has long been recognized as a central factor affecting software maintainability, modifiability, and comprehension. Consequently, a large body of software engineering research has proposed coupling metrics to quantify the degree of interdependence between software units. The dominant metrics in both research and practice, however, share a common simplifying assumption: dependencies are treated as uniform edges, and coupling is approximated by counting their number or structural arrangement. While a smaller body of work has explored information-flow-aware approaches (e.g., Vovel metrics), these remain peripheral to mainstream adoption and do not directly address the question of *how much context* is required for sound reasoning.

This paper argues that coupling metrics should explicitly account for the variation in reasoning scope across dependencies. We adopt the notion of an *information footprint*, defined as the set of program elements that may need to be considered to reason soundly about a given code unit. This notion is closely related to principles of *local reasoning* studied in programming language research, which emphasize confining reasoning to a limited and well-defined footprint. In this tradition, information hiding—restricting access to implementation details behind well-specified interfaces (Parnas, 1972)—is the primary mechanism that enables local reasoning: when a boundary successfully hides internal decisions, reasoning about a dependent unit can proceed without crossing that boundary. To clarify scope: we do not aim to establish a formal correspondence between coupling metrics and proof systems such as separation logic. Rather, local reasoning serves as a *motivating perspective*—a principled lens for distinguishing dependencies by the amount of information they expose, without requiring formal verification machinery.

Building on this perspective, we introduce **Context-Footprint (CF)**, a static coupling metric that estimates the information footprint induced by a code unit’s dependencies. CF treats abstraction boundaries—such as interfaces, abstract classes, and immutable value objects—as potential cut points that limit dependency traversal. Because static analysis cannot precisely determine which external information will be required in all future modification scenarios, CF is deliberately defined as a conservative approximation. This design choice prioritizes robustness and interpretability over maximal precision.

The goal of this work is to complement existing coupling metrics by capturing a dimension of coupling that edge-counting metrics systematically ignore. To evaluate whether CF provides additional explanatory value, we conduct an empirical study on real-world software maintenance tasks. The study examines whether CF predicts code modifiability in AI-assisted programming tasks, and whether this predictive power persists after accounting for traditional coupling measures.

### **Contributions**

This paper makes the following contributions:

1. **A reasoning-scope perspective on coupling.**
    
    We argue that the *volume of context* required for sound local reasoning is a more fundamental measure of coupling than edge count alone. This perspective reframes coupling as a question of information exposure: how much external detail must be consulted to understand a code unit, rather than how many dependencies it has.
    
2. **The Context-Footprint (CF) metric.**
    
    We propose CF, a static coupling metric that operationalizes the reasoning-scope perspective: CF estimates the total information footprint induced by a code unit's dependencies, using abstraction boundaries as traversal cut points that distinguish high-exposure dependencies from well-encapsulated ones.
    
3. **Empirical evidence of CF's explanatory value.**
    
    We provide empirical evidence that CF captures a dimension of coupling not explained by existing metrics, using a large-scale study on realistic software maintenance tasks. [Placeholder: summarize key finding, e.g., "CF explains X% additional variance in task success beyond CBO."]
    

Together, these contributions demonstrate that coupling can be meaningfully quantified not merely by counting dependencies, but by estimating the reasoning context they impose—and that this distinction has measurable consequences for real-world software maintenance.

## 2. Problem Statement

### 2.1 What Existing Metrics Fail to Capture

The information-agnostic nature of edge-counting metrics leads to two practical consequences, both documented in the empirical literature:

- **Incomplete prediction of modification effort.** Coupling values derived from edge-counting metrics explain only part of the variance in actual maintenance cost. For example, Wilkie and Kitchenham [2000] found that CBO alone is "inadequate to predict those classes prone to change ripples," because it ignores how much information flows through each dependency. More recent work on Vovel metrics [2021] demonstrates that incorporating information volume significantly improves fault prediction beyond CBO, confirming that edge counts miss a consequential dimension.
- **Limited actionable guidance.** Edge-counting metrics can indicate that a module is "highly coupled," but cannot explain whether this coupling arises from unavoidable domain interactions or from avoidable leakage of implementation details. Consequently, they offer little insight into *how* coupling might be reduced without changing system behavior.

From the perspective of local reasoning, the relevant quantity is not the number of dependencies, but the *extent of external context* that a developer—or an automated agent—may need to inspect to reason soundly about a change. Existing metrics do not attempt to estimate this quantity.

### 2.2 Problem Scope and Design Goals

This work addresses the following problem:

> *What quantity should a coupling metric estimate to capture reasoning cost, rather than merely dependency count? And how can this quantity be approximated practically using static analysis?*
> 

We deliberately constrain the scope of this problem in three ways:

- **Static analysis.** CF targets the reasoning scope encountered during code comprehension and modification—activities performed over source code as written, not over runtime program states. Both human developers and AI agents read and reason about static artifacts; consequently, a metric grounded in static dependencies directly reflects the information an agent must process. Dynamic information (e.g., actual call frequencies or runtime object graphs) may refine estimates of execution behavior, but lies outside the scope of the reasoning task CF is designed to characterize.
- **Syntactic approximation of boundary strength.** Whether an abstraction boundary genuinely supports local reasoning is a semantic judgment that automated analysis cannot make—it would require understanding intent, behavioral contracts, and implicit assumptions. In this work, we approximate boundary strength through syntactic criteria: the presence of type annotations, documentation, and structural patterns such as immutability. This makes the boundary predicate a deliberate extension point in the CF framework; future implementations may employ more sophisticated strategies (e.g., LLM-based evaluation of specification quality) to narrow the gap between syntactic proxy and semantic reality.
- **Pragmatic locality.** We use local reasoning principles as an engineering heuristic for identifying plausible abstraction boundaries, not as a formal verification criterion.

**One explicit non-goal deserves mention:** context volume is not the only dimension affecting modification difficulty—intra-unit complexity (e.g., cyclomatic complexity, nesting depth) also contributes to reasoning burden. CF is designed to capture the magnitude of external information exposure, a dimension that edge-counting metrics systematically ignore, rather than to subsume all factors influencing modifiability. In our empirical evaluation, we control for cyclomatic complexity to isolate CF's unique contribution.

*Section 3 introduces Context-Footprint (CF), a metric designed to satisfy these goals.*

## 3. Context-Footprint Metric

### 3.1 Overview and Intuition

The Context-Footprint metric estimates the total volume of program context that may need to be consulted to reason soundly about a given code unit. Rather than counting dependencies, CF asks: *how much source code must an agent—human or automated—potentially read to understand this unit's behavior?*

CF is built on three key ideas.

**Conditional traversal over a dependency graph.** CF models a codebase as a directed graph of functions and variables, connected by edges representing reading dependencies—relationships where understanding one unit may require consulting another. Given a target unit, CF traverses this graph outward, accumulating the source code volume of each reachable node. However, not all edges are traversed unconditionally: traversal is *selective*, governed by the strength of abstraction boundaries encountered along the way.

**Abstraction boundaries as cut points.** The central mechanism that controls CF's traversal is *information hiding*. When a dependency is mediated by a well-specified abstraction—an interface with documented behavioral contracts, an immutable value whose state is fully determined at construction—the abstraction boundary acts as a *cut point*: traversal stops, and the implementation behind the boundary is excluded from the context footprint. Conversely, when a dependency exposes implementation details—through under-specified interfaces, mutable shared state, or missing type annotations—the boundary fails, and traversal continues into the dependency's internals. In this way, CF directly rewards architectural practices that confine reasoning to narrow, well-defined boundaries, and penalizes designs that leak internal complexity.

**Syntactic boundary approximation.** Because true specification completeness cannot be assessed by automated analysis (Section 2.2), CF approximates boundary strength through syntactic criteria—type annotations, documentation coverage, and structural patterns such as immutability. This makes boundary evaluation an extensible component of the framework, and different implementations may employ heuristics of varying sophistication.

### 3.2 Dependency Graph

We model a software system as a directed graph $G = (V, E)$, where each node $v \in V$ represents either a **function** (or method) or a **variable** (module-level or class-level state). Type definitions—classes, interfaces, structs, enums—are not graph nodes; instead, they are stored in a separate **Type Registry** and referenced by type identifiers from node attributes (e.g., parameter types, return types, variable types).

This design reflects a functional decomposition of program structure: functions represent *execution logic*, variables represent *state*, and types serve as *descriptive attributes* that constrain the behavior of functions and variables. This separation simplifies the graph model while preserving the information needed for traversal decisions—type information is consulted during boundary evaluation (Section 3.3) without introducing type-level edges into the traversal graph.

**Edge types.** Each directed edge $(u, v, k)$ represents a forward dependency from $u$ to $v$ of kind $k$. We define five edge kinds:

- **Call**: $u \to v$ where function $u$ may invoke function $v$.
- **Read**: $u \to v$ where function $u$ may read variable $v$.
- **Write**: $u \to v$ where function $u$ may modify variable $v$.
- **Annotates**: $u \to v$ where code unit $u$ is decorated by $v$ (e.g., Python decorators). The edge points from the decorated unit to the decorator, since the decorator may alter behavior.
- **ImplementedBy**: $m \to c$ where $m$ is an interface method and $c$ is a concrete implementation of that method. This edge connects functions to functions—not types to types—enabling per-method traversal decisions rather than per-class expansion.

**Graph construction principle.** The dependency graph must be a conservative over-approximation of all potential forward dependencies. If reasoning about $u$ may require consulting $v$ through any of the five edge kinds above, under any feasible usage context, the corresponding edge must be present. Different analyses may approximate these edges with varying precision (e.g., class hierarchy analysis vs. points-to analysis for Call edges), provided the conservativeness requirement is satisfied.

### 3.3 Traversal and Pruning Predicate

The dependency graph defined in Section 3.2 captures all potential forward dependencies between code units. CF computes the reasoning footprint by traversing this graph outward from a target unit—but not all edges are followed unconditionally. This section defines *how* traversal proceeds and *when* it stops.

#### From Invoking to Understanding

Critically, the notion of "dependency" in CF differs from compile-time or call-graph dependencies used in traditional coupling metrics. Rather than asking "does $u$ invoke $v$?", CF asks: **"does understanding** $u$ **require reading** $v$**?"** This cognitive framing has two consequences for traversal.

First, **forward traversal along outgoing edges** follows the five edge kinds defined in Section 3.2. When function $u$ calls $v$, reads variable $v$, or is decorated by $v$, understanding $u$ may require consulting $v$—so traversal proceeds from $u$ to $v$ along the corresponding edge, subject to boundary evaluation.

Second, and less obviously, understanding a unit sometimes requires inspecting entities that *reference* it—a direction opposite to the graph's edge orientation. We call this **reverse exploration**: traversal that follows existing edges *backward*, from target to source. Two situations motivate reverse exploration:

- **Call-in exploration.** Consider a function $v$ with a loosely specified parameter list—no type annotations, no documentation, and high degrees of freedom in how arguments may be supplied. To understand what values $v$ actually receives, a reader must inspect *call sites*: the places where $v$ is invoked. This requires traversing incoming Call edges in reverse—from $v$ to its callers.
- **Shared-state write exploration.** When a function $u$ reads a mutable variable $S$, understanding $u$ requires knowing the possible values of $S$. This in turn requires inspecting all units that may write to $S$—obtained by traversing incoming Write edges of $S$ in reverse.

Neither call-in nor shared-state write exploration introduces new edge types into the graph. Both are traversal strategies over the existing edge set: call-in exploration follows Call edges backward; shared-state write exploration follows Write edges backward from a variable node to its writers.

#### Edge-Aware Pruning Predicate

Not all reachable edges should be traversed. CF controls traversal through an **edge-aware pruning predicate** $P(E_{in}, v, E_{out})$, which determines, for each candidate next step, whether traversal should continue. Here $v$ is the current node, $E_{in}$ is the edge through which $v$ was reached, and $E_{out}$ is the candidate outgoing edge (or reverse exploration) being considered.

The predicate is edge-aware rather than node-level because *how a node was reached affects what should be explored next*. For example, a function $v$ with incomplete type annotations would normally trigger call-in exploration to understand its usage context. But if $v$ was reached via a Call edge from some caller $u$, the calling context is already known—we know what arguments $u$ passes—and call-in exploration would be redundant. This context-sensitivity cannot be captured by a per-node predicate.

$P$ evaluates to **true** (continue traversal) or **false** (stop; the edge acts as a cut point). The predicate operates with a fundamental asymmetry between forward and reverse directions:

**Core asymmetry.** Forward edges check *target* specification; reverse exploration checks *source* specification. This reflects the cognitive reality of code comprehension: forward dependencies ask "what does this thing I'm calling do?", while reverse exploration asks "what inputs might I receive?" or "what state might I observe?"

#### Forward Edge Rules: Target Specification Completeness

For forward edges (Call, Read, Annotates, ImplementedBy), traversal stops when the *target* $v$ provides sufficient specification for reasoning without further inspection:

1. **Interface abstraction.** If $v$ is accessed through an interface or abstract type accompanied by documentation describing its behavioral contract (preconditions, postconditions, side effects), traversal stops at the interface. Concrete implementations behind the interface are excluded from the context footprint.
2. **Immutability.** If $v$ is an immutable value object (no mutable fields, no observable state changes after construction), its behavior is fully determined by its construction. Traversal stops at $v$.
3. **Abstract factory.** If $v$ is a function that returns an abstract type (interface or protocol) with sufficient documentation, traversal stops at $v$ regardless of $v$'s own documentation quality. The caller only interacts with the returned interface, not the factory's implementation details.

**Write edges** are unconditionally traversed: writing to a variable may affect its state, and the variable node must be evaluated for its own dependencies—including shared-state write exploration if the variable is mutable.

#### Reverse Exploration Rules: Source Specification Completeness

For reverse exploration (call-in, shared-state write), the decision depends on whether the *current node*'s own specification is sufficient to constrain its behavior without inspecting external context.

**Call-in exploration.** When the traversal reaches a function $v$, it considers whether to explore $v$'s callers (by following incoming Call edges in reverse). The decision depends on both $v$'s specification quality and how $v$ was reached:

- **If** $v$ **was reached via a Call edge** → Do not explore callers. The calling context is already known from the incoming edge.
- **If** $v$ **has a complete specification** (fully typed parameters, documented preconditions) → Do not explore callers. The specification suffices to understand $v$'s expected inputs.
- **Otherwise** → Explore all callers. The specification is insufficient, and call-site inspection is needed to understand $v$'s usage context.

**Shared-state write exploration.** When traversal reaches a mutable variable $S$ (via a Read edge), it explores all writers of $S$—obtained by following incoming Write edges of $S$ in reverse. This exploration is unconditional: if $S$ is mutable and externally scoped, understanding any reader of $S$ requires knowing what values $S$ may hold, which requires inspecting all units that may write to $S$. This mechanism directly penalizes broad variable scope: the wider $S$'s scope, the more writers exist, and the larger the context footprint.

### 3.4 Context-Footprint Computation

CF is defined for **function nodes only**. Variable nodes participate in the dependency graph as structural connectors—mediating Read, Write, and shared-state write exploration—but are not themselves targets of CF computation. This reflects the metric's purpose: CF measures the reasoning cost of understanding a function's behavior, while variables serve as state that links functions together.

Given a target function $u_0$, CF is computed by traversing the dependency graph starting from $u_0$, following all forward edges and reverse explorations not cut by the pruning predicate $P$ (Section 3.3). Let $R(u_0)$ denote the set of nodes visited during this traversal—including $u_0$ itself and all reachable function and variable nodes. Note that $R(u_0)$ may contain variable nodes encountered during traversal; their $size(v)$ contributes to the total footprint, since reading a variable's declaration is part of the reasoning cost.

**Cycle handling.** If traversal encounters a node that has already been visited, it stops at that node—its context volume has already been counted. This standard graph traversal rule ensures $R(u_0)$ is well-defined even in the presence of cycles (e.g., mutual recursion, circular type references).

The Context-Footprint of $u_0$ is defined as:

$$
CF(u_0) = \sum_{v \in R(u_0)} size(v)
$$

where $size(v)$ is a non-negative measure of the information content of unit $v$. In this work, we instantiate $size(v)$ as the LLM token count of $v$'s source code—i.e., the number of tokens produced by a language model tokenizer (e.g., BPE). This choice directly reflects the context window cost when an LLM agent loads the unit into its prompt. Alternative measures (e.g., AST node count, lexical token count) are permissible provided they are consistently applied.

**Note on identifier verbosity.** Using LLM token count as $size(v)$ introduces a potential confound: BPE tokenizers split long identifiers into multiple subword tokens, so good engineering practice—descriptive names like `calculateMonthlyRevenue` vs. terse `calc`—inflates token counts without increasing semantic complexity. This means CF may penalize well-named code relative to cryptic code. However, this effect may be partially self-correcting within the CF framework: a more sophisticated specification completeness evaluator (Section 3.5) that recognizes descriptive function names as implicit specification—e.g., `calculateMonthlyRevenue` conveys behavioral intent that `calc` does not—would more readily classify well-named functions as qualified boundaries, terminating traversal and excluding their entire downstream subgraph from the footprint. In such implementations, the token count inflation from verbose identifiers is offset, potentially substantially, by the CF reduction from stronger boundary recognition. Nonetheless, for implementations using simple syntactic heuristics, normalization strategies—such as replacing identifiers with canonical placeholders before counting, or using AST node counts that are identifier-length-agnostic—remain advisable to decouple CF from naming conventions.

This definition is independent of how the dependency graph is constructed or how the pruning predicate is approximated, provided the semantic requirements above are satisfied.

### 3.5 Permissible Implementation Variations

CF allows flexibility in how its components are instantiated. The algorithm defines two explicit extension points and one source of implementation-dependent precision:

- **Size function.** The choice of $size(v)$ (LLM token count, AST node count, etc.) affects absolute CF values but not relative comparisons within a consistently-measured codebase.
- **Specification completeness assessment.** The pruning predicate requires judging whether a unit's specification is "sufficient for reasoning." In this work, we approximate completeness via simple coverage heuristics (e.g., presence of docstrings, type annotations). In practice, implementations may employ more sophisticated approaches—including LLM-based semantic evaluation of documentation, or recognition of descriptive naming as implicit specification (Section 3.4)—to narrow the gap between syntactic proxy and semantic reality.
- **Graph construction precision.** The dependency graph must satisfy the conservativeness requirement of Section 3.2, but implementations may vary in the precision of their static analysis—particularly for alias and effect analysis (which determines the set of writers in shared-state write exploration), and for dynamic language features such as reflection, dynamic dispatch, or higher-order functions. Greater precision reduces false edges without violating conservativeness.

These variations affect the conservativeness and absolute scale of CF values, but do not alter their qualitative interpretation as conservative approximations of reasoning context. For empirical comparisons, implementations should be held constant across conditions.

## 4. Empirical Evaluation

We evaluate Context-Footprint through an empirical study on realistic software maintenance tasks. The study examines whether CF explains variance in task success beyond what traditional coupling and complexity metrics capture, using a large-scale benchmark of real-world GitHub issues.

We use LLM-based agents as experimental subjects (Section 4.1), then describe the study design and present results.

### 4.1 LLM-Based Agents as Experimental Subjects

We adopt LLM-based agents as experimental subjects. LLM-based coding assistants (Claude Code, Cursor) and autonomous agents (Devin, OpenHands) are rapidly becoming primary executors of code comprehension, modification, and debugging in production environments. In this context, demonstrating that CF predicts LLM-based agent success is directly valuable—these agents are themselves the end users of code comprehension, and CF provides actionable guidance for writing code that is more maintainable by the agents that will actually maintain it.

Beyond direct relevance, LLM-based agents offer methodological advantages as experimental subjects:

- **Consistency:** No fatigue, learning effects, or inter-subject variability.
- **Reproducibility:** Identical prompts and temperature settings yield comparable runs.
- **Scalability:** Large-scale experiments (500 tasks × multiple trials) are feasible within practical time and cost constraints.
- **Explicit constraints:** Context window limits are measurable and controllable parameters, directly aligned with what CF estimates.

**Limitations.** LLM-based agents also introduce experiment-specific concerns. Results are tied to specific model versions and may not generalize across model generations. Agent behavior is sensitive to prompt design and retrieval strategy, which are confounds not present in traditional human studies. Even at low temperature settings, non-determinism remains—we mitigate this through multiple runs per task (Section 4.2.5). These limitations are further discussed in Section 4.3.

---

### 4.2 Study Design: SWE-bench Verified Correlation Study

#### 4.2.1 Objective

This study examines the **predictive validity** of CF: does CF explain variance in real-world software maintenance task success beyond what traditional coupling and complexity metrics capture?

**Core Hypothesis:** CF predicts LLM-based agent repair success rate beyond what traditional metrics (CC, CBO, Vovel) capture.

#### 4.2.2 Dataset: SWE-bench Verified

**SWE-bench Verified** contains 500 human-validated software engineering tasks derived from real GitHub issues and pull requests across 12 open-source Python repositories. Each task requires understanding an issue description, locating relevant code, and producing a correct patch.

Key advantages:

- **High quality:** Human-validated to confirm solvability.
- **Realistic tasks:** Derived from actual maintenance workflows.
- **Standardized evaluation:** Official test suites with clear pass/fail criteria.
- **Difficulty annotations:** Tasks include estimated resolution time (15 min - 1 hour, 1-4 hours, etc.).
- **Pure Python:** All 500 tasks are Python-only, enabling consistent CF computation.

#### 4.2.3 Variables

**Dependent Variables:**

| Variable | Definition | Source |
| --- | --- | --- |
| **Success** | Task repair success rate (Pass@k) | Agent experiment (3-5 runs per task) |
| **Token Usage** | Context tokens consumed by the agent | Agent call logs |

**Independent Variables:**

| Variable | Definition | Computation |
| --- | --- | --- |
| **CF (Task-level)** | Context-Footprint of the target function(s) | CF algorithm with union for multi-function patches |
| **CF_p90** | 90th percentile CF across all functions in the codebase | Codebase-level CF distribution |
| **CBO** | Coupling Between Objects (edge-counting) | pydeps or static analysis |
| **CC** | Cyclomatic Complexity of target function(s) | Static analysis (radon, lizard) |
| **Vovel** | Information-volume coupling: sum of parameter/return type sizes across method invocations | Custom implementation or weighted CBO proxy |

We include **Vovel** as the information-volume baseline—CF's closest "competitor"—to demonstrate that CF captures a dimension beyond what Vovel measures (transitive context and abstraction boundaries).

**Multi-Function CF Computation.** When a task's gold patch modifies multiple functions, we compute:

$$
CF_{task} = |Union(TraversalNodes(f_1), TraversalNodes(f_2), ...)|  
$$

This union operation avoids double-counting shared dependencies (e.g., common utilities).

#### 4.2.4 Codebase-Level CF Analysis

Beyond task-level CF, we compute **codebase-level CF distribution** to capture "localization difficulty":

| Metric | Definition | Hypothesis |
| --- | --- | --- |
| **CF_p50** | Median CF of all public functions in the codebase | Reflects "typical" function complexity; robust to outliers |
| **CF_p90** | 90th percentile CF in the codebase | Captures complexity of hardest 10% functions; higher → harder to localize |
| **CF_target_percentile** | Target function's CF percentile in the distribution | Captures relative complexity within project |

**Note on percentile selection.** We use percentile-based metrics (p50, p90) rather than mean/std because CF distributions are typically heavy-tailed—a few high-coupling functions can dominate mean and inflate std. Percentiles are more robust to outliers. We evaluate multiple percentiles (p75, p90, p95) in the analysis and report the best-performing one, with others shown as robustness checks.

**Rationale:** Even if a target function has moderate CF, the agent may struggle to *locate* it in a codebase where codebase-level CF is high. Codebase-level metrics help distinguish "CF affects localization" from "CF affects repair."

#### 4.2.5 Experimental Procedure

We run LLM experiments ourselves rather than relying on leaderboard results. This enables:

- Recording **token usage** (not provided by official leaderboards).
- Logging **retrieval behavior** (which files the agent accessed).
- Supporting richer analysis: distinguishing localization failures from repair failures.

**Agent Configuration:**

| **Model** | gpt-oss-120b |  |
| --- | --- | --- |
| **Runs per task** | 3-5 |  |
| **Runs per task** | 3-5 |  |
| **Temperature** | 0.2 |  |

We use a single mid-tier open-source model (gpt-oss-120b) for the primary experiment. If results warrant further investigation, we may supplement with a SOTA proprietary model to assess whether CF's predictive power varies with agent capability.

#### 4.2.6 Hypotheses

- **H1:** Task-level CF is negatively correlated with repair success rate and positively correlated with agent token consumption.
- **H2:** CF's predictive power for both Success and Token Usage persists after controlling for CC, CBO, and Vovel.
- **H3:** Codebase-level CF ($\text{CF}_{p90}$) independently predicts task difficulty and token consumption beyond task-level CF and traditional metrics.

#### 4.2.7 Statistical Models

Each hypothesis is tested through a dedicated statistical comparison. We use two families of models: **logistic regression** for Success (binary) and **linear regression** for Token Usage (continuous). All models include project fixed effects $\gamma_{\text{project}}$ to control for repository-level variation. All continuous predictors are standardized (zero mean, unit variance) prior to model fitting.

#### H1: Marginal Predictive Validity of CF

H1 tests whether task-level CF alone predicts task outcomes, establishing baseline predictive validity before introducing control variables. We fit two univariate models:

**H1a — Success (Logistic):**

$$
\text{logit}(P(\text{Success})) = \beta_0 + \beta_1 \cdot \text{CF} + \gamma_{\text{project}}
$$

**H1b — Token Usage (Linear):**

$$
\text{TokenUsage} = \beta_0 + \beta_1 \cdot \text{CF} + \gamma_{\text{project}} + \epsilon
$$

**Validation:** $\beta_1$ is significantly negative for Success ($p < 0.05$) and significantly positive for Token Usage. We report AUC for H1a and $R^2$ for H1b.

#### H2: Incremental Validity Beyond Traditional Metrics

H2 tests whether CF provides explanatory power beyond CC, CBO, and Vovel. We compare nested models for each dependent variable:

**Model A (Baseline):**

$$
\text{logit}(P(\text{Success})) = \beta_0 + \beta_1 \cdot \text{CC} + \beta_2 \cdot \text{CBO} + \beta_3 \cdot \text{Vovel} + \gamma_{\text{project}}
$$

**Model B (+ CF):**

$$
\text{logit}(P(\text{Success})) = \beta_0 + \beta_1 \cdot \text{CC} + \beta_2 \cdot \text{CBO} + \beta_3 \cdot \text{Vovel} + \beta_4 \cdot \text{CF} + \gamma_{\text{project}}
$$

The same nested comparison (Model A′ vs. Model B′) is repeated with Token Usage as the dependent variable using OLS regression.

**Validation:**

1. **Likelihood ratio test (LRT):** Model B significantly improves over Model A ($p < 0.05$).
2. **Coefficient significance:** $\beta_4$ (CF) is significant after controlling for CC, CBO, Vovel.
3. **Discrimination / fit improvement:** $\Delta\text{AUC}$ for Success; $\Delta R^2$ for Token Usage.
4. **Effect size:** Odds ratio for a one-standard-deviation increase in CF (Success); standardized $\beta_4$ (Token Usage).

#### H3: Independent Contribution of Codebase-Level CF

H3 tests whether codebase-level complexity ($\text{CF}_{p90}$) predicts task difficulty beyond task-level CF and traditional metrics.

**Model C (Baseline with task-level CF):**

$$
\text{logit}(P(\text{Success})) = \beta_0 + \beta_1 \cdot \text{CC} + \beta_2 \cdot \text{CBO} + \beta_3 \cdot \text{Vovel} + \beta_4 \cdot \text{CF} + \gamma_{\text{project}}
$$

**Model D (+ CF_p90):**

$$
\text{logit}(P(\text{Success})) = \beta_0 + \beta_1 \cdot \text{CC} + \beta_2 \cdot \text{CBO} + \beta_3 \cdot \text{Vovel} + \beta_4 \cdot \text{CF} + \beta_5 \cdot \text{CF}_{p90} + \gamma_{\text{project}}
$$

The same nested comparison (Model C′ vs. Model D′) is repeated for Token Usage.

**Validation:**

1. **LRT:** Model D significantly improves over Model C.
2. **Coefficient significance:** $\beta_5$ ($\text{CF}_{p90}$) is significant after controlling for task-level CF.
3. **Effect size:** Odds ratio for a one-standard-deviation increase in $\text{CF}_{p90}$.

**Note on project fixed effects and CF_p90.** Because $\text{CF}_{p90}$ is a codebase-level variable and each task belongs to one of 12 repositories, $\text{CF}_{p90}$ varies only across projects—not within projects. If project fixed effects absorb all between-project variance, $\text{CF}_{p90}$ becomes unidentifiable. We address this by using random intercepts for projects (mixed-effects logistic regression) in H3, which partially pool project-level variance and allow $\text{CF}_{p90}$ to be estimated. We report both fixed-effect and mixed-effect specifications as robustness checks.

#### 4.2.8 Supplementary Analyses

1. **Stratified Analysis**
    - By difficulty: 15 min - 1 hour vs. 1-4 hours tasks
    - By project: Is CF effect consistent across different repositories?
2. **Mediation Analysis (Token Usage)**
    - Does CF's effect on Success operate *through* increased token consumption (mediation), or does CF predict Success even after controlling for Token Usage (direct effect)?
3. **Localization vs. Repair Analysis** (if retrieval logs available)
    - Do high-CF tasks show higher rates of incorrect file retrieval?
    - Does codebase-level CF primarily affect localization stage?
4. **Patch Size Sensitivity Analysis**
    - Gold patch size (number of functions modified) is mechanically correlated with $\text{CF}_{task}$ through the union operation and may independently affect task difficulty. However, patch size is a post-hoc variable derived from the ground-truth solution—not observable before the task is attempted—and may function as a mediator (high CF → more code to modify → larger patch → lower success) rather than a confounder. Including it as a primary control risks blocking the causal pathway from CF to Success.
    - We therefore treat patch size as a sensitivity check rather than a main-model covariate. We re-fit the H2 models (Model A/B) with patch size added as an additional control and report the change in CF's coefficient magnitude and significance. If CF's effect substantially attenuates, this suggests partial mediation through patch scope; if CF remains significant, it confirms that CF captures reasoning difficulty beyond mere modification scope.

#### 4.2.9 Results

**[Placeholder: Results tables and figures to be inserted after experiment completion.]**

**Expected Contributions:**

If hypotheses are supported, we can claim:

1. **CF is a valid predictor:** CF significantly predicts LLM-based agent repair success after controlling for CC, CBO, and Vovel.
2. **CF captures distinct information:** CF measures "context volume for reasoning"—a dimension not captured by complexity (CC), edge-counting (CBO), or signature-level information volume (Vovel).
3. **Codebase-level CF affects localization:** High CF_p90 codebases are harder to navigate, independent of target function complexity.

---

### 4.3 Threats to Validity

#### Construct Validity

- **CF implementation fidelity:** Our CF implementation may not perfectly instantiate the theoretical definition. We mitigate this by documenting all operationalization choices and releasing the implementation.
- **Token count as size proxy:** As noted in Section 3.4, LLM token count may be confounded by identifier naming conventions.

#### Internal Validity

- **Confounding variables:** Observational design cannot establish causation. We control for known confounds (CC, CBO, Vovel, project effects) but cannot rule out unmeasured factors.
- **Agent retrieval strategy:** Different agents use different retrieval strategies. CF may correlate with retrieval difficulty rather than repair difficulty. We mitigate this by logging retrieval behavior where possible.
- **Patch size confound.** $\text{CF}_{task}$ is computed over the union of traversal nodes from gold-patch functions; tasks requiring larger patches mechanically produce higher CF values while also being independently harder to fix. Because patch size is derived from the ground-truth solution (not observable a priori) and may lie on the causal path from CF to Success (i.e., act as a mediator rather than a confounder), we do not include it in the primary models. Instead, we report a sensitivity analysis (Section 4.2.8) that re-fits the H2 models with patch size as an additional covariate to quantify how much of CF's effect operates through modification scope.

#### External Validity

- **Language specificity:** This study uses Python codebases exclusively. Generalization to statically-typed languages remains to be validated.
- **Model generalization:** Results are tied to specific model versions and agent architectures; generalization across model generations remains to be validated.
- **Dataset scope:** SWE-bench Verified covers 12 Python repositories. Results may not generalize to all software domains.

#### Statistical Conclusion Validity

- **Sample size:** 500 tasks × 3-5 runs = 1,500–2,500 observations. Power analysis confirms adequate sensitivity for medium effect sizes.
- **Multiple comparisons:** We apply Bonferroni correction for stratified analyses.

---

## 5. Related Work

This work sits at the intersection of software coupling metrics and programming language research on local reasoning. We first review coupling metrics (Section 5.1), then discuss local reasoning principles from PL research that motivate CF's design (Section 5.2), and finally position CF relative to its closest intellectual relative—the Vovel metrics (Section 5.3).

### 5.1 Coupling Metrics: From Edge Counting to Information Volume

The dominant coupling metrics in software engineering research share a common characteristic: they count dependency edges without distinguishing the *magnitude* of information each edge carries.

**Edge-Counting Metrics.** Chidamber and Kemerer's **Coupling Between Objects (CBO)** counts distinct classes to which a given class is coupled. Li and Henry's **Message Passing Coupling (MPC)** counts outgoing method invocations. Robert Martin's **Afferent/Efferent Coupling (Ca/Ce)** counts package-level dependencies. All treat dependencies as uniform edges—to CBO, a reference to a 500-line "God Class" is equivalent to a reference to a 10-line interface.

**Dynamic and Weighted Coupling.** Arisholm et al. defined dynamic coupling measures based on runtime call traces, capturing *actual* rather than *potential* coupling. Recent work weights edges by invocation frequency, recognizing that "many theoretical edges are rarely exercised in practice." These approaches move beyond uniform edge counting, but weight by *usage frequency* rather than *reasoning cost*.

**Semantic and Evolutionary Coupling.** Information retrieval techniques (LSI, TF-IDF) compute semantic similarity between code units, capturing "implicit" coupling through shared concepts. Mining version control history reveals *logical coupling*: files that frequently change together. Both represent dimensions orthogonal to structural coupling.

**Information Flow Metrics.** Henry and Kafura proposed an information flow complexity metric:

$$
Complexity = Length \times (Fan\text{-}in \times Fan\text{-}out)^2
$$

This formula approximates "information throughput" by squaring the product of incoming and outgoing connections—an early attempt to move beyond simple edge counting toward information-theoretic intuitions.

**The Gap.** None of these metrics directly answer: *How much external information must an agent load to reason soundly about a code unit?* They measure dependency *presence*, *frequency*, or *structural intensity*—not the *context volume* required for local reasoning.

### 5.2 Local Reasoning: The Theoretical Intuition Behind CF

Context-Footprint originates from a simple belief: **complex systems can only be reasoned about effectively when decomposed into subsystems with well-defined boundaries**. This intuition—that reasoning should be *local*—turns out to have rigorous theoretical support in programming language research.

**The Frame Problem in Verification.** The challenge of local reasoning was formalized in program verification. To prove a property about a code fragment, one must reason not only about what the code *does*, but also about what it *does not* affect. Without clear boundaries, proving anything requires global knowledge of the entire system—an approach that does not scale.

**Separation Logic and the Frame Rule.** Reynolds [2002] and O'Hearn et al. [2001] developed Separation Logic precisely to enable modular verification. The key insight is the *frame rule*: if a program fragment only touches a subset of memory (its "footprint"), then properties of the untouched portion are automatically preserved. This allows compositional reasoning—proving correctness of parts in isolation.

The term "footprint" in our metric name is a deliberate nod to this tradition. In Separation Logic, a footprint is the minimal heap region a program needs; in CF, it is the minimal information volume an agent needs.

**Information Hiding as Boundary Construction.** Earlier, Parnas [1972] argued that modules should hide "design decisions likely to change," and Liskov & Guttag [1986] formalized this through Abstract Data Types. These are not merely coding conventions—they are boundary-construction techniques that enable local reasoning by limiting what external observers need to know.

**Complexity as a Unifying Lens.** More recently, Ousterhout [2018] offered a practitioner-oriented synthesis of these ideas, defining software complexity as "anything related to the structure of a software system that makes it hard to understand and modify the system," and identifying two root causes: *dependencies* and *obscurity*. This framing aligns directly with the local reasoning perspective: dependencies determine how far reasoning must extend beyond a focal unit, while obscurity—the absence of specification sufficient for local reasoning—determines whether a dependency can be safely abstracted away. CF can be viewed as an operationalization of this complexity model: dependencies correspond to edges in the traversal graph, and obscurity corresponds to the failure of the pruning predicate $P$, which forces traversal to continue.

**CF's Relationship to These Ideas.** We do not claim that CF is a formal verification method or that it inherits the mathematical guarantees of Separation Logic. Rather, CF is an *engineering heuristic* inspired by the same underlying principle: **the quality of a boundary determines how much information must cross it for reasoning to succeed**.

- Separation Logic asks: "What memory does this code touch?"
- CF asks: "What information must an agent load to understand this code?"

Both questions seek to minimize the scope of reasoning. The key difference is that CF trades formal precision for practical measurability—we accept a conservative approximation in exchange for applicability to real-world, dynamically-typed codebases where formal verification is often infeasible.

### 5.3 Vovel Metrics: CF's Closest Relative

The **Vovel metrics** (Vovel-in, Vovel-out) are CF's closest intellectual relatives, explicitly designed to capture both the *degree* of coupling and the *volume* of information flow.

**Vovel's Approach.** Vovel computes information volume by summing the sizes of parameters and return types across method invocations. A method that passes a complex object carries more "information" than one passing a primitive. Empirical evaluation demonstrates that Vovel significantly outperforms CBO in fault prediction, validating the intuition that "not all edges are equal."

**How CF Differs.** Despite shared motivation, CF and Vovel differ in three fundamental ways:

1. **Scope of Measurement.** Vovel measures information *transferred* through method signatures. CF measures the total context *required for reasoning*—including transitive dependencies that may need to be inspected even if no data flows through them.
2. **Abstraction Boundaries.** Vovel treats all dependencies uniformly once their "information volume" is computed. CF explicitly models abstraction boundaries: a dependency on a well-documented interface contributes only the interface's size to CF, not the implementation behind it. This captures the engineering insight that good abstractions *reduce* reasoning scope.
3. **Traversal vs. Aggregation.** Vovel aggregates local information at each edge. CF performs graph traversal with conditional termination—the footprint expands until reaching boundaries that support local reasoning. This yields qualitatively different behavior: two units with identical Vovel scores may have vastly different CF values if one's dependencies are mediated by strong abstractions.

**Why the Difference Matters.** Consider two classes with identical CBO and similar Vovel scores. Class A depends on five well-documented interfaces; Class B depends on five concrete implementations with undocumented side effects. Vovel may rate them similarly (similar parameter complexity), but CF will assign a much higher value to Class B (traversal continues through concrete implementations). Our empirical studies test whether this distinction predicts modifiability.

### 5.4 Summary

| **Metric Family** | **What It Measures** | **CF's Distinction** |  |
| --- | --- | --- | --- |
| Edge-counting (CBO, MPC, Ca/Ce) | Number of dependencies | CF weights by context volume, not count |  |
| Dynamic coupling (DIC, DEC) | Runtime invocation frequency | CF measures reasoning cost, not usage frequency |  |
| Semantic coupling (CSM, CSBC) | Conceptual similarity | CF measures structural information exposure |  |
| Information flow (Henry-Kafura) | Fan-in × Fan-out intensity | CF models abstraction as traversal boundaries |  |
| **Vovel** | **Parameter/return type volume** | **CF includes transitive context + abstraction cut points** |  |

CF bridges classical coupling theory with PL research on local reasoning. By explicitly modeling abstraction boundaries and providing a conservative approximation of reasoning scope, CF addresses a question prior metrics do not: *How much external information must an agent load to achieve semantic completeness for local reasoning?*