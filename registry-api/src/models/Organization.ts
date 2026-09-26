import { v4 as uuid } from "uuid";

export type OrganizationRole = "owner" | "admin" | "maintainer";

export interface IOrganizationMember {
  userId: string;
  role: OrganizationRole;
  joinedAt: Date;
}

export interface IOrganization {
  id: string;
  slug: string;
  name: string;
  members: IOrganizationMember[];
  createdAt: Date;
  updatedAt: Date;
}

export class OrganizationStore {
  private organizations = new Map<string, IOrganization>();

  async create(slug: string, name: string, ownerId: string): Promise<IOrganization> {
    const organization: IOrganization = {
      id: uuid(),
      slug,
      name,
      members: [{ userId: ownerId, role: "owner", joinedAt: new Date() }],
      createdAt: new Date(),
      updatedAt: new Date(),
    };
    this.organizations.set(organization.id, organization);
    return organization;
  }

  async findBySlug(slug: string): Promise<IOrganization | null> {
    return [...this.organizations.values()].find((org) => org.slug === slug) || null;
  }

  async all(): Promise<IOrganization[]> {
    return [...this.organizations.values()];
  }

  async addMember(
    slug: string,
    userId: string,
    role: OrganizationRole,
  ): Promise<IOrganization | null> {
    const organization = await this.findBySlug(slug);
    if (!organization) return null;
    const existing = organization.members.find((member) => member.userId === userId);
    if (existing) existing.role = role;
    else organization.members.push({ userId, role, joinedAt: new Date() });
    organization.updatedAt = new Date();
    return organization;
  }

  async roleFor(slug: string, userId: string): Promise<OrganizationRole | null> {
    const organization = await this.findBySlug(slug);
    return organization?.members.find((member) => member.userId === userId)?.role || null;
  }

  async clear(): Promise<void> {
    this.organizations.clear();
  }
}

export const organizationStore = new OrganizationStore();