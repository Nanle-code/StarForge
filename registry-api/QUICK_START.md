# StarForge Registry API - Quick Start

## Prerequisites

- Node.js 18+
- npm or yarn
- (Optional) Docker & Docker Compose
- (Optional) MongoDB

## Setup (5 minutes)

### 1. Install dependencies

```bash
cd registry-api
npm install
```

### 2. Set up environment

```bash
cp .env.example .env
# Edit .env if needed, defaults should work for development
```

### 3. Start development server

```bash
npm run dev
```

Server starts on `http://localhost:3000`

## Testing the API

### 1. Health Check

```bash
curl http://localhost:3000/health
```

### 2. Create Account

```bash
curl -X POST http://localhost:3000/api/auth/signup \
  -H "Content-Type: application/json" \
  -d '{
    "email": "test@example.com",
    "username": "testuser",
    "password": "password123"
  }'
```

### 3. Search Templates

```bash
curl -X POST http://localhost:3000/api/templates/search \
  -H "Content-Type: application/json" \
  -d '{"query": "counter"}'
```

### 4. Get Template Details

```bash
curl http://localhost:3000/api/templates/simple-counter/1.0.0
```

### 5. Query Ownership Audit History

```bash
curl http://localhost:3000/api/templates/simple-counter/ownership-history
```

### 6. Transfer Template Ownership

```bash
curl -X POST http://localhost:3000/api/templates/simple-counter/transfer-ownership \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <token>" \
  -d '{"new_username": "new_owner"}'
```

## Using Docker

```bash
docker-compose up
```

This starts:

- Registry API on port 3000
- MongoDB on port 27017

Access web UI: `http://localhost:3000`

View logs:

```bash
docker-compose logs -f registry-api
```

Stop:

```bash
docker-compose down
```

## CLI Integration

### 1. Configure registry

```bash
starforge registry config --url http://localhost:3000
```

### 2. Search templates

```bash
starforge registry search counter
```

### 3. Login

```bash
starforge registry login
```

### 4. Publish template

```bash
starforge registry publish ./my-template \
  --name my-template \
  --author "Your Name" \
  --description "My awesome template" \
  --tags "example,educational"
```

### 5. Install template

```bash
starforge registry install my-template
```

### 6. Rate template

```bash
starforge registry review my-template --rating 5 --comment "Great!"
```

### 7. Check status

```bash
starforge registry status
```

## Web Interface

Visit `http://localhost:3000`

Features:

- Browse and search templates
- View ratings and reviews
- Login/Signup
- Template details
- One-click install commands

## Database Setup (Optional)

For MongoDB persistence:

```bash
docker run -d -p 27017:27017 mongo:6.0
```

Update `.env`:

```
MONGODB_URI=mongodb://localhost:27017/starforge-registry
```

Restart API:

```bash
npm run dev
```

## Production Build

```bash
npm run build
npm start
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

## Common Commands

**Development:**

```bash
npm run dev              # Start with auto-reload
npm run lint            # Check code style
npm run build           # Compile TypeScript
npm test                # Run tests
```

**Production:**

```bash
npm run build           # Compile TypeScript
npm start               # Start server
npm run lint            # Check code before deploy
```

## Troubleshooting

**Port 3000 in use:**

- Change PORT in `.env`
- Or kill: `lsof -i :3000 | grep LISTEN | awk '{print $2}' | xargs kill`

**Module not found:**

```bash
npm install
rm -rf node_modules && npm install
```

**TypeScript errors:**

```bash
npm run build
# Fix errors before running
```

**MongoDB errors:**

- Verify MongoDB is running
- Check connection string in `.env`

**API errors:**

- Check console output for detailed errors
- Verify JWT_SECRET is set
- Check request headers and body format

## Next Steps

1. Deploy to your server/cloud platform
2. Configure DNS for registry domain
3. Set up HTTPS/TLS certificate
4. Update CLI default registry URL
5. Promote to users for publishing templates
6. Monitor usage and performance
7. Plan Phase 2 features

## Documentation

- **Full API docs:** [README.md](./README.md)
- **Implementation guide:** [REMOTE_REGISTRY_IMPLEMENTATION.md](../REMOTE_REGISTRY_IMPLEMENTATION.md)
- **CLI commands:** [DEVELOPER_GUIDE.md](../DEVELOPER_GUIDE.md)
- **Architecture:** [ARCHITECTURE.md](../ARCHITECTURE.md)

## Support

- **Issues:** https://github.com/Nanle-code/StarForge/issues
- **Questions:** https://github.com/Nanle-code/StarForge/discussions
