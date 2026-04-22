-- Migration 004: `meta` table for cross-cutting key-value DB metadata.
--
-- Introduced to stamp the parser version (task #118) so rlm can detect
-- when the binary's parser vocabulary has changed since the DB was
-- last written, and trigger a full reindex. Built as a generic
-- key-value store so future single-row settings (index UUID, last
-- rlm-binary path, project marker fingerprint) can land here without
-- another migration.

CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
