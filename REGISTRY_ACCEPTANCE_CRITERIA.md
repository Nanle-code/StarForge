# Remote Template Registry - Acceptance Criteria

## Overview

This document defines the acceptance criteria for the Remote Template Registry implementation.

## Acceptance Criteria

### ✅ 1. Remote Template Search Works

**Criteria**: Users can search the remote registry with various filters

- [ ] **Search by query**: User can search by template name, description, or tags
  - Test: `starforge registry search "counter"`
  - Expected: Returns matching templates with names, versions, authors, ratings

- [ ] **Filter by tags**: User can filter templates by multiple tags
  - Test: `starforge registry search "defi" --tags "example,educational"`
  - Expected: Only templates with ALL specified tags are returned

- [ ] **Filter by verified status**: User can filter for verified templates only
  - Test: `starforge registry search "" --verified`
  - Expected: Only verified templates are returned

- [ ] **Quality score filter**: User can filter by minimum quality score
  - Test: `starforge registry search "" --min-quality 75`
  - Expected: Only templates with quality ≥ 75 are returned

- [ ] **Pagination**: Search results are paginated
  - Test: `starforge registry search "template" --limit 5`
  - Expected: Returns max 5 results, supports offset

- [ ] **Web UI search**: Users can search via web interface
  - Test: Visit `http://localhost:3000`, search for templates
  - Expected: Results display with ratings, download counts, verification status

### ✅ 2. Template Download and Installation from Remote

**Criteria**: Users can download and install templates from the registry

- [ ] **Download template**: User can download a template from remote registry
  - Test: `starforge registry install simple-counter`
  - Expected: Template is downloaded and installed to local registry

- [ ] **Specific version download**: User can download specific template version
  - Test: `starforge registry install simple-counter --version 1.0.0`
  - Expected: Specific version is downloaded

- [ ] **Latest version resolution**: Default to latest version if not specified
  - Test: `starforge registry install simple-counter`
  - Expected: Latest available version is installed

- [ ] **ZIP extraction**: Downloaded templates are properly extracted
  - Test: Install template and verify structure
  - Expected: Template files are in `~/.starforge/templates/storage/<name>/`

- [ ] **Template validation**: Only valid templates can be installed
  - Test: Attempt to install invalid template
  - Expected: Installation fails with validation error

- [ ] **Download counter increments**: Each download increments template's download count
  - Test: Download template, check registry
  - Expected: `downloads` field increases

- [ ] **Local caching**: Downloaded templates are cached locally
  - Test: Install template twice
  - Expected: Second install uses cached copy

### ✅ 3. User Authentication and Publishing

**Criteria**: Users can authenticate and publish templates to registry

- [ ] **Signup new account**: User can create new registry account
  - Test: `starforge registry signup`
  - Expected: Account created, JWT token returned, stored locally

- [ ] **Email validation**: Signup validates email format
  - Test: Attempt signup with invalid email
  - Expected: Signup fails with validation error

- [ ] **Username uniqueness**: Signup checks username is unique
  - Test: Attempt signup with existing username
  - Expected: Signup fails, error message shown

- [ ] **Password strength**: Signup enforces password requirements
  - Test: Attempt signup with password < 8 characters
  - Expected: Signup fails with message

- [ ] **Login to registry**: User can login with credentials
  - Test: `starforge registry login`
  - Expected: JWT token obtained, stored in `~/.starforge/registry.toml`

- [ ] **Token persistence**: Login token persists locally
  - Test: Login, check `~/.starforge/registry.toml`
  - Expected: Token is stored, survives shell restart

- [ ] **Logout**: User can logout
  - Test: `starforge registry logout`
  - Expected: Token removed, user is logged out

- [ ] **Publish template**: Authenticated user can publish template
  - Test: `starforge registry publish ./my-template --name "my-tpl" --author "John" --description "test"`
  - Expected: Template published, appears in search results

- [ ] **Template validation on publish**: Template structure is validated
  - Test: Attempt publish with invalid template (missing Cargo.toml)
  - Expected: Publish fails with descriptive error

- [ ] **Metadata requirements**: Publish requires all metadata fields
  - Test: Publish without description, author, etc.
  - Expected: Publish fails, shows missing field

- [ ] **ZIP archive creation**: Templates are stored as ZIP archives
  - Test: Publish template, check storage directory
  - Expected: ZIP file exists in `storage/templates/`

### ✅ 4. Template Versioning and Updates

**Criteria**: Templates support semantic versioning and updates

- [ ] **Version tagging**: Templates are tagged with semantic versions
  - Test: Publish template with `--version 1.0.0`
  - Expected: Version stored and queryable

- [ ] **Multiple versions**: Same template can have multiple versions
  - Test: Publish template v1.0.0, then v1.0.1
  - Expected: Both versions available in registry

- [ ] **Latest version selection**: When no version specified, latest is used
  - Test: Install template without specifying version
  - Expected: Latest version is installed

- [ ] **Version compatibility check**: CLI version constraints are checked
  - Test: Publish template with `--cli-version-min 0.2.0`
  - Expected: Template only installable on CLI ≥ 0.2.0

- [ ] **Version dependency display**: Version info shown in search results
  - Test: Search templates
  - Expected: Version, update date, and compatibility info displayed

### ✅ 5. Web Interface for Template Browsing

**Criteria**: Users can browse and interact with templates via web UI

- [ ] **Web UI loads**: Web interface accessible at registry URL
  - Test: Visit `http://localhost:3000`
  - Expected: Web UI loads with search box and template list

- [ ] **Search in UI**: Users can search templates in web interface
  - Test: Type in search box, press search
  - Expected: Templates matching query are displayed

- [ ] **Template display**: Templates shown with metadata
  - Test: View search results
  - Expected: Shows name, version, description, author, tags, rating, downloads

- [ ] **Rating display**: Template ratings prominently displayed
  - Test: View template in UI
  - Expected: Shows average rating, star distribution, review count

- [ ] **Verification badge**: Verified templates marked clearly
  - Test: View verified template in UI
  - Expected: Verification badge visible

- [ ] **Login in UI**: Users can login/signup via web interface
  - Test: Click login button
  - Expected: Modal prompts for credentials

- [ ] **Install command display**: One-click copy of install command
  - Test: Click template in UI
  - Expected: Install command shown and copyable

- [ ] **Rating submission from UI**: Users can rate templates from UI
  - Test: (After login) Click rate button
  - Expected: Rating modal appears, can submit 1-5 stars

### ✅ 6. Template Rating and Review System

**Criteria**: Users can rate and review templates

- [ ] **Rate templates**: Authenticated users can give 1-5 star rating
  - Test: `starforge registry review simple-counter --rating 5`
  - Expected: Rating recorded, average updated

- [ ] **Add comments**: Users can add text review
  - Test: `starforge registry review simple-counter --rating 4 --comment "Great template!"`
  - Expected: Comment stored with rating

- [ ] **Update reviews**: Users can update their own reviews
  - Test: Post review, then post new review for same template
  - Expected: Previous review is replaced

- [ ] **View reviews**: Users can view all reviews for template
  - Test: Visit template details in UI
  - Expected: Recent reviews displayed with ratings and comments

- [ ] **Rating aggregation**: Average rating calculated correctly
  - Test: Post reviews (5, 4, 3 stars), check average
  - Expected: Average = 4.0, distribution correct

- [ ] **Rating statistics**: Rating distribution shown (1/2/3/4/5 stars)
  - Test: View template with multiple reviews
  - Expected: Shows breakdown: 3 five-stars, 2 four-stars, etc.

- [ ] **Validation**: Rating must be 1-5
  - Test: Attempt rating with score 6 or 0
  - Expected: Error message, rating rejected

## Implementation Status

### Phase 1: MVP (Complete)

- ✅ Remote template search API
- ✅ Client-side search implementation
- ✅ User authentication (signup/login/logout)
- ✅ Template publishing endpoint
- ✅ Template download functionality
- ✅ Basic web UI
- ✅ Review/rating system

### Phase 2: Polish (In Progress)

- [ ] Production deployment guide
- [ ] Performance optimization
- [ ] MongoDB integration (instead of in-memory)
- [x] Publisher authentication & ownership enforcement
- [x] Mutation rate limiting middleware
- [x] Auditable ownership history log & ownership transfers
- [ ] Advanced filtering options

### Phase 3: Community (Planned)

- [ ] User profiles
- [ ] Featured templates
- [ ] Template recommendations
- [ ] Community discussions
- [ ] Moderation tools

## Testing Checklist

### Manual Testing

- [ ] Test all search filters independently
- [ ] Test all search filters in combination
- [ ] Test template upload with various metadata combinations
- [ ] Test template download with no connection (should fail gracefully)
- [ ] Test authentication with wrong credentials
- [ ] Test template operations without authentication (should be allowed for read-only)
- [ ] Test with large template files (verify 50MB limit)
- [ ] Test with invalid ZIP files
- [ ] Test rating system with multiple users
- [ ] Test web UI on different browsers

### Automated Testing

```bash
cd registry-api
npm test
```

Should pass all test suites:

- Authentication tests
- Template publishing tests
- Search tests
- Review/rating tests
- Error handling tests

### Integration Testing

1. Full workflow from CLI:
   - Signup account
   - Publish template
   - Search for template
   - Install template
   - Rate template

2. Full workflow from web UI:
   - Browse templates
   - Login/signup
   - View ratings and reviews

3. Conflict handling:
   - Duplicate publication
   - Invalid metadata
   - Missing files

## Performance Criteria

- [ ] Search results return within 500ms
- [ ] Template download completes within 30s (for typical 10MB)
- [ ] Web UI search responds within 1s
- [ ] Rating submission completes within 2s
- [ ] API handles 100 concurrent requests

## Security Criteria

- [ ] Passwords never logged or transmitted in plain text
- [ ] JWT tokens expire after 7 days
- [ ] ZIP files validated before storage
- [ ] File uploads limited to 50MB
- [ ] Input validation on all endpoints
- [ ] SQL injection prevention (N/A for current in-memory, but ready for MongoDB)
- [ ] CORS properly configured

## Sign-Off

- [ ] All acceptance criteria met
- [ ] All automated tests passing
- [ ] Manual testing completed
- [ ] Performance benchmarks achieved
- [ ] Security audit passed
- [ ] Documentation complete
- [ ] Ready for production deployment

## Notes

- Current implementation uses in-memory stores; MongoDB integration ready for production
- Rate limiting recommended before production deployment
- Consider CDN for template file downloads in future phases
- Email verification recommended for production (currently not implemented)
- Two-factor authentication recommended for security-conscious users
