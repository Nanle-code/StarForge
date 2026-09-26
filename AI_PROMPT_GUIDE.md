# AI Prompt Engineering Guide

This guide describes the prompt design methodology, system templates, and prompt engineering best practices used by StarForge.

---

## 1. Overview

StarForge leverages carefully engineered prompts to ensure that underlying LLMs generate syntactically correct, gas-optimized, and secure Soroban smart contract code, as well as highly structured documentation. 

Every prompt incorporates context injection, role definition, formatting constraints, and domain-specific knowledge about Stellar's Soroban SDK (such as `Env`, `Address`, storage options, and testing frameworks).

---

## 2. Core System Prompts

StarForge defines system contexts that configure the behavior of the model before accepting user instructions.

### A. Soroban System Context (`ollama.rs`)
Used as the foundational header for local AI assistant tasks:
```rust
You are an expert Stellar and Soroban smart-contract developer assistant integrated into the StarForge CLI. Help developers write, review, audit, and optimise Soroban contracts written in Rust. Produce idiomatic Rust that compiles with `soroban-sdk`. Keep answers concise and actionable.
```

### B. Interactive Contract Generator System Context (`generate.rs`)
Instructs the LLM to skip descriptive conversation and return raw compilable code:
```rust
You are an expert Soroban smart contract developer. Write ONLY valid, compilable Rust code for Soroban. Include `#![no_std]`, proper `#[contract]`, `#[contractimpl]`, and `#[contracttype]` macros. Include helpful comments and basic test scaffolding when appropriate. Do NOT wrap your response in markdown code fences — output only the raw code.
```

### C. Documentation Enrichment System Context (`ai_docs.rs`)
Forces the LLM to respond in a strict JSON format matching the engine's internal structure:
```rust
You enrich Soroban smart-contract documentation. 
Return JSON with keys: architecture (string), security (string), 
functions (array of {name, description, examples}). 
Do not invent ABI members that are not provided. Keep examples accurate.
```

---

## 3. Dedicated Task Templates

StarForge includes predefined prompt wrappers for specialized auditing, testing, and optimization tasks.

### A. Security Auditing Prompt
Identifies vulnerabilities, storage inefficiencies, and authorization defects:
```markdown
[System Context]
Audit the following Soroban contract for security vulnerabilities, 
storage inefficiencies, and potential exploits. List each issue with its 
severity (Critical / High / Medium / Low) and a recommended fix.

```rust
[CONTRACT_CODE]
```
```

### B. Plain-English Contract Explainer
Extracts the storage layout model and functional behaviors:
```markdown
[System Context]
Explain what the following Soroban smart contract does in plain English. 
Include a summary of its storage model, entry-point functions, and any notable design 
patterns.

```rust
[CONTRACT_CODE]
```
```

### C. Test Suite Scaffolding Prompt
Generates tests targeting the official Soroban SDK testing framework:
```markdown
[System Context]
Generate a comprehensive test suite for the following Soroban contract 
using the soroban-sdk testing harness. Cover happy paths, edge cases, and failure 
conditions.

```rust
[CONTRACT_CODE]
```
```

### D. Gas Optimization Prompt
Rewrites logic to minimize CPU instructions, transaction sizing, and storage fees:
```markdown
[System Context]
Identify gas optimisation opportunities in the following Soroban contract 
and rewrite it to minimise resource consumption while preserving behaviour.

```rust
[CONTRACT_CODE]
```
```

---

## 4. Prompt Engineering Best Practices

When adding new prompts or modifying assistants, follow these rules:

1. **Zero-Response Conversational Filler**: For contract generation, instruct the LLM to emit raw Rust code without greetings, explanations, or code-block ticks to prevent syntax errors when saving files.
2. **Explicit Dependency Versions**: Ensure prompts emphasize compatibility with current `soroban-sdk` versions (e.g., proper namespace usages, updated storage APIs like `env.storage().instance()`).
3. **Structured Formats over Prose**: If the output is parsed programmatically, demand JSON formats and supply a clear template schema inside the system instructions.
4. **Iterative Context Retention**: When building agents, pass the user's previous requests and generated previews back to the model as conversational history to permit cumulative changes.
5. **No Hallucinations on the API/ABI**: Never let the LLM guess function names. In the doc enricher, we strictly supply the extracted functions list and instruct the model not to add or modify function signatures.
