import express, { Request, Response } from "express";
import { verifyToken } from "../middleware/auth";
import { organizationStore, OrganizationRole } from "../models/Organization";
import { userStore } from "../models/User";

const router = express.Router();
const roles: OrganizationRole[] = ["owner", "admin", "maintainer"];

router.post("/", verifyToken, async (req: Request, res: Response) => {
  const { slug, name } = req.body;
  if (!slug || !name) return res.status(400).json({ error: "slug and name are required" });
  if (!/^[a-z0-9][a-z0-9-]{1,38}[a-z0-9]$/.test(slug)) {
    return res.status(400).json({ error: "Invalid organization slug" });
  }
  if (await organizationStore.findBySlug(slug)) {
    return res.status(409).json({ error: "Organization slug already exists" });
  }
  const organization = await organizationStore.create(slug, name, req.userId!);
  return res.status(201).json({ success: true, organization });
});

router.get("/", async (_req: Request, res: Response) => {
  res.json({ success: true, organizations: await organizationStore.all() });
});

router.post("/:slug/members", verifyToken, async (req: Request, res: Response) => {
  const { user_id, username, role = "maintainer" } = req.body;
  const organization = await organizationStore.findBySlug(req.params.slug);
  if (!organization) return res.status(404).json({ error: "Organization not found" });
  const actorRole = await organizationStore.roleFor(req.params.slug, req.userId!);
  if (actorRole !== "owner" && actorRole !== "admin") {
    return res.status(403).json({ error: "Only organization owners and admins can manage members" });
  }
  if (!roles.includes(role)) return res.status(400).json({ error: "Invalid organization role" });
  const user = user_id ? await userStore.findById(user_id) : await userStore.findByUsername(username);
  if (!user) return res.status(404).json({ error: "User not found" });
  const updated = await organizationStore.addMember(req.params.slug, user.id, role);
  return res.json({ success: true, organization: updated });
});

export default router;