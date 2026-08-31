import { v4 as uuid } from "uuid";

export type OwnershipAction =
  | "PUBLISH"
  | "TRANSFER_OWNERSHIP"
  | "UPDATE"
  | "UNPUBLISH";

export interface IOwnershipRecord {
  id: string;
  templateId: string;
  templateName: string;
  version: string;
  publisherId: string;
  publisherUsername?: string;
  previousPublisherId?: string;
  action: OwnershipAction;
  timestamp: Date;
  ipAddress?: string;
  metadata?: Record<string, any>;
}

export class OwnershipHistoryStore {
  private records: IOwnershipRecord[] = [];

  async record(
    event: Omit<IOwnershipRecord, "id" | "timestamp"> & { timestamp?: Date },
  ): Promise<IOwnershipRecord> {
    const newRecord: IOwnershipRecord = {
      id: uuid(),
      timestamp: event.timestamp || new Date(),
      ...event,
    };
    this.records.push(newRecord);
    return newRecord;
  }

  async getHistoryForTemplate(templateName: string): Promise<IOwnershipRecord[]> {
    return this.records
      .filter((r) => r.templateName.toLowerCase() === templateName.toLowerCase())
      .sort((a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime());
  }

  async getHistoryByTemplateId(templateId: string): Promise<IOwnershipRecord[]> {
    return this.records
      .filter((r) => r.templateId === templateId)
      .sort((a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime());
  }

  async all(): Promise<IOwnershipRecord[]> {
    return [...this.records];
  }

  async clear(): Promise<void> {
    this.records = [];
  }
}

export const ownershipHistoryStore = new OwnershipHistoryStore();
