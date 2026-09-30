import request from "supertest";
import { Request, Response, NextFunction } from "express";
import app from "../index";
import {
  createRateLimiter,
  identifyClient,
  resetRateLimiterStore,
} from "../middleware/rateLimiter";
import { templateStore } from "../routes/templates";
import { userStore } from "../models/User";

// ---------------------------------------------------------------------------
// Helpers for driving a limiter directly with a fake clock
// ---------------------------------------------------------------------------

interface Outcome {
  status: number;
  headers: Record<string, string>;
  body: any;
  nextCalled: boolean;
}

function call(
  limiter: ReturnType<typeof createRateLimiter>,
  req: Partial<Request>,
): Outcome {
  const outcome: Outcome = { status: 200, headers: {}, body: null, nextCalled: false };
  const res = {
    setHeader: (k: string, v: string) => {
      outcome.headers[k] = v;
    },
    status: (code: number) => {
      outcome.status = code;
      return {
        json: (body: any) => {
          outcome.body = body;
        },
      };
    },
  } as unknown as Response;
  const next: NextFunction = () => {
    outcome.nextCalled = true;
  };
  limiter(req as Request, res, next);
  return outcome;
}

function fakeClock(start = 1_700_000_000_000) {
  let t = start;
  return {
    now: () => t,
    advance: (ms: number) => {
      t += ms;
    },
  };
}

// ---------------------------------------------------------------------------
// Limiter behaviour
// ---------------------------------------------------------------------------

describe("Token bucket rate limiter", () => {
  it("allows requests under the limit and decrements remaining quota", () => {
    const clock = fakeClock();
    const limiter = createRateLimiter({ name: "test", max: 3, windowMs: 60_000, now: clock.now });
    const req = { userId: "alice", ip: "10.0.0.1" };

    const remaining = [0, 1, 2].map(() => {
      const out = call(limiter, req);
      expect(out.nextCalled).toBe(true);
      expect(out.headers["RateLimit-Limit"]).toBe("3");
      expect(out.headers["RateLimit-Policy"]).toBe("3;w=60");
      return out.headers["RateLimit-Remaining"];
    });

    expect(remaining).toEqual(["2", "1", "0"]);
  });

  it("returns 429 with fair-use headers and does not call the handler once exhausted", () => {
    const clock = fakeClock();
    const limiter = createRateLimiter({ name: "test", max: 3, windowMs: 60_000, now: clock.now });
    const req = { userId: "alice", ip: "10.0.0.1" };
    for (let i = 0; i < 3; i++) call(limiter, req);

    const blocked = call(limiter, req);

    expect(blocked.nextCalled).toBe(false);
    expect(blocked.status).toBe(429);
    expect(blocked.body).toEqual({
      error: "Rate limit exceeded. Too many test requests.",
      retryAfter: 20,
    });
    // One token every 20 s; three tokens (a full bucket) in 60 s.
    expect(blocked.headers["Retry-After"]).toBe("20");
    expect(blocked.headers["RateLimit-Limit"]).toBe("3");
    expect(blocked.headers["RateLimit-Remaining"]).toBe("0");
    expect(blocked.headers["RateLimit-Reset"]).toBe("60");
    expect(blocked.headers["X-RateLimit-Reset"]).toBe(
      String(Math.ceil(clock.now() / 1000) + 60),
    );
  });

  it("reports Retry-After as the time until the next token, not the full window", () => {
    const clock = fakeClock();
    const limiter = createRateLimiter({ max: 3, windowMs: 60_000, now: clock.now });
    const req = { userId: "alice" };
    for (let i = 0; i < 3; i++) call(limiter, req);

    clock.advance(15_000); // three quarters of one token has refilled
    const blocked = call(limiter, req);

    expect(blocked.status).toBe(429);
    expect(blocked.headers["Retry-After"]).toBe("5");
    expect(blocked.headers["RateLimit-Reset"]).toBe("45");
  });

  it("replenishes tokens over time so a client can make requests again", () => {
    const clock = fakeClock();
    const limiter = createRateLimiter({ max: 3, windowMs: 60_000, now: clock.now });
    const req = { userId: "alice" };
    for (let i = 0; i < 3; i++) call(limiter, req);
    expect(call(limiter, req).status).toBe(429);

    // Exactly Retry-After later, one request is allowed again.
    clock.advance(20_000);
    const afterOneToken = call(limiter, req);
    expect(afterOneToken.nextCalled).toBe(true);
    expect(afterOneToken.headers["RateLimit-Remaining"]).toBe("0");

    // A full idle window restores the whole burst, and never more than it.
    clock.advance(10 * 60_000);
    const afterWindow = call(limiter, req);
    expect(afterWindow.nextCalled).toBe(true);
    expect(afterWindow.headers["RateLimit-Remaining"]).toBe("2");
    expect(afterWindow.headers["RateLimit-Reset"]).toBe("20");
  });

  it("keeps separate buckets for different authenticated users", () => {
    const clock = fakeClock();
    const limiter = createRateLimiter({ max: 1, windowMs: 60_000, now: clock.now });

    expect(call(limiter, { userId: "alice", ip: "10.0.0.1" }).status).toBe(200);
    expect(call(limiter, { userId: "alice", ip: "10.0.0.1" }).status).toBe(429);
    // Same IP, different user: independent bucket.
    expect(call(limiter, { userId: "bob", ip: "10.0.0.1" }).status).toBe(200);
  });

  it("limits anonymous callers per IP with the anonymous capacity", () => {
    const clock = fakeClock();
    const limiter = createRateLimiter({
      max: 5,
      anonymousMax: 2,
      windowMs: 60_000,
      now: clock.now,
    });

    const first = call(limiter, { ip: "10.0.0.1" });
    expect(first.headers["RateLimit-Limit"]).toBe("2");
    expect(call(limiter, { ip: "10.0.0.1" }).status).toBe(200);
    expect(call(limiter, { ip: "10.0.0.1" }).status).toBe(429);
    expect(call(limiter, { ip: "10.0.0.2" }).status).toBe(200);
    // Signing in moves the caller to its own, larger, user bucket.
    const signedIn = call(limiter, { userId: "alice", ip: "10.0.0.1" });
    expect(signedIn.status).toBe(200);
    expect(signedIn.headers["RateLimit-Limit"]).toBe("5");
  });

  it("does not let a user id collide with an IP bucket", () => {
    expect(identifyClient({ userId: "10.0.0.1" } as Request).key).toBe("user:10.0.0.1");
    expect(identifyClient({ ip: "10.0.0.1" } as Request).key).toBe("ip:10.0.0.1");
  });

  it("charges requests without any identity to one shared bucket instead of skipping the limit", () => {
    const clock = fakeClock();
    const limiter = createRateLimiter({ max: 2, windowMs: 60_000, now: clock.now });

    expect(identifyClient({} as Request)).toEqual({ key: "ip:unknown", authenticated: false });
    expect(call(limiter, {}).status).toBe(200);
    expect(call(limiter, { ip: "" }).status).toBe(200);
    expect(call(limiter, {}).status).toBe(429);
  });

  it("falls back to safe defaults for invalid configuration", () => {
    const limiter = createRateLimiter({
      windowMs: Number.NaN,
      max: 0,
      anonymousMax: -1,
      maxTrackedClients: -10,
    });
    const out = call(limiter, { ip: "10.0.0.1" });

    expect(out.nextCalled).toBe(true);
    expect(out.headers["RateLimit-Limit"]).toBe("10");
    expect(out.headers["RateLimit-Policy"]).toBe("10;w=60");
  });

  it("bounds memory by evicting least recently used buckets", () => {
    const clock = fakeClock();
    const limiter = createRateLimiter({
      max: 1,
      windowMs: 60_000,
      maxTrackedClients: 2,
      now: clock.now,
    });

    expect(call(limiter, { ip: "10.0.0.1" }).status).toBe(200);
    expect(call(limiter, { ip: "10.0.0.1" }).status).toBe(429);
    // Two more clients push 10.0.0.1 out of the table...
    call(limiter, { ip: "10.0.0.2" });
    call(limiter, { ip: "10.0.0.3" });
    // ...so it starts again from a fresh bucket.
    expect(call(limiter, { ip: "10.0.0.1" }).status).toBe(200);
  });
});

// ---------------------------------------------------------------------------
// Endpoint wiring
// ---------------------------------------------------------------------------

describe("Registry publish/search rate limiting", () => {
  let token: string;
  let otherToken: string;
  // Frozen clock so refill never happens between requests unless a test
  // advances it explicitly.
  let now: number;

  const signup = async (username: string) => {
    const res = await request(app)
      .post("/api/auth/signup")
      .send({ email: `${username}@example.com`, username, password: "password123" });
    return res.body.token as string;
  };

  const publish = (authToken: string, name: string) =>
    request(app)
      .post("/api/templates/publish")
      .set("Authorization", `Bearer ${authToken}`)
      .send({
        name,
        version: "1.0.0",
        description: "Rate limit test",
        author: "Tester",
        content: Buffer.from("content").toString("base64"),
      });

  const search = () => request(app).post("/api/templates/search").send({ query: "token" });

  beforeEach(async () => {
    now = Date.now();
    jest.spyOn(Date, "now").mockImplementation(() => now);
    resetRateLimiterStore();
    await templateStore.clear();
    await userStore.clear();
    token = await signup("rl-alice");
    otherToken = await signup("rl-bob");
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it("applies the publish policy with standard headers and blocks the 11th publish", async () => {
    for (let i = 0; i < 10; i++) {
      const res = await publish(token, `rl-template-${i}`);
      expect(res.status).toBe(201);
      expect(res.headers["ratelimit-limit"]).toBe("10");
      expect(res.headers["ratelimit-remaining"]).toBe(String(9 - i));
      expect(res.headers["ratelimit-policy"]).toBe("10;w=60");
    }

    const blocked = await publish(token, "rl-template-over");
    expect(blocked.status).toBe(429);
    expect(blocked.body.error).toBe("Rate limit exceeded. Too many publish requests.");
    expect(blocked.headers["ratelimit-limit"]).toBe("10");
    expect(blocked.headers["ratelimit-remaining"]).toBe("0");
    // 10 per 60 s: the next token is 6 s away, a full bucket 60 s away.
    expect(blocked.headers["retry-after"]).toBe("6");
    expect(blocked.headers["ratelimit-reset"]).toBe("60");
    expect(blocked.body.retryAfter).toBe(6);

    // The rejected publish was never processed.
    const lookup = await request(app).get("/api/templates/rl-template-over");
    expect(lookup.status).toBe(404);

    // Another publisher is unaffected.
    expect((await publish(otherToken, "rl-bob-template")).status).toBe(201);
  });

  it("limits anonymous search per IP and ignores spoofed X-Forwarded-For", async () => {
    for (let i = 0; i < 30; i++) {
      const res = await search();
      expect(res.status).toBe(200);
      expect(res.headers["ratelimit-limit"]).toBe("30");
    }

    const blocked = await search();
    expect(blocked.status).toBe(429);
    expect(blocked.body.error).toBe("Rate limit exceeded. Too many search requests.");
    expect(blocked.headers["retry-after"]).toBe("2");
    expect(blocked.headers["ratelimit-remaining"]).toBe("0");
    expect(blocked.headers["ratelimit-reset"]).toBe("60");

    // A client cannot get a fresh bucket by claiming another address.
    const spoofed = await search().set("X-Forwarded-For", "203.0.113.7");
    expect(spoofed.status).toBe(429);

    // Suggestions share the search bucket.
    const suggestions = await request(app).get("/api/templates/search/suggestions?q=to");
    expect(suggestions.status).toBe(429);

    // An invalid token is treated as anonymous, not as a new identity.
    const badToken = await search().set("Authorization", "Bearer not-a-real-token");
    expect(badToken.status).toBe(429);
  });

  it("gives authenticated searchers their own larger bucket", async () => {
    for (let i = 0; i < 30; i++) await search();
    expect((await search()).status).toBe(429);

    const res = await search().set("Authorization", `Bearer ${token}`);
    expect(res.status).toBe(200);
    expect(res.headers["ratelimit-limit"]).toBe("60");
    expect(res.headers["ratelimit-remaining"]).toBe("59");
  });

  it("keeps publish and search quotas independent", async () => {
    for (let i = 0; i < 10; i++) await publish(token, `rl-indep-${i}`);
    expect((await publish(token, "rl-indep-over")).status).toBe(429);

    const res = await search().set("Authorization", `Bearer ${token}`);
    expect(res.status).toBe(200);
    expect(res.headers["ratelimit-remaining"]).toBe("59");
  });

  it("lets a client search again after its bucket replenishes", async () => {
    for (let i = 0; i < 30; i++) await search();
    const blocked = await search();
    expect(blocked.status).toBe(429);

    now += Number(blocked.headers["retry-after"]) * 1000;
    const retried = await search();
    expect(retried.status).toBe(200);
    expect(retried.headers["ratelimit-remaining"]).toBe("0");

    now += 60_000;
    const refilled = await search();
    expect(refilled.status).toBe(200);
    expect(refilled.headers["ratelimit-remaining"]).toBe("29");
  });

  it("never admits more concurrent requests than the bucket holds", async () => {
    const responses = await Promise.all(Array.from({ length: 45 }, () => search()));
    const statuses = responses.map((r) => r.status);

    expect(statuses.filter((s) => s === 200)).toHaveLength(30);
    expect(statuses.filter((s) => s === 429)).toHaveLength(15);
    const remaining = responses
      .filter((r) => r.status === 200)
      .map((r) => Number(r.headers["ratelimit-remaining"]))
      .sort((a, b) => a - b);
    expect(remaining).toEqual(Array.from({ length: 30 }, (_, i) => i));
  });
});
