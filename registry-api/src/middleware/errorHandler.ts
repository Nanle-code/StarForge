import { Request, Response, NextFunction } from "express";
import logger from "../utils/logger";

class ApiError extends Error {
  constructor(
    public statusCode: number,
    message: string,
  ) {
    super(message);
  }
}

const errorHandler = (
  err: any,
  req: Request,
  res: Response,
  _next: NextFunction,
) => {
  logger.error("Request error", err);

  if (err instanceof ApiError) {
    return res.status(err.statusCode).json({ error: err.message });
  }

  if (err.name === "ValidationError") {
    return res
      .status(400)
      .json({ error: "Validation failed", details: err.message });
  }

  res.status(500).json({ error: "Internal server error" });
};

export { ApiError, errorHandler as default };
