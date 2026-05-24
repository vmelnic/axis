.PHONY: help up down restart check serve schema schema-apply schema-reset migrate migrate-apply generate seed logs

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

# Infrastructure
up: ## Start all services (postgres, redis, meilisearch, prometheus, grafana, mailcatcher)
	docker compose up -d

down: ## Stop all services
	docker compose down

restart: ## Restart all services
	docker compose down && docker compose up -d

# Server
serve: ## Start axis HTTP server
	./scripts/serve.sh

check: ## Check infrastructure, project, and database status
	./scripts/check.sh

# Database
schema: ## Generate SQL DDL to sql/schema.sql
	./scripts/schema.sh

schema-apply: ## Generate and apply schema (idempotent)
	./scripts/schema.sh --apply

schema-reset: ## Drop all tables and recreate schema
	./scripts/schema.sh --reset

migrate: ## Generate migration SQL to sql/migrate.sql
	./scripts/migrate.sh

migrate-apply: ## Generate and apply migrations
	./scripts/migrate.sh --apply

seed: ## Insert dev seed data (admin, provider, client, categories)
	./scripts/seed.sh

# Code generation
generate: ## Regenerate all artifacts (SQL, OpenAPI, TypeScript, GraphQL, etc.)
	./scripts/generate.sh

# Logs
logs: ## Tail all service logs (or: make logs SVC=postgres)
	./scripts/logs.sh $(SVC)
