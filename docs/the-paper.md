## Abstract

Coupling metrics are widely used as proxies for software maintainability, yet the dominant approach in research and practice quantifies coupling by counting dependencies—implicitly assuming all dependencies impose equal reasoning cost. This assumption ignores a fundamental distinction: dependencies mediated by narrow interfaces expose far less information than dependencies on concrete implementations, even when the edge count is identical.

We introduce **Context-Footprint (CF)**, a static coupling metric that estimates the *total context volume* required to reason soundly about a code unit. CF models abstraction boundaries—interfaces, immutable types, and explicit specifications—as cut points that limit dependency traversal, yielding a conservative upper bound on reasoning scope. The metric is grounded in principles of *local reasoning* from programming language research, which emphasize confining analysis to a well-defined footprint of state and specification.

We evaluate CF through two empirical studies: a controlled ablation experiment isolating abstraction effects on code modifiability, and a large-scale observational analysis on realistic maintenance benchmarks. **[Placeholder: Key findings—e.g., "Results show that CF explains X% additional variance in task success beyond traditional metrics such as CBO."]**

CF complements existing coupling metrics by capturing a dimension they systematically overlook: the *magnitude* of information exposure, not merely the *presence* of dependencies.

## **1. Introduction**

Coupling has long been recognized as a central factor affecting software maintainability, modifiability, and comprehension. Consequently, a large body of software engineering research has proposed coupling metrics to quantify the degree of interdependence between software units. The dominant metrics in both research and practice, however, share a common simplifying assumption: dependencies are treated as uniform edges, and coupling is approximated by counting their number or structural arrangement. While a smaller body of work has explored information-flow-aware approaches (e.g., Vovel metrics), these remain peripheral to mainstream adoption and do not directly address the question of *how much context* is required for sound reasoning.

The assumption that dependencies are interchangeable—that each edge in a dependency graph carries equal weight—obscures an important practical distinction. Different dependencies expose substantially different amounts of information to a developer attempting to understand or modify a piece of code. A dependency mediated by a narrow interface or abstract specification typically requires reasoning about far less external information than a dependency on concrete implementation details. As a result, two code units with an identical number of dependencies may impose very different reasoning scopes during maintenance tasks.

This paper argues that coupling metrics should explicitly account for this variation in reasoning scope. We adopt the notion of an *information footprint*, defined as the set of program elements that may need to be considered to reason soundly about a given code unit. This notion is closely related to principles of *local reasoning* studied in programming language research, which emphasize confining reasoning to a limited and well-defined footprint. To clarify scope: we do not aim to establish a formal correspondence between coupling metrics and proof systems such as separation logic. Rather, local reasoning serves as a *motivating perspective*—a principled lens for distinguishing dependencies by the amount of information they expose, without requiring formal verification machinery.

Building on this perspective, we introduce **Context-Footprint (CF)**, a static coupling metric that estimates the information footprint induced by a code unit’s dependencies. CF treats abstraction boundaries—such as interfaces, abstract classes, and immutable value objects—as potential cut points that limit dependency traversal. Because static analysis cannot precisely determine which external information will be required in all future modification scenarios, CF is deliberately defined as a conservative upper bound. This design choice prioritizes robustness and interpretability over maximal precision.

The goal of this work is not to replace existing coupling metrics, but to complement them by capturing a dimension of coupling that edge-counting metrics systematically ignore. To evaluate whether CF provides additional explanatory value, we conduct an empirical study combining controlled experiments and large-scale observational analysis. These experiments examine whether CF is associated with code modifiability in AI-assisted programming tasks, and whether this association persists after accounting for traditional coupling measures.

---

### **Contributions**

This paper makes the following contributions:

1. **A reasoning-scope perspective on coupling.**
    
    We identify a fundamental limitation of dependency-counting metrics: they conflate dependencies with vastly different reasoning costs, treating a reference to a 500-line implementation identically to a reference to a 10-line interface.
    
2. **The Context-Footprint (CF) metric.**
    
    We propose CF, a static coupling metric that estimates the information footprint induced by dependencies, explicitly accounting for abstraction boundaries and specification strength. CF is deliberately defined as a conservative upper bound to ensure robustness under incomplete static information.
    
3. **Empirical evidence of CF's explanatory value.**
    
    We provide empirical evidence that CF captures a dimension of coupling not explained by existing metrics, using controlled experiments and observational analysis on realistic software maintenance tasks. [Placeholder: summarize key finding, e.g., "CF explains X% additional variance in task success beyond CBO."]
    

Together, these contributions position CF as a practical and theoretically motivated addition to the existing family of coupling metrics.

## 2. Problem Statement

### 2.1 What Existing Metrics Fail to Capture

The core limitation of existing coupling metrics is not that they are incorrect, but that they are *information-agnostic*. By treating all dependencies as equivalent edges, these metrics fail to capture the variation in reasoning scope induced by different forms of abstraction.

This limitation leads to two practical issues, both recognized in the empirical literature:

- **Incomplete prediction of modification effort.** Coupling values derived from edge-counting metrics explain only part of the variance in actual maintenance cost. For example, Wilkie and Kitchenham [2000] found that CBO alone is "inadequate to predict those classes prone to change ripples," because it ignores how much information flows through each dependency. More recent work on Vovel metrics [2021] demonstrates that incorporating information volume significantly improves fault prediction beyond CBO, confirming that edge counts miss a consequential dimension.
- **Limited actionable guidance.** Edge-counting metrics can indicate that a module is "highly coupled," but cannot explain whether this coupling arises from unavoidable domain interactions or from avoidable leakage of implementation details. Consequently, they offer little insight into *how* coupling might be reduced without changing system behavior.

From the perspective of local reasoning, the relevant quantity is not the number of dependencies, but the *extent of external context* that a developer—or an automated agent—may need to inspect to reason soundly about a change. Existing metrics do not attempt to estimate this quantity.

To clarify scope: context volume is not the only dimension affecting modification difficulty. Intra-unit complexity (e.g., cyclomatic complexity, nesting depth) also contributes to reasoning burden. CF is designed to capture a dimension that edge-counting metrics *systematically ignore*—the magnitude of external information exposure—rather than to subsume all factors influencing modifiability. In our empirical evaluation, we control for cyclomatic complexity to isolate CF's unique contribution.

---

### 2.2 Problem Scope and Design Goals

This work addresses the following problem:

> How can we define a coupling metric that estimates the total context volume required for sound local reasoning about a code unit, while remaining practical to compute using static analysis?
> 

We deliberately constrain the scope of this problem in three ways:

- **Static analysis.** While dynamic information may refine estimates of actual runtime behavior, static metrics are more broadly applicable and better aligned with existing tooling and empirical studies.
- **Conservative upper bound.** Because future modification tasks are unknown, the metric should err on the side of including potentially relevant context rather than attempting precise but brittle predictions.
- **Pragmatic locality.** We do not seek a formally verified notion of locality. Instead, we draw inspiration from local reasoning principles to guide the identification of abstraction boundaries that plausibly limit dependency traversal in practice.

These design goals lead to **Context-Footprint (CF)**, a coupling metric that estimates the information footprint induced by a code unit’s dependencies. CF explicitly distinguishes dependencies based on how much external context they expose, rather than treating all dependencies as equal.

## 3. Context-Footprint Metric

### 3.1 Intuition: Context as a Traversable Dependency Space

The Context-Footprint (CF) metric is designed to estimate the total program context—including the target unit itself and its transitively reachable dependencies—that may be required to reason soundly about a given code unit. Intuitively, this context corresponds to the portion of the dependency graph that cannot be safely ignored when the unit is inspected or modified.

From a local reasoning perspective, a dependency limits reasoning scope if it enforces a stable abstraction boundary. When such a boundary is respected, reasoning about a code unit can proceed without inspecting the internal structure of its dependencies. Conversely, when a dependency exposes internal representation details or mutable shared state, reasoning may require traversing beyond the immediate dependency to understand its effects.

CF operationalizes this intuition by treating dependencies as *potentially traversable edges* in a dependency graph. Whether traversal continues across an edge depends on the strength of the abstraction boundary it represents.

---

### 3.2 Modeling Dependencies and Traversal

We model a software system as a directed graph $G = (V, E)$, where each node $v \in V$ represents a program element. In this work, we distinguish three kinds of elements: **functions** (or methods), **type definitions** (classes, interfaces, structs), and **variables** (module-level or class-level state). Each directed edge $(u, v) \in E$ represents a *reading dependency*: to fully understand $u$, one may need to consult $v$.

Critically, this notion of dependency differs from the compile-time or call-graph dependencies used in traditional coupling metrics. Rather than asking "does $u$ invoke $v$?", we ask: **"does understanding** $u$ **require reading** $v$**?"** This cognitive framing leads to edges that traditional metrics do not capture.

Consider a function with a loosely specified parameter list—no type annotations, no documentation, and high degrees of freedom in how arguments may be supplied. To understand what values this function actually receives, a reader must inspect *call sites*: the places where the function is invoked. This induces a **call-in dependency** from the function to its callers—a direction opposite to the usual call-graph edge. Similarly, a function's parameter list induces dependencies on the types of its parameters, because understanding the function's behavior requires knowing what operations those types support.

Given a target unit $u_0$, the naïve approach to estimating its reasoning context would be to consider all nodes reachable from $u_0$ in $G$. However, this over-approximates the actual reasoning scope, as many dependencies are mediated by abstractions that limit information exposure.

CF refines reachability by introducing the notion of *conditional traversal*. An edge $(u, v)$ is traversed only if the dependency does not cross a sufficiently strong abstraction boundary. In other words, traversal proceeds only when understanding $u$ potentially requires inspecting the internal details of $v$.

This approach yields a dependency subgraph rooted at $u_0$, representing the maximal external context that may need to be considered under conservative assumptions.

---

### 3.3 Abstraction Boundaries as Cut Points

A key design decision in CF is the identification of abstraction boundaries that terminate dependency traversal. These boundaries are not assumed to be perfect or formally verified; instead, they are treated as *plausible cut points* that typically confine reasoning in practice.

Examples of such boundaries include:

- Dependencies through interfaces or abstract base types that do not expose concrete implementations.
- Immutable value objects whose internal state cannot be modified after construction.
- Explicit specifications—including documentation, contracts, and type annotations—that constrain observable behavior without revealing representation details.

Critically, **syntactic abstraction alone is insufficient**. An interface without documentation describing its behavioral contract provides no reasoning boundary: a reader who encounters an undocumented [`Repository.save](http://Repository.save)(entity)` method cannot determine whether it validates input, triggers side effects, or throws exceptions without inspecting the implementation. For CF purposes, such an interface does not constitute a cut point. Only when an abstraction is accompanied by specification sufficient to reason about its behavior—without consulting the implementation—does it qualify as a boundary.

When a dependency crosses such a boundary, CF assumes that traversal may safely stop, and the internal dependencies of the target node are excluded from the context footprint.

This design choice reflects a bias toward soundness. If a boundary is incorrectly identified as strong when it is not, CF may underestimate the true reasoning scope. To mitigate this risk, CF adopts conservative criteria for boundary recognition, favoring false positives (continued traversal) over false negatives (premature termination).

---

### 3.4 Conservative Upper-Bound Semantics

CF is deliberately defined as a conservative upper bound on reasoning scope. The metric does not attempt to predict the minimal context required for a specific future modification task. Instead, it estimates the maximal context that *may* be required across a plausible range of tasks.

This choice is motivated by two considerations.

First, future modification scenarios are inherently unknown at measurement time. Any attempt to precisely predict task-specific reasoning scope would rely on assumptions that are difficult to validate empirically.

Second, conservative upper bounds align better with the interpretability of coupling metrics. A higher CF value indicates that more external information might be required in the worst case, providing a monotonic signal that can be compared across code units.

As a result, CF prioritizes robustness and consistency over fine-grained precision.

## 3.5 Formal Definition of Context-Footprint

This section formally defines Context-Footprint (CF) as a graph-based metric over code units. The definition specifies the required semantic structure of the dependency graph and the traversal rules used to compute CF, while remaining independent of any particular implementation or tooling.

### 3.5.1 Code Units and Dependency Graph

As defined in Section 3.2, a code unit may be a **function** (or method), a **type definition** (class, interface, struct), or a **variable** (module-level or class-level state). Local variables and parameters are internal to their enclosing function. Anonymous functions (lambdas, closures) are likewise internal to their lexically enclosing unit.

Let $V$ denote the set of code units in a codebase. CF is defined over a directed labeled graph $G = (V, E)$, where each edge $(u, v, k) \in E$ represents a *reading dependency* of kind $k$: understanding $u$ may require consulting $v$ (see Section 3.2 for the cognitive framing of this relation).

**Graph construction principle.** The dependency graph must be a conservative over-approximation of all potential reading dependencies. If reasoning about $u$ may require consulting $v$ under *any* feasible usage context, an edge must be present—regardless of whether that edge will ultimately be traversed during CF computation. Graph construction captures *potential* information flow; traversal decisions are deferred to the boundary predicate (Section 3.5.2).

We distinguish two classes of reading dependencies:

**Forward dependencies** (from a unit to entities it references):

- *Call edges*: $u \to v$ if $u$ may invoke $v$.
- *Data read edges*: $u \to v$ if $u$ may read a variable or field defined by $v$.
- *Type dependency edges*: $u \to v$ if reasoning about $u$ requires knowledge of a type defined in $v$ (e.g., parameter types, return types, field types).
- *Inheritance edges*: $u \to v$ if $u$ depends on behavioral contracts defined in $v$ via inheritance or interface implementation.

**Reverse dependencies** (from a unit to entities that affect it indirectly):

- *Call-in edges*: $u \to v$ if $v$ is a caller of $u$. These edges capture the possibility that understanding $u$'s behavior may require inspecting how it is invoked.
- *Shared-state write edges*: $u \to v$ if $u$ reads a mutable variable $S$ whose scope exceeds $u$'s own scope (e.g., module-level variables, class fields, global state), and $v$ may write to $S$. This rule effectively **penalizes broad variable scope**: the wider $S$'s scope, the more potential writers exist, and the more edges are added to the graph.

These edge kinds are semantic categories; different analyses may approximate them with varying precision, provided the conservativeness requirement is satisfied.

### 3.5.2 Boundary Predicate

CF incorporates abstraction boundaries through a boundary predicate $B(u, v, k)$, which determines whether traversal along an edge $(u, v, k)$ should stop. Intuitively, $B$ evaluates to true when the dependency relationship provides sufficient specification to reason about behavior without further traversal.

The boundary predicate operates differently for forward and reverse dependencies, reflecting their distinct roles in the dependency graph.

**Core asymmetry.** Forward edges check *target* specification; reverse edges check *source* specification. This reflects the cognitive reality of code comprehension: forward dependencies ask "what does this thing I'm calling do?", while reverse dependencies ask "what inputs might I receive?" or "what state might I observe?"

#### Forward Dependencies: Target Specification Completeness

For forward edges (call, data-read, type, inheritance), traversal stops when the *target* $v$ provides sufficient specification. The predicate assesses:

1. **Interface abstraction.** If $v$ is accessed through an interface or abstract type, and that interface is accompanied by documentation describing its behavioral contract (preconditions, postconditions, side effects), traversal stops at the interface. The concrete implementation behind the interface is excluded from the context footprint.
2. **Immutability.** If $v$ is an immutable value object (no mutable fields, no observable state changes after construction), its behavior is fully determined by its construction. Traversal stops at $v$.
3. **Type completeness.** If $v$ is a type definition with fully specified method signatures, documented semantics, and no hidden state mutations, traversal may stop. Conversely, a type with undocumented methods or implicit side effects does not qualify as a boundary.

#### Reverse Dependencies: Source Specification Completeness

For reverse edges (call-in, shared-state write), the traversal decision depends on the *source* $u$—specifically, whether $u$'s own specification is sufficient to constrain its behavior without inspecting external context.

1. **Call-in edges.** Traversal along a call-in edge $u \to v$ (where $v$ calls $u$) proceeds only when $u$'s specification is *incomplete*:
    - **Traverse** if $u$ lacks type annotations, has loosely typed parameters (e.g., `Any`, `Object`), or has no documentation constraining valid inputs. In such cases, understanding $u$ requires inspecting how it is actually invoked.
    - **Stop** if $u$ has a complete specification: fully typed parameters, documented preconditions, and explicit contracts. The specification alone supports reasoning about $u$'s behavior.
2. **Shared-state write edges.** Shared-state write edges exist only when $S$ is mutable and externally scoped. If such an edge is present in the graph, traversal always proceeds—understanding $u$ requires knowing the possible values of $S$, which in turn requires inspecting all units that may write to $S$.

#### Conservative Approximation Principle

Implementations must approximate $B$ conservatively: it is acceptable to treat fewer edges as boundaries than an ideal oracle would (resulting in continued traversal and higher CF values), but implementations must not treat edges as boundaries unless the specification genuinely supports reasoning without further inspection.

When in doubt, traverse. This principle ensures that CF remains a sound upper bound on reasoning scope, even when specification quality is ambiguous or difficult to assess automatically.

### 3.5.3 Context-Footprint Computation

Given a target unit $u_0$, CF is computed by traversing the dependency graph starting from $u_0$, following all outgoing edges not cut by the boundary predicate. Let $R(u_0)$ denote the set of units reachable from $u_0$ under these traversal rules.

**Cycle handling.** If traversal encounters a node that has already been visited, it stops at that node—its context volume has already been counted. This standard graph traversal rule ensures $R(u_0)$ is well-defined even in the presence of cycles (e.g., mutual recursion, circular type references).

The Context-Footprint of $u_0$ is defined as:

$$
CF(u_0) = \sum_{v \in R(u_0)} size(v)
$$

where $size(v)$ is a non-negative measure of the information content of unit $v$. In this work, we instantiate $size(v)$ as the token count of $v$'s source code. Alternative measures (e.g., AST node count, cyclomatic complexity) are permissible provided they are consistently applied.

By definition, $R(u_0)$ includes $u_0$ itself. Consequently, CF measures the *total* context volume required for reasoning about $u_0$—comprising both the target unit and its transitively reachable dependencies. This aligns with the local reasoning perspective: the footprint for sound reasoning includes the focal code, not merely its external dependencies.

**Note on specification size.** When a unit $v$ serves as a boundary (i.e., traversal stops at $v$), only $v$'s specification contributes to CF, not its implementation. In practice, this means counting the size of the interface or contract rather than the concrete class behind it.

This definition is independent of how the dependency graph is constructed or how the boundary predicate is approximated, provided the semantic requirements above are satisfied.

**Note on identifier verbosity (Threat to Validity).** Using token count as $size(v)$ introduces a potential confound: good engineering practice encourages descriptive, verbose identifier names (e.g., `calculateMonthlyRevenue` vs. `calc`), which inflate token counts without increasing semantic complexity. This means CF may penalize well-named code relative to terse, cryptic code. Future work should explore normalization strategies—such as replacing identifiers with canonical placeholders before counting, or using AST node counts that are identifier-length-agnostic—to decouple CF from naming conventions.

### 3.5.4 Permissible Implementation Variations

CF allows flexibility in how dependency information is obtained and approximated. Implementations may vary in:

- **Call graph precision.** Different call graph construction techniques (e.g., class hierarchy analysis, points-to analysis) yield different levels of precision. More precise analyses produce tighter CF estimates; less precise analyses remain valid but more conservative.
- **Alias and effect analysis.** Shared-state write edges depend on determining which units may write to a given variable. Implementations may use varying levels of alias or effect analysis precision.
- **Dynamic language features.** Treatment of reflection, dynamic dispatch, or higher-order functions may vary, provided the conservativeness requirement is satisfied.
- **Size function.** The choice of $size(v)$ (token count, AST node count, etc.) affects absolute CF values but not relative comparisons within a consistently-measured codebase.
- **Specification completeness assessment.** The boundary predicate requires judging whether a unit's documentation is "sufficient for reasoning." In this work, we approximate completeness via simple coverage heuristics (e.g., presence of docstrings, type annotations). In practice, implementations may employ more sophisticated approaches—including LLM-based semantic evaluation of comments—to assess whether documentation genuinely supports local reasoning without inspecting implementation details.

These variations affect the conservativeness and absolute scale of CF values, but do not alter their qualitative interpretation as upper bounds on reasoning context. For empirical comparisons, implementations should be held constant across conditions.

---

## 4. Empirical Evaluation

We evaluate Context-Footprint through two complementary studies. **Study A** is a controlled experiment that isolates the effect of abstraction boundaries on code modifiability by systematically ablating the components that CF treats as traversal cut points. **Study B** is an observational analysis that examines whether CF explains variance in task success on realistic software maintenance benchmarks beyond what traditional coupling and complexity metrics capture.

Both studies use large language models (LLMs) as experimental subjects. We justify this choice in Section 4.1, then describe each study's design and present results.

---

### 4.1 LLMs as Experimental Subjects

We adopt LLMs as controlled experimental subjects rather than human developers. This choice is motivated by methodological considerations, not by a claim that LLMs are cognitive models of human programmers.

**Constraint-Level Alignment.** We observe that LLMs and human developers share analogous constraints at the operational level:

1. **Bounded Context.** LLMs have explicit context windows; humans have working memory limits. In both cases, the *effective* reasoning capacity does not scale linearly with nominal capacity.
2. **Degradation with Irrelevant Information.** Empirical studies demonstrate that LLM performance degrades when relevant information is buried in long contexts (Liu et al., 2023) or diluted by irrelevant material (Shi et al., 2023)—phenomena analogous to attention limits in human cognition.
3. **Sensitivity to Specification Quality.** Both humans and LLMs benefit from explicit contracts and documentation that reduce the need to inspect implementation details.

**Experimental Advantages.** Using LLMs provides:

- **Consistency:** No fatigue, learning effects, or inter-subject variability.
- **Reproducibility:** Identical prompts and temperature settings yield comparable runs.
- **Scalability:** Large-scale experiments are feasible within practical time and cost constraints.
- **Explicit Constraints:** Context limits are measurable and controllable parameters.

We make a limited claim: if architectural patterns designed to support human local reasoning (interface segregation, dependency inversion, explicit specifications) also improve LLM performance, this suggests these patterns may be broadly adaptive for *any* bounded-resource reasoning agent. We do not claim that LLM behavior generalizes to all aspects of human software comprehension.

---

### 4.2 Study A: Controlled Ablation Experiment

#### 4.2.1 Objective

Study A tests the **construct validity** of CF by examining whether the abstraction boundaries that CF treats as traversal cut points—type annotations, documentation, and interface abstraction—have measurable effects on code modifiability.

If CF's boundary predicate correctly identifies factors that limit reasoning scope, then *removing* these boundaries should:

1. Increase CF values (more context required for reasoning).
2. Decrease task success rates (modifiability degrades).

#### 4.2.2 Dataset: BugsInPy

We use **BugsInPy**, a curated benchmark of 493 real-world Python bugs with accompanying test suites. BugsInPy provides:

- **Ecological validity:** Bugs encountered by real developers in production projects.
- **Ground truth:** Each bug has a verified fix and regression tests.
- **Appropriate granularity:** Most bugs are localized to individual functions or classes.

**Sample Selection Criteria.** We select a subset of bugs (target: 30–50) satisfying:

1. Bug is localized within a method body (excludes import errors, configuration issues).
2. Target function has external dependencies (enables inlining transformation).
3. Context size is moderate (500–2,000 tokens) to avoid floor/ceiling effects.

#### 4.2.3 Experimental Conditions

We construct four experimental conditions through automated code transformations:

| Condition | Transformation | CF Theoretical Effect |
| --- | --- | --- |
| **G1: Baseline** | Original code with all type hints, docstrings, and abstractions intact | Low CF |
| **G2: No-Type** | Remove all type annotations using `strip-hints` | Higher CF (Signature Completeness boundary fails) |
| **G3: No-Doc** | Remove all docstrings and inline comments | Higher CF (Documentation Completeness boundary fails) |
| **G4: Inlined** | Inline all called functions into the target using LibCST | Highest CF (Abstraction boundaries eliminated) |

**Transformation Tools:**

- `strip-hints`: Automated type annotation removal.
- `LibCST`: AST manipulation for docstring removal and function inlining.
- Custom scripts: CF computation for each variant.

#### 4.2.4 Procedure

For each selected bug and each experimental condition:

1. Apply the corresponding transformation to produce the code variant.
2. Compute CF for the target function in each variant.
3. Construct a prompt containing: (a) the transformed code context, (b) the bug description from BugsInPy, and (c) instructions to generate a patch.
4. Generate patches using an LLM (model: **[Placeholder: e.g., GPT-3.5-Turbo or DeepSeek-V3]**).
5. Apply each generated patch and run the BugsInPy test suite.
6. Record: condition, CF value, token count, and pass/fail outcome.

Each condition is run **5 times per bug** (temperature = 0.2) to account for sampling variance. The primary metric is **Pass@1 rate**: the proportion of trials producing a correct fix.

#### 4.2.5 Hypotheses

- **H1:** CF values increase monotonically across conditions: G1 < G2 ≈ G3 < G4.
- **H2:** Pass@1 rates decrease monotonically: G1 > G2 ≈ G3 > G4.
- **H3:** Within each condition, higher CF values are associated with lower Pass@1 rates.

#### 4.2.6 Results

**[Placeholder: Results tables and figures to be inserted after experiment completion.]**

**Expected Visualization:** A scatter plot with:

- X-axis: Token consumption (context size)
- Y-axis: Pass@1 rate
- Color: Experimental condition (G1–G4)

Predicted pattern: G1 (Baseline) clusters in the upper-left (low tokens, high success); G4 (Inlined) clusters in the lower-right (high tokens, low success).

---

### 4.3 Study B: Observational Analysis on SWE-bench

#### 4.3.1 Objective

Study B examines the **predictive validity** of CF: does CF explain variance in real-world software maintenance task success beyond what traditional coupling and complexity metrics capture?

Unlike Study A, Study B does not manipulate code structure. Instead, it analyzes the relationship between pre-existing code characteristics and task outcomes on a standardized benchmark.

#### 4.3.2 Dataset: SWE-bench Lite

**SWE-bench Lite** contains 300 curated software engineering tasks derived from real GitHub issues and pull requests. Each task requires understanding an issue description, locating relevant code, and producing a correct patch.

Key advantages:

- **Realistic tasks:** Derived from actual maintenance workflows.
- **Standardized evaluation:** Official test suites and published leaderboard results.
- **Diverse projects:** Tasks span multiple Python repositories with varying architectural styles.

#### 4.3.3 Variables

**Dependent Variable:**

- **Success** (binary): Whether a given model produces a patch that passes all tests for a given task.

**Independent Variables (computed for the target file(s) of each task):**

| Variable | Description | Source |
| --- | --- | --- |
| **CF** | Context-Footprint of the target function(s) | This work |
| **LOC** | Lines of code | Baseline size metric |
| **CC** | Cyclomatic Complexity | Intra-unit complexity control |
| **CBO** | Coupling Between Objects | Traditional coupling metric (pydeps) |
| **Fan-out** | Number of outgoing dependencies | Structural coupling |

**Control Variables:**

- **Project:** Fixed effect for repository (accounts for project-specific difficulty).
- **Task Type:** Categorical (bug fix, feature addition, refactoring) if available.

#### 4.3.4 Statistical Model

We use logistic regression to model the probability of task success:

**Model 1 (Baseline):**

$$
\text{logit}(P(\text{Success})) = \beta_0 + \beta_1 \cdot \text{LOC} + \beta_2 \cdot \text{CC} + \beta_3 \cdot \text{CBO} + \beta_4 \cdot \text{Fan-out} + \gamma_{\text{project}}
$$

**Model 2 (CF Added):**

$$
\text{logit}(P(\text{Success})) = \beta_0 + \beta_1 \cdot \text{LOC} + \beta_2 \cdot \text{CC} + \beta_3 \cdot \text{CBO} + \beta_4 \cdot \text{Fan-out} + \beta_5 \cdot \text{CF} + \gamma_{\text{project}}
$$

**Incremental Validity Tests:**

1. **Coefficient Significance:** Is $\beta_5$ (CF coefficient) significantly different from zero ($p < 0.05$) in Model 2?
2. **Likelihood Ratio Test:** Does Model 2 fit significantly better than Model 1?
3. **AUC Improvement:** Does adding CF increase the area under the ROC curve?
4. **Effect Size:** What is the odds ratio associated with a one-standard-deviation increase in CF?

#### 4.3.5 Robustness Checks

To assess generalizability, we perform stratified analyses:

- **By task complexity:** Low (LOC < 100), Medium (100–500), High (> 500).
- **By baseline coupling:** Low-CBO vs. High-CBO projects.
- **By model:** Separate analyses for different LLM agents (if leaderboard provides per-model results).

#### 4.3.6 Results

**[Placeholder: Regression tables, AUC comparison, and stratified analysis to be inserted after data collection.]**

**Key Metrics to Report:**

- Model 1 vs. Model 2: Likelihood ratio $chi^2$, $Delta$AUC
- CF coefficient: $beta_5$, 95% CI, $p$-value, odds ratio
- Variance explained: McFadden's pseudo-$R^2$ for both models

---

### 4.4 Threats to Validity

#### Construct Validity

- **CF implementation fidelity:** Our CF implementation may not perfectly instantiate the theoretical definition. We mitigate this by documenting all operationalization choices and releasing the implementation.
- **Token count as size proxy:** As noted in Section 3.5.3, token count may be confounded by identifier naming conventions.

#### Internal Validity

- **Transformation artifacts (Study A):** Automated transformations may introduce subtle changes beyond the intended ablation. We manually inspect a random sample of transformations.
- **Confounding variables (Study B):** Observational design cannot establish causation. We control for known confounds but cannot rule out unmeasured factors.

#### External Validity

- **Language specificity:** Both studies use Python codebases. Generalization to statically-typed languages remains to be validated.
- **LLM as proxy:** Results may not fully transfer to human developers, though constraint-level alignment provides partial justification.

#### Statistical Conclusion Validity

- **Sample size:** Study A targets 30–50 bugs × 4 conditions × 5 trials = 600–1,000 observations. Study B uses 300 tasks. Power analysis confirms adequate sensitivity for medium effect sizes.
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

**CF's Relationship to These Ideas.** We do not claim that CF is a formal verification method or that it inherits the mathematical guarantees of Separation Logic. Rather, CF is an *engineering heuristic* inspired by the same underlying principle: **the quality of a boundary determines how much information must cross it for reasoning to succeed**.

- Separation Logic asks: "What memory does this code touch?"
- CF asks: "What information must an agent load to understand this code?"

Both questions seek to minimize the scope of reasoning. The key difference is that CF trades formal precision for practical measurability—we accept a conservative upper bound in exchange for applicability to real-world, dynamically-typed codebases where formal verification is often infeasible.

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

CF bridges classical coupling theory with PL research on local reasoning. By explicitly modeling abstraction boundaries and providing a conservative upper bound on reasoning scope, CF addresses a question prior metrics do not: *How much external information must an agent load to achieve semantic completeness for local reasoning?*

---
