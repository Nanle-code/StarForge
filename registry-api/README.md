# StarForge Remote Template Registry API

A centralized remote template registry API that allows global template sharing, versioning, and community contributions. Creates a template marketplace similar to npm or crates.io.

## Features

- ✓ Remote template search with filters (tags, verified, quality score)
- ✓ Template download and installation from remote
- ✓ User authentication with JWT tokens
- ✓ Publisher authentication and strict template name ownership enforcement
- ✓ Rate-limited publish, mutation and search operations with fair-use headers
- ✓ Auditable template ownership history log and ownership transfer capabilities
- ✓ Organization namespaces (`@org/template`) with owner, admin, and maintainer roles
- ✓ Two-party ownership transfers requiring confirmation by the receiving party
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

Publish and search endpoints are rate limited with a per-client token bucket.
Authenticated callers are limited per user and anonymous callers per IP
address (the connection's peer address; `X-Forwarded-For` is not trusted).

| Policy | Endpoints | Default limit |
|---|---|---|
| `publish` | `POST /api/templates/publish`, `POST /api/templates/:name/transfer-ownership`, `POST /api/templates/:name/transfer-ownership/confirm` | 10 per minute per user |
| `search` | `POST /api/templates/search`, `GET /api/templates/search/suggestions` | 60 per minute per user, 30 per minute per IP when anonymous |

- **Environment Configuration:** `PUBLISH_RATE_LIMIT_MAX`,
  `PUBLISH_RATE_LIMIT_WINDOW_MS`, `SEARCH_RATE_LIMIT_MAX`,
  `SEARCH_RATE_LIMIT_ANON_MAX`, `SEARCH_RATE_LIMIT_WINDOW_MS`.
- **Response Headers:** `RateLimit-Limit`, `RateLimit-Remaining`,
  `RateLimit-Reset` (seconds until the quota is fully restored) and
  `RateLimit-Policy` on every limited response; `Retry-After` (seconds until
  the next request is accepted) on `429 Too Many Requests`. The legacy
  `X-RateLimit-*` headers are still sent (`X-RateLimit-Reset` is a Unix time).

See [RATE_LIMITING.md](./RATE_LIMITING.md) for how clients are identified, the
header semantics, deployment caveats, and how clients should back off.

### Ownership Enforcement & Migration Notes

- **Publisher Authentication**: When publishing a template name for the first time, the publisher's user identity (`publisherId`) is bound to the template name.
- **Ownership Verification**: Subsequent version releases under an existing template name are restricted to the registered owner (HTTP `403 Forbidden` if attempted by a non-owner).
- **Ownership Transfers**: The registered owner may transfer template ownership to another registered user (`POST /api/templates/:name/transfer-ownership`).
- **Auditable Audit Log**: All publish events and ownership transfers are appended to an immutable audit trail (`GET /api/templates/:name/ownership-history`).

## API Endpoints

The complete OpenAPI 3.0 contract is available in [`openapi.json`](./openapi.json)
and as an asset on each GitHub release at
[releases/latest/download/openapi.json](https://github.com/Nanle-code/StarForge/releases/latest/download/openapi.json).

### Authentication

- `POST /api/auth/signup` - Create account
- `POST /api/auth/login` - Login (returns JWT token)
- `POST /api/auth/verify` - Verify token

### Templates

- `POST /api/templates/search` - Search registry (rate-limited)
- `GET /api/templates/:name/ownership-history` - Query template ownership audit history
- `POST /api/templates/:name/transfer-ownership` - Transfer template ownership (auth required, rate-limited)
- `POST /api/templates/:name/transfer-ownership/confirm` - Confirm a pending ownership transfer
- `GET /api/templates/:name/:version` - Get template details
- `POST /api/templates/publish` - Publish template (publisher auth required, rate-limited)
- `GET /api/templates/:name/:version/download` - Download template

### Organizations

- `POST /api/orgs` - Create an organization; the creator becomes its owner
- `GET /api/orgs` - List organizations
- `POST /api/orgs/:slug/members` - Add or update a member role (`owner`, `admin`, or `maintainer`)

Publish with an `org` field to create the canonical `@org/template` name. The
organization member roles are owner/admin/maintainer: maintainers can publish,
while owners and admins manage membership and confirm organization transfers.

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

### Organization Publish and Transfer Confirmation

```bash
starforge registry org create stellar-tools --name "Stellar Tools"
starforge registry org add-member stellar-tools teammate --role maintainer
starforge registry publish ./my-template --org stellar-tools
```

Ownership transfer requests return HTTP `202` and a `transfer_id`. The
receiving user or organization admin confirms that ID with the confirmation
endpoint; the event is added to ownership history only after confirmation.

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
