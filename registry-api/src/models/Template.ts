export interface ITemplate {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  tags: string[];
  // Structured functionality keywords (e.g. ["escrow", "refund", "timeout"])
  // used by search to match "find a template that does X" style queries
  // even when the words aren't in the description.
  functionality?: string[];
  license?: string;
  repository?: string;
  homepage?: string;
  documentation?: string;
  downloads: number;
  verified: boolean;
  publisherId: string;
  createdAt: Date;
  updatedAt: Date;
  ratings: {
    average: number;
    count: number;
    distribution: { [key: number]: number };
  };
  downloadUrl: string;
}

export class TemplateStore {
  private templates: Map<string, ITemplate> = new Map();

  async create(template: ITemplate): Promise<ITemplate> {
    this.templates.set(template.id, template);
    return template;
  }

  async findById(id: string): Promise<ITemplate | null> {
    return this.templates.get(id) || null;
  }

  async findByNameAndVersion(
    name: string,
    version?: string,
  ): Promise<ITemplate | null> {
    for (const tpl of this.templates.values()) {
      if (tpl.name === name && (!version || tpl.version === version)) {
        return tpl;
      }
    }
    return null;
  }

  /** All templates currently in the store, in no particular order. */
  async all(): Promise<ITemplate[]> {
    return Array.from(this.templates.values());
  }

  async findByName(name: string): Promise<ITemplate[]> {
    const results: ITemplate[] = [];
    for (const tpl of this.templates.values()) {
      if (tpl.name === name) {
        results.push(tpl);
      }
    }
    return results.sort(
      (a, b) =>
        new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime(),
    );
  }

  async search(
    query: string,
    tags?: string[],
    verified?: boolean,
    minQuality?: number,
  ): Promise<ITemplate[]> {
    const results: ITemplate[] = [];
    const queryLower = query.toLowerCase();

    for (const tpl of this.templates.values()) {
      if (
        tags &&
        !tags.every((tag) =>
          tpl.tags.some((t) => t.toLowerCase() === tag.toLowerCase()),
        )
      ) {
        continue;
      }

      if (verified && !tpl.verified) {
        continue;
      }

      if (
        !query ||
        tpl.name.toLowerCase().includes(queryLower) ||
        tpl.description.toLowerCase().includes(queryLower) ||
        tpl.tags.some((t) => t.toLowerCase().includes(queryLower))
      ) {
        results.push(tpl);
      }
    }

    return results.sort((a, b) => b.downloads - a.downloads);
  }

  async update(
    id: string,
    updates: Partial<ITemplate>,
  ): Promise<ITemplate | null> {
    const tpl = this.templates.get(id);
    if (!tpl) return null;
    const updated = { ...tpl, ...updates };
    this.templates.set(id, updated);
    return updated;
  }

  async incrementDownloads(id: string): Promise<void> {
    const tpl = this.templates.get(id);
    if (tpl) {
      tpl.downloads++;
      this.templates.set(id, tpl);
    }
  }

  async delete(id: string): Promise<boolean> {
    return this.templates.delete(id);
  }

  async findPublisherForName(name: string): Promise<string | null> {
    for (const tpl of this.templates.values()) {
      if (tpl.name.toLowerCase() === name.toLowerCase()) {
        return tpl.publisherId;
      }
    }
    return null;
  }

  async updatePublisherForName(
    name: string,
    newPublisherId: string,
  ): Promise<number> {
    let count = 0;
    for (const [id, tpl] of this.templates.entries()) {
      if (tpl.name.toLowerCase() === name.toLowerCase()) {
        this.templates.set(id, {
          ...tpl,
          publisherId: newPublisherId,
          updatedAt: new Date(),
        });
        count++;
      }
    }
    return count;
  }

  async clear(): Promise<void> {
    this.templates.clear();
  }
}

