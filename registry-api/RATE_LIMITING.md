# Registry API rate limiting

The registry applies fair-use limits to its publish and search endpoints so
that one client cannot starve everyone else. This page is the contract for
API clients: which endpoints are limited, how callers are identified, what the
limits are, and how to back off.

The implementation is `src/middleware/rateLimiter.ts`.

## Limited endpoints

| Policy | Endpoints | Authenticated caller | Anonymous caller |
|---|---|---|---|
| `publish` | `POST /api/templates/publish`, `POST /api/templates/:name/transfer-ownership`, `POST /api/templates/:name/transfer-ownership/confirm` | 10 requests per 60 s per user | n/a (these endpoints require a token) |
| `search` | `POST /api/templates/search`, `GET /api/templates/search/suggestions` | 60 requests per 60 s per user | 30 requests per 60 s per IP address |

Endpoints in the same policy share one quota per client. Search and
suggestions draw from the same `search` bucket, and publishing and ownership
transfers draw from the same `publish` bucket. The two policies are
independent: exhausting one does not affect the other. No other endpoint is
rate limited by this middleware.

### How the limit works

Each client has a **token bucket** per policy. The bucket holds up to the
limit (for example 10 for `publish`), each request spends one token, and
tokens refill continuously at the limit per 60 seconds (one `publish` token
every 6 seconds, one anonymous `search` token every 2 seconds). A client can
burst up to the limit, and can then keep going at the refill rate. After 60
idle seconds the bucket is full again.

### Configuration

Limits are set with environment variables. Invalid values (non-numeric, zero
or negative) are ignored and the default applies.

| Variable | Default | Meaning |
|---|---|---|
| `PUBLISH_RATE_LIMIT_MAX` | `10` | `publish` bucket size and tokens refilled per window |
| `PUBLISH_RATE_LIMIT_WINDOW_MS` | `60000` | `publish` refill window |
| `SEARCH_RATE_LIMIT_MAX` | `60` | `search` bucket for authenticated callers |
| `SEARCH_RATE_LIMIT_ANON_MAX` | `30` | `search` bucket for anonymous callers, per IP |
| `SEARCH_RATE_LIMIT_WINDOW_MS` | `60000` | `search` refill window |

The defaults are deliberately conservative. Publishing is a rare, heavyweight
operation, so 10 per minute leaves room for scripted multi-template releases
without allowing floods. Search is interactive, so authenticated callers get
one request per second on average, and anonymous callers get half of that
because many of them can share one address.

## How clients are identified

| Request | Charged to |
|---|---|
| Valid `Authorization: Bearer <token>` | The user id in the token (`user:<id>`) |
| No token, or an invalid or expired token on a search endpoint | The client IP address (`ip:<address>`) |
| No usable address | One shared `ip:unknown` bucket |

- **Authenticated callers are limited per user**, not per address, so users
  behind the same NAT or office proxy do not share a quota. Signing in on a
  search endpoint moves a caller from the per-IP anonymous bucket to their own,
  larger, user bucket.
- **Anonymous callers are limited per IP address.** The address is the TCP
  peer address of the connection (Express `req.ip` without `trust proxy`).
  `X-Forwarded-For`, `Forwarded` and similar headers are **not** trusted,
  because any client can set them, and trusting them would let a client get a
  fresh quota on every request.
- An invalid token never creates a new identity; the request falls back to the
  IP bucket.
- User and IP buckets are namespaced, so a user id can never share a bucket
  with an address.

### Deployment notes

- **Reverse proxies.** If the API runs behind a load balancer or reverse
  proxy, every anonymous request arrives from the proxy's address and all
  anonymous clients share one search bucket. Authenticated limits are not
  affected. The application does not currently enable Express's
  `trust proxy`. To limit anonymous clients individually behind a proxy, that
  setting has to be configured for the specific trusted proxy hops. Do not
  enable it for arbitrary hops, or `X-Forwarded-For` becomes spoofable.
- **Multiple processes.** Buckets live in memory in each API process. With N
  instances behind a load balancer, a client can receive up to N times the
  configured limit. A shared store would be needed for exact global limits.
- **Memory.** Each limiter tracks at most 10,000 clients. A full bucket is
  identical to a new one, so idle clients are dropped first; if every tracked
  client is mid-burst, the least recently seen are evicted.

## Response headers

Every response from a limited endpoint, successful or not, carries:

| Header | Value |
|---|---|
| `RateLimit-Limit` | Bucket size that applies to this caller (for example `10`, or `30` for anonymous search) |
| `RateLimit-Remaining` | Whole requests the caller can make right now without waiting |
| `RateLimit-Reset` | Seconds until the bucket is completely refilled, that is, until `RateLimit-Remaining` would equal `RateLimit-Limit` if the caller sent nothing more |
| `RateLimit-Policy` | `<limit>;w=<window seconds>`, for example `10;w=60` |

These follow the field names of the IETF HTTPAPI *RateLimit header fields*
draft, with `RateLimit-Reset` as delta-seconds.

For compatibility with existing clients the API also sends
`X-RateLimit-Limit`, `X-RateLimit-Remaining` and `X-RateLimit-Reset`. The first
two match the `RateLimit-*` values. `X-RateLimit-Reset` is the same reset
moment as an absolute Unix time in seconds. New clients should use the
`RateLimit-*` headers.

The headers are listed in the CORS `Access-Control-Expose-Headers`, so browser
clients can read them.

## When the limit is exceeded

The request is rejected **before** it is processed (nothing is published and
no search runs) with:

```http
HTTP/1.1 429 Too Many Requests
Retry-After: 6
RateLimit-Limit: 10
RateLimit-Remaining: 0
RateLimit-Reset: 60
RateLimit-Policy: 10;w=60
Content-Type: application/json

{"error":"Rate limit exceeded. Too many publish requests.","retryAfter":6}
```

- `Retry-After` is the number of seconds until the **next single request**
  will be accepted (always at least 1). It is also returned as `retryAfter` in
  the body.
- The body follows the API's usual `{ "error": "..." }` shape.

## How clients should back off

1. **Read the headers.** Treat `RateLimit-Remaining` as a budget. Slow down
   before it reaches 0 rather than after.
2. **On `429`, wait at least `Retry-After` seconds** before sending another
   request to the same policy. Never retry sooner. The request will be
   rejected again and still counted against you.
3. **If you get `429` again, back off exponentially with jitter.** For example,
   wait `max(Retry-After, min(cap, base * 2^attempt))` seconds multiplied by a
   random factor between 0.5 and 1, with `base = 1` and `cap = 60`.
4. **Give up after a bounded number of attempts** (for example 5) and surface
   the error, instead of retrying forever.
5. **Do not parallelise around the limit.** Concurrent requests draw from the
   same bucket; opening more connections does not add quota.
6. **Authenticate for search** if you run an integration that searches
   regularly. Authenticated callers get their own, larger, bucket instead of
   sharing an address-based one.
7. **Debounce type-ahead** calls to `/search/suggestions`. They count against
   the same bucket as full searches.

Tight retry loops that ignore these headers mostly generate 429 responses, and
they use up capacity the registry could spend on other clients. When many
clients retry at the same moment, jitter spreads the retries out, so they do
not all collide again.

Example (TypeScript):

```ts
async function withBackoff(send: () => Promise<Response>, maxAttempts = 5) {
  for (let attempt = 0; ; attempt++) {
    const res = await send();
    if (res.status !== 429 || attempt + 1 >= maxAttempts) return res;

    const retryAfter = Number(res.headers.get("Retry-After") ?? "1");
    const backoff = Math.min(60, 2 ** attempt);
    const waitSeconds = Math.max(retryAfter, backoff * (0.5 + Math.random() / 2));
    await new Promise((resolve) => setTimeout(resolve, waitSeconds * 1000));
  }
}
```
