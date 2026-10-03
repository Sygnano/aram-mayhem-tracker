/**
 * The errors a handler can answer with. Every error leaves the service as JSON,
 * `{ "error": string, "retryAfter": number | null }`, so the desktop app never has to parse HTML.
 */

export class ApiError extends Error {
  readonly statusCode: number;
  /** Seconds, sent both in the body and as a `Retry-After` header. */
  readonly retryAfter: number | null;

  constructor(statusCode: number, message: string, retryAfter: number | null = null) {
    super(message);
    this.name = new.target.name;
    this.statusCode = statusCode;
    this.retryAfter = retryAfter;
  }
}

/**
 * The store has nothing for this key yet and upstream could not fill it. The app falls back to its
 * own last-good copy, so `retryAfter` matters more than the message.
 */
export class UnavailableError extends ApiError {
  constructor(detail: string) {
    super(503, `data not available yet: ${detail}`, 30);
  }
}

export class BadRequestError extends ApiError {
  constructor(detail: string) {
    super(400, `bad request: ${detail}`);
  }
}

/** Upstream has no such document: an unknown champion id, most often. */
export class NotFoundError extends ApiError {
  constructor(detail: string) {
    super(404, `not found: ${detail}`);
  }
}

/** No or wrong admin token on a write. */
export class UnauthorizedError extends ApiError {
  constructor(detail: string) {
    super(401, `unauthorized: ${detail}`);
  }
}

/** The write is switched off on this deploy (no `ADMIN_TOKEN`). */
export class ForbiddenError extends ApiError {
  constructor(detail: string) {
    super(403, `forbidden: ${detail}`);
  }
}

/** The request is about a state the service is no longer in: a crawl for a version since replaced. */
export class ConflictError extends ApiError {
  constructor(detail: string) {
    super(409, `conflict: ${detail}`);
  }
}

/** The service-wide request budget is spent. The app keeps what it has and asks again. */
export class TooManyRequestsError extends ApiError {
  constructor() {
    super(429, "too many requests", 1);
  }
}

/** An unsuccessful upstream HTTP status, kept so retries, the failure hold and 404s can branch on it. */
export class UpstreamStatusError extends Error {
  readonly status: number;

  constructor(status: number, url: string, note?: string) {
    super(`GET ${url}: upstream returned HTTP ${status}${note ? ` ${note}` : ""}`);
    this.name = "UpstreamStatusError";
    this.status = status;
  }
}

/** The upstream HTTP status behind an error, if there is one. */
export function upstreamStatus(error: unknown): number | null {
  return error instanceof UpstreamStatusError ? error.status : null;
}

/** The JSON body every error response carries. */
export interface ErrorBody {
  error: string;
  retryAfter: number | null;
}

/**
 * Turns anything a handler threw into a status and a body.
 *
 * A 404 from upstream is a statement about an immutable path, an unknown champion id as a rule, so
 * it is reported as a 404 rather than as an internal error the caller would retry. Any other
 * unexpected error is answered generically, because it may carry details about the store or
 * upstream; `internal` is true so the caller logs it in full.
 */
export function toErrorResponse(error: unknown): {
  statusCode: number;
  body: ErrorBody;
  internal: boolean;
} {
  const mapped =
    upstreamStatus(error) === 404
      ? new NotFoundError("upstream has no data for this champion")
      : error;

  if (mapped instanceof ApiError) {
    return {
      statusCode: mapped.statusCode,
      body: { error: mapped.message, retryAfter: mapped.retryAfter },
      internal: false,
    };
  }
  // Fastify's own client errors: a body that is not JSON, a schema that rejects the query, and so on.
  if (isClientError(mapped)) {
    return {
      statusCode: mapped.statusCode,
      body: { error: `bad request: ${mapped.message}`, retryAfter: null },
      internal: false,
    };
  }
  return { statusCode: 500, body: { error: "internal error", retryAfter: null }, internal: true };
}

function isClientError(error: unknown): error is { statusCode: number; message: string } {
  if (typeof error !== "object" || error === null) {
    return false;
  }
  const { statusCode, message } = error as { statusCode?: unknown; message?: unknown };
  return (
    typeof statusCode === "number" &&
    statusCode >= 400 &&
    statusCode < 500 &&
    typeof message === "string"
  );
}
