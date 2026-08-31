import { Request, Response, NextFunction } from "express";

interface RateLimitRecord {
  count: number;
  resetTime: number;
}

const store = new Map<string, RateLimitRecord>();

function getEnvNumber(val: string | undefined, defaultVal: number): number {
  if (!val) return defaultVal;
  const parsed = parseInt(val, 10);
  if (isNaN(parsed) || parsed <= 0) return defaultVal;
  return parsed;
}

function sanitizeNumber(val: number | undefined, defaultVal: number): number {
  if (val === undefined || isNaN(val) || val <= 0) return defaultVal;
  return val;
}

export interface RateLimiterOptions {
  windowMs?: number;
  max?: number;
}

export const createRateLimiter = (options?: RateLimiterOptions) => {
  return (req: Request, res: Response, next: NextFunction) => {
    const windowMs = sanitizeNumber(
      options?.windowMs,
      getEnvNumber(process.env.PUBLISH_RATE_LIMIT_WINDOW_MS, 60000),
    );
    const max = sanitizeNumber(
      options?.max,
      getEnvNumber(process.env.PUBLISH_RATE_LIMIT_MAX, 10),
    );

    const identifier = req.userId || req.ip || "anonymous";
    const now = Date.now();

    let record = store.get(identifier);

    if (!record || now >= record.resetTime) {
      record = {
        count: 0,
        resetTime: now + windowMs,
      };
    }

    record.count += 1;
    store.set(identifier, record);

    const remaining = Math.max(0, max - record.count);
    const resetTimeSeconds = Math.ceil(record.resetTime / 1000);
    const retryAfterSeconds = Math.ceil((record.resetTime - now) / 1000);

    res.setHeader("X-RateLimit-Limit", max.toString());
    res.setHeader("X-RateLimit-Remaining", remaining.toString());
    res.setHeader("X-RateLimit-Reset", resetTimeSeconds.toString());

    if (record.count > max) {
      res.setHeader("Retry-After", retryAfterSeconds.toString());
      return res.status(429).json({
        error: "Rate limit exceeded. Too many mutation requests.",
        retryAfter: retryAfterSeconds,
      });
    }

    next();
  };
};

export const resetRateLimiterStore = () => {
  store.clear();
};

export const mutationRateLimiter = createRateLimiter();
