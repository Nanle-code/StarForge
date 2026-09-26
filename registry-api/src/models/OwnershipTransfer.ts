import { v4 as uuid } from "uuid";

export interface IPendingOwnershipTransfer {
  id: string;
  templateName: string;
  requestedBy: string;
  targetUserId?: string;
  targetOrganizationSlug?: string;
  createdAt: Date;
}

export class OwnershipTransferStore {
  private transfers = new Map<string, IPendingOwnershipTransfer>();

  async create(
    transfer: Omit<IPendingOwnershipTransfer, "id" | "createdAt">,
  ): Promise<IPendingOwnershipTransfer> {
    const pending = { ...transfer, id: uuid(), createdAt: new Date() };
    this.transfers.set(pending.id, pending);
    return pending;
  }

  async find(id: string): Promise<IPendingOwnershipTransfer | null> {
    return this.transfers.get(id) || null;
  }

  async delete(id: string): Promise<void> {
    this.transfers.delete(id);
  }

  async clear(): Promise<void> {
    this.transfers.clear();
  }
}

export const ownershipTransferStore = new OwnershipTransferStore();