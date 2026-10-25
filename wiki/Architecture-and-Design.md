# Architecture and Design Philosophy

Aven represents a paradigm shift from traditional imperative and functional programming to **declarative relational invariant systems**.

---

## 1. The Core Problem: Function Directionality in Engineering

In conventional programming languages (Rust, C, Python, Go), algorithms are defined as one-way functions:

```python
# In conventional languages: 3 separate functions for 1 physical law
def force(mass, acceleration):
    return mass * acceleration

def mass(force, acceleration):
    return force / acceleration

def acceleration(force, mass):
    return force / mass
```

In physical engineering, industrial telemetry, aerospace, and datacenter infrastructure, this directional pinning causes massive technical debt:
1. **Combinatorial Explosion:** A system with $N$ interconnected variables requires $\mathcal{O}(N!)$ permutations of functions to support arbitrary sensor configurations.
2. **Duplicated Verification:** Validating bounds (e.g. $a \le 9.8\,\text{m/s}^2$) must be reimplemented across every single directional permutation.
3. **Sensor Dropouts:** If a sensor fails, the entire procedural execution graph crashes unless fallback branches are manually coded.

### The Aven Solution: Relations
In Aven, you write the mathematical invariant once:

```aven
relation Mechanics {
    force = mass * acceleration
}
```

When observed inputs arrive (e.g., `force = 100`, `acceleration = 9.8`), Aven dynamically inspects the dependency graph, isolates the unknown (`mass`), and solves it analytically or via Newton-Raphson relaxation.

---

## 2. Zero-Dependency Pure Rust Philosophy

The entire `avenc` toolchain is written in standard library Rust (`std`) with **zero third-party crate dependencies**:

- **No Third-Party Parsers (Nom/Pest):** Custom recursive descent parser with precise span diagnostics.
- **No Third-Party Serialization (Serde):** Custom pure-`std` JSON tokenizer and deserializer.
- **No Third-Party Math/Optimization Libraries:** Custom symbolic differentiator and guarded Newton-Raphson relaxation engine.
- **No Third-Party Cryptography (Ring/Sha2):** Native FIPS 180-4 standard SHA-256 implementation in pure Rust.
- **No Third-Party Network Stacks (Tokio/Reqwest):** Native `std::net::TcpStream` and socket buffer management.

### Why Zero Dependencies?
1. **Auditable Security:** Eliminates supply-chain vulnerabilities, malicious package dependencies, and complex build trees.
2. **Instant Compilation:** Clean compile from scratch takes under 5 seconds.
3. **Cross-Platform Portability:** Compiles effortlessly across x86_64, aarch64, Windows, RHEL 9 Linux, macOS, and bare-metal targets.

---

## 3. Mathematical Solving Engine

Aven uses a dual-engine architecture to evaluate equations:

### Analytical Substitution
When an unknown variable appears linearly or can be inverted analytically (such as additions, subtractions, multiplications, divisions, powers, and radicals), Aven rearranges the equation tree directly:
- $y = x^2 \implies x = \sqrt{y}$
- $y = \text{ceil}(x / k) \implies$ invert through upper bound search

### Guarded Newton-Raphson Relaxation
When non-linear coupling occurs, Aven applies multivariate Newton-Raphson relaxation with automatic symbolic differentiation:

$$x_{n+1} = x_n - \frac{f(x_n)}{f'(x_n)}$$

To prevent IEEE-754 division-by-zero or square root domain violations, Aven relations incorporate mathematical domain guards:
```aven
safe_temp_k = ambient_temp_k + 0.0001
speed_of_sound_ms = sqrt(1.4 * 287.058 * safe_temp_k)
mach_number = velocity_ms / (speed_of_sound_ms + 0.0001)
```

---

## 4. Direction Pinning & C Code Generation (`avenc emit-c`)

While the relational solver is ideal for dynamic telemetry and exploration, safety-critical embedded systems (PLCs, automotive ECUs, flight controllers) cannot tolerate runtime solver relaxation.

Aven solves this via **Direction Pinning**:
1. When all input variables are fixed or declared in a `forward` block, `avenc` performs a topological sort on the equation dependency graph.
2. The compiler verifies there are zero circular dependencies.
3. `avenc emit-c` outputs clean, branchless, zero-overhead ANSI C code with no solver runtime.

```c
// Generated autonomously by avenc emit-c
double solve_server_sizing(double reqs_per_second, double avg_latency_ms) {
    double raw_workers = (reqs_per_second * avg_latency_ms) / 1000.0;
    double allocated_workers = ceil(raw_workers / 0.70);
    return allocated_workers * 20.0 + 4096.0;
}
```

---

## 5. Provenance & Freshness Enforcement

Every computed fact is stored in an immutable, append-only **Fact Ledger**:
- **SHA-256 Provenance Hashing:** Proves the cryptographic integrity of inputs and outputs.
- **Time-to-Live (`ttl_ms`):** Telemetry vectors carry freshness timeouts. If an upstream sensor stops reporting, `is_stale` is flagged, and Aven halts downstream decisions with `DECISION BLOCKED` / `HOLD` rather than executing on stale ghost data.
