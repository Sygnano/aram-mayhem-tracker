# Build context is this folder alone (Railway's root directory points here), so nothing outside
# `service/` is available or needed.

FROM node:24-bookworm-slim AS builder
WORKDIR /build
# pnpm is pinned in package.json; installing that exact version avoids depending on corepack.
RUN npm install --global --no-fund --no-audit pnpm@12.3.4

# Dependencies first, so a source-only change reuses this layer.
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile

COPY tsconfig.json tsconfig.build.json ./
COPY src ./src
RUN pnpm build && pnpm prune --prod

FROM node:24-bookworm-slim AS runtime
ENV NODE_ENV=production

# The service account. The container starts as root so the entrypoint can take ownership of the
# volume Railway mounts at runtime, then drops to this user before exec'ing the service.
RUN groupadd --system --gid 10001 app \
    && useradd --system --uid 10001 --gid 10001 --no-create-home app \
    && mkdir -p /data && chown app:app /data

# The database lives on the Railway volume mounted at /data, not in the image.
ENV DATABASE_PATH=/data/cache.db

WORKDIR /app
COPY --from=builder /build/package.json ./
COPY --from=builder /build/node_modules ./node_modules
COPY --from=builder /build/dist ./dist
COPY migrations ./migrations
COPY src/admin.html ./src/admin.html
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
# Copied from a checkout that may not preserve the executable bit (Windows, for one).
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

# Railway overrides PORT; this keeps `docker run -p 8080:8080` working locally.
ENV PORT=8080
EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["node", "dist/server.js"]
