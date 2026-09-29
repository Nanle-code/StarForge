import request from "supertest";
import app from "../index";

describe("Intelligent Template Search", () => {
  let token: string;

  const publish = async (overrides: Record<string, unknown>) => {
    const res = await request(app)
      .post("/api/templates/publish")
      .set("Authorization", `Bearer ${token}`)
      .send({
        version: "1.0.0",
        author: "Stellar Community",
        authors: ["Stellar Community"],
        attribution: "Copyright (c) 2026 Stellar Community",
        license: "MIT",
        content: Buffer.from("content").toString("base64"),
        ...overrides,
      });
    return res.body.template_id as string;
  };

  beforeAll(async () => {
    const signup = await request(app).post("/api/auth/signup").send({
      email: "search-tester@example.com",
      username: "search-tester",
      password: "password123",
    });
    token = signup.body.token;

    // Seed a small, thematically distinct catalog.
    await publish({
      name: "uniswap-style-dex",
      description: "Automated market maker for swapping tokens",
      tags: ["defi", "dex", "amm", "swap"],
      functionality: ["liquidity-pool", "token-swap"],
    });
    await publish({
      name: "dao-governance",
      description: "On-chain proposal creation and voting mechanisms",
      tags: ["dao", "governance", "voting"],
      functionality: ["proposal-creation", "vote-tally"],
    });
    await publish({
      name: "multisig-wallet",
      description: "Threshold-based multi-signature wallet",
      tags: ["wallet", "multisig", "security"],
      functionality: ["threshold-approval"],
    });
    await publish({
      name: "nft-marketplace",
      description: "Mint, list, and trade non-fungible collectibles",
      tags: ["nft", "marketplace", "collectible"],
      functionality: ["minting", "listing"],
    });
  });

  describe("Natural language search", () => {
    it("finds templates from a conversational query", async () => {
      const res = await request(app).post("/api/templates/search").send({
        query: "I need something for swapping tokens",
      });

      expect(res.status).toBe(200);
      expect(res.body.success).toBe(true);
      const names = res.body.results.map((r: any) => r.name);
      expect(names[0]).toBe("uniswap-style-dex");
      expect(res.body.results[0].match_score).toBeGreaterThan(0);
    });

    it("finds templates via description keywords", async () => {
      const res = await request(app).post("/api/templates/search").send({
        query: "threshold based approval wallet",
      });

      expect(res.status).toBe(200);
      const names = res.body.results.map((r: any) => r.name);
      expect(names[0]).toBe("multisig-wallet");
    });
  });

  describe("Semantic matching", () => {
    it("matches governance template via a synonym not present in its text", async () => {
      // "poll" never appears in the dao-governance template's text, but it
      // is a semantic neighbour of "voting" / "governance".
      const res = await request(app).post("/api/templates/search").send({
        query: "poll based decision making",
      });

      expect(res.status).toBe(200);
      const names = res.body.results.map((r: any) => r.name);
      expect(names).toContain("dao-governance");
    });

    it("matches nft template when searching for collectibles", async () => {
      const res = await request(app).post("/api/templates/search").send({
        query: "collectible digital art you can mint",
      });

      expect(res.status).toBe(200);
      const names = res.body.results.map((r: any) => r.name);
      expect(names[0]).toBe("nft-marketplace");
    });
  });

  describe("Advanced filtering", () => {
    it("filters by tags", async () => {
      const res = await request(app).post("/api/templates/search").send({
        query: "",
        tags: ["security"],
      });

      expect(res.status).toBe(200);
      const names = res.body.results.map((r: any) => r.name);
      expect(names).toEqual(["multisig-wallet"]);
    });

    it("filters by minimum downloads", async () => {
      const res = await request(app).post("/api/templates/search").send({
        query: "",
        min_downloads: 999999,
      });

      expect(res.status).toBe(200);
      expect(res.body.results).toEqual([]);
    });

    it("combines a query with tag filters", async () => {
      const res = await request(app).post("/api/templates/search").send({
        query: "trade",
        tags: ["nft"],
      });

      expect(res.status).toBe(200);
      const names = res.body.results.map((r: any) => r.name);
      expect(names).toEqual(["nft-marketplace"]);
    });
  });

  describe("Sorting", () => {
    it("sorts by most recent when requested", async () => {
      const res = await request(app).post("/api/templates/search").send({
        query: "",
        sort_by: "recent",
      });

      expect(res.status).toBe(200);
      const dates = res.body.results.map((r: any) =>
        new Date(r.created_at).getTime(),
      );
      const sorted = [...dates].sort((a, b) => b - a);
      expect(dates).toEqual(sorted);
    });
  });

  describe("Search suggestions", () => {
    it("suggests matching template names and tags for a prefix", async () => {
      const res = await request(app).get(
        "/api/templates/search/suggestions?q=dao",
      );

      expect(res.status).toBe(200);
      expect(res.body.success).toBe(true);
      const values = res.body.suggestions.map((s: any) => s.value);
      expect(values).toContain("dao-governance");
    });
  });

  describe("Find similar templates", () => {
    it("finds templates similar to a given template id", async () => {
      const searchRes = await request(app)
        .post("/api/templates/search")
        .send({ query: "dao governance" });
      const target = searchRes.body.results[0];

      const res = await request(app).get(
        `/api/templates/${target.id}/similar`,
      );

      expect(res.status).toBe(200);
      expect(res.body.success).toBe(true);
      expect(Array.isArray(res.body.results)).toBe(true);
    });

    it("returns 404 for an unknown template id", async () => {
      const res = await request(app).get(
        "/api/templates/not-a-real-id/similar",
      );
      expect(res.status).toBe(404);
    });
  });

  describe("Trending templates", () => {
    it("surfaces templates with recent download activity", async () => {
      const dexTemplate = await request(app).get(
        "/api/templates/uniswap-style-dex/latest",
      );
      const downloadUrl = dexTemplate.body.download_url;

      await request(app).get(downloadUrl.replace("/api/templates", "/api/templates"));

      const res = await request(app).get("/api/templates/discover/trending");

      expect(res.status).toBe(200);
      expect(res.body.success).toBe(true);
      const names = res.body.results.map((r: any) => r.name);
      expect(names).toContain("uniswap-style-dex");
    });
  });

  describe("Personalized recommendations", () => {
    it("falls back to popular templates for anonymous users", async () => {
      const res = await request(app).get(
        "/api/templates/discover/recommended",
      );

      expect(res.status).toBe(200);
      expect(res.body.personalized).toBe(false);
      expect(Array.isArray(res.body.results)).toBe(true);
    });

    it("personalizes results once a user has engagement history", async () => {
      const signup = await request(app).post("/api/auth/signup").send({
        email: "nft-fan@example.com",
        username: "nft-fan",
        password: "password123",
      });
      const nftFanToken = signup.body.token;

      // Build up affinity toward NFT templates.
      const nftTemplate = await request(app).get(
        "/api/templates/nft-marketplace/latest",
      );
      await request(app)
        .get(nftTemplate.body.download_url)
        .set("Authorization", `Bearer ${nftFanToken}`);
      await request(app)
        .get(nftTemplate.body.download_url)
        .set("Authorization", `Bearer ${nftFanToken}`);

      const res = await request(app)
        .get("/api/templates/discover/recommended")
        .set("Authorization", `Bearer ${nftFanToken}`);

      expect(res.status).toBe(200);
      expect(res.body.personalized).toBe(true);
      expect(res.body.results[0].name).toBe("nft-marketplace");
    });

    it("personalizes the general search results for a returning user", async () => {
      const signup = await request(app).post("/api/auth/signup").send({
        email: "dex-fan@example.com",
        username: "dex-fan",
        password: "password123",
      });
      const dexFanToken = signup.body.token;

      const dexTemplate = await request(app).get(
        "/api/templates/uniswap-style-dex/latest",
      );
      await request(app)
        .get(dexTemplate.body.download_url)
        .set("Authorization", `Bearer ${dexFanToken}`);

      const res = await request(app)
        .post("/api/templates/search")
        .set("Authorization", `Bearer ${dexFanToken}`)
        .send({ query: "" , sort_by: "relevance" });

      expect(res.status).toBe(200);
      expect(res.body.personalized).toBe(true);
    });
  });

  describe("Usage analytics", () => {
    it("tracks search queries and interaction totals", async () => {
      await request(app).post("/api/templates/search").send({
        query: "governance",
      });

      const res = await request(app).get("/api/templates/analytics/summary");

      expect(res.status).toBe(200);
      expect(res.body.success).toBe(true);
      expect(res.body.totals.searches).toBeGreaterThan(0);
      expect(Array.isArray(res.body.top_queries)).toBe(true);
    });
  });
});
