import express, { Request, Response } from "express";
import jwt from "jsonwebtoken";
import bcrypt from "bcryptjs";
import { v4 as uuid } from "uuid";
import { IUser, userStore } from "../models/User";
import logger from "../utils/logger";

const router = express.Router();

// Signup
router.post("/signup", async (req: Request, res: Response) => {
  try {
    const { email, username, password } = req.body;

    if (!email || !username || !password) {
      return res
        .status(400)
        .json({ error: "Email, username, and password required" });
    }

    if (password.length < 8) {
      return res
        .status(400)
        .json({ error: "Password must be at least 8 characters" });
    }

    const existing = await userStore.findByEmail(email);
    if (existing) {
      return res.status(409).json({ error: "Email already registered" });
    }

    const existingUsername = await userStore.findByUsername(username);
    if (existingUsername) {
      return res.status(409).json({ error: "Username already taken" });
    }

    const passwordHash = await bcrypt.hash(password, 10);
    const user: IUser = {
      id: uuid(),
      email,
      username,
      passwordHash,
      createdAt: new Date(),
      updatedAt: new Date(),
      verified: false,
    };

    await userStore.create(user);

    const token = jwt.sign(
      { id: user.id, email: user.email },
      process.env.JWT_SECRET || "secret",
      {
        expiresIn: (process.env.JWT_EXPIRATION as any) || "7d",
      },
    );

    logger.info(`User signed up: ${email}`);

    return res.status(201).json({
      success: true,
      message: "Account created successfully",
      token,
      username: user.username,
    });
  } catch (err) {
    logger.error("Signup error", err);
    res.status(500).json({ error: "Signup failed" });
  }
});

// Login
router.post("/login", async (req: Request, res: Response) => {
  try {
    const { email, password } = req.body;

    if (!email || !password) {
      return res.status(400).json({ error: "Email and password required" });
    }

    const user = await userStore.findByEmail(email);
    if (!user) {
      return res.status(401).json({ error: "Invalid credentials" });
    }

    const validPassword = await bcrypt.compare(password, user.passwordHash);
    if (!validPassword) {
      return res.status(401).json({ error: "Invalid credentials" });
    }

    const token = jwt.sign(
      { id: user.id, email: user.email },
      process.env.JWT_SECRET || "secret",
      {
        expiresIn: (process.env.JWT_EXPIRATION as any) || "7d",
      },
    );

    logger.info(`User logged in: ${email}`);

    return res.json({
      success: true,
      message: "Logged in successfully",
      token,
      username: user.username,
    });
  } catch (err) {
    logger.error("Login error", err);
    res.status(500).json({ error: "Login failed" });
  }
});

// Verify token
router.post("/verify", (req: Request, res: Response) => {
  const token = req.headers.authorization?.split(" ")[1];

  if (!token) {
    return res.status(401).json({ error: "No token provided" });
  }

  try {
    const decoded = jwt.verify(token, process.env.JWT_SECRET || "secret");
    res.json({ success: true, user: decoded });
  } catch (err) {
    res.status(401).json({ error: "Invalid token" });
  }
});

export default router;
