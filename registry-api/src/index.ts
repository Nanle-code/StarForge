import express from "express";
import cors from "cors";
import helmet from "helmet";
import compression from "compression";
import dotenv from "dotenv";
import path from "path";

// Import API routes for the remote template registry
import authRoutes from "./routes/auth";
import templateRoutes from "./routes/templates";
import reviewRoutes from "./routes/reviews";
import organizationRoutes from "./routes/organizations";
import errorHandler from "./middleware/errorHandler";
import logger from "./utils/logger";

dotenv.config();

const app = express();
const PORT = process.env.PORT || 3000;

// Security middleware
app.use(helmet());
app.use(compression());
app.use(
  cors({
    origin: process.env.CORS_ORIGIN || "*",
    credentials: true,
  }),
);

// Body parsing middleware
app.use(express.json({ limit: "50mb" }));
app.use(express.urlencoded({ limit: "50mb", extended: true }));

// Serve the public registry portal from the same origin as the API. Keeping
// this relative to the compiled entrypoint also works in Docker and previews.
app.use(express.static(path.join(__dirname, "../public")));

// Request logging
app.use((req, res, next) => {
  logger.info(`${req.method} ${req.path}`);
  next();
});

// Health check endpoint
app.get("/health", (req, res) => {
  res.json({ status: "ok", timestamp: new Date().toISOString() });
});

// API Routes
app.use("/api/auth", authRoutes);
app.use("/api/templates", templateRoutes);
app.use("/api/reviews", reviewRoutes);
app.use("/api/orgs", organizationRoutes);

// 404 handler
app.use((req, res) => {
  res.status(404).json({ error: "Not found" });
});

// Error handler
app.use(errorHandler);

// Start server (skipped when this module is imported by the test suite,
// so multiple test files can require `app` without fighting over the port)
if (require.main === module) {
  app.listen(PORT, () => {
    logger.info(`StarForge Registry API running on port ${PORT}`);
    logger.info(`Environment: ${process.env.NODE_ENV || "development"}`);
  });
}

export default app;
