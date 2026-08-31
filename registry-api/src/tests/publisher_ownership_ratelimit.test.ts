import request from "supertest";
import app from "../index";
import { resetRateLimiterStore, createRateLimiter } from "../middleware/rateLimiter";
import { ownershipHistoryStore } from "../models/OwnershipHistory";
import { templateStore } from "../routes/templates";
import { userStore } from "../models/User";
import { Request, Response, NextFunction } from "express";

describe("Publisher Authentication, Rate Limiting & Ownership History", () => {
  let tokenUserA: string;
  let userIdA: string;
  let tokenUserB: string;
  let userIdB: string;

  beforeEach(async () => {
    resetRateLimiterStore();
    await ownershipHistoryStore.clear();
    await templateStore.clear();
    await userStore.clear();

    // Register User A
    const signupA = await request(app).post("/api/auth/signup").send({
      email: "publisherA@example.com",
      username: "publisherA",
      password: "password123",
    });
    tokenUserA = signupA.body.token;

    const verifyA = await request(app)
      .post("/api/auth/verify")
      .set("Authorization", `Bearer ${tokenUserA}`);
    userIdA = verifyA.body.user.id;

    // Register User B
    const signupB = await request(app).post("/api/auth/signup").send({
      email: "publisherB@example.com",
      username: "publisherB",
      password: "password123",
    });
    tokenUserB = signupB.body.token;

    const verifyB = await request(app)
      .post("/api/auth/verify")
      .set("Authorization", `Bearer ${tokenUserB}`);
    userIdB = verifyB.body.user.id;
  });

  describe("Primary Flow", () => {
    it("should authenticate publisher, create auditable history, and allow owner transfers", async () => {
      // 1. User A publishes template
      const pubRes = await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({
          name: "starforge-escrow",
          version: "1.0.0",
          description: "Stellar smart contract escrow template",
          author: "Publisher A",
          tags: ["escrow", "soroban"],
          content: Buffer.from("dummy-wasm-content").toString("base64"),
        });

      expect(pubRes.status).toBe(201);
      expect(pubRes.body.success).toBe(true);
      expect(pubRes.body.template_id).toBeDefined();

      // 2. Fetch ownership history
      const historyRes = await request(app).get(
        "/api/templates/starforge-escrow/ownership-history",
      );

      expect(historyRes.status).toBe(200);
      expect(historyRes.body.success).toBe(true);
      expect(historyRes.body.template_name).toBe("starforge-escrow");
      expect(historyRes.body.history.length).toBe(1);
      expect(historyRes.body.history[0].action).toBe("PUBLISH");
      expect(historyRes.body.history[0].publisherId).toBe(userIdA);
      expect(historyRes.body.history[0].publisherUsername).toBe("publisherA");

      // 3. User A transfers ownership to User B
      const transferRes = await request(app)
        .post("/api/templates/starforge-escrow/transfer-ownership")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({ new_username: "publisherB" });

      expect(transferRes.status).toBe(200);
      expect(transferRes.body.success).toBe(true);
      expect(transferRes.body.new_publisher_id).toBe(userIdB);

      // 4. Verify auditable ownership history after transfer
      const updatedHistoryRes = await request(app).get(
        "/api/templates/starforge-escrow/ownership-history",
      );

      expect(updatedHistoryRes.status).toBe(200);
      expect(updatedHistoryRes.body.history.length).toBe(2);
      expect(updatedHistoryRes.body.history[1].action).toBe("TRANSFER_OWNERSHIP");
      expect(updatedHistoryRes.body.history[1].previousPublisherId).toBe(userIdA);
      expect(updatedHistoryRes.body.history[1].publisherId).toBe(userIdB);
      expect(updatedHistoryRes.body.history[1].publisherUsername).toBe("publisherB");

      // 5. User B (new owner) can now publish new version v1.1.0
      const pubVersion2 = await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserB}`)
        .send({
          name: "starforge-escrow",
          version: "1.1.0",
          description: "Updated escrow template by publisher B",
          author: "Publisher B",
          content: Buffer.from("v1.1.0-content").toString("base64"),
        });

      expect(pubVersion2.status).toBe(201);
      expect(pubVersion2.body.success).toBe(true);
    });
  });

  describe("Boundary Cases", () => {
    it("should enforce rate limit threshold and set headers when threshold exceeded", async () => {
      // Send requests up to rate limit threshold
      for (let i = 0; i < 10; i++) {
        const res = await request(app)
          .post("/api/templates/publish")
          .set("Authorization", `Bearer ${tokenUserA}`)
          .send({
            name: `template-rl-${i}`,
            version: "1.0.0",
            description: "Test template",
            author: "Publisher A",
            content: Buffer.from("test").toString("base64"),
          });
        expect(res.status).toBe(201);
        expect(res.headers["x-ratelimit-limit"]).toBe("10");
        expect(parseInt(res.headers["x-ratelimit-remaining"])).toBe(9 - i);
      }

      // 11th request exceeds limit
      const blockedRes = await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({
          name: "template-rl-11",
          version: "1.0.0",
          description: "Over limit",
          author: "Publisher A",
          content: Buffer.from("test").toString("base64"),
        });

      expect(blockedRes.status).toBe(429);
      expect(blockedRes.body.error).toContain("Rate limit exceeded");
      expect(blockedRes.headers["retry-after"]).toBeDefined();
      expect(blockedRes.headers["x-ratelimit-remaining"]).toBe("0");
    });

    it("should handle unsupported environment configurations gracefully with safe defaults", () => {
      const customLimiter = createRateLimiter({ windowMs: -1000, max: -5 });
      const mockReq = { userId: "test-user-id", ip: "127.0.0.1" } as Request;
      const headers: Record<string, string> = {};
      let statusCode = 200;
      let jsonBody: any = null;

      const mockRes = {
        setHeader: (k: string, v: string) => {
          headers[k] = v;
        },
        status: (code: number) => {
          statusCode = code;
          return {
            json: (body: any) => {
              jsonBody = body;
            },
          };
        },
      } as Response;

      const mockNext: NextFunction = jest.fn();

      customLimiter(mockReq, mockRes, mockNext);
      expect(mockNext).toHaveBeenCalled();
      expect(statusCode).toBe(200);
      expect(jsonBody).toBeNull();
      expect(headers["X-RateLimit-Limit"]).toBe("10"); // Default fallback
    });
  });

  describe("Failure Paths", () => {
    it("should reject publish attempts under existing template name owned by a different publisher", async () => {
      // User A publishes "unique-name"
      await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({
          name: "unique-name",
          version: "1.0.0",
          description: "Published by User A",
          author: "Publisher A",
          content: Buffer.from("content-a").toString("base64"),
        });

      // User B attempts to publish "unique-name"
      const res = await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserB}`)
        .send({
          name: "unique-name",
          version: "1.0.1",
          description: "Attempted by User B",
          author: "Publisher B",
          content: Buffer.from("content-b").toString("base64"),
        });

      expect(res.status).toBe(403);
      expect(res.body.error).toContain("owned by another publisher");
    });

    it("should reject publish requests missing required fields", async () => {
      const res = await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({
          name: "incomplete-template",
          // missing version, description, author, content
        });

      expect(res.status).toBe(400);
      expect(res.body.error).toContain("Missing required fields");
    });

    it("should reject unauthorized ownership transfers by non-owners", async () => {
      // User A publishes template
      await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({
          name: "protected-template",
          version: "1.0.0",
          description: "Template",
          author: "Publisher A",
          content: Buffer.from("test").toString("base64"),
        });

      // User B tries to transfer ownership of User A's template
      const res = await request(app)
        .post("/api/templates/protected-template/transfer-ownership")
        .set("Authorization", `Bearer ${tokenUserB}`)
        .send({ new_username: "publisherB" });

      expect(res.status).toBe(403);
      expect(res.body.error).toContain("only the template owner can transfer ownership");
    });

    it("should return 404 when transferring ownership to a non-existent publisher", async () => {
      await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({
          name: "template-transfer-test",
          version: "1.0.0",
          description: "Template",
          author: "Publisher A",
          content: Buffer.from("test").toString("base64"),
        });

      const res = await request(app)
        .post("/api/templates/template-transfer-test/transfer-ownership")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({ new_username: "non_existent_user_999" });

      expect(res.status).toBe(404);
      expect(res.body.error).toContain("Target publisher not found");
    });

    it("should reject duplicate version publishing by the same owner", async () => {
      await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({
          name: "dup-version-template",
          version: "1.0.0",
          description: "Initial release",
          author: "Publisher A",
          content: Buffer.from("test").toString("base64"),
        });

      const dupRes = await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${tokenUserA}`)
        .send({
          name: "dup-version-template",
          version: "1.0.0",
          description: "Duplicate release",
          author: "Publisher A",
          content: Buffer.from("test-dup").toString("base64"),
        });

      expect(dupRes.status).toBe(409);
      expect(dupRes.body.error).toContain("already published");
    });
  });
});
