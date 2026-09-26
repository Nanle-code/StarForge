import request from "supertest";
import app from "../index";

describe("Registry API", () => {
  let token: string;

  describe("Authentication", () => {
    it("should signup a new user", async () => {
      const response = await request(app).post("/api/auth/signup").send({
        email: "test@example.com",
        username: "testuser",
        password: "password123",
      });

      expect(response.status).toBe(201);
      expect(response.body.success).toBe(true);
      expect(response.body.token).toBeDefined();
      expect(response.body.username).toBe("testuser");

      token = response.body.token;
    });

    it("should reject duplicate email", async () => {
      await request(app).post("/api/auth/signup").send({
        email: "test@example.com",
        username: "testuser2",
        password: "password123",
      });

      const response = await request(app).post("/api/auth/signup").send({
        email: "test@example.com",
        username: "testuser3",
        password: "password123",
      });

      expect(response.status).toBe(409);
      expect(response.body.error).toContain("already registered");
    });

    it("should login with correct credentials", async () => {
      const response = await request(app).post("/api/auth/login").send({
        email: "test@example.com",
        password: "password123",
      });

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
      expect(response.body.token).toBeDefined();
    });

    it("should reject wrong password", async () => {
      const response = await request(app).post("/api/auth/login").send({
        email: "test@example.com",
        password: "wrongpassword",
      });

      expect(response.status).toBe(401);
      expect(response.body.error).toContain("Invalid");
    });

    it("should verify valid token", async () => {
      const response = await request(app)
        .post("/api/auth/verify")
        .set("Authorization", `Bearer ${token}`);

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
      expect(response.body.user).toBeDefined();
    });
  });

  describe("Templates", () => {
    it("should search templates", async () => {
      const response = await request(app).post("/api/templates/search").send({
        query: "counter",
        limit: 10,
      });

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
      expect(Array.isArray(response.body.results)).toBe(true);
    });

    it("should publish a template when authenticated", async () => {
      const response = await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${token}`)
        .send({
          name: "test-counter",
          version: "1.0.0",
          description: "Test counter template",
          author: "Test User",
          tags: ["example", "test"],
          license: "MIT",
          content: Buffer.from("test content").toString("base64"),
        });

      expect(response.status).toBe(201);
      expect(response.body.success).toBe(true);
      expect(response.body.template_id).toBeDefined();

    });

    it("should reject publish without authentication", async () => {
      const response = await request(app).post("/api/templates/publish").send({
        name: "test-template",
        version: "1.0.0",
        description: "Test",
        author: "Test",
        content: "base64content",
      });

      expect(response.status).toBe(401);
    });

    it("should get template details", async () => {
      const response = await request(app).get(
        "/api/templates/test-counter/1.0.0",
      );

      expect(response.status).toBe(200);
      expect(response.body.name).toBe("test-counter");
      expect(response.body.version).toBe("1.0.0");
    });
  });

  describe("Reviews", () => {
    let templateId: string;

    beforeEach(async () => {
      // Create a template to review
      const pubResponse = await request(app)
        .post("/api/templates/publish")
        .set("Authorization", `Bearer ${token}`)
        .send({
          name: "review-test",
          version: "1.0.0",
          description: "Template for review testing",
          author: "Test User",
          tags: ["test"],
          content: Buffer.from("test").toString("base64"),
        });

      templateId = pubResponse.body.template_id;
    });

    it("should post a review", async () => {
      const response = await request(app)
        .post(`/api/reviews/template/${templateId}/reviews`)
        .set("Authorization", `Bearer ${token}`)
        .send({
          rating: 5,
          comment: "Great template!",
        });

      expect(response.status).toBe(201);
      expect(response.body.success).toBe(true);
    });

    it("should reject invalid rating", async () => {
      const response = await request(app)
        .post(`/api/reviews/template/${templateId}/reviews`)
        .set("Authorization", `Bearer ${token}`)
        .send({
          rating: 10,
          comment: "Invalid rating",
        });

      expect(response.status).toBe(400);
    });

    it("should get reviews for template", async () => {
      await request(app)
        .post(`/api/reviews/template/${templateId}/reviews`)
        .set("Authorization", `Bearer ${token}`)
        .send({
          rating: 4,
          comment: "Good",
        });

      const response = await request(app).get(
        `/api/reviews/template/${templateId}`,
      );

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
      expect(Array.isArray(response.body.reviews)).toBe(true);
    });

    it("should update existing review", async () => {
      await request(app)
        .post(`/api/reviews/template/${templateId}/reviews`)
        .set("Authorization", `Bearer ${token}`)
        .send({
          rating: 3,
          comment: "Initial review",
        });

      const response = await request(app)
        .post(`/api/reviews/template/${templateId}/reviews`)
        .set("Authorization", `Bearer ${token}`)
        .send({
          rating: 5,
          comment: "Updated review",
        });

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
    });
  });

  describe("Health Check", () => {
    it("should return health status", async () => {
      const response = await request(app).get("/health");

      expect(response.status).toBe(200);
      expect(response.body.status).toBe("ok");
      expect(response.body.timestamp).toBeDefined();
    });
  });

  describe("Error Handling", () => {
    it("should return 404 for unknown endpoint", async () => {
      const response = await request(app).get("/api/unknown");

      expect(response.status).toBe(404);
      expect(response.body.error).toBeDefined();
    });

    it("should return 401 for missing token", async () => {
      const response = await request(app).post("/api/templates/publish").send({
        name: "test",
        version: "1.0.0",
      });

      expect(response.status).toBe(401);
    });
  });
});
