FROM node:24.11.1-bookworm-slim AS build
WORKDIR /app
RUN corepack enable
COPY apps/web/package.json apps/web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY apps/web ./
ENV API_INTERNAL_ORIGIN=http://127.0.0.1:18080
ENV NEXT_TELEMETRY_DISABLED=1
RUN pnpm build

FROM node:24.11.1-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1
ENV API_INTERNAL_ORIGIN=http://127.0.0.1:18080
COPY --from=build --chown=node:node /app ./
USER node
CMD ["npm", "run", "start", "--", "--port", "13000"]
