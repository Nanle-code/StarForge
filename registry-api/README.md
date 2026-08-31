# StarForge Remote Template Registry API

A centralized remote template registry API that allows global template sharing, versioning, and community contributions. Creates a template marketplace similar to npm or crates.io.

## Features

- ✓ Remote template search with filters (tags, verified, quality score)
- ✓ Template download and installation from remote
- ✓ User authentication with JWT tokens
- ✓ Publisher authentication and strict template name ownership enforcement
- ✓ Rate-limited publish and mutation operations
- ✓ Auditable template ownership history log and ownership transfer capabilities
- ✓ Template rating and review system
- ✓ Web interface for template browsing
- ✓ RESTful API for CLI integration

## Quick Start

```bash
npm install
cp .env.example .env
npm run dev
```

Server runs on `http://localhost:3000`

## Docker

```bash
docker-compose up
```

Starts Registry API + MongoDB

## Rate Limiting & Security

All template mutation operations (`POST /api/templates/publish`, `POST /api/templates/:name/transfer-ownership`) are rate-limited per publisher/IP.

- **Environment Configuration:**
  - `PUBLISH_RATE_LIMIT_WINDOW_MS`: Rate limit window in milliseconds (default: `60000` ms / 1 minute).
  - `PUBLISH_RATE_LIMIT_MAX`: Maximum mutation requests allowed per window (default: `10`).
- **Response Headers:**
  - `X-RateLimit-Limit`: Maximum allowable requests per window.
  - `X-RateLimit-Remaining`: Remaining allowable requests in the current window.
  - `X-RateLimit-Reset`: UTC epoch timestamp in seconds when the rate limit window resets.
  - `Retry-After`: Seconds to wait before retrying when HTTP `429 Too Many Requests` is returned.

### Ownership Enforcement & Migration Notes

- **Publisher Authentication**: When publishing a template name for the first time, the publisher's user identity (`publisherId`) is bound to the template name.
- **Ownership Verification**: Subsequent version releases under an existing template name are restricted to the registered owner (HTTP `403 Forbidden` if attempted by a non-owner).
- **Ownership Transfers**: The registered owner may transfer template ownership to another registered user (`POST /api/templates/:name/transfer-ownership`).
- **Auditable Audit Log**: All publish events and ownership transfers are appended to an immutable audit trail (`GET /api/templates/:name/ownership-history`).

## API Endpoints

### Authentication

- `POST /api/auth/signup` - Create account
- `POST /api/auth/login` - Login (returns JWT token)
- `POST /api/auth/verify` - Verify token

### Templates

- `POST /api/templates/search` - Search registry
- `GET /api/templates/:name/ownership-history` - Query template ownership audit history
- `POST /api/templates/:name/transfer-ownership` - Transfer template ownership (auth required, rate-limited)
- `GET /api/templates/:name/:version` - Get template details
- `POST /api/templates/publish` - Publish template (publisher auth required, rate-limited)
- `GET /api/templates/:name/:version/download` - Download template

### Reviews

- `GET /api/reviews/template/:templateId` - Get reviews
- `POST /api/reviews/template/:templateId/reviews` - Post review (auth required)

## Request Examples

### Search Templates

```bash
curl -X POST http://localhost:3000/api/templates/search \
  -H "Content-Type: application/json" \
  -d '{
    "query": "counter",
    "tags": ["example"],
    "verified": true,
    "limit": 20,
    "offset": 0
  }'
```

### Get Ownership History

```bash
curl http://localhost:3000/api/templates/my-template/ownership-history
```

### Transfer Ownership

```bash
curl -X POST http://localhost:3000/api/templates/my-template/transfer-ownership \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{
    "new_username": "new_owner"
  }'
```

### Publish Template

```bash
curl -X POST http://localhost:3000/api/templates/publish \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{
    "name": "my-template",
    "version": "1.0.0",
    "description": "My template",
    "author": "Your Name",
    "tags": ["example"],
    "content": "<base64-encoded-zip>"
  }'
```

## CLI Integration

```bash
starforge registry search counter
starforge registry login
starforge registry publish ./my-template
starforge registry install my-template
starforge registry review my-template --rating 5
```

## Production Deployment

```bash
npm run build
NODE_ENV=production npm start
```

With Docker:

```bash
docker build -t starforge-registry:latest .
docker run -d -p 3000:3000 \
  -e NODE_ENV=production \
  -e JWT_SECRET=your-secret \
  -e MONGODB_URI=your-db \
  starforge-registry:latest
```

## Development

```bash
npm run dev      # Development server
npm run lint     # Lint code
npm run build    # Build TypeScript
npm test         # Run tests
```

## Documentation

- [Quick Start Guide](./QUICK_START.md)
- [Implementation Guide](../REMOTE_REGISTRY_IMPLEMENTATION.md)
- [Developer Guide](../DEVELOPER_GUIDE.md)
- [Architecture](../ARCHITECTURE.md)

## Support

- **Issues:** https://github.com/Nanle-code/StarForge/issues
- **Discussions:** https://github.com/Nanle-code/StarForge/discussions

## License

MIT
