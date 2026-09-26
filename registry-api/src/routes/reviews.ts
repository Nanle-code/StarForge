import express, { Request, Response } from "express";
import { v4 as uuid } from "uuid";
import { ReviewStore, IReview } from "../models/Review";
import { TemplateStore } from "../models/Template";
import { verifyToken, optionalAuth } from "../middleware/auth";
import logger from "../utils/logger";

const router = express.Router();
const reviewStore = new ReviewStore();
const templateStore = new TemplateStore();

// Get reviews for a template
router.get(
  "/template/:templateId",
  optionalAuth,
  async (req: Request, res: Response) => {
    try {
      const templateId = req.params.templateId as string;
      const reviews = await reviewStore.findByTemplateId(templateId);

      res.json({
        success: true,
        reviews: reviews.map((r) => ({
          id: r.id,
          rating: r.rating,
          comment: r.comment,
          created_at: r.createdAt,
        })),
        total: reviews.length,
      });
    } catch (err) {
      logger.error("Get reviews error", err);
      res.status(500).json({ error: "Failed to fetch reviews" });
    }
  },
);

// Post a review
router.post(
  "/template/:templateId/reviews",
  verifyToken,
  async (req: Request, res: Response) => {
    try {
      const templateId = req.params.templateId as string;
      const { rating, comment } = req.body;

      if (rating < 1 || rating > 5) {
        return res
          .status(400)
          .json({ error: "Rating must be between 1 and 5" });
      }

      // Check if user already reviewed this template
      const existing = await reviewStore.findByUserAndTemplate(
        req.userId!,
        templateId,
      );
      if (existing) {
        // Update existing review
        await reviewStore.update(existing.id, {
          rating,
          comment,
          updatedAt: new Date(),
        });

        // Recalculate template ratings
        await recalculateTemplateRatings(templateId);

        logger.info(`Review updated: ${templateId} by user ${req.userId}`);
        return res.json({
          success: true,
          message: "Review updated",
        });
      }

      const review: IReview = {
        id: uuid(),
        templateId,
        userId: req.userId!,
        rating,
        comment,
        createdAt: new Date(),
        updatedAt: new Date(),
      };

      await reviewStore.create(review);
      await recalculateTemplateRatings(templateId);

      logger.info(`Review posted: ${templateId} by user ${req.userId}`);

      res.status(201).json({
        success: true,
        message: "Review posted successfully",
      });
    } catch (err) {
      logger.error("Post review error", err);
      res.status(500).json({ error: "Failed to post review" });
    }
  },
);

async function recalculateTemplateRatings(templateId: string): Promise<void> {
  const reviews = await reviewStore.findByTemplateId(templateId);

  if (reviews.length === 0) {
    const tpl = await templateStore.findById(templateId);
    if (tpl) {
      tpl.ratings = { average: 0, count: 0, distribution: {} };
      await templateStore.update(templateId, tpl);
    }
    return;
  }

  const distribution: { [key: number]: number } = {
    1: 0,
    2: 0,
    3: 0,
    4: 0,
    5: 0,
  };
  let sum = 0;

  for (const review of reviews) {
    sum += review.rating;
    distribution[review.rating]++;
  }

  const average = sum / reviews.length;
  const tpl = await templateStore.findById(templateId);

  if (tpl) {
    tpl.ratings = {
      average,
      count: reviews.length,
      distribution,
    };
    await templateStore.update(templateId, tpl);
  }
}

export default router;
