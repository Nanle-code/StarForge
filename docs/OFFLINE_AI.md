# Offline-First AI Mode & Provider Parity

StarForge supports AI assistance through two families of providers:

- **Local (offline)** — a self-hosted [Ollama](https://ollama.ai) instance running
  on the developer's machine. No API keys, no network egress, no per-token cost.
- **Cloud** — hosted LLM APIs (OpenAI, Anthropic) that require an API key and
  network access.

Because **many contributors cannot use cloud AI** (no API keys, air-gapped
environments, or budget constraints), StarForge is **offline-first**: local
Ollama is a first-class, fully supported provider. This document is the
canonical **feature-parity matrix** and mode reference.

> This file is the documentation mirror of the authoritative registry in
> [`src/utils/ai_offline.rs`](../src/utils/ai_offline.rs). Keep the two in sync
> when AI features are added, removed, or reclassified.

---

## AI mode

StarForge runs in one of three configured modes, controlled by the
`$STARFORGE_AI_MODE` environment variable or the `starforge ai offline --mode <m>`
flag.

| Mode | Meaning |
|------|---------|
| `offline` | **Never** contact a cloud provider. Only local models are used. Cloud-only features fail clearly. |
| `online` | Cloud providers are allowed (secrets / keys required). |
| `auto` (default) | Offline when Ollama is reachable, otherwise online. |

`starforge ai offline` reports the configured and effective mode, whether
Ollama is running, and whether cloud access is allowed.

```sh
# Inspect the effective mode and the parity matrix (works fully offline)
starforge ai offline

# Force offline mode for this invocation of a cloud-only command
STARFORGE_AI_MODE=offline starforge generate contract "an NFT contract"

# Verify a command works in the current mode
starforge ai offline check generate

# Verify a model is installed locally
starforge ai offline check-model codellama:7b
```

---

## Feature-parity matrix

Rows are classified as:

- **Local** — works fully offline with a local Ollama model.
- **Hybrid** — works offline when a local model is present, otherwise routes to
  a cloud provider.
- **Cloud-only** — has **no** offline path; requires a cloud provider.

> Every row below is rendered by `starforge ai offline` and enforced by the
> offline guard at the top of each command handler.

| Command | Description | Support | Providers |
|---------|-------------|---------|-----------|
| `ai status` | Show Ollama installation and runtime status | Local | Ollama |
| `ai models` | List locally available models | Local | Ollama |
| `ai pull` | Download a model into the local store | Local | Ollama |
| `ai ask` | Ask the local LLM a free-form Soroban question | Local | Ollama |
| `ai audit` | AI security audit of a Soroban contract | Local | Ollama |
| `ai explain` | Plain-English explanation of a Soroban contract | Local | Ollama |
| `ai test` | Generate a test suite for a contract | Local | Ollama |
| `ai optimise` | Suggest gas optimisations for a contract | Local | Ollama |
| `ai profile` | AI-driven WASM performance profiling | Local | Ollama |
| `ai compare-profiles` | Comparative analysis of two profile snapshots | Local | Ollama |
| `ai patterns` | Pattern recognition and anti-pattern detection | Local | Ollama |
| `ai library` | Browse the built-in pattern library | Local | Ollama |
| `ai pattern-feedback` | Record feedback on a pattern result | Local | Ollama |
| `ai cache` | Manage the AI request cache | Local | Ollama |
| `ai analytics` | AI test analytics | Local | Database |
| `ai telemetry` | AI usage telemetry | Local | Database |
| `ai chat` | Interactive AI chat | Hybrid | Ollama / OpenAI / Anthropic |
| `ai test-gen` | Generate tests with AI | Hybrid | Ollama / OpenAI / Anthropic |
| `ai property-test` | AI-assisted property testing | Hybrid | Ollama / OpenAI / Anthropic |
| `ai recommend` | AI contract recommendations | Hybrid | Ollama / OpenAI / Anthropic |
| `ai search` | AI-assisted code search | Hybrid | Ollama / OpenAI / Anthropic |
| `ai plan` | AI project planning | Hybrid | Ollama / OpenAI / Anthropic |
| `ai feedback` | AI feedback collection and analysis | Hybrid | Ollama / OpenAI / Anthropic |
| `ai debug` | AI debugging assistant | Hybrid | Ollama / OpenAI / Anthropic |
| `ai accessibility` | AI accessibility features | Hybrid | Ollama / OpenAI / Anthropic |
| `ai-model route` | Route a task to the optimal provider and model | Hybrid | Ollama / OpenAI / Anthropic |
| `ai audit-service` | AI security audit service with static offline analyses | Hybrid | Ollama / OpenAI / Anthropic |
| `generate` | Generate a contract from a natural-language prompt | **Cloud-only** | OpenAI |
| `explain` | Explain a contract using AI | **Cloud-only** | OpenAI |

---

## What works offline

In **offline mode** (`STARFORGE_AI_MODE=offline`, or `auto` with Ollama running)
the entire **Local** column above works without any network or API key, provided
you have installed Ollama and pulled the needed model:

```sh
# One-time setup
ollama serve          # or install the desktop app which autostarts it
starforge ai pull codellama:7b

# These all work with no cloud provider
starforge ai status
starforge ai ask "how do I persist state in Soroban?"
starforge ai audit contract.rs
starforge ai explain contract.rs
starforge ai test contract.rs
starforge ai optimise contract.rs
```

---

## What fails clearly offline

Cloud-only features (`generate`, `explain`) have no offline path. When offline
mode is active they **fail fast with a clear, actionable error** instead of a
confusing network timeout:

```text
'generate' is a cloud-only AI feature and is unavailable in offline mode.

Offline mode only talks to a local Ollama instance and never contacts a cloud
provider.

To proceed you can either:
 1. Run with cloud access:    starforge ai offline --mode online
 2. Or unset $STARFORGE_AI_MODE when cloud providers are allowed.

Run `starforge ai offline` to see which commands are available offline.
```

The exit code is a normal failure (exit 1) so CI and scripts can branch on it.

---

## Unavailable-model errors

Before local inference can run, the requested model must exist in the local
Ollama store. If it does not, the error is equally explicit:

```text
Model 'mistral' is not available in the local Ollama store.

Pull it first with:
  starforge ai pull mistral

Or list the installed models with `starforge ai models` and pick one of them.
```

Check a model directly:

```sh
starforge ai offline check-model codellama:7b   # ok
starforge ai offline check-model mistral        # fails clearly
```

---

## Design notes

- **Single source of truth.** The parity matrix lives in
  `src/utils/ai_offline.rs` and is the only place features are classified.
- **Offline is the safe default.** `Offline` mode never leaks a request to a
  cloud provider; `Auto` prefers local when available.
- **Bare-name model matching.** `check-model codellama` matches any installed
  `codellama:*`; `check-model codellama:13b` requires that exact tag.
