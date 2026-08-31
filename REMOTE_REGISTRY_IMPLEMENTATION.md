# Remote Template Registry - Implementation Guide

Complete implementation details for the Remote Template Registry feature for StarForge.

## Overview

Centralized template marketplace similar to npm or crates.io enabling:

- Global template sharing
- Versioning and dependency management
- Community contributions
- User authentication and publishing
- Template rating/review system
- Web interface for browsing

## Architecture

### Client (Rust CLI)

- Location: `src/commands/registry.rs`, `src/utils/registry.rs`
- HTTP client using `ureq` (synchronous)
- JWT token authentication
- Config storage in `~/.starforge/registry.toml`

### Server (Node.js/Express)

- Location: `registry-api/`
- REST API with authentication
- In-memory stores (MongoDB ready)
- Template storage as ZIP archives
- Review/rating system
- Web UI for browsing

### Database

- **Users**: email, username, password hash, metadata
- **Templates**: name, version, description, tags, ratings, download URL
- **Reviews**: template ID, user ID, rating (1-5), comment

## Project Structure

```
registry-api/
├── src/
│   ├── index.ts              # Express app
│   ├── routes/
│   │   ├── auth.ts          # Auth endpoints
│   │   ├── templates.ts     # Template endpoints
│   │   └── reviews.ts       # Review endpoints
│   ├── models/
│   │   ├── User.ts
│   │   ├── Template.ts
│   │   └── Review.ts
│   ├── middleware/
│   │   ├── auth.ts          # JWT verification
│   │   └── errorHandler.ts
│   ├── utils/
│   │   └── logger.ts
│   └── tests/
│       └── api.test.ts
├── public/
│   └── index.html           # Web UI
├── package.json
├── tsconfig.json
├── Dockerfile
└── docker-compose.yml
```

## Features Implemented

### ✓ Remote Template Search

- Full-text search on name, description, tags
- Filter by verified status, quality score, tags
- Pagination support (limit, offset)
- Relevance-based ranking

### ✓ Template Versioning

- Semantic versioning support
- CLI version compatibility checks
- Multiple versions of same template
- Latest version resolution

### ✓ User Authentication

- Signup/login with email and password
- JWT token-based auth
- Bcrypt password hashing (10 rounds)
- Token expiration (7 days default)

### ✓ Template Publishing

- Authenticated upload via ZIP
- Metadata validation
- Version management
- Publisher tracking

### ✓ Rating & Review System

- 1-5 star ratings
- User comments
- Average rating calculation
- Rating distribution tracking

### ✓ Web Interface

- Template browsing with search
- Login/signup forms
- Template details
- Rating display

### ✓ CLI Integration

Commands:

- `registry search <query>` - Search remote
- `registry login` - Authenticate
- `registry publish <path>` - Publish template
- `registry install <name>` - Download from remote
- `registry review <name>` - Rate template
- `registry status` - Show login status
- `registry config --url <url>` - Configure endpoint

## API Endpoints

### Authentication

**POST /api/auth/signup**

```json
Request: { email, username, password }
Response: { success, token, username }
```

**POST /api/auth/login**

```json
Request: { email, password }
Response: { success, token, username }
```

**POST /api/auth/verify**

```
Headers: Authorization: Bearer <token>
Response: { success, user }
```

### Templates

**POST /api/templates/search**

```json
Request: { query, tags[], verified?, min_quality?, limit, offset }
Response: { success, results[], total, limit, offset }
```

**GET /api/templates/:name/:version**

```json
Response: {
  id, name, version, description, author, tags,
  license, repository, homepage, documentation,
  downloads, verified, ratings, download_url
}
```

**POST /api/templates/publish** (auth required)

```json
Request: {
  name, version, description, author, tags,
  license, repository, homepage, documentation,
  content (base64)
}
Response: { success, message, template_id, url }
```

**GET /api/templates/:name/:version/download**

```
Response: [binary zip file]
```

### Reviews

**GET /api/reviews/template/:templateId**

```json
Response: { success, reviews[], total }
```

**POST /api/reviews/template/:templateId/reviews** (auth required)

```json
Request: { rating (1-5), comment? }
Response: { success, message }
```

## Workflows

### Publishing a Template

1. User runs: `starforge registry login`
   - Prompts for email/password
   - Sends to `/api/auth/login`
   - Stores JWT token in `~/.starforge/registry.toml`

2. User runs: `starforge registry publish --name my-template ...`
   - Creates ZIP archive of template
   - Base64 encodes ZIP
   - Sends to `/api/templates/publish` with auth token
   - Server stores template and metadata

3. Template appears in search results

### Searching & Installing

1. User runs: `starforge registry search "counter"`
   - CLI calls `/api/templates/search`
   - Server returns matching templates
   - CLI displays results with ratings, downloads

2. User runs: `starforge registry install simple-counter`
   - CLI calls `/api/templates/simple-counter/latest`
   - Server returns download URL
   - CLI downloads ZIP from `/api/templates/.../download`
   - CLI extracts and installs locally
   - Download count incremented

## Configuration

### Registry URL

- Default: `https://registry.starforge.dev`
- Override: `export STARFORGE_TEMPLATE_REGISTRY_URL=http://localhost:3000`
- CLI command: `starforge registry config --url http://localhost:3000`

### Local Config

File: `~/.starforge/registry.toml`

```toml
[registry]
url = "https://registry.starforge.dev"
token = "eyJ..."
username = "alice"
email = "alice@example.com"
```

### Environment Variables (Server)

| Variable       | Description                | Default             |
| -------------- | -------------------------- | ------------------- |
| PORT           | API server port            | 3000                |
| NODE_ENV       | development/production     | development         |
| JWT_SECRET     | Secret for signing tokens  | secret              |
| JWT_EXPIRATION | Token expiration           | 7d                  |
| MONGODB_URI    | MongoDB connection         | localhost:27017     |
| STORAGE_DIR    | Template storage directory | ./storage/templates |
| MAX_FILE_SIZE  | Max upload size            | 50MB                |
| CORS_ORIGIN    | CORS allowed origins       | \*                  |

## Deployment

### Local Development

```bash
cd registry-api
npm install
npm run dev
```

### Docker Compose

```bash
docker-compose up
```

Runs API + MongoDB

### Production

```bash
npm run build
NODE_ENV=production npm start
```

With Docker:

```bash
docker build -t starforge-registry:latest .
docker run -d -p 3000:3000 \
  -e NODE_ENV=production \
  -e JWT_SECRET=<random> \
  -e MONGODB_URI=<production-db> \
  starforge-registry:latest
```

## Security

### Passwords

- Hashed with bcrypt (10 rounds, ~100ms per hash)
- Never stored in plaintext
- Always use HTTPS in production

### Tokens

- JWT with expiration (7 days default)
- Stored locally in config file
- Transmitted in Authorization header: `Bearer <token>`

### File Uploads

- Limited to 50MB (configurable)
- Stored as ZIP archives
- Validated before storage
- Served from `/storage/templates/` directory

### Input Validation

- All inputs validated before processing
- Email format validation
- Username uniqueness check
- Password strength requirements
- Template metadata validation

### Rate Limiting (Recommended)

- 5 signups/hour per IP
- 10 login attempts/15min per IP
- 100 searches/hour per IP

### Archive Extraction (Client-Side)

`templates::extract_zip_archive` (used by both `registry install` and local
`.zip` template sources — see [`src/utils/templates.rs`](src/utils/templates.rs))
treats every downloaded archive as untrusted before any of its contents
touch disk. The whole archive is rejected — no partial extraction — if any
entry:

- uses an **absolute path** (e.g. `/etc/passwd`) or a **`..`
  parent-traversal component** that would resolve outside the archive root,
- is a **symlink** (detected via the entry's Unix mode bits), or
- would still resolve **outside the destination directory** after joining,
  as a final defense-in-depth check (zip-slip).

A malicious or corrupted archive — from a compromised registry, a
tampered download, or a hostile third-party `.zip` template — therefore
fails extraction cleanly with a descriptive error instead of silently
dropping the dangerous entries or writing files outside the intended
install location.

## Database Schema (MongoDB)

### Users Collection

```javascript
{
  _id: ObjectId,
  email: String (unique, indexed),
  username: String (unique, indexed),
  passwordHash: String,
  createdAt: Date,
  updatedAt: Date,
  verified: Boolean
}
```

### Templates Collection

```javascript
{
  _id: ObjectId,
  name: String (indexed),
  version: String,
  description: String,
  author: String,
  tags: [String],
  license: String,
  repository: String,
  homepage: String,
  documentation: String,
  downloads: Number,
  verified: Boolean,
  publisherId: ObjectId,
  createdAt: Date,
  updatedAt: Date,
  ratings: {
    average: Number,
    count: Number,
    distribution: { 1: N, 2: N, 3: N, 4: N, 5: N }
  },
  downloadUrl: String,
  storageKey: String
}
```

### Reviews Collection

```javascript
{
  _id: ObjectId,
  templateId: ObjectId,
  userId: ObjectId,
  rating: Number (1-5),
  comment: String,
  createdAt: Date,
  updatedAt: Date
}
```

## Testing

### Unit Tests

```bash
npm test
```

### Manual Testing

See `QUICK_START.md` for curl examples

### Load Testing

Simulate high concurrency with templates, searches, and downloads

## Monitoring & Logging

### Logging Levels

- **INFO**: User actions, API calls
- **WARN**: Potential issues
- **ERROR**: Failures, exceptions
- **DEBUG**: Development troubleshooting

### Metrics

- Signups/logins per day
- Templates published per day
- Search queries per day
- Template downloads per day
- Average response time
- Error rate
- Storage usage

## Roadmap

### Phase 2

- Template categories/subcategories
- Featured/trending templates
- Template recommendations
- Dependency graph visualization

### Phase 3

- User profiles/portfolios
- Template discussions
- Community moderation
- Badge system

### Phase 4

- Analytics dashboard
- Performance metrics
- Usage insights

### Phase 5

- Private registry instances
- Organization accounts
- Access control lists
- Audit logging

## Support & Contribution

- **Issues**: https://github.com/Nanle-code/StarForge/issues
- **Discussions**: https://github.com/Nanle-code/StarForge/discussions
- **Contributing**: See CONTRIBUTING.md

## References

- [Quick Start](./registry-api/QUICK_START.md)
- [API Documentation](./registry-api/README.md)
- [Architecture](./ARCHITECTURE.md)
- [Developer Guide](./DEVELOPER_GUIDE.md)
