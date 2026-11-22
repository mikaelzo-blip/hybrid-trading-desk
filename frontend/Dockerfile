# syntax=docker/dockerfile:1
FROM node:22-alpine AS builder
WORKDIR /app
COPY package.json package-lock.json* ./
RUN npm ci || npm install
COPY . .
RUN npm run build

FROM node:22-alpine
WORKDIR /app
ENV NODE_ENV=production
COPY --from=builder /app ./
ENV FRONTEND_PORT=3000
EXPOSE 3000
CMD ["npm", "run", "start", "--", "-p", "3000"]
