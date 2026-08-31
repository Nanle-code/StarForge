# Horizon Client Reliability and Retry Policy

## 1. Overview
StarForge uses bounded timeouts and exponential backoff retry policies for Horizon HTTP requests to guard against transient network partitions and rate limits.

## 2. Reliability Specifications
- **Bounded Timeout**: 10-second default request timeout on the shared HTTP client.
- **Retry Count**: Maximum of 3 attempts with exponential backoff (`150ms -> 300ms -> 600ms`).
- **Retry Classification**: Retries on transient 5xx server errors, HTTP 429 Too Many Requests, and low-level connection resets.
- **Permanent Errors**: 4xx client errors (e.g. 404 Account Not Found, 400 Bad Request) fail fast without retrying.
