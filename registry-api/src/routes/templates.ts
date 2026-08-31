import express, { Request, Response } from "express";
import { v4 as uuid } from "uuid";
import { TemplateStore, ITemplate } from "../models/Template";
import { searchAnalytics } from "../models/SearchAnalytics";
import { searchEngine, SearchOptions } from "../services/searchEngine";
import { verifyToken, optionalAuth } from "../middleware/auth";
import { mutationRateLimiter } from "../middleware/rateLimiter";
import { ownershipHistoryStore } from "../models/OwnershipHistory";
import { userStore } from "../models/User";
import logger from "../utils/logger";
import fs from "fs";
import path from "path";

const router = express.Router();
export const templateStore = new TemplateStore();

const STORAGE_DIR = process.env.STORAGE_DIR || "./storage/templates";

// Ensure storage directory exists
if (!fs.existsSync(STORAGE_DIR)) {
  fs.mkdirSync(STORAGE_DIR, { recursive: true });
}

// Shared response shape for a template across every endpoint below.
function serializeTemplate(tpl: ITemplate) {
  return {
    id: tpl.id,
    name: tpl.name,
    version: tpl.version,
    description: tpl.description,
    author: tpl.author,
    tags: tpl.tags,
    functionality: tpl.functionality || [],
    license: tpl.license,
    repository: tpl.repository,
    homepage: tpl.homepage,
    documentation: tpl.documentation,
    downloads: tpl.downloads,
    verified: tpl.verified,
    created_at: tpl.createdAt,
    updated_at: tpl.updatedAt,
    ratings: {
      average_rating: tpl.ratings.average,
      review_count: tpl.ratings.count,
      five_star: tpl.ratings.distribution[5] || 0,
      four_star: tpl.ratings.distribution[4] || 0,
      three_star: tpl.ratings.distribution[3] || 0,
      two_star: tpl.ratings.distribution[2] || 0,
      one_star: tpl.ratings.distribution[1] || 0,
    },
    download_url: tpl.downloadUrl,
  };
}

// ---------------------------------------------------------------------------
// Intelligent search
// ---------------------------------------------------------------------------
//
// Natural language + semantic search over the template corpus, with
// personalization, filtering, sorting, and usage analytics. See
// registry-api/INTELLIGENT_SEARCH.md for the full design notes.

router.post("/search", optionalAuth, async (req: Request, res: Response) => {
  try {
    const {
      query = "",
      tags,
      verified,
      min_quality,
      min_downloads,
      license,
      author,
      date_from,
      date_to,
      sort_by = "relevance",
      limit = 20,
      offset = 0,
    } = req.body;

    const allTemplates = await templateStore.all();

    const filterOptions: SearchOptions = {
      tags,
      verified,
      license,
      minQuality: min_quality,
      minDownloads: min_downloads,
      author,
      dateFrom: date_from,
      dateTo: date_to,
    };

    let scored = searchEngine.search(allTemplates, query, filterOptions);

    // Personalization: nudge templates that match this user's historical
    // tag / author affinity (built from their view & download history).
    let personalized = false;
    if (req.userId) {
      const { tagScores, authorScores } = searchAnalytics.getUserAffinity(
        req.userId,
        (id) => allTemplates.find((t) => t.id === id),
      );
      if (tagScores.size > 0 || authorScores.size > 0) {
        personalized = true;
        scored = scored.map((r) => {
          let boost = 0;
          for (const tag of r.template.tags) {
            boost += (tagScores.get(tag.toLowerCase()) || 0) * 0.05;
          }
          boost += (authorScores.get(r.template.author) || 0) * 0.03;
          return { ...r, relevanceScore: r.relevanceScore + boost };
        });
      }
    }

    // Trending boost: recently popular templates surface slightly higher,
    // which helps discovery even when the query is broad.
    const trending = searchAnalytics.getTrendingTemplateIds();
    const trendingScores = new Map(trending.map((t) => [t.templateId, t.score]));
    if (trendingScores.size > 0) {
      scored = scored.map((r) => ({
        ...r,
        relevanceScore:
          r.relevanceScore + (trendingScores.get(r.template.id) || 0) * 0.01,
      }));
    }

    switch (sort_by) {
      case "downloads":
        scored.sort((a, b) => b.template.downloads - a.template.downloads);
        break;
      case "rating":
        scored.sort(
          (a, b) => b.template.ratings.average - a.template.ratings.average,
        );
        break;
      case "recent":
        scored.sort(
          (a, b) =>
            new Date(b.template.createdAt).getTime() -
            new Date(a.template.createdAt).getTime(),
        );
        break;
      case "trending":
        scored.sort(
          (a, b) =>
            (trendingScores.get(b.template.id) || 0) -
            (trendingScores.get(a.template.id) || 0),
        );
        break;
      case "relevance":
      default:
        if (query && query.trim()) {
          scored.sort((a, b) => b.relevanceScore - a.relevanceScore);
        } else {
          scored.sort((a, b) => b.template.downloads - a.template.downloads);
        }
    }

    const total = scored.length;
    const paginated = scored.slice(offset, offset + limit);

    searchAnalytics.recordSearch(
      req.userId,
      query,
      filterOptions as Record<string, unknown>,
      total,
    );

    res.json({
      success: true,
      results: paginated.map((r) => ({
        ...serializeTemplate(r.template),
        match_score: Number(r.relevanceScore.toFixed(4)),
        matched_terms: r.matchedTerms,
      })),
      total,
      limit,
      offset,
      personalized,
    });
  } catch (err) {
    logger.error("Search error", err);
    res.status(500).json({ error: "Search failed" });
  }
});

// Search suggestions / autocomplete: past popular queries plus matching
// template names and tags for the given prefix.
router.get(
  "/search/suggestions",
  optionalAuth,
  async (req: Request, res: Response) => {
    try {
      const q = String(req.query.q || "");
      const limit = Number(req.query.limit) || 10;
      const prefix = q.toLowerCase().trim();

      const allTemplates = await templateStore.all();
      const nameMatches = new Set<string>();
      const tagMatches = new Set<string>();

      if (prefix) {
        for (const tpl of allTemplates) {
          if (tpl.name.toLowerCase().includes(prefix)) {
            nameMatches.add(tpl.name);
          }
          for (const tag of tpl.tags) {
            if (tag.toLowerCase().startsWith(prefix)) {
              tagMatches.add(tag);
            }
          }
        }
      }

      const queryMatches = searchAnalytics.getSuggestions(prefix, limit);

      const suggestions = [
        ...queryMatches.map((value) => ({ type: "query", value })),
        ...[...nameMatches].slice(0, limit).map((value) => ({
          type: "template",
          value,
        })),
        ...[...tagMatches].slice(0, limit).map((value) => ({
          type: "tag",
          value,
        })),
      ].slice(0, limit);

      res.json({ success: true, suggestions });
    } catch (err) {
      logger.error("Suggestions error", err);
      res.status(500).json({ error: "Failed to fetch suggestions" });
    }
  },
);

// Trending templates: recent view/click/download activity, weighted, with
// a most-downloaded fallback so the endpoint is useful from a cold start.
router.get(
  "/discover/trending",
  optionalAuth,
  async (req: Request, res: Response) => {
    try {
      const limit = Number(req.query.limit) || 10;
      const windowDays = Number(req.query.window_days) || 7;

      const allTemplates = await templateStore.all();
      const trending = searchAnalytics.getTrendingTemplateIds(
        windowDays * 24 * 60 * 60 * 1000,
        limit,
      );
      const byId = new Map(allTemplates.map((t) => [t.id, t]));

      let results = trending
        .map((t) => byId.get(t.templateId))
        .filter((t): t is ITemplate => Boolean(t));

      if (results.length < limit) {
        const seen = new Set(results.map((t) => t.id));
        const fallback = [...allTemplates]
          .filter((t) => !seen.has(t.id))
          .sort((a, b) => b.downloads - a.downloads)
          .slice(0, limit - results.length);
        results = [...results, ...fallback];
      }

      res.json({ success: true, results: results.map(serializeTemplate) });
    } catch (err) {
      logger.error("Trending error", err);
      res.status(500).json({ error: "Failed to fetch trending templates" });
    }
  },
);

// Personalized recommendations based on the current user's view/download
// history. Falls back to most-downloaded templates for anonymous users or
// users without enough history yet.
router.get(
  "/discover/recommended",
  optionalAuth,
  async (req: Request, res: Response) => {
    try {
      const limit = Number(req.query.limit) || 10;
      const allTemplates = await templateStore.all();

      const fallback = () =>
        res.json({
          success: true,
          results: [...allTemplates]
            .sort((a, b) => b.downloads - a.downloads)
            .slice(0, limit)
            .map(serializeTemplate),
          personalized: false,
        });

      if (!req.userId) return fallback();

      const { tagScores, authorScores } = searchAnalytics.getUserAffinity(
        req.userId,
        (id) => allTemplates.find((t) => t.id === id),
      );

      if (tagScores.size === 0 && authorScores.size === 0) return fallback();

      const scored = allTemplates.map((tpl) => {
        let score = 0;
        for (const tag of tpl.tags) {
          score += tagScores.get(tag.toLowerCase()) || 0;
        }
        score += authorScores.get(tpl.author) || 0;
        return { tpl, score };
      });

      scored.sort((a, b) => b.score - a.score || b.tpl.downloads - a.tpl.downloads);

      res.json({
        success: true,
        results: scored.slice(0, limit).map((s) => serializeTemplate(s.tpl)),
        personalized: true,
      });
    } catch (err) {
      logger.error("Recommendation error", err);
      res.status(500).json({ error: "Failed to fetch recommendations" });
    }
  },
);

// Search usage analytics summary (top queries, trending ids, totals).
router.get(
  "/analytics/summary",
  optionalAuth,
  async (req: Request, res: Response) => {
    try {
      const limit = Number(req.query.limit) || 10;
      res.json({
        success: true,
        top_queries: searchAnalytics.getTopQueries(limit),
        trending_template_ids: searchAnalytics.getTrendingTemplateIds(
          7 * 24 * 60 * 60 * 1000,
          limit,
        ),
        totals: searchAnalytics.getEventCounts(),
      });
    } catch (err) {
      logger.error("Analytics summary error", err);
      res.status(500).json({ error: "Failed to fetch analytics" });
    }
  },
);

// Find templates similar to a given one (by TF-IDF document similarity).
router.get(
  "/:id/similar",
  optionalAuth,
  async (req: Request, res: Response) => {
    try {
      const { id } = req.params;
      const limit = Number(req.query.limit) || 5;

      const allTemplates = await templateStore.all();
      const target = allTemplates.find((t) => t.id === id);
      if (!target) {
        return res.status(404).json({ error: "Template not found" });
      }

      const similar = searchEngine.findSimilar(allTemplates, target.id, limit);

      res.json({
        success: true,
        results: similar.map((r) => ({
          ...serializeTemplate(r.template),
          similarity_score: Number(r.relevanceScore.toFixed(4)),
        })),
      });
    } catch (err) {
      logger.error("Similar templates error", err);
      res.status(500).json({ error: "Failed to fetch similar templates" });
    }
  },
);

// Get template ownership history
router.get(
  "/:name/ownership-history",
  optionalAuth,
  async (req: Request, res: Response) => {
    try {
      const { name } = req.params;
      const history = await ownershipHistoryStore.getHistoryForTemplate(name);
      res.json({
        success: true,
        template_name: name,
        history,
      });
    } catch (err) {
      logger.error("Ownership history error", err);
      res.status(500).json({ error: "Failed to fetch ownership history" });
    }
  },
);

// Transfer template ownership
router.post(
  "/:name/transfer-ownership",
  verifyToken,
  mutationRateLimiter,
  async (req: Request, res: Response) => {
    try {
      const { name } = req.params;
      const { new_publisher_id, new_username } = req.body;

      if (!new_publisher_id && !new_username) {
        return res.status(400).json({
          error: "Missing new_publisher_id or new_username in request body",
        });
      }

      if (!req.userId) {
        return res.status(401).json({ error: "Unauthorized" });
      }

      const templates = await templateStore.findByName(name);
      if (templates.length === 0) {
        return res.status(404).json({ error: "Template not found" });
      }

      const currentOwnerId = await templateStore.findPublisherForName(name);
      if (currentOwnerId !== req.userId) {
        return res.status(403).json({
          error: "Forbidden: only the template owner can transfer ownership",
        });
      }

      let targetUser = null;
      if (new_publisher_id) {
        targetUser = await userStore.findById(new_publisher_id);
      } else if (new_username) {
        targetUser = await userStore.findByUsername(new_username);
      }

      if (!targetUser) {
        return res.status(404).json({ error: "Target publisher not found" });
      }

      await templateStore.updatePublisherForName(name, targetUser.id);
      const currentUser = await userStore.findById(req.userId);

      await ownershipHistoryStore.record({
        templateId: templates[0].id,
        templateName: name,
        version: templates[0].version,
        publisherId: targetUser.id,
        publisherUsername: targetUser.username,
        previousPublisherId: req.userId,
        action: "TRANSFER_OWNERSHIP",
        ipAddress: req.ip,
        metadata: {
          transferred_by: currentUser?.username || req.userId,
        },
      });

      logger.info(
        `Ownership of ${name} transferred from ${req.userId} to ${targetUser.id}`,
      );

      res.json({
        success: true,
        message: `Ownership of ${name} successfully transferred to ${targetUser.username}`,
        new_publisher_id: targetUser.id,
      });
    } catch (err) {
      logger.error("Transfer ownership error", err);
      res.status(500).json({ error: "Transfer ownership failed" });
    }
  },
);

// Get template by name and version
router.get(
  "/:name/:version",
  optionalAuth,
  async (req: Request, res: Response) => {
    try {
      const { name, version } = req.params;
      const versionQuery = version === "latest" ? undefined : version;

      const results = await templateStore.findByName(name);
      if (results.length === 0) {
        return res.status(404).json({ error: "Template not found" });
      }

      let tpl = results[0];
      if (versionQuery) {
        tpl = results.find((t) => t.version === versionQuery) || results[0];
      }

      searchAnalytics.recordInteraction(req.userId, tpl.id, "view");

      res.json(serializeTemplate(tpl));
    } catch (err) {
      logger.error("Get template error", err);
      res.status(500).json({ error: "Failed to fetch template" });
    }
  },
);

// Publish template
router.post("/publish", verifyToken, mutationRateLimiter, async (req: Request, res: Response) => {
  try {
    const {
      name,
      version,
      description,
      author,
      tags,
      functionality,
      license,
      repository,
      homepage,
      documentation,
      content,
    } = req.body;

    if (!name || !version || !description || !author || !content) {
      return res.status(400).json({ error: "Missing required fields" });
    }

    if (!req.userId) {
      return res.status(401).json({ error: "Unauthorized publisher" });
    }

    const publisher = await userStore.findById(req.userId);

    // Check ownership of template name across publishers
    const existingOwnerId = await templateStore.findPublisherForName(name);
    if (existingOwnerId && existingOwnerId !== req.userId) {
      return res
        .status(403)
        .json({ error: "Forbidden: template name owned by another publisher" });
    }

    // Check if exact version already exists
    const existing = await templateStore.findByNameAndVersion(name, version);
    if (existing) {
      return res
        .status(409)
        .json({ error: "Template version already published" });
    }

    // Save template content
    const templateId = uuid();
    const fileName = `${name}-${version}-${templateId}.zip`;
    const filePath = path.join(STORAGE_DIR, fileName);

    const buffer = Buffer.from(content, "base64");
    fs.writeFileSync(filePath, buffer);

    const template: ITemplate = {
      id: templateId,
      name,
      version,
      description,
      author,
      tags: tags || [],
      functionality: functionality || [],
      license,
      repository,
      homepage,
      documentation,
      downloads: 0,
      verified: false,
      publisherId: req.userId,
      createdAt: new Date(),
      updatedAt: new Date(),
      ratings: { average: 0, count: 0, distribution: {} },
      downloadUrl: `/api/templates/${name}/${version}/download`,
    };

    await templateStore.create(template);

    // Record auditable ownership event
    await ownershipHistoryStore.record({
      templateId,
      templateName: name,
      version,
      publisherId: req.userId,
      publisherUsername: publisher?.username || author,
      action: "PUBLISH",
      ipAddress: req.ip,
      metadata: {
        license,
        tags: tags || [],
        repository,
      },
    });

    logger.info(`Template published: ${name}@${version}`);

    res.status(201).json({
      success: true,
      message: "Template published successfully",
      template_id: templateId,
      url: `/registry/template/${name}/${version}`,
    });
  } catch (err) {
    logger.error("Publish error", err);
    res.status(500).json({ error: "Publish failed" });
  }
});





// Download template
router.get(
  "/:name/:version/download",
  optionalAuth,
  async (req: Request, res: Response) => {
    try {
      const { name, version } = req.params;

      const results = await templateStore.findByName(name);
      const tpl = results.find((t) => t.version === version) || results[0];

      if (!tpl) {
        return res.status(404).json({ error: "Template not found" });
      }

      await templateStore.incrementDownloads(tpl.id);
      searchAnalytics.recordInteraction(req.userId, tpl.id, "download");

      const filePath = path.join(
        STORAGE_DIR,
        `${tpl.name}-${tpl.version}-${tpl.id}.zip`,
      );

      if (!fs.existsSync(filePath)) {
        return res.status(404).json({ error: "Template file not found" });
      }

      res.download(filePath, `${tpl.name}-${tpl.version}.zip`);
    } catch (err) {
      logger.error("Download error", err);
      res.status(500).json({ error: "Download failed" });
    }
  },
);

export default router;
