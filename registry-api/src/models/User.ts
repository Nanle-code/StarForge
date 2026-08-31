export interface IUser {
  id: string;
  username: string;
  email: string;
  passwordHash: string;
  createdAt: Date;
  updatedAt: Date;
  verified: boolean;
}

// In-memory user storage (replace with MongoDB in production)
export class UserStore {
  private users: Map<string, IUser> = new Map();

  async create(user: IUser): Promise<IUser> {
    this.users.set(user.id, user);
    return user;
  }

  async findById(id: string): Promise<IUser | null> {
    return this.users.get(id) || null;
  }

  async findByEmail(email: string): Promise<IUser | null> {
    for (const user of this.users.values()) {
      if (user.email === email) {
        return user;
      }
    }
    return null;
  }

  async findByUsername(username: string): Promise<IUser | null> {
    for (const user of this.users.values()) {
      if (user.username === username) {
        return user;
      }
    }
    return null;
  }

  async update(id: string, updates: Partial<IUser>): Promise<IUser | null> {
    const user = this.users.get(id);
    if (!user) return null;
    const updated = { ...user, ...updates };
    this.users.set(id, updated);
    return updated;
  }

  async clear(): Promise<void> {
    this.users.clear();
  }
}

export const userStore = new UserStore();

