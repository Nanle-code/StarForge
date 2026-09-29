import { Request, Response, NextFunction } from "express";

/**
 * Fair-use rate limiting for the registry's publish and search endpoints.
 *
 * Each limiter is a token bucket per client: the bucket holds up to `max`
 * tokens, every request spends one, and tokens refill continuously at
 * `max` per `windowMs`. A client that stays idle for a full window is back to
 * `max` requests; a client that bursts is limited to the refill rate after
 * that. See registry-api/RATE_LIMITING.md for the client-facing contract.
 *
 * State is kept in memory, per process. The limiter is synchronous, so each
 * check-and-spend runs to completion before Node handles another request and
 * concurrent requests cannot overspend a bucket.
 */

export interface RateLimiterOptions {
  /** Policy name used in error messages and `RateLimit-Policy`. */
  name?: string;
  /** Refill window in milliseconds. */
  windowMs?: number;
  /** Bucket capacity, and tokens refilled per window, for authenticated callers. */
  max?: number;
  /** Bucket capacity for anonymous (IP-identified) callers. Defaults to `max`. */
  anonymousMax?: number;
  /** Upper bound on buckets kept in memory. */
  maxTrackedClients?: number;
  /** Clock, injectable for tests. */
  now?: () => number;
}

interface Bucket {
  tokens: number;
  updatedAt: number;
}

export interface ClientIdentity {
  /** Bucket key, namespaced so a user id can never collide with an IP. */
  key: string;
  authenticated: boolean;
}

const DEFAULT_WINDOW_MS = 60_000;
const DEFAULT_PUBLISH_MAX = 10;
const DEFAULT_SEARCH_MAX = 60;
const DEFAULT_SEARCH_ANON_MAX = 30;
const DEFAULT_MAX_TRACKED_CLIENTS = 10_000;

function getEnvNumber(val: string | undefined, defaultVal: number): number {
  if (!val) return defaultVal;
  const parsed = parseInt(val, 10);
  if (isNaN(parsed) || parsed <= 0) return defaultVal;
  return parsed;
}

function sanitizeNumber(val: number | undefined, defaultVal: number): number {
  if (val === undefined || !Number.isFinite(val) || val <= 0) return defaultVal;
  return val;
}

/**
 * Who a request is charged to.
 *
 * - Authenticated requests (a JWT already verified by `verifyToken` or
 *   `optionalAuth`) are charged to the user id.
 * - Everything else is charged to `req.ip`. The app does not enable Express's
 *   `trust proxy`, so this is the TCP peer address and client-supplied
 *   `X-Forwarded-For` headers cannot change it.
 * - If no address is available the request is charged to one shared
 *   `ip:unknown` bucket, so missing identity never bypasses the limit.
 */
export function identifyClient(req: Request): ClientIdentity {
  if (typeof req.userId === "string" && req.userId.length > 0) {
    return { key: `user:${req.userId}`, authenticated: true };
  }
  const ip = typeof req.ip === "string" && req.ip.length > 0 ? req.ip : "unknown";
  return { key: `ip:${ip}`, authenticated: false };
}

const limiterStores = new Set<Map<string, Bucket>>();

export const createRateLimiter = (options?: RateLimiterOptions) => {
  const name = options?.name ?? "publish";
  const windowMs = sanitizeNumber(
    options?.windowMs,
    getEnvNumber(process.env.PUBLISH_RATE_LIMIT_WINDOW_MS, DEFAULT_WINDOW_MS),
  );
  const max = Math.floor(
    sanitizeNumber(options?.max, getEnvNumber(process.env.PUBLISH_RATE_LIMIT_MAX, DEFAULT_PUBLISH_MAX)),
  );
  const anonymousMax = Math.floor(sanitizeNumber(options?.anonymousMax, max));
  const maxTrackedClients = Math.floor(
    sanitizeNumber(options?.maxTrackedClients, DEFAULT_MAX_TRACKED_CLIENTS),
  );
  const now = options?.now ?? (() => Date.now());
  const windowSeconds = Math.ceil(windowMs / 1000);

  const buckets = new Map<string, Bucket>();
  limiterStores.add(buckets);

  const refill = (bucket: Bucket, capacity: number, at: number) => {
    const elapsed = Math.max(0, at - bucket.updatedAt);
    bucket.tokens = Math.min(capacity, bucket.tokens + (elapsed * capacity) / windowMs);
    bucket.updatedAt = at;
  };

  // A full bucket is indistinguishable from a new one, so dropping it loses
  // nothing. When the table is full, drop every full bucket; if that is not
  // enough, evict least recently used buckets down to 90% of the cap so the
  // sweep is not repeated for every new client during a flood.
  const makeRoom = (at: number) => {
    if (buckets.size < maxTrackedClients) return;
    for (const [key, bucket] of buckets) {
      const capacity = key.startsWith("user:") ? max : anonymousMax;
      refill(bucket, capacity, at);
      if (bucket.tokens >= capacity) buckets.delete(key);
    }
    if (buckets.size < maxTrackedClients) return;
    const target = Math.floor(maxTrackedClients * 0.9);
    while (buckets.size > target) {
      const oldest = buckets.keys().next().value;
      if (oldest === undefined) break;
      buckets.delete(oldest);
    }
  };

  return (req: Request, res: Response, next: NextFunction) => {
    const at = now();
    const identity = identifyClient(req);
    const capacity = identity.authenticated ? max : anonymousMax;
    const ratePerMs = capacity / windowMs;

    let bucket = buckets.get(identity.key);
    if (bucket) {
      // Re-insert so Map iteration order is least recently used first.
      buckets.delete(identity.key);
      refill(bucket, capacity, at);
    } else {
      makeRoom(at);
      bucket = { tokens: capacity, updatedAt: at };
    }
    buckets.set(identity.key, bucket);

    const allowed = bucket.tokens >= 1;
    if (allowed) bucket.tokens -= 1;

    const remaining = Math.floor(bucket.tokens);
    // Seconds until the bucket is full again, i.e. until `remaining` would
    // equal the limit if the client sent nothing more.
    const resetSeconds = Math.ceil((capacity - bucket.tokens) / ratePerMs / 1000);

    res.setHeader("RateLimit-Limit", capacity.toString());
    res.setHeader("RateLimit-Remaining", remaining.toString());
    res.setHeader("RateLimit-Reset", resetSeconds.toString());
    res.setHeader("RateLimit-Policy", `${capacity};w=${windowSeconds}`);
    // Legacy headers, kept for existing clients. X-RateLimit-Reset is an
    // absolute Unix time in seconds.
    res.setHeader("X-RateLimit-Limit", capacity.toString());
    res.setHeader("X-RateLimit-Remaining", remaining.toString());
    res.setHeader("X-RateLimit-Reset", Math.ceil(at / 1000 + resetSeconds).toString());

    if (!allowed) {
      // Seconds until one whole token is available again.
      const retryAfterSeconds = Math.max(1, Math.ceil((1 - bucket.tokens) / ratePerMs / 1000));
      res.setHeader("Retry-After", retryAfterSeconds.toString());
      return res.status(429).json({
        error: `Rate limit exceeded. Too many ${name} requests.`,
        retryAfter: retryAfterSeconds,
      });
    }

    next();
  };
};

/** Clears every limiter's buckets. Used by the test suite. */
export const resetRateLimiterStore = () => {
  limiterStores.forEach((store) => store.clear());
};

/**
 * Publish policy: template publish and ownership-transfer mutations share
 * one bucket per publisher. These routes require authentication.
 * Defaults: 10 requests per 60 s (PUBLISH_RATE_LIMIT_MAX,
 * PUBLISH_RATE_LIMIT_WINDOW_MS).
 */
export const publishRateLimiter = createRateLimiter({ name: "publish" });

/** Previous name of the publish limiter, kept for existing imports. */
export const mutationRateLimiter = publishRateLimiter;

/**
 * Search policy: template search and search suggestions. Defaults: 60
 * requests per 60 s for authenticated callers (SEARCH_RATE_LIMIT_MAX) and 30
 * per 60 s per IP for anonymous callers (SEARCH_RATE_LIMIT_ANON_MAX), over
 * SEARCH_RATE_LIMIT_WINDOW_MS.
 */
export const searchRateLimiter = createRateLimiter({
  name: "search",
  windowMs: getEnvNumber(process.env.SEARCH_RATE_LIMIT_WINDOW_MS, DEFAULT_WINDOW_MS),
  max: getEnvNumber(process.env.SEARCH_RATE_LIMIT_MAX, DEFAULT_SEARCH_MAX),
  anonymousMax: getEnvNumber(process.env.SEARCH_RATE_LIMIT_ANON_MAX, DEFAULT_SEARCH_ANON_MAX),
});
